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
/// the feed stays exploitable, the contract documents the cut. Sized for
/// a WHOLE podcast becoming one story (a kids' series easily runs past
/// a hundred episodes); the wire stays bounded by the summary excerpt.
pub const MAX_RSS_ITEMS: usize = 500;

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
    /// The URL of the remote enclosure, if present.
    pub enclosure_url: Option<String>,
    /// The MIME type of the enclosure (audio/*, image/*, etc.), if present.
    pub enclosure_type: Option<String>,
    /// The item's own artwork (`<itunes:image href>`), if any — the
    /// channel artwork is the fallback, resolved by the creation.
    pub image_url: Option<String>,
    /// The item's `<pubDate>` as Unix seconds, when it parses as an
    /// RFC 2822 date; drives the chronological ordering of the analysis.
    pub published_at: Option<i64>,
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
    /// The channel artwork (`<itunes:image href>`, else `<image><url>`),
    /// the fallback image of every item without its own.
    pub channel_image_url: Option<String>,
    /// The exploitable items in LISTENING order: chronological (oldest
    /// first) when every retained item carries a parseable `pubDate`,
    /// else the feed's own order (see [`order_items_chronologically`]).
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
            channel_image_url: None,
            items: Vec::new(),
            findings,
            state,
        }
    }
}

/// Order the retained items for LISTENING: a podcast feed lists its
/// newest episode first, while a story box plays a series from its first
/// episode. When EVERY item carries a parseable `pubDate`, the items are
/// stably sorted oldest-first (ties keep the feed order); when any date is
/// missing or unreadable the feed order is kept unchanged — chronology is
/// never guessed.
pub fn order_items_chronologically(items: &mut [RssItem]) {
    if items.iter().all(|item| item.published_at.is_some()) {
        items.sort_by_key(|item| item.published_at);
    }
}

/// Parse an RSS `<pubDate>` (RFC 822 / RFC 2822) into Unix seconds. Pure
/// and tolerant of the common real-world spellings: an optional weekday,
/// a 2-digit year, a named zone (`GMT`, `UT`, `UTC`, `Z`, the US zones)
/// or a numeric offset. `None` for anything else — a date that cannot be
/// proven never orders a feed.
pub fn parse_rss_pub_date(raw: &str) -> Option<i64> {
    let mut tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    // The optional leading weekday (`Fri,` / `Fri`).
    if tokens[0].trim_end_matches(',').len() == 3
        && tokens[0]
            .trim_end_matches(',')
            .chars()
            .all(|c| c.is_ascii_alphabetic())
        && tokens.len() >= 5
    {
        tokens.remove(0);
    }
    if tokens.len() < 4 {
        return None;
    }
    let day: i64 = tokens[0].parse().ok()?;
    let month = match tokens[1].to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let mut year: i64 = tokens[2].parse().ok()?;
    if tokens[2].len() == 2 {
        // RFC 822 two-digit years (RFC 2822 §4.3 interpretation).
        year += if year < 50 { 2000 } else { 1900 };
    }
    let mut clock = tokens[3].split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = match clock.next() {
        Some(value) => value.parse().ok()?,
        None => 0,
    };
    if clock.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
        || !(1900..=9999).contains(&year)
    {
        return None;
    }
    let offset_seconds: i64 = match tokens.get(4) {
        None => 0,
        Some(zone) => parse_rss_zone(zone)?,
    };
    if tokens.len() > 5 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset_seconds)
}

/// The zone token of an RFC 2822 date, as seconds EAST of UTC.
fn parse_rss_zone(zone: &str) -> Option<i64> {
    match zone.to_ascii_uppercase().as_str() {
        "GMT" | "UT" | "UTC" | "Z" => return Some(0),
        "EST" => return Some(-5 * 3_600),
        "EDT" => return Some(-4 * 3_600),
        "CST" => return Some(-6 * 3_600),
        "CDT" => return Some(-5 * 3_600),
        "MST" => return Some(-7 * 3_600),
        "MDT" => return Some(-6 * 3_600),
        "PST" => return Some(-8 * 3_600),
        "PDT" => return Some(-7 * 3_600),
        _ => {}
    }
    let (sign, digits) = match zone.as_bytes().first()? {
        b'+' => (1, &zone[1..]),
        b'-' => (-1, &zone[1..]),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits[2..].parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

/// Days since 1970-01-01 of a proleptic Gregorian civil date (Howard
/// Hinnant's `days_from_civil`); `None` for a day past the month's end.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day > month_days {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
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

/// The findings persisted for ONE ingested feed — what the created story's
/// durable state, chip and report speak of. Envelope + format are
/// recognized by construction (a blocked feed never reaches an accept),
/// the `(Source, Ambiguous)` floor is always present; the TITLE aspect is
/// recognized only when the channel title survived as the story title
/// (else the `Histoire de {hôte}` fallback applied — an ambiguity); the
/// STRUCTURE aspect is recognized when no ingested item needed a cleaning
/// adjustment; and when any ingested item references an enclosure the
/// MEDIA aspect is recognized iff EVERY referenced audio was downloaded,
/// else the `(Media, Missing)` finding derives `Partial`.
pub fn rss_feed_findings(
    items: &[&RssItem],
    title_recognized: bool,
    audio_missing: bool,
) -> Vec<RecognitionFinding> {
    let mut findings = vec![
        RecognitionFinding::recognized(RecognitionAspect::Envelope),
        RecognitionFinding::recognized(RecognitionAspect::FormatVersion),
        RecognitionFinding::ambiguous(RecognitionAspect::Source),
    ];
    findings.push(if title_recognized {
        RecognitionFinding::recognized(RecognitionAspect::Title)
    } else {
        RecognitionFinding::ambiguous(RecognitionAspect::Title)
    });
    let any_adjusted = items
        .iter()
        .any(|item| item.title_adjusted || item.text_adjusted);
    findings.push(if any_adjusted {
        RecognitionFinding::ambiguous(RecognitionAspect::Structure)
    } else {
        RecognitionFinding::recognized(RecognitionAspect::Structure)
    });
    if items.iter().any(|item| item.has_enclosure) {
        findings.push(RecognitionFinding {
            aspect: RecognitionAspect::Media,
            category: if audio_missing {
                RecognitionCategory::Missing
            } else {
                RecognitionCategory::Recognized
            },
            message: None,
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
        item.enclosure_url,
        item.enclosure_type,
        item.image_url,
        item.published_at,
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
        ChannelImageUrl,
        ItemTitle,
        ItemDescription,
        ItemGuid,
        ItemLink,
        ItemPubDate,
    }

    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut root_seen = false;
    let mut root_is_rss2 = false;
    let mut channel_title: Option<String> = None;
    // The channel artwork: `<itunes:image href>` wins over `<image><url>`
    // (the podcast-standard, usually higher-resolution artwork).
    let mut channel_itunes_image: Option<String> = None;
    let mut channel_image_url: Option<String> = None;
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
        enclosure_url: Option<String>,
        enclosure_type: Option<String>,
        image_url: Option<String>,
        pub_date: Option<String>,
    }

    fn is_item_path(stack: &[Vec<u8>]) -> bool {
        stack.len() == 3 && stack[0] == b"rss" && stack[1] == b"channel" && stack[2] == b"item"
    }

    fn is_channel_path(stack: &[Vec<u8>]) -> bool {
        stack.len() == 2 && stack[0] == b"rss" && stack[1] == b"channel"
    }

    /// The `href` of an `<itunes:image>` tag, trimmed; `None` when absent
    /// or unreadable (never a verdict — artwork is optional).
    fn itunes_image_href(start: &BytesStart<'_>) -> Option<String> {
        if start.name().as_ref() != b"itunes:image" {
            return None;
        }
        for attr in start.attributes() {
            let Ok(attr) = attr else { continue };
            if attr.key.as_ref() == b"href" {
                let Ok(value) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
                    continue;
                };
                let value = value.trim().to_string();
                return (!value.is_empty()).then_some(value);
            }
        }
        None
    }

    /// The `url` / `type` of an `<enclosure>` tag into the draft.
    fn read_enclosure(start: &BytesStart<'_>, draft: &mut DraftItem) {
        draft.has_enclosure = true;
        for attr in start.attributes() {
            let Ok(attr) = attr else { continue };
            if attr.key.as_ref() == b"url" {
                let Ok(value) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
                    continue;
                };
                draft.enclosure_url = Some(value.trim().to_string());
            } else if attr.key.as_ref() == b"type" {
                let Ok(value) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
                    continue;
                };
                draft.enclosure_type = Some(value.trim().to_string());
            }
        }
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
            enclosure_url: draft.enclosure_url,
            enclosure_type: draft.enclosure_type,
            image_url: draft.image_url,
            published_at: draft.pub_date.as_deref().and_then(parse_rss_pub_date),
        })
    }

    fn capture_for(stack: &[Vec<u8>], name: &[u8], in_item: bool) -> Capture {
        if in_item && is_item_path(stack) {
            return match name {
                b"title" => Capture::ItemTitle,
                b"description" => Capture::ItemDescription,
                b"guid" => Capture::ItemGuid,
                b"link" => Capture::ItemLink,
                b"pubDate" => Capture::ItemPubDate,
                _ => Capture::None,
            };
        }
        if is_channel_path(stack) && name == b"title" {
            return Capture::ChannelTitle;
        }
        if stack.len() == 3
            && stack[0] == b"rss"
            && stack[1] == b"channel"
            && stack[2] == b"image"
            && name == b"url"
        {
            return Capture::ChannelImageUrl;
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
                if let Some(draft) = current.as_mut().filter(|_| is_item_path(&stack)) {
                    if name == b"enclosure" {
                        read_enclosure(&start, draft);
                    } else if let Some(href) = itunes_image_href(&start) {
                        draft.image_url = Some(href);
                    }
                } else if current.is_none() && is_channel_path(&stack) {
                    if let Some(href) = itunes_image_href(&start) {
                        channel_itunes_image = Some(href);
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
                if let Some(draft) = current.as_mut().filter(|_| is_item_path(&stack)) {
                    if start.name().as_ref() == b"enclosure" {
                        read_enclosure(&start, draft);
                    } else if let Some(href) = itunes_image_href(&start) {
                        draft.image_url = Some(href);
                    }
                } else if current.is_none() && is_channel_path(&stack) {
                    if let Some(href) = itunes_image_href(&start) {
                        channel_itunes_image = Some(href);
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
                    Capture::ChannelImageUrl => {
                        let trimmed = text.trim().to_string();
                        if !trimmed.is_empty() {
                            channel_image_url = Some(trimmed);
                        }
                        capture = Capture::None;
                    }
                    Capture::ItemTitle
                    | Capture::ItemDescription
                    | Capture::ItemGuid
                    | Capture::ItemLink
                    | Capture::ItemPubDate => {
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
                                Capture::ItemPubDate => draft.pub_date = Some(value),
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
    order_items_chronologically(&mut items);
    let findings = exploitable_flow_findings();
    let state = rss_import_state(&findings);
    RssAnalysis {
        channel_title,
        channel_image_url: channel_itunes_image.or(channel_image_url),
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
            enclosure_url: None,
            enclosure_type: None,
            image_url: None,
            published_at: None,
        }
    }

    #[test]
    fn feed_findings_always_carry_the_source_ambiguity_floor() {
        let item = plain_item();
        let findings = rss_feed_findings(&[&item], true, false);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Source
                && f.category == RecognitionCategory::Ambiguous));
        assert!(findings.iter().any(|f| f.aspect == RecognitionAspect::Title
            && f.category == RecognitionCategory::Recognized));
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Structure
                && f.category == RecognitionCategory::Recognized));
        // No enclosure anywhere: no Media aspect at all.
        assert!(!findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Media));
        assert_eq!(rss_import_state(&findings), ImportState::NeedsReview);
    }

    #[test]
    fn a_fallback_title_is_a_title_ambiguity() {
        let item = plain_item();
        let findings = rss_feed_findings(&[&item], false, false);
        assert!(findings.iter().any(|f| f.aspect == RecognitionAspect::Title
            && f.category == RecognitionCategory::Ambiguous));
    }

    #[test]
    fn any_adjusted_item_is_a_structure_ambiguity() {
        let clean = plain_item();
        let adjusted = RssItem {
            text_adjusted: true,
            ..plain_item()
        };
        let findings = rss_feed_findings(&[&clean, &adjusted], true, false);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Structure
                && f.category == RecognitionCategory::Ambiguous));
        let titled = RssItem {
            title_adjusted: true,
            ..plain_item()
        };
        let findings = rss_feed_findings(&[&titled], true, false);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Structure
                && f.category == RecognitionCategory::Ambiguous));
    }

    #[test]
    fn a_missing_audio_is_a_missing_media_finding_and_a_partial_state() {
        let item = RssItem {
            has_enclosure: true,
            ..plain_item()
        };
        let findings = rss_feed_findings(&[&item], true, true);
        assert!(findings
            .iter()
            .any(|f| f.aspect == RecognitionAspect::Media
                && f.category == RecognitionCategory::Missing));
        assert_eq!(rss_import_state(&findings), ImportState::Partial);
        // Every audio downloaded: the Media aspect is recognized.
        let findings = rss_feed_findings(&[&item], true, false);
        assert!(findings.iter().any(|f| f.aspect == RecognitionAspect::Media
            && f.category == RecognitionCategory::Recognized));
        assert_eq!(rss_import_state(&findings), ImportState::NeedsReview);
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
                message: None,
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
        // (→ the Structure ambiguity of the feed findings), exactly like
        // escaped-then-cleaned markup would.
        assert!(item.title_adjusted, "stripped title markup must flag");
        assert!(item.text_adjusted, "stripped text markup must flag");
        let findings = rss_feed_findings(&[item], true, false);
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

    // ===== artwork, dates and the listening order =====

    #[test]
    fn pub_dates_parse_the_real_world_spellings_and_refuse_the_rest() {
        assert_eq!(
            parse_rss_pub_date("Fri, 28 Aug 2026 04:42:00 +0200"),
            Some(1_787_884_920)
        );
        // Without the weekday, in UTC (named and numeric), seconds optional.
        assert_eq!(parse_rss_pub_date("1 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_rss_pub_date("1 Jan 1970 00:00 +0000"), Some(0));
        assert_eq!(
            parse_rss_pub_date("Thu, 01 Jan 1970 01:00:00 +0100"),
            Some(0)
        );
        assert_eq!(
            parse_rss_pub_date("1 Jan 1970 00:00:00 EST"),
            Some(5 * 3_600)
        );
        // A two-digit year follows the RFC 2822 interpretation.
        assert_eq!(
            parse_rss_pub_date("1 Jan 70 00:00:00 GMT"),
            parse_rss_pub_date("1 Jan 1970 00:00:00 GMT")
        );
        // Refusals: an impossible day, an unknown zone, junk, a trailing token.
        assert_eq!(parse_rss_pub_date("31 Feb 2026 00:00:00 GMT"), None);
        assert_eq!(parse_rss_pub_date("1 Jan 2026 00:00:00 XYZ"), None);
        assert_eq!(parse_rss_pub_date("demain"), None);
        assert_eq!(parse_rss_pub_date(""), None);
        assert_eq!(parse_rss_pub_date("1 Jan 2026 00:00:00 GMT extra"), None);
    }

    #[test]
    fn a_dated_feed_is_ordered_oldest_first_for_listening() {
        // A podcast feed lists its newest episode first; the story plays
        // the series from its first episode.
        let analysis = parse_rss(
            "<rss version=\"2.0\"><channel><title>Série</title>\
             <item><title>Trois</title><guid>3</guid><pubDate>Wed, 03 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>Deux</title><guid>2</guid><pubDate>Tue, 02 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>Un</title><guid>1</guid><pubDate>Mon, 01 Mar 2026 08:00:00 +0100</pubDate></item>\
             </channel></rss>"
                .as_bytes(),
        );
        let titles: Vec<&str> = analysis.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Un", "Deux", "Trois"]);
        assert!(analysis.items.iter().all(|i| i.published_at.is_some()));
    }

    #[test]
    fn a_feed_with_one_undated_item_keeps_its_own_order() {
        // Chronology is never guessed: one missing (or unreadable) date
        // and the feed order stands, ties included.
        let analysis = parse_rss(
            "<rss version=\"2.0\"><channel><title>Série</title>\
             <item><title>Trois</title><guid>3</guid><pubDate>Wed, 03 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>Deux</title><guid>2</guid><pubDate>bientôt</pubDate></item>\
             <item><title>Un</title><guid>1</guid><pubDate>Mon, 01 Mar 2026 08:00:00 +0100</pubDate></item>\
             </channel></rss>"
                .as_bytes(),
        );
        let titles: Vec<&str> = analysis.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Trois", "Deux", "Un"]);
        assert_eq!(analysis.items[1].published_at, None);
    }

    #[test]
    fn same_dated_items_keep_the_feed_order() {
        let analysis = parse_rss(
            "<rss version=\"2.0\"><channel><title>Série</title>\
             <item><title>A</title><guid>a</guid><pubDate>Mon, 01 Mar 2026 08:00:00 +0100</pubDate></item>\
             <item><title>B</title><guid>b</guid><pubDate>Mon, 01 Mar 2026 08:00:00 +0100</pubDate></item>\
             </channel></rss>"
                .as_bytes(),
        );
        let titles: Vec<&str> = analysis.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["A", "B"]);
    }

    #[test]
    fn item_and_channel_artwork_are_read_with_the_itunes_tag_winning() {
        let analysis = parse_rss(
            "<rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\"><channel>\
             <title>Série</title>\
             <image><url> https://exemple.fr/rss.jpg </url><title>Série</title></image>\
             <itunes:image href=\"https://exemple.fr/itunes.jpg\"/>\
             <item><title>Avec</title><guid>1</guid><itunes:image href=\"https://exemple.fr/ep1.jpg\"/></item>\
             <item><title>Sans</title><guid>2</guid></item>\
             <item><title>Ouvert</title><guid>3</guid><itunes:image href=\"https://exemple.fr/ep3.jpg\"></itunes:image></item>\
             </channel></rss>"
                .as_bytes(),
        );
        assert_eq!(
            analysis.channel_image_url.as_deref(),
            Some("https://exemple.fr/itunes.jpg")
        );
        assert_eq!(
            analysis.items[0].image_url.as_deref(),
            Some("https://exemple.fr/ep1.jpg")
        );
        assert_eq!(analysis.items[1].image_url, None);
        assert_eq!(
            analysis.items[2].image_url.as_deref(),
            Some("https://exemple.fr/ep3.jpg")
        );
    }

    #[test]
    fn the_channel_image_url_is_the_artwork_fallback() {
        let analysis = parse_rss(
            "<rss version=\"2.0\"><channel><title>Série</title>\
             <image><url>https://exemple.fr/rss.jpg</url></image>\
             <item><title>Un</title><guid>1</guid></item>\
             </channel></rss>"
                .as_bytes(),
        );
        assert_eq!(
            analysis.channel_image_url.as_deref(),
            Some("https://exemple.fr/rss.jpg")
        );
        // An item-level <image> is not the channel artwork.
        let analysis = parse_rss(
            "<rss version=\"2.0\"><channel><title>Série</title>\
             <item><title>Un</title><guid>1</guid><image><url>https://exemple.fr/x.jpg</url></image></item>\
             </channel></rss>"
                .as_bytes(),
        );
        assert_eq!(analysis.channel_image_url, None);
        assert_eq!(analysis.items[0].image_url, None);
    }

    #[test]
    fn artwork_and_date_changes_alter_the_fingerprint() {
        let same = rss_item_fingerprint(&plain_item());
        assert_ne!(
            rss_item_fingerprint(&RssItem {
                image_url: Some("https://exemple.fr/a.jpg".into()),
                ..plain_item()
            }),
            same
        );
        assert_ne!(
            rss_item_fingerprint(&RssItem {
                published_at: Some(1),
                ..plain_item()
            }),
            same
        );
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
