//! RSS 2.0 ingestion domain — pure, bounded, event-driven parse.
//!
//! Turns the RAW bytes of an already transport-bounded fetch into a typed
//! analysis: the exploitable items of an RSS 2.0 feed, the flow-level
//! findings and the derived durable state. NO I/O happens here — the
//! application layer fetches (see `infrastructure::device::rss_source`),
//! this module only consumes bytes, so the whole matrix is testable
//! without a network.
//!
//! Discipline (the exact calque of `structured_folder.rs`):
//!
//! - a feed-STATE problem (unreadable XML, a non-RSS-2.0 root, zero
//!   exploitable item) is a typed VERDICT, never an `AppError`; only
//!   transport crosses as an error.
//! - the parse is EVENT-DRIVEN (never a DOM) and adds its OWN bounds on
//!   top of the transport cap: element depth, retained item count, text
//!   length. Standard XML entities and CDATA are decoded; custom entities
//!   and DTD content are rendered verbatim, NEVER resolved (`quick-xml`
//!   builds no DTD and resolves no external entity by construction — no
//!   XXE, no entity bomb).
//! - an ingestion NEVER produces a `recognized` story: the nominal
//!   `(Source, Ambiguous)` finding is emitted for EVERY ingestion and the
//!   dedicated state derivation ([`rss_import_state`]) floors at
//!   `NeedsReview` even on an (unreachable) all-recognized set.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::domain::story::{normalize_title, validate_title};

use super::recognition::{
    recognition_quality, ImportState, RecognitionAspect, RecognitionCategory, RecognitionFinding,
    RecognitionQuality,
};

/// The `story_local_imports.source_format_version` written for an `rss`
/// provenance row (forward guard, mirrors the other flows).
pub const RSS_SOURCE_FORMAT_VERSION: u64 = 1;

/// Ceiling on the XML element nesting depth. A real RSS 2.0 document sits
/// at depth 4-5; a deeper one is hostile or malformed and blocks as an
/// unreadable envelope — a typed verdict, never a crash.
pub const MAX_RSS_XML_DEPTH: usize = 32;

/// Ceiling on the RETAINED exploitable items (anti-DoS: bounds the wire
/// payload and the review surface). Items beyond the bound are IGNORED —
/// the feed stays exploitable, the contract documents the cut.
pub const MAX_RSS_ITEMS: usize = 100;

/// Ceiling on one item's cleaned narrative text, in Unicode scalar values
/// (aligned with the folder flow's `MAX_FOLDER_NODE_TEXT_CHARS`, itself a
/// mirror of the editor's write-path bound). Beyond it the text is
/// truncated and the adjustment becomes a finding.
pub const MAX_RSS_ITEM_TEXT_CHARS: usize = 65_536;

/// Ceiling on one item's cleaned TITLE, in Unicode scalar values — applied
/// AT PARSE TIME so every downstream carrier (the preview DTO, a
/// `TitleLink` reference, the content fingerprint) is bounded mechanically
/// and both fetches stay coherent. Far above the canonical title bound
/// (the fallback applies anyway) yet a hard stop against a hostile feed
/// shipping text-sized titles; truncation is a `title_adjusted` finding.
pub const MAX_RSS_ITEM_TITLE_CHARS: usize = 1_024;

/// Ceiling on the feed address, in Unicode scalar values.
pub const MAX_RSS_URL_CHARS: usize = 2048;

/// Ceiling on the HOST carried as the provenance `source_name`. 96 keeps
/// the `Histoire de {hôte}` fallback title inside the canonical
/// `MAX_TITLE_CHARS` bound WITHOUT truncation (every real-world feed host
/// is far shorter; a longer one is refused honestly at the address gate).
const MAX_RSS_HOST_CHARS: usize = 96;

/// The fallback title prefix — frozen in `product-language.md` (`Histoire
/// de {hôte}`). Owned by the domain so the host gate can prove the
/// fallback ALWAYS survives the canonical title validation.
pub const RSS_FALLBACK_TITLE_PREFIX: &str = "Histoire de ";

/// One exploitable feed item, already cleaned and bounded. `title` and
/// `text` carry the CLEANED values (HTML stripped, whitespace collapsed,
/// truncated); the `*_adjusted` flags remember that a transformation (or
/// an absence) occurred — each becomes an `Ambiguous` finding on the item
/// actually ingested ([`rss_item_findings`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssItem {
    /// Cleaned candidate title (may be empty — the creation falls back to
    /// `Histoire de {hôte}`).
    pub title: String,
    /// True when the raw title was absent/empty, transformed by the
    /// cleaning, or would not survive the canonical title validation
    /// (the fallback title will apply — a review step either way).
    pub title_adjusted: bool,
    /// Cleaned narrative text (the item description).
    pub text: String,
    /// True when the description was absent/empty or transformed (HTML
    /// stripped, whitespace collapsed, truncated).
    pub text_adjusted: bool,
    pub guid: Option<String>,
    pub link: Option<String>,
    /// The item references a remote enclosure (podcast audio…). NEVER
    /// downloaded — becomes the `(Media, Missing)` finding at ingestion.
    pub has_enclosure: bool,
}

/// The stable reference of one previewed item, round-tripped by the
/// frontend and re-resolved from zero at accept time: strict `guid` when
/// the item carries one, else the exact cleaned (`title`, `link`) couple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RssItemRef {
    Guid(String),
    TitleLink { title: String, link: Option<String> },
}

/// The full outcome of parsing a fetched feed: the flow-level findings
/// (exactly what the preview shows), the derived durable state, the
/// channel title and the bounded exploitable items. A blocked verdict
/// carries no item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RssAnalysis {
    pub channel_title: Option<String>,
    pub items: Vec<RssItem>,
    pub findings: Vec<RecognitionFinding>,
    pub state: ImportState,
}

impl RssAnalysis {
    /// True iff the feed is blocked (nothing selectable, nothing creatable).
    pub fn is_blocked(&self) -> bool {
        self.state == ImportState::Blocked
    }

    /// The verdict for a byte stream that is not readable XML (malformed,
    /// non-UTF-8, over the depth bound): a single `Envelope` blocking
    /// finding — the calque of the folder flow's `envelope_blocked`.
    fn envelope_blocked() -> Self {
        Self::blocked(vec![RecognitionFinding::blocking(
            RecognitionAspect::Envelope,
        )])
    }

    /// The verdict for a readable XML document whose root is not the
    /// listed RSS 2.0 shape (an Atom `<feed>`, anything else): the
    /// envelope is recognized, the format blocks.
    fn format_blocked() -> Self {
        Self::blocked(vec![
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::blocking(RecognitionAspect::FormatVersion),
        ])
    }

    /// The verdict for a well-formed RSS 2.0 feed holding ZERO exploitable
    /// item: envelope + format recognized, the structure blocks.
    fn empty_blocked() -> Self {
        Self::blocked(vec![
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
            RecognitionFinding::blocking(RecognitionAspect::Structure),
        ])
    }

    fn blocked(findings: Vec<RecognitionFinding>) -> Self {
        let state = rss_import_state(&findings);
        Self {
            channel_title: None,
            items: Vec::new(),
            findings,
            state,
        }
    }
}

/// The RSS state derivation (dedicated per-flow derivation — the
/// `.rustory` and folder ones are untouched): any `Blocking` → `Blocked`
/// (nothing is created); else any `Missing` (a non-downloaded enclosure)
/// → `Partial`; else → `NeedsReview` — NEVER `Recognized`. The floor is
/// structural in the derivation itself: even an (unreachable)
/// all-recognized finding set derives `NeedsReview`, and the nominal
/// `(Source, Ambiguous)` finding makes that case impossible anyway.
pub fn rss_import_state(findings: &[RecognitionFinding]) -> ImportState {
    if recognition_quality(findings) == RecognitionQuality::Unusable {
        return ImportState::Blocked;
    }
    if findings
        .iter()
        .any(|f| f.category == RecognitionCategory::Missing)
    {
        ImportState::Partial
    } else {
        ImportState::NeedsReview
    }
}

/// The flow-level findings of an exploitable feed — exactly what the
/// preview surfaces: envelope + format recognized, and the NOMINAL
/// provenance ambiguity every ingestion carries.
fn exploitable_flow_findings() -> Vec<RecognitionFinding> {
    vec![
        RecognitionFinding::recognized(RecognitionAspect::Envelope),
        RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
        RecognitionFinding::ambiguous(RecognitionAspect::Source),
    ]
}

/// The findings persisted for ONE ingested item — what the created
/// story's durable state, chip and report speak of. Envelope + format are
/// recognized by construction (a blocked feed never reaches an accept),
/// the `(Source, Ambiguous)` floor is always present, the item's own
/// adjustments surface as ambiguities, and a referenced enclosure is the
/// `(Media, Missing)` finding (state `Partial`).
pub fn rss_item_findings(item: &RssItem) -> Vec<RecognitionFinding> {
    let mut findings = vec![
        RecognitionFinding::recognized(RecognitionAspect::Envelope),
        RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
        RecognitionFinding::ambiguous(RecognitionAspect::Source),
    ];
    findings.push(if item.title_adjusted {
        RecognitionFinding::ambiguous(RecognitionAspect::Title)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Title)
    });
    findings.push(if item.text_adjusted {
        RecognitionFinding::ambiguous(RecognitionAspect::Structure)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Structure)
    });
    if item.has_enclosure {
        findings.push(RecognitionFinding {
            aspect: RecognitionAspect::Media,
            category: RecognitionCategory::Missing,
        });
    }
    findings
}

/// The stable reference of a previewed item ([`RssItemRef`] semantics).
pub fn rss_item_ref(item: &RssItem) -> RssItemRef {
    match &item.guid {
        Some(guid) => RssItemRef::Guid(guid.clone()),
        None => RssItemRef::TitleLink {
            title: item.title.clone(),
            link: item.link.clone(),
        },
    }
}

/// The canonical fingerprint of one previewed item — the proof of WHAT the
/// user actually reread. SHA-256 over an unambiguous JSON array of every
/// ingestion-relevant field; the accept recomputes it on the FRESH parse
/// and refuses ANY divergence (same guid but a different text/title/link/
/// enclosure ⇒ the source changed since the preview — never a creation
/// from content the user never saw).
pub fn rss_item_fingerprint(item: &RssItem) -> String {
    let canonical = serde_json::json!([
        item.title,
        item.text,
        item.guid,
        item.link,
        item.has_enclosure,
    ]);
    // Serializing a small array of plain scalars cannot fail in practice.
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    crate::domain::story::content_checksum_bytes(&bytes)
}

/// Resolve a round-tripped reference against a FRESH parse (the accept
/// re-fetches; the reference is a pointer, never an authority). The match
/// must be UNIQUE: a missing item OR an ambiguous match is `None` — the
/// caller refuses honestly (`La source a changé depuis la récupération.`),
/// NEVER an approximate match. A `TitleLink` reference only ever considers
/// the guid-LESS items — mirroring its emission ([`rss_item_ref`]), so a
/// guid-carrying item sharing the same (title, link) can never shadow it
/// into a false ambiguity (and a guid-less item that GAINED a guid since
/// the preview diverges through the content fingerprint anyway).
pub fn resolve_rss_item<'a>(items: &'a [RssItem], reference: &RssItemRef) -> Option<&'a RssItem> {
    let mut matches = items.iter().filter(|item| match reference {
        RssItemRef::Guid(guid) => item.guid.as_deref() == Some(guid.as_str()),
        RssItemRef::TitleLink { title, link } => {
            item.guid.is_none() && &item.title == title && &item.link == link
        }
    });
    let found = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(found)
}

/// Parse the raw bytes of a fetched feed into the typed analysis. Pure and
/// deterministic; every bound above is applied here. The bytes are already
/// transport-bounded by the caller.
pub fn parse_rss(bytes: &[u8]) -> RssAnalysis {
    let mut reader = Reader::from_reader(bytes);
    let mut buf = Vec::new();

    /// What the element stack currently points at, derived from the path.
    #[derive(PartialEq, Eq, Clone, Copy)]
    enum Capture {
        None,
        ChannelTitle,
        ItemTitle,
        ItemDescription,
        ItemGuid,
        ItemLink,
    }

    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut root_seen = false;
    let mut root_is_rss2 = false;
    let mut channel_title: Option<String> = None;
    let mut items: Vec<RssItem> = Vec::new();
    // The references already retained — deduplication runs INLINE so the
    // item cap only ever counts NOVEL references (duplicates never squat
    // the quota while unique items get dropped).
    let mut seen_refs: Vec<RssItemRef> = Vec::new();
    let mut current: Option<DraftItem> = None;
    let mut capture = Capture::None;
    // The depth of the CAPTURED element: the capture settles only when ITS
    // element closes, so a child tag inside a captured field (inline HTML
    // markup in a description…) keeps accumulating the descendant text
    // instead of silently dropping the field.
    let mut capture_depth: usize = 0;
    // A child ELEMENT was skipped inside the captured field: the markup
    // was stripped from the ingested value, which is an adjustment the
    // findings must surface exactly like escaped-then-cleaned markup.
    let mut capture_saw_markup = false;
    let mut text = String::new();

    #[derive(Default)]
    struct DraftItem {
        title: Option<String>,
        title_markup_stripped: bool,
        description: Option<String>,
        text_markup_stripped: bool,
        guid: Option<String>,
        link: Option<String>,
        has_enclosure: bool,
    }

    fn is_item_path(stack: &[Vec<u8>]) -> bool {
        stack.len() == 3 && stack[0] == b"rss" && stack[1] == b"channel" && stack[2] == b"item"
    }

    /// Clean the draft's fields and decide exploitability: an item with
    /// neither a usable title nor a usable text has nothing to ingest.
    fn finalize_item(draft: DraftItem) -> Option<RssItem> {
        let raw_title = draft.title.unwrap_or_default();
        let (title, title_cleaned) = clean_rss_text(&raw_title);
        // The TITLE gets its own, much tighter wire bound (the preview DTO,
        // the `TitleLink` reference and the fingerprint all carry it).
        let title_char_count = title.chars().count();
        let title: String = title.chars().take(MAX_RSS_ITEM_TITLE_CHARS).collect();
        let title_truncated = title_char_count > MAX_RSS_ITEM_TITLE_CHARS;
        let raw_text = draft.description.unwrap_or_default();
        let (text, text_cleaned) = clean_rss_text(&raw_text);
        if title.is_empty() && text.is_empty() {
            return None;
        }
        // The fallback title (`Histoire de {hôte}`) applies whenever the
        // cleaned candidate would not survive the canonical validation —
        // absent, over the length bound, or carrying denied code points.
        let title_survives = !title.is_empty() && validate_title(&normalize_title(&title)).is_ok();
        let guid = draft
            .guid
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty());
        let link = draft
            .link
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        Some(RssItem {
            title_adjusted: title_cleaned
                || title_truncated
                || draft.title_markup_stripped
                || !title_survives,
            text_adjusted: text_cleaned || draft.text_markup_stripped || text.is_empty(),
            title,
            text,
            guid,
            link,
            has_enclosure: draft.has_enclosure,
        })
    }

    fn capture_for(stack: &[Vec<u8>], name: &[u8], in_item: bool) -> Capture {
        if in_item && is_item_path(stack) {
            return match name {
                b"title" => Capture::ItemTitle,
                b"description" => Capture::ItemDescription,
                b"guid" => Capture::ItemGuid,
                b"link" => Capture::ItemLink,
                _ => Capture::None,
            };
        }
        if stack.len() == 2 && stack[0] == b"rss" && stack[1] == b"channel" && name == b"title" {
            return Capture::ChannelTitle;
        }
        Capture::None
    }

    /// The three-way outcome of the root gate.
    enum RootGate {
        /// The listed `<rss version="2.0">` root.
        Rss2,
        /// A readable root that is not the listed shape (Atom `<feed>`, a
        /// versionless or other-version `<rss>`…) — the format verdict.
        NotRss2,
        /// The root TAG itself is malformed (an unreadable attribute) —
        /// the unreadable-envelope verdict, never silently skipped.
        Malformed,
    }

    /// The root gate: the FIRST element must be `<rss version="2.0">`
    /// exactly. EVERY attribute of the root tag is walked and attribute
    /// errors PROPAGATE (a malformed root tag is an unreadable envelope)
    /// instead of being flattened away — a broken root must never pass as
    /// RSS 2.0 just because a readable `version="2.0"` sits next to the
    /// malformed part.
    fn root_gate(start: &BytesStart<'_>) -> RootGate {
        if start.name().as_ref() != b"rss" {
            return RootGate::NotRss2;
        }
        let mut is_listed_version = false;
        for attr in start.attributes() {
            let Ok(attr) = attr else {
                return RootGate::Malformed;
            };
            if attr.key.as_ref() == b"version" {
                let Ok(value) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
                    return RootGate::Malformed;
                };
                is_listed_version = value.trim() == "2.0";
            }
        }
        if is_listed_version {
            RootGate::Rss2
        } else {
            RootGate::NotRss2
        }
    }

    loop {
        match reader.read_event_into(&mut buf) {
            // Malformed XML / non-UTF-8 content: the unreadable-envelope
            // verdict, never a crash and never a transport error.
            Err(_) => return RssAnalysis::envelope_blocked(),
            Ok(Event::Eof) => {
                // A body truncated MID-DOCUMENT (open elements at EOF) is
                // NOT well-formed XML: the raw reader has no document-state
                // tracking of its own, so the stack is the truth here — an
                // invisible truncation must never pass as a healthy feed.
                if !stack.is_empty() {
                    return RssAnalysis::envelope_blocked();
                }
                break;
            }
            Ok(Event::Start(start)) => {
                if stack.len() + 1 > MAX_RSS_XML_DEPTH {
                    return RssAnalysis::envelope_blocked();
                }
                if !root_seen {
                    root_seen = true;
                    match root_gate(&start) {
                        RootGate::Malformed => return RssAnalysis::envelope_blocked(),
                        RootGate::NotRss2 => return RssAnalysis::format_blocked(),
                        RootGate::Rss2 => root_is_rss2 = true,
                    }
                } else if stack.is_empty() {
                    // Content AFTER the closed root element (a second
                    // document, a stray tag): not well-formed XML — its
                    // items must never become selectable around the gate.
                    return RssAnalysis::envelope_blocked();
                }
                let name = start.name().as_ref().to_vec();
                // Inside a captured field, a child element (inline HTML
                // markup in a description…) is an opaque tag: keep the
                // capture and its accumulated text, just track the depth —
                // and remember the markup was STRIPPED from the ingested
                // value (an adjustment the findings must surface).
                if capture != Capture::None {
                    capture_saw_markup = true;
                    stack.push(name);
                    continue;
                }
                if is_item_path(&stack) && name == b"item" {
                    // Nested <item> inside an item is not RSS 2.0 — treat
                    // it as an opaque unknown element (no new draft).
                    stack.push(name);
                    continue;
                }
                if stack.len() == 2
                    && stack[0] == b"rss"
                    && stack[1] == b"channel"
                    && name == b"item"
                {
                    current = Some(DraftItem::default());
                    stack.push(name);
                    continue;
                }
                capture = capture_for(&stack, &name, current.is_some());
                if capture != Capture::None {
                    text.clear();
                    capture_saw_markup = false;
                    // The captured element's own depth, once pushed.
                    capture_depth = stack.len() + 1;
                }
                if current.is_some() && is_item_path(&stack) && name == b"enclosure" {
                    if let Some(draft) = current.as_mut() {
                        draft.has_enclosure = true;
                    }
                }
                stack.push(name);
            }
            Ok(Event::Empty(start)) => {
                if stack.len() + 1 > MAX_RSS_XML_DEPTH {
                    return RssAnalysis::envelope_blocked();
                }
                if !root_seen {
                    // A self-closed root cannot be a usable RSS document.
                    return RssAnalysis::format_blocked();
                }
                if stack.is_empty() {
                    // A self-closed element AFTER the closed root: content
                    // outside the document — not well-formed XML.
                    return RssAnalysis::envelope_blocked();
                }
                if capture != Capture::None {
                    // A self-closed child (`<br/>`…) inside a captured
                    // field: stripped markup, exactly like an open child.
                    capture_saw_markup = true;
                    continue;
                }
                if current.is_some()
                    && is_item_path(&stack)
                    && start.name().as_ref() == b"enclosure"
                {
                    if let Some(draft) = current.as_mut() {
                        draft.has_enclosure = true;
                    }
                }
            }
            Ok(Event::Text(content)) => {
                if capture != Capture::None {
                    let Ok(decoded) = content.decode() else {
                        return RssAnalysis::envelope_blocked();
                    };
                    text.push_str(&decoded);
                }
            }
            Ok(Event::CData(content)) => {
                if capture != Capture::None {
                    let Ok(decoded) = content.decode() else {
                        return RssAnalysis::envelope_blocked();
                    };
                    text.push_str(&decoded);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if capture != Capture::None {
                    // Character references and the five predefined XML
                    // entities are decoded; a custom entity is rendered
                    // VERBATIM (`&name;`), never resolved (no DTD lookup).
                    match reference.resolve_char_ref() {
                        Err(_) => return RssAnalysis::envelope_blocked(),
                        Ok(Some(ch)) => text.push(ch),
                        Ok(None) => {
                            let Ok(name) = reference.decode() else {
                                return RssAnalysis::envelope_blocked();
                            };
                            match resolve_predefined_entity(&name) {
                                Some(resolved) => text.push_str(resolved),
                                None => {
                                    text.push('&');
                                    text.push_str(&name);
                                    text.push(';');
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                // The element being closed sits at depth `stack.len()`.
                let closing_depth = stack.len();
                let closed = stack.pop();
                let Some(closed) = closed else {
                    // An unmatched end tag is malformed XML.
                    return RssAnalysis::envelope_blocked();
                };
                // A captured field settles ONLY when ITS element closes —
                // a child tag inside it kept accumulating text (the
                // depth-aware capture), so this End may be the child's.
                let settles_capture = capture != Capture::None && closing_depth == capture_depth;
                match capture {
                    _ if !settles_capture => {}
                    Capture::None => {}
                    Capture::ChannelTitle => {
                        let (cleaned, _) = clean_rss_text(&text);
                        if !cleaned.is_empty() {
                            channel_title = Some(cleaned);
                        }
                        capture = Capture::None;
                    }
                    Capture::ItemTitle
                    | Capture::ItemDescription
                    | Capture::ItemGuid
                    | Capture::ItemLink => {
                        if let Some(draft) = current.as_mut() {
                            let value = std::mem::take(&mut text);
                            match capture {
                                Capture::ItemTitle => {
                                    draft.title = Some(value);
                                    draft.title_markup_stripped |= capture_saw_markup;
                                }
                                Capture::ItemDescription => {
                                    draft.description = Some(value);
                                    draft.text_markup_stripped |= capture_saw_markup;
                                }
                                Capture::ItemGuid => draft.guid = Some(value),
                                Capture::ItemLink => draft.link = Some(value),
                                _ => unreachable!(),
                            }
                        }
                        capture = Capture::None;
                    }
                }
                if closed == b"item" && stack.len() == 2 {
                    if let Some(draft) = current.take() {
                        if items.len() < MAX_RSS_ITEMS {
                            if let Some(item) = finalize_item(draft) {
                                // Inline de-duplication: only a NOVEL
                                // reference consumes the item quota (the
                                // accept resolution demands uniqueness).
                                let reference = rss_item_ref(&item);
                                if !seen_refs.contains(&reference) {
                                    seen_refs.push(reference);
                                    items.push(item);
                                }
                            }
                        }
                        // Beyond the bound: silently ignored (documented
                        // at the contract) — the feed stays exploitable.
                    }
                }
            }
            // Comments, the XML declaration, processing instructions and
            // DOCTYPE content are ignored — never resolved, never captured.
            Ok(Event::Comment(_))
            | Ok(Event::Decl(_))
            | Ok(Event::PI(_))
            | Ok(Event::DocType(_)) => {}
        }
        buf.clear();
    }

    if !root_seen || !root_is_rss2 {
        // No root element at all (empty / whitespace-only document).
        return RssAnalysis::envelope_blocked();
    }
    if items.is_empty() {
        return RssAnalysis::empty_blocked();
    }
    let findings = exploitable_flow_findings();
    let state = rss_import_state(&findings);
    RssAnalysis {
        channel_title,
        items,
        findings,
        state,
    }
}

/// Clean one text field, PURELY: strip HTML tags (a `<` opens a tag only
/// when followed by an ASCII letter, `/`, `!` or `?` — a literal `<` in
/// prose survives), collapse every whitespace run into one space, trim,
/// and truncate at [`MAX_RSS_ITEM_TEXT_CHARS`]. Returns the cleaned text
/// and whether ANY transformation changed the input (→ a finding).
pub fn clean_rss_text(raw: &str) -> (String, bool) {
    let stripped = strip_html_tags(raw);
    let collapsed: String = {
        let mut out = String::with_capacity(stripped.len());
        let mut in_whitespace = false;
        for ch in stripped.chars() {
            if ch.is_whitespace() {
                in_whitespace = true;
                continue;
            }
            if in_whitespace && !out.is_empty() {
                out.push(' ');
            }
            in_whitespace = false;
            out.push(ch);
        }
        out
    };
    let truncated: String = collapsed.chars().take(MAX_RSS_ITEM_TEXT_CHARS).collect();
    let adjusted = truncated != raw;
    (truncated, adjusted)
}

/// Remove `<…>` HTML/XML tag runs from prose. Conservative: a `<` starts
/// a tag ONLY when followed by an ASCII letter, `/`, `!` or `?`; the
/// closing `>` is only honored OUTSIDE a quoted attribute value (so
/// `<a href="x>y">` is swallowed whole — no tag debris leaks into the
/// prose); an unterminated tag swallows to the end (it cannot be prose).
fn strip_html_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<'
            && matches!(
                chars.peek(),
                Some(next) if next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?')
            )
        {
            let mut in_quote: Option<char> = None;
            for inner in chars.by_ref() {
                match in_quote {
                    Some(quote) if inner == quote => in_quote = None,
                    Some(_) => {}
                    None if inner == '"' || inner == '\'' => in_quote = Some(inner),
                    None if inner == '>' => break,
                    None => {}
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// True iff `url` is a supported feed address: `http`/`https` only, no
/// userinfo, a non-empty sober host, bounded length. Pure — the SINGLE
/// authority on the address (the UI only gates on non-emptiness before
/// invoking; Rust owns every other rule).
pub fn is_supported_feed_url(url: &str) -> bool {
    feed_url_host(url).is_some()
}

/// Extract the HOST of a supported feed address — the ONLY fragment that
/// ever reaches the provenance row or a diagnostic line (a full feed URL
/// can carry private tokens in its query string: PII). `None` iff the
/// address is not supported ([`is_supported_feed_url`]).
pub fn feed_url_host(url: &str) -> Option<String> {
    if url.is_empty() || url.chars().count() > MAX_RSS_URL_CHARS {
        return None;
    }
    let rest = strip_supported_scheme(url)?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    // An IPv6 literal host (`[::1]`) cannot be carried as a sober
    // provenance source name (it embeds `:`); refused honestly.
    if authority.starts_with('[') {
        return None;
    }
    let mut parts = authority.splitn(2, ':');
    let host = parts.next().unwrap_or("");
    if let Some(port) = parts.next() {
        // A structurally impossible port is an INVALID ADDRESS — it must
        // never reach the fetch and come back as a lying "unreachable".
        match port.parse::<u32>() {
            Ok(value) if (1..=65_535).contains(&value) => {}
            _ => return None,
        }
    }
    if !is_sober_feed_host(host) {
        return None;
    }
    Some(host.to_string())
}

/// Strip a SUPPORTED scheme prefix (`http://` / `https://`, ASCII
/// case-insensitive). Any other scheme (`file:`, `data:`, `ftp:`, an
/// unknown one) is refused — never fetched. Boundary-safe on arbitrary
/// user input: `get(..len)` returns `None` when the cut would fall inside
/// a multi-byte character (e.g. `http:/é…`) instead of panicking — the
/// invalid address stays a typed refusal, never a worker crash.
fn strip_supported_scheme(url: &str) -> Option<&str> {
    for scheme in ["http://", "https://"] {
        if let (Some(prefix), Some(rest)) = (url.get(..scheme.len()), url.get(scheme.len()..)) {
            if prefix.eq_ignore_ascii_case(scheme) {
                return Some(rest);
            }
        }
    }
    None
}

/// The sobriety rules a host must satisfy to be carried as the provenance
/// `source_name`: non-empty, bounded, free of path separators / `:` /
/// control characters, not a dot navigation — the same floor the DB CHECK
/// and the report line demand of every source name. On top, the
/// `Histoire de {hôte}` fallback title built from this host must survive
/// the canonical title validation (no denied formatting code point, no
/// over-length) — proven HERE so the creation can never insert an invalid
/// title.
fn is_sober_feed_host(host: &str) -> bool {
    if host.is_empty() || host.chars().count() > MAX_RSS_HOST_CHARS {
        return false;
    }
    if host
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control() || c.is_whitespace())
    {
        return false;
    }
    if host == "." || host == ".." {
        return false;
    }
    let fallback = format!("{RSS_FALLBACK_TITLE_PREFIX}{host}");
    validate_title(&normalize_title(&fallback)).is_ok() && normalize_title(&fallback) == fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(items: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel><title>Mon flux</title>{items}</channel></rss>"
        )
    }

    fn simple_item(title: &str, description: &str) -> String {
        format!("<item><title>{title}</title><description>{description}</description></item>")
    }

    // ===== nominal parse =====

    #[test]
    fn nominal_feed_parses_items_channel_title_and_floor_state() {
        let xml = feed(&format!(
            "{}{}",
            simple_item("Episode 1", "Premier texte."),
            simple_item("Episode 2", "Deuxième texte.")
        ));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.channel_title.as_deref(), Some("Mon flux"));
        assert_eq!(analysis.items.len(), 2);
        assert_eq!(analysis.items[0].title, "Episode 1");
        assert_eq!(analysis.items[0].text, "Premier texte.");
        assert!(!analysis.items[0].title_adjusted);
        assert!(!analysis.items[0].text_adjusted);
        assert_eq!(analysis.state, ImportState::NeedsReview);
        assert!(!analysis.is_blocked());
    }

    #[test]
    fn flow_findings_carry_the_nominal_source_ambiguity() {
        let xml = feed(&simple_item("Episode", "Texte."));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(
            analysis.findings,
            vec![
                RecognitionFinding::recognized(RecognitionAspect::Envelope),
                RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
                RecognitionFinding::ambiguous(RecognitionAspect::Source),
            ]
        );
    }

    #[test]
    fn cdata_and_standard_entities_are_decoded() {
        let xml = feed(
            "<item><title>Fable &amp; Cie &quot;demain&quot;</title><description><![CDATA[Il était < une > fois.]]></description></item>",
        );
        let analysis = parse_rss(xml.as_bytes());
        // Standard entities in regular text ARE decoded…
        assert_eq!(analysis.items[0].title, "Fable & Cie \"demain\"");
        // …while CDATA content is raw character data by the XML spec:
        // nothing inside is interpreted (the `<` here is followed by a
        // space, so the tag-stripper keeps it as prose).
        assert_eq!(analysis.items[0].text, "Il était < une > fois.");
    }

    #[test]
    fn character_references_are_decoded() {
        let xml = feed(&simple_item("&#201;t&#xE9;", "Plage &#38; soleil."));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items[0].title, "Été");
        assert_eq!(analysis.items[0].text, "Plage & soleil.");
    }

    #[test]
    fn a_custom_entity_is_rendered_verbatim_never_resolved() {
        let xml = feed(&simple_item("Episode", "Contenu &custom; conservé."));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items[0].text, "Contenu &custom; conservé.");
        // The transformation flag does not fire for a verbatim entity.
        assert!(!analysis.items[0].text_adjusted);
    }

    #[test]
    fn guid_link_and_enclosure_are_captured() {
        let xml = feed(
            "<item><title>Ep</title><description>T.</description><guid>abc-123</guid><link>https://exemple.fr/ep</link><enclosure url=\"https://exemple.fr/ep.mp3\" length=\"1\" type=\"audio/mpeg\"/></item>",
        );
        let analysis = parse_rss(xml.as_bytes());
        let item = &analysis.items[0];
        assert_eq!(item.guid.as_deref(), Some("abc-123"));
        assert_eq!(item.link.as_deref(), Some("https://exemple.fr/ep"));
        assert!(item.has_enclosure);
    }

    #[test]
    fn an_open_enclosure_element_counts_too() {
        let xml = feed(
            "<item><title>Ep</title><enclosure url=\"https://exemple.fr/ep.mp3\" length=\"1\" type=\"audio/mpeg\"></enclosure></item>",
        );
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.items[0].has_enclosure);
    }

    #[test]
    fn an_item_without_title_or_text_is_not_exploitable() {
        let xml = feed(&format!(
            "{}<item><guid>only-a-guid</guid></item>",
            simple_item("Episode", "Texte.")
        ));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items.len(), 1);
        assert_eq!(analysis.items[0].title, "Episode");
    }

    #[test]
    fn items_with_duplicate_references_keep_the_first_only() {
        let xml = feed(&format!(
            "{}{}",
            "<item><title>Ep</title><description>Un.</description><guid>dup</guid></item>",
            "<item><title>Autre</title><description>Deux.</description><guid>dup</guid></item>"
        ));
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items.len(), 1);
        assert_eq!(analysis.items[0].text, "Un.");
    }

    // ===== verdicts =====

    #[test]
    fn unreadable_xml_is_the_envelope_blocking_verdict() {
        let analysis = parse_rss(b"pas du xml <<<");
        assert_eq!(
            analysis.findings,
            vec![RecognitionFinding::blocking(RecognitionAspect::Envelope)]
        );
        assert_eq!(analysis.state, ImportState::Blocked);
        assert!(analysis.items.is_empty());
    }

    #[test]
    fn non_utf8_bytes_are_the_envelope_blocking_verdict() {
        let mut xml = feed(&simple_item("Episode", "Texte.")).into_bytes();
        // Corrupt one text byte into invalid UTF-8.
        let position = xml
            .windows(7)
            .position(|w| w == b"Episode")
            .expect("marker");
        xml[position] = 0xFF;
        let analysis = parse_rss(&xml);
        assert!(analysis.is_blocked());
        assert_eq!(
            analysis.findings[0],
            RecognitionFinding::blocking(RecognitionAspect::Envelope)
        );
    }

    #[test]
    fn an_atom_feed_is_the_format_blocking_verdict() {
        let xml = "<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Atom</title></feed>";
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(
            analysis.findings,
            vec![
                RecognitionFinding::recognized(RecognitionAspect::Envelope),
                RecognitionFinding::blocking(RecognitionAspect::FormatVersion),
            ]
        );
        assert_eq!(analysis.state, ImportState::Blocked);
    }

    #[test]
    fn a_versionless_or_other_version_rss_root_blocks_on_format() {
        for root in ["<rss>", "<rss version=\"0.91\">"] {
            let xml = format!("{root}<channel><item><title>t</title></item></channel></rss>");
            let analysis = parse_rss(xml.as_bytes());
            assert_eq!(
                analysis.state,
                ImportState::Blocked,
                "root {root} must block"
            );
            assert!(analysis
                .findings
                .iter()
                .any(|f| f.aspect == RecognitionAspect::FormatVersion
                    && f.category == RecognitionCategory::Blocking));
        }
    }

    #[test]
    fn a_feed_with_zero_exploitable_item_blocks_on_structure() {
        let xml = feed("<item><guid>seulement-un-guid</guid></item>");
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(
            analysis.findings,
            vec![
                RecognitionFinding::recognized(RecognitionAspect::Envelope),
                RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
                RecognitionFinding::blocking(RecognitionAspect::Structure),
            ]
        );
        assert_eq!(analysis.state, ImportState::Blocked);
    }

    #[test]
    fn an_empty_document_is_the_envelope_blocking_verdict() {
        for bytes in [&b""[..], &b"   \n  "[..]] {
            let analysis = parse_rss(bytes);
            assert_eq!(analysis.state, ImportState::Blocked);
            assert_eq!(
                analysis.findings[0],
                RecognitionFinding::blocking(RecognitionAspect::Envelope)
            );
        }
    }

    // ===== bounds =====

    #[test]
    fn depth_beyond_the_bound_is_the_envelope_blocking_verdict() {
        let mut xml = String::from("<rss version=\"2.0\"><channel>");
        for _ in 0..MAX_RSS_XML_DEPTH {
            xml.push_str("<a>");
        }
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.is_blocked());
        assert_eq!(
            analysis.findings[0],
            RecognitionFinding::blocking(RecognitionAspect::Envelope)
        );
    }

    #[test]
    fn items_beyond_the_bound_are_ignored_and_the_feed_stays_exploitable() {
        let mut inner = String::new();
        for index in 0..(MAX_RSS_ITEMS + 10) {
            inner.push_str(&simple_item(&format!("Episode {index}"), "Texte."));
        }
        let analysis = parse_rss(feed(&inner).as_bytes());
        assert_eq!(analysis.items.len(), MAX_RSS_ITEMS);
        assert_eq!(analysis.state, ImportState::NeedsReview);
    }

    #[test]
    fn item_text_beyond_the_bound_is_truncated_with_a_finding() {
        let long = "a".repeat(MAX_RSS_ITEM_TEXT_CHARS + 100);
        let xml = feed(&simple_item("Episode", &long));
        let analysis = parse_rss(xml.as_bytes());
        let item = &analysis.items[0];
        assert_eq!(item.text.chars().count(), MAX_RSS_ITEM_TEXT_CHARS);
        assert!(item.text_adjusted);
    }

    // ===== cleaning =====

    #[test]
    fn clean_strips_html_tags_and_flags_the_adjustment() {
        let (text, adjusted) = clean_rss_text("<p>Bonjour <strong>toi</strong></p>");
        assert_eq!(text, "Bonjour toi");
        assert!(adjusted);
    }

    #[test]
    fn clean_collapses_whitespace_runs() {
        let (text, adjusted) = clean_rss_text("Un  \n\t deux   trois ");
        assert_eq!(text, "Un deux trois");
        assert!(adjusted);
    }

    #[test]
    fn clean_keeps_a_literal_less_than_in_prose() {
        let (text, adjusted) = clean_rss_text("2 < 3 et 4 > 1");
        assert_eq!(text, "2 < 3 et 4 > 1");
        assert!(!adjusted);
    }

    #[test]
    fn clean_is_identity_on_already_clean_text() {
        let (text, adjusted) = clean_rss_text("Texte déjà propre.");
        assert_eq!(text, "Texte déjà propre.");
        assert!(!adjusted);
    }

    #[test]
    fn an_unterminated_tag_swallows_to_the_end() {
        let (text, adjusted) = clean_rss_text("Bonjour <em oups");
        assert_eq!(text, "Bonjour");
        assert!(adjusted);
    }

    // ===== per-item findings + state =====

    fn plain_item() -> RssItem {
        RssItem {
            title: "Episode".into(),
            title_adjusted: false,
            text: "Texte.".into(),
            text_adjusted: false,
            guid: Some("g1".into()),
            link: None,
            has_enclosure: false,
        }
    }

    #[test]
    fn item_findings_always_carry_the_source_ambiguity_floor() {
        let findings = rss_item_findings(&plain_item());
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Source
                && f.category == RecognitionCategory::Ambiguous));
        assert_eq!(rss_import_state(&findings), ImportState::NeedsReview);
    }

    #[test]
    fn an_adjusted_title_is_a_title_ambiguity() {
        let item = RssItem {
            title_adjusted: true,
            ..plain_item()
        };
        let findings = rss_item_findings(&item);
        assert!(findings.iter().any(|f| f.aspect == RecognitionAspect::Title
            && f.category == RecognitionCategory::Ambiguous));
    }

    #[test]
    fn an_adjusted_text_is_a_structure_ambiguity() {
        let item = RssItem {
            text_adjusted: true,
            ..plain_item()
        };
        let findings = rss_item_findings(&item);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Structure
                && f.category == RecognitionCategory::Ambiguous));
    }

    #[test]
    fn an_enclosure_is_a_missing_media_finding_and_a_partial_state() {
        let item = RssItem {
            has_enclosure: true,
            ..plain_item()
        };
        let findings = rss_item_findings(&item);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Media
                && f.category == RecognitionCategory::Missing));
        assert_eq!(rss_import_state(&findings), ImportState::Partial);
    }

    #[test]
    fn rss_state_never_derives_recognized_even_on_an_all_recognized_set() {
        // The floor is structural in the derivation itself: this input is
        // unreachable (the Source ambiguity is always emitted) but the
        // derivation still refuses `Recognized`.
        let findings = [
            RecognitionFinding::recognized(RecognitionAspect::Envelope),
            RecognitionFinding::recognized(RecognitionAspect::Title),
        ];
        assert_eq!(rss_import_state(&findings), ImportState::NeedsReview);
    }

    #[test]
    fn rss_state_blocking_dominates_everything() {
        let findings = [
            RecognitionFinding::ambiguous(RecognitionAspect::Source),
            RecognitionFinding {
                aspect: RecognitionAspect::Media,
                category: RecognitionCategory::Missing,
            },
            RecognitionFinding::blocking(RecognitionAspect::Structure),
        ];
        assert_eq!(rss_import_state(&findings), ImportState::Blocked);
    }

    #[test]
    fn an_absent_title_flags_the_adjustment_for_the_fallback() {
        let xml = feed("<item><description>Texte seul.</description></item>");
        let analysis = parse_rss(xml.as_bytes());
        let item = &analysis.items[0];
        assert_eq!(item.title, "");
        assert!(item.title_adjusted);
        // The absent description case, mirrored:
        let xml = feed("<item><title>Titre seul</title></item>");
        let analysis = parse_rss(xml.as_bytes());
        let item = &analysis.items[0];
        assert_eq!(item.text, "");
        assert!(item.text_adjusted);
        assert!(!item.title_adjusted);
    }

    #[test]
    fn a_title_over_the_canonical_bound_flags_the_adjustment() {
        let long_title = "t".repeat(200);
        let xml = feed(&simple_item(&long_title, "Texte."));
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.items[0].title_adjusted);
        // The candidate itself is kept verbatim (the fallback applies at
        // creation, not at parse time).
        assert_eq!(analysis.items[0].title.chars().count(), 200);
    }

    // ===== item references =====

    #[test]
    fn item_ref_prefers_the_guid_strictly() {
        let item = plain_item();
        assert_eq!(rss_item_ref(&item), RssItemRef::Guid("g1".into()));
        let no_guid = RssItem {
            guid: None,
            link: Some("https://exemple.fr/ep".into()),
            ..plain_item()
        };
        assert_eq!(
            rss_item_ref(&no_guid),
            RssItemRef::TitleLink {
                title: "Episode".into(),
                link: Some("https://exemple.fr/ep".into()),
            }
        );
    }

    #[test]
    fn resolve_finds_a_unique_guid_match() {
        let items = [plain_item()];
        let found = resolve_rss_item(&items, &RssItemRef::Guid("g1".into()));
        assert!(found.is_some());
        assert!(resolve_rss_item(&items, &RssItemRef::Guid("autre".into())).is_none());
    }

    #[test]
    fn resolve_falls_back_to_exact_title_and_link() {
        let items = [RssItem {
            guid: None,
            link: Some("https://exemple.fr/ep".into()),
            ..plain_item()
        }];
        let exact = RssItemRef::TitleLink {
            title: "Episode".into(),
            link: Some("https://exemple.fr/ep".into()),
        };
        assert!(resolve_rss_item(&items, &exact).is_some());
        let wrong_link = RssItemRef::TitleLink {
            title: "Episode".into(),
            link: None,
        };
        assert!(resolve_rss_item(&items, &wrong_link).is_none());
    }

    #[test]
    fn a_title_link_reference_never_matches_a_guid_carrying_item() {
        // A guid-carrying item sharing the same (title, link) must not
        // shadow the guid-less one into a false ambiguity: the TitleLink
        // reference is only ever EMITTED for guid-less items, so the
        // resolution only considers those.
        let items = [
            RssItem {
                guid: Some("g".into()),
                link: None,
                ..plain_item()
            },
            RssItem {
                guid: None,
                link: None,
                text: "Deuxième texte.".into(),
                ..plain_item()
            },
        ];
        let reference = RssItemRef::TitleLink {
            title: "Episode".into(),
            link: None,
        };
        let found = resolve_rss_item(&items, &reference).expect("unique guid-less match");
        assert_eq!(found.text, "Deuxième texte.");
    }

    #[test]
    fn resolve_refuses_an_ambiguous_match() {
        // Two distinct items may still collide on (title, link) when their
        // guids differ — a TitleLink reference must then refuse.
        let items = [
            RssItem {
                guid: Some("g1".into()),
                ..plain_item()
            },
            RssItem {
                guid: Some("g2".into()),
                text: "Autre texte.".into(),
                ..plain_item()
            },
        ];
        let by_title = RssItemRef::TitleLink {
            title: "Episode".into(),
            link: None,
        };
        assert!(resolve_rss_item(&items, &by_title).is_none());
        // Each guid stays uniquely resolvable.
        assert!(resolve_rss_item(&items, &RssItemRef::Guid("g2".into())).is_some());
    }

    // ===== feed URL validation =====

    #[test]
    fn supported_urls_are_accepted_one_by_one() {
        for url in [
            "http://exemple.fr/flux.xml",
            "https://exemple.fr/flux.xml",
            "HTTPS://exemple.fr/flux.xml",
            "http://127.0.0.1:8000/feed.xml",
            "https://exemple.fr",
            "https://exemple.fr/chemin?query=1#frag",
        ] {
            assert!(is_supported_feed_url(url), "{url} must be accepted");
        }
    }

    #[test]
    fn unsupported_urls_are_refused_one_by_one() {
        let over_bound = format!("https://exemple.fr/{}", "a".repeat(MAX_RSS_URL_CHARS));
        for url in [
            "",
            "exemple.fr/flux.xml",
            "ftp://exemple.fr/flux.xml",
            "file:///etc/passwd",
            "data:text/xml,<rss/>",
            "https://user:pass@exemple.fr/flux.xml",
            "https://@exemple.fr/",
            "https:///chemin",
            "https://[::1]:8000/feed.xml",
            "https://exemple.fr:port/",
            "https://exemple.fr:/",
            "https://exe mple.fr/",
            over_bound.as_str(),
        ] {
            assert!(!is_supported_feed_url(url), "{url} must be refused");
        }
    }

    #[test]
    fn an_invalid_address_cutting_a_multibyte_char_refuses_without_panicking() {
        // `http:/é…`: byte 7 falls INSIDE the two-byte `é` — a naive
        // `url[..7]` slice would panic; the boundary-safe gate must return
        // a plain refusal instead (the closed `url_invalid` taxonomy).
        for url in [
            "http:/éxemple.fr",
            "https:/écho.fr/flux",
            "héttp://exemple.fr",
        ] {
            assert!(!is_supported_feed_url(url), "{url} must be refused");
            assert_eq!(feed_url_host(url), None);
        }
    }

    #[test]
    fn a_malformed_root_attribute_is_the_envelope_verdict_not_a_format_pass() {
        // The root tag carries `version="2.0"` AND a malformed attribute
        // (an unquoted value): the attribute error must PROPAGATE to the
        // unreadable-envelope verdict — never be silently skipped into an
        // "RSS 2.0 accepted" pass.
        let xml =
            "<rss version=\"2.0\" bad=oops><channel><item><title>t</title></item></channel></rss>";
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.is_blocked());
        assert_eq!(
            analysis.findings[0],
            RecognitionFinding::blocking(RecognitionAspect::Envelope)
        );
    }

    #[test]
    fn a_captured_field_with_child_elements_keeps_its_descendant_text() {
        // Inline (XML-valid) markup INSIDE a captured field: the capture is
        // depth-aware, so the descendant text accumulates and the field
        // settles when ITS element closes — never dropped, never truncated.
        let xml = feed(
            "<item><title>Fable <b>en gras</b> finale</title>\
             <description>Bonjour <b>toi</b> et <i>lui</i>, la <em>fin</em>.</description>\
             <guid>g-nested</guid></item>",
        );
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items.len(), 1);
        let item = &analysis.items[0];
        assert_eq!(item.title, "Fable en gras finale");
        assert_eq!(item.text, "Bonjour toi et lui, la fin.");
        assert_eq!(item.guid.as_deref(), Some("g-nested"));
        // The stripped child markup IS an adjustment: the ingested value
        // no longer carries the author's inline tags — both fields flag it
        // (→ the Title/Structure ambiguities), exactly like escaped-then-
        // cleaned markup would.
        assert!(item.title_adjusted, "stripped title markup must flag");
        assert!(item.text_adjusted, "stripped text markup must flag");
        let findings = rss_item_findings(item);
        assert!(findings.iter().any(|f| f.aspect == RecognitionAspect::Title
            && f.category == RecognitionCategory::Ambiguous));
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Structure
                && f.category == RecognitionCategory::Ambiguous));
    }

    #[test]
    fn a_field_without_child_markup_stays_unflagged() {
        // The markup tracker must not leak across captures: a plain field
        // parsed right after a markup-carrying one stays clean.
        let xml = feed(
            "<item><title>Avec <b>markup</b></title><description>Texte propre.</description><guid>g-1</guid></item>\
             <item><title>Titre propre</title><description>Texte propre aussi.</description><guid>g-2</guid></item>",
        );
        let analysis = parse_rss(xml.as_bytes());
        assert_eq!(analysis.items.len(), 2);
        assert!(analysis.items[0].title_adjusted);
        assert!(!analysis.items[0].text_adjusted, "clean description");
        assert!(!analysis.items[1].title_adjusted, "clean second title");
        assert!(!analysis.items[1].text_adjusted);
    }

    #[test]
    fn a_truncated_document_is_the_envelope_blocking_verdict() {
        // A body cut MID-DOCUMENT (open elements at EOF) must never pass
        // as a healthy feed — the truncation would be invisible otherwise.
        let xml = "<rss version=\"2.0\"><channel><item><title>t</title><description>d</description></item>";
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.is_blocked());
        assert_eq!(
            analysis.findings[0],
            RecognitionFinding::blocking(RecognitionAspect::Envelope)
        );
    }

    #[test]
    fn content_after_the_closed_root_is_the_envelope_blocking_verdict() {
        // A second document after `</rss>` is not well-formed XML: its
        // items must never become selectable around the format gate.
        let mut xml = feed(&simple_item("Episode", "Texte."));
        xml.push_str(
            "<rss version=\"0.91\"><channel><item><title>intrus</title></item></channel></rss>",
        );
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.is_blocked());
        assert_eq!(
            analysis.findings[0],
            RecognitionFinding::blocking(RecognitionAspect::Envelope)
        );
        // A stray self-closed element after the root blocks the same way.
        let mut xml = feed(&simple_item("Episode", "Texte."));
        xml.push_str("<stray/>");
        let analysis = parse_rss(xml.as_bytes());
        assert!(analysis.is_blocked());
    }

    #[test]
    fn an_item_title_beyond_its_own_bound_is_truncated_with_a_finding() {
        let long_title = "t".repeat(MAX_RSS_ITEM_TITLE_CHARS + 50);
        let xml = feed(&simple_item(&long_title, "Texte."));
        let analysis = parse_rss(xml.as_bytes());
        let item = &analysis.items[0];
        assert_eq!(item.title.chars().count(), MAX_RSS_ITEM_TITLE_CHARS);
        assert!(item.title_adjusted, "truncation is a review step");
    }

    #[test]
    fn duplicates_never_squat_the_item_quota() {
        // MAX_RSS_ITEMS duplicates of one reference followed by 10 unique
        // items: inline de-duplication only counts NOVEL references, so
        // the unique items survive the cap.
        let mut inner = String::new();
        for _ in 0..MAX_RSS_ITEMS {
            inner.push_str(
                "<item><title>Doublon</title><description>d.</description><guid>dup</guid></item>",
            );
        }
        for index in 0..10 {
            inner.push_str(&format!(
                "<item><title>Unique {index}</title><description>u.</description><guid>u-{index}</guid></item>"
            ));
        }
        let analysis = parse_rss(feed(&inner).as_bytes());
        assert_eq!(analysis.items.len(), 11, "1 dup + 10 unique retained");
    }

    #[test]
    fn an_out_of_range_port_is_an_invalid_address_not_an_unreachable_source() {
        for url in [
            "https://exemple.fr:99999999/flux.xml",
            "https://exemple.fr:0/flux.xml",
        ] {
            assert!(!is_supported_feed_url(url), "{url} must be refused");
        }
        assert!(is_supported_feed_url("https://exemple.fr:65535/flux.xml"));
    }

    #[test]
    fn the_tag_stripper_honors_quoted_attribute_values() {
        // A `>` inside a quoted attribute value must not close the tag —
        // no tag debris may leak into the ingested prose.
        let (text, adjusted) = clean_rss_text("<a href=\"x>y\">lien</a>");
        assert_eq!(text, "lien");
        assert!(adjusted);
        let (text, _) = clean_rss_text("<a href='x>y'>lien</a>");
        assert_eq!(text, "lien");
    }

    #[test]
    fn item_fingerprint_covers_every_ingestion_relevant_field() {
        let base = plain_item();
        let same = rss_item_fingerprint(&plain_item());
        assert_eq!(rss_item_fingerprint(&base), same, "deterministic");
        for (label, mutated) in [
            (
                "title",
                RssItem {
                    title: "Autre".into(),
                    ..plain_item()
                },
            ),
            (
                "text",
                RssItem {
                    text: "Autre texte.".into(),
                    ..plain_item()
                },
            ),
            (
                "guid",
                RssItem {
                    guid: Some("g-autre".into()),
                    ..plain_item()
                },
            ),
            (
                "link",
                RssItem {
                    link: Some("https://exemple.fr/autre".into()),
                    ..plain_item()
                },
            ),
            (
                "enclosure",
                RssItem {
                    has_enclosure: true,
                    ..plain_item()
                },
            ),
        ] {
            assert_ne!(
                rss_item_fingerprint(&mutated),
                same,
                "a {label} change must change the fingerprint"
            );
        }
    }

    #[test]
    fn host_extraction_drops_the_port_and_never_the_scheme_rules() {
        assert_eq!(
            feed_url_host("http://127.0.0.1:8000/feed.xml").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            feed_url_host("https://Exemple.FR/flux.xml").as_deref(),
            Some("Exemple.FR")
        );
        assert_eq!(feed_url_host("ftp://exemple.fr/"), None);
    }

    #[test]
    fn a_host_that_would_break_the_fallback_title_is_refused() {
        // Too long for `Histoire de {hôte}` to stay a canonical title.
        let long_host = format!("https://{}.fr/", "a".repeat(120));
        assert!(!is_supported_feed_url(&long_host));
        // A denied formatting code point (RLO bidi override) in the host:
        // sober for the DB floor, but the fallback title would be refused
        // by the canonical validation — the address gate refuses first.
        assert!(!is_supported_feed_url(
            "https://exem\u{202E}ple.fr/flux.xml"
        ));
        // The fallback built from any ACCEPTED host is a valid title.
        let host = feed_url_host("https://exemple.fr/flux.xml").expect("host");
        let fallback = format!("{RSS_FALLBACK_TITLE_PREFIX}{host}");
        assert!(validate_title(&normalize_title(&fallback)).is_ok());
    }
}
