//! Pure parsing of a web podcast page into honest episode references:
//! the JSON-LD `RadioEpisode` / `ItemList` node shapes, the honest DOM
//! fallback (one episode per titled audio media, in document order),
//! the preview honesty filter, and the self-delimiting preview checksum
//! framing. ZERO network: nothing in this module dispatches a request —
//! the `ItemList` pass that re-fetches each entry page lives in the
//! parent module (`episodes_from_item_list`), the sole network boundary
//! of the extraction.

use scraper::{Element, ElementRef, Html, Selector};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// One extracted episode from the web page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebEpisode {
    pub title: String,
    pub summary: String,
    pub audio_url: Option<String>,
    pub image_url: Option<String>,
}

/// The `@type` values recognized as a podcast episode on an episode page
/// (schema.org). Everything else is ignored: the extraction never guesses
/// an episode out of a non-episode node.
const EPISODE_JSONLD_TYPES: [&str; 4] = [
    "RadioEpisode",
    "PodcastEpisode",
    "BroadcastEpisode",
    "MusicRecording",
];

/// Collect the JSON-LD nodes carrying an `@type`, recursing into
/// `@graph` arrays (the shape of the real sample pages).
pub(super) fn collect_jsonld_nodes<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    if node.get("@type").is_some() {
        out.push(node);
    }
    if let Some(graph) = node.get("@graph").and_then(Value::as_array) {
        for entry in graph {
            collect_jsonld_nodes(entry, out);
        }
    }
}

/// The `@type` of a node: a plain string or the first element of an
/// array (both are valid JSON-LD shapes).
pub(super) fn jsonld_type(node: &Value) -> Option<&str> {
    match node.get("@type")? {
        Value::String(text) => Some(text.as_str()),
        Value::Array(types) => types.first().and_then(Value::as_str),
        _ => None,
    }
}

/// The JSON-LD payloads embedded in the page: every
/// `<script type="application/ld+json">` body that parses as JSON.
/// Unparsable scripts are skipped — the page content is untrusted and a
/// broken payload is never a crash.
pub(super) fn jsonld_payloads(document: &Html) -> Vec<Value> {
    let selector = match Selector::parse("script[type='application/ld+json']") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    document
        .select(&selector)
        .filter_map(|element| serde_json::from_str(element.text().collect::<String>().trim()).ok())
        .collect()
}

/// The audio media url of an episode node: `contentUrl` on the node
/// itself or on its `mainEntity` (the AudioObject shape of the sample
/// pages), trimmed, non-empty.
fn audio_content_url(node: &Value) -> Option<String> {
    let direct = node.get("contentUrl").and_then(Value::as_str);
    let nested = node
        .get("mainEntity")
        .and_then(|entity| entity.get("contentUrl"))
        .and_then(Value::as_str);
    let url = direct.or(nested)?.trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_owned())
    }
}

/// The image url of an episode node: the `image` field as a plain
/// string, an `ImageObject` carrying a `url`, or an array whose first
/// element is one of the two — carried as-is (no resolution, mirroring
/// the audio `contentUrl`), trimmed and non-empty, or `None`.
fn jsonld_image_url(node: &Value) -> Option<String> {
    node.get("image").and_then(image_url_from_jsonld_value)
}

fn image_url_from_jsonld_value(value: &Value) -> Option<String> {
    match value {
        Value::String(url) => trimmed_url(url),
        Value::Object(fields) => fields
            .get("url")
            .and_then(Value::as_str)
            .and_then(trimmed_url),
        Value::Array(images) => images.first().and_then(image_url_from_jsonld_value),
        _ => None,
    }
}

/// A trimmed, non-empty url string, or `None`.
fn trimmed_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// One episode from one JSON-LD node, or `None` when the node is not a
/// recognized episode type, carries no non-empty `name`, or no audio
/// `contentUrl` — the extraction emits only honest episodes (no title
/// fallback, no invented media). The image, when the node provides one
/// (`image` as a string, an `ImageObject`, or an array), is carried on
/// the episode as-is — never resolved, mirroring the audio url.
fn episode_from_jsonld_node(node: &Value) -> Option<WebEpisode> {
    if !jsonld_type(node).is_some_and(|ty| EPISODE_JSONLD_TYPES.contains(&ty)) {
        return None;
    }
    let title = node
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())?
        .to_owned();
    let audio_url = audio_content_url(node)?;
    let summary = node
        .get("description")
        .and_then(Value::as_str)
        .map(|text| text.trim().to_owned())
        .unwrap_or_default();
    let image_url = jsonld_image_url(node);
    Some(WebEpisode {
        title,
        summary,
        audio_url: Some(audio_url),
        image_url,
    })
}

/// Resolve a (possibly relative) url against the page that referenced
/// it: absolute `http(s)://` urls pass through unchanged, a
/// protocol-relative `//authority/…` url inherits the base scheme, a
/// `/root-relative` url inherits the base scheme and authority, and a
/// bare relative url is resolved against the base path's directory (the
/// base's last path segment, when any, counts as a file).
pub(super) fn resolve_url(base: &str, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_owned());
    }
    let scheme_end = base.find("://")?;
    let scheme = &base[..scheme_end];
    let rest = &base[scheme_end + 3..]; // authority[/path]
    if let Some(protocol_relative) = raw.strip_prefix("//") {
        return Some(format!("{scheme}://{protocol_relative}"));
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, path),
        None => (rest, ""),
    };
    if let Some(root_relative) = raw.strip_prefix('/') {
        return Some(format!("{scheme}://{authority}/{root_relative}"));
    }
    let directory = match path.rfind('/') {
        Some(index) => &path[..=index],
        None => "",
    };
    Some(format!("{scheme}://{authority}/{directory}{raw}"))
}

/// The episode carried by one episode page: the first recognized
/// episode JSON-LD node of the page, or `None` (the page is then
/// ignored, never a failure — the extraction reports what it finds).
pub(super) fn episode_from_episode_page(html: &str) -> Option<WebEpisode> {
    let document = Html::parse_document(html);
    let payloads = jsonld_payloads(&document);
    let mut nodes: Vec<&Value> = Vec::new();
    for payload in &payloads {
        collect_jsonld_nodes(payload, &mut nodes);
    }
    nodes.iter().find_map(|node| episode_from_jsonld_node(node))
}

/// Extract the episodes of an already-parsed page honestly: one episode per
/// titled audio media present in the page — an `<a>` whose href ends with
/// a known audio extension, or an `<audio>` (or its `<source>`) carrying
/// a src — in DOCUMENT ORDER. The title is the media's own anchor text or
/// aria-label, never a heading and never an invented fallback: a media
/// without any title is not an episode and is skipped. The audio url is
/// resolved against the page and kept once per url; the image of the
/// media's container is carried only when the page provides one.
pub(super) fn parse_web_episodes(document: &Html, base_url: &str) -> Vec<WebEpisode> {
    let mut episodes = Vec::new();
    let media_selector = match Selector::parse("a[href], audio[src], audio source[src]") {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    let mut seen_audio_urls: Vec<String> = Vec::new();
    for media in document.select(&media_selector) {
        let Some((title, raw_audio)) = dom_episode_media(&media) else {
            continue;
        };
        let Some(audio_url) = resolve_url(base_url, raw_audio) else {
            continue;
        };
        if seen_audio_urls.contains(&audio_url) {
            continue;
        }
        seen_audio_urls.push(audio_url.clone());
        let container = media_container(&media);
        let image_url = container
            .as_ref()
            .and_then(|container| container_image_url(container, base_url));
        let summary = container
            .as_ref()
            .map_or_else(String::new, container_summary);
        episodes.push(WebEpisode {
            title,
            summary,
            audio_url: Some(audio_url),
            image_url,
        });
    }
    episodes
}

/// The known audio file extensions of an `<a>` media link, compared in
/// lower case against the end of the href.
const DOM_AUDIO_EXTENSIONS: [&str; 6] = [".mp3", ".m4a", ".ogg", ".wav", ".aac", ".opus"];

/// The honest (title, raw audio url) pair of one matched media element,
/// or `None` when the media cannot be titled: the title is the anchor
/// text for an `<a>` (else its `aria-label`), the `aria-label` of the
/// media for an `<audio>`, and of its `<audio>` parent for a `<source>`;
/// an empty title or an empty url yields `None` — nothing is invented.
fn dom_episode_media<'a>(media: &ElementRef<'a>) -> Option<(String, &'a str)> {
    let name = media.value().name();
    let title = match name {
        "a" => {
            let text = media.text().collect::<String>().trim().to_owned();
            if text.is_empty() {
                aria_label(media)?
            } else {
                text
            }
        }
        "audio" => aria_label(media)?,
        "source" => aria_label(&media.parent_element()?)?,
        _ => return None,
    };
    let raw_audio = match name {
        "a" => {
            let raw = media.attr("href")?.trim();
            let lower = raw.to_ascii_lowercase();
            if !DOM_AUDIO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
                return None;
            }
            raw
        }
        "audio" | "source" => {
            let raw = media.attr("src")?.trim();
            if raw.is_empty() {
                return None;
            }
            raw
        }
        _ => return None,
    };
    Some((title, raw_audio))
}

/// The trimmed, non-empty `aria-label` of an element, or `None`.
fn aria_label(element: &ElementRef<'_>) -> Option<String> {
    element
        .attr("aria-label")
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
}

/// The container of a matched media: its closest parent element — except
/// for a `<source>`, whose container is the parent of its `<audio>` (the
/// `<audio>` belongs to the media, not to its container).
fn media_container<'a>(media: &ElementRef<'a>) -> Option<ElementRef<'a>> {
    let parent = media.parent_element()?;
    if media.value().name() == "source" {
        parent.parent_element()
    } else {
        Some(parent)
    }
}

/// The image of a media container: the first `img[src]` it carries with a
/// non-empty src, resolved against the page — `None` when the page
/// provides none: the image stays optional and is never invented.
fn container_image_url(container: &ElementRef<'_>, base_url: &str) -> Option<String> {
    let selector = Selector::parse("img[src]").ok()?;
    container.select(&selector).find_map(|img| {
        img.attr("src")
            .map(str::trim)
            .filter(|src| !src.is_empty())
            .and_then(|src| resolve_url(base_url, src))
    })
}

/// The summary of a media container: the text of its first `<p>`,
/// trimmed, empty when absent.
fn container_summary(container: &ElementRef<'_>) -> String {
    let selector = match Selector::parse("p") {
        Ok(selector) => selector,
        Err(_) => return String::new(),
    };
    container
        .select(&selector)
        .next()
        .map(|paragraph| paragraph.text().collect::<String>().trim().to_owned())
        .unwrap_or_default()
}

/// An extracted episode reaches the preview only when it is honest end
/// to end: a non-empty title (after trim) AND a present, non-empty audio
/// media url. Untitled or audio-less elements are rejected here, before
/// the preview — the preview never carries an episode it could not play.
pub(super) fn keep_valid_episodes(episodes: Vec<WebEpisode>) -> Vec<WebEpisode> {
    episodes
        .into_iter()
        .filter(|episode| {
            !episode.title.trim().is_empty()
                && episode
                    .audio_url
                    .as_deref()
                    .map(|url| !url.trim().is_empty())
                    .unwrap_or(false)
        })
        .collect()
}

/// The self-delimiting frame of an ordered episode SET: the episode
/// count as a big-endian u32, then — per episode, in document order —
/// the three fingerprint fields, each prefixed by its byte length as a
/// big-endian u32 (`title`, resolved audio url, resolved image url).
/// A carried byte (U+0000 included) can never be mistaken for a field
/// boundary, so the frame round-trips to exactly ONE episode set —
/// two DIFFERENT episode sets always frame to different bytes.
fn framed_episode_set(episodes: &[WebEpisode], base_url: &str) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&(episodes.len() as u32).to_be_bytes());
    for episode in episodes {
        let audio = resolved_media_field(episode.audio_url.as_deref(), base_url);
        let image = resolved_media_field(episode.image_url.as_deref(), base_url);
        for field in [episode.title.as_bytes(), audio.as_bytes(), image.as_bytes()] {
            frame.extend_from_slice(&(field.len() as u32).to_be_bytes());
            frame.extend_from_slice(field);
        }
    }
    frame
}

/// The stable fingerprint of a previewed episode SET: the SHA-256 (hex)
/// of its self-delimiting [`framed_episode_set`] — every episode's
/// `title`, RESOLVED audio url and RESOLVED image url in document
/// order. Preview and accept compute it on the SAME extraction, so a
/// diverged page (an added, removed or re-pointed episode) is the
/// honest `SourceChanged` refusal — the preview is a pointer to this
/// exact state, never content.
pub(super) fn page_checksum_of(episodes: &[WebEpisode], base_url: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(framed_episode_set(episodes, base_url))
    )
}

/// One episode media reference, absolute: `resolve_url` is idempotent on
/// absolute urls, so a JSON-LD episode already absolute and a DOM episode
/// carried relative both fingerprint the same value. An unresolvable
/// reference keeps its raw value (it is still an honest content difference);
/// an absent reference contributes the empty string.
fn resolved_media_field(raw: Option<&str>, base_url: &str) -> String {
    match raw.map(str::trim).filter(|raw| !raw.is_empty()) {
        Some(raw) => resolve_url(base_url, raw).unwrap_or_else(|| raw.to_owned()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_web_episodes_returns_empty_on_no_episode() {
        let html = "<html><body><p>No episodes here</p></body></html>";
        let episodes = parse_web_episodes(
            &Html::parse_document(html),
            "https://fixture.example.org/page",
        );
        assert!(episodes.is_empty());
    }

    /// `resolve_url` is a pure helper: each branch pins how a relative
    /// episode url of the list page is resolved against it.
    #[test]
    fn test_resolve_url_pins_every_relative_branch() {
        let base = "https://www.example.com/shows/selection/episodes/ep1";
        assert_eq!(
            resolve_url(base, "https://media.example.org/a.m4a"),
            Some("https://media.example.org/a.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "//cdn.example.org/b.m4a"),
            Some("https://cdn.example.org/b.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "/media/c.m4a"),
            Some("https://www.example.com/media/c.m4a".to_owned())
        );
        assert_eq!(
            resolve_url(base, "d.m4a"),
            Some("https://www.example.com/shows/selection/episodes/d.m4a".to_owned())
        );
        // A base with no path: a bare relative url sits at the root.
        assert_eq!(
            resolve_url("https://www.example.com", "e.m4a"),
            Some("https://www.example.com/e.m4a".to_owned())
        );
        // A blank url is not resolvable and never a crash.
        assert_eq!(resolve_url(base, "   "), None);
    }

    /// Property (architect-audit): over every GENERATED (base, raw) shape
    /// — scheme × sober host[:port] × path shape × url branch — the
    /// resolution is an absolute http(s) url, STABLE under re-resolution
    /// (idempotent), and keeps the base authority: a protocol-relative
    /// raw is the only override. The shapes are generated, not picked:
    /// this pins the resolver's invariants beyond the hand-picked
    /// examples of `test_resolve_url_pins_every_relative_branch`.
    #[test]
    fn test_resolve_url_property_absolute_idempotent_authority_stable() {
        let schemes = ["http", "https"];
        let hosts = [
            "host.example",
            "www.example.fr:8080",
            "sub.domain.example:443",
        ];
        let paths = ["", "/", "/shows", "/shows/selection", "/a/b/c"];
        let raws = [
            "https://other.example.org/x/y.m4a",
            "http://other.example.org/z.ogg",
            "//cdn.example.org/track.m4a",
            "/root/relative.mp3",
            "bare.m4a",
            "sub/dir/file.m4a",
        ];
        for scheme in schemes {
            for host in hosts {
                for path in paths {
                    let base = format!("{scheme}://{host}{path}");
                    for raw in raws {
                        let resolved = resolve_url(&base, raw)
                            .unwrap_or_else(|| panic!("a sober base and a non-blank raw must resolve: base={base:?} raw={raw:?}"));
                        assert!(
                            resolved.starts_with("http://") || resolved.starts_with("https://"),
                            "a resolved url must be absolute http(s): base={base:?} raw={raw:?} got {resolved:?}"
                        );
                        // Idempotency: resolving an already-resolved url is the identity.
                        assert_eq!(
                            resolve_url(&base, &resolved),
                            Some(resolved.clone()),
                            "re-resolution must be the identity: base={base:?} raw={raw:?}"
                        );
                        // Authority: the base authority, unless a
                        // protocol-relative raw overrides it.
                        let expected_authority = if raw.starts_with("http://")
                            || raw.starts_with("https://")
                            || raw.starts_with("//")
                        {
                            raw.trim_start_matches("https://")
                                .trim_start_matches("http://")
                                .trim_start_matches("//")
                                .split('/')
                                .next()
                                .unwrap_or("")
                        } else {
                            host
                        };
                        let authority = resolved
                            .strip_prefix("http://")
                            .or_else(|| resolved.strip_prefix("https://"))
                            .and_then(|rest| rest.split('/').next())
                            .unwrap_or("");
                        assert_eq!(
                            authority, expected_authority,
                            "authority must be preserved (protocol-relative is the only override): base={base:?} raw={raw:?}"
                        );
                    }
                }
            }
        }
    }

    /// S1/S3 (DOM): one episode per titled audio media of the page, in
    /// document order — the link text as title, the media url as audio,
    /// the container image when present and nothing when absent, the
    /// first `<p>` of the container as summary, a `<source>` titled by
    /// its `<audio>` aria-label; a media without any title is not emitted.
    #[test]
    fn test_parse_web_episodes_extracts_dom_episodes_in_document_order() {
        let html = "<html><body>\
            <h1>Ma sélection</h1>\
            <section>\
              <a href=\"https://fixture.example.org/media/episode-un.m4a\">Episode un</a>\
              <img src=\"https://fixture.example.org/images/episode-un.jpg\" alt=\"Episode un\">\
            </section>\
            <section>\
              <a href=\"https://fixture.example.org/media/episode-deux.ogg\">Episode deux</a>\
              <p>Résumé deux.</p>\
            </section>\
            <section>\
              <audio src=\"https://fixture.example.org/media/episode-trois.wav\" aria-label=\"Episode trois\"></audio>\
            </section>\
            <section>\
              <audio aria-label=\"Episode source\"><source src=\"https://fixture.example.org/media/episode-source.opus\"></source></audio>\
            </section>\
            <a href=\"https://fixture.example.org/media/sans-titre.mp3\"></a>\
          </body></html>";
        let episodes = parse_web_episodes(
            &Html::parse_document(html),
            "https://fixture.example.org/page",
        );
        assert_eq!(
            episodes.len(),
            4,
            "one episode per titled audio media, in document order, got: {episodes:?}"
        );
        assert_eq!(episodes[0].title, "Episode un");
        assert_eq!(
            episodes[0].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-un.m4a")
        );
        assert_eq!(
            episodes[0].image_url.as_deref(),
            Some("https://fixture.example.org/images/episode-un.jpg")
        );
        assert_eq!(episodes[1].title, "Episode deux");
        assert_eq!(
            episodes[1].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-deux.ogg")
        );
        assert_eq!(
            episodes[1].summary, "Résumé deux.",
            "the first <p> of the container is the summary, never invented"
        );
        assert!(
            episodes[1].image_url.is_none(),
            "an absent image must stay absent, not invented"
        );
        assert_eq!(episodes[2].title, "Episode trois");
        assert_eq!(
            episodes[2].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-trois.wav")
        );
        assert!(episodes[2].image_url.is_none());
        assert_eq!(episodes[3].title, "Episode source");
        assert_eq!(
            episodes[3].audio_url.as_deref(),
            Some("https://fixture.example.org/media/episode-source.opus"),
            "a <source> without src on its <audio> is still one honest episode"
        );
        assert!(episodes[3].image_url.is_none());
    }

    /// S1 (honesty): an audio media without any title (empty link text,
    /// no aria-label) is NOT emitted — the extraction never falls back
    /// to an invented "Épisode sans titre".
    #[test]
    fn test_parse_web_episodes_never_invents_a_fallback_title() {
        let html = "<html><body>\
            <article>\
              <a href=\"https://fixture.example.org/media/orphelin.mp3\"></a>\
            </article>\
          </body></html>";
        let episodes = parse_web_episodes(
            &Html::parse_document(html),
            "https://fixture.example.org/page",
        );
        assert!(
            !episodes
                .iter()
                .any(|episode| episode.title == "Épisode sans titre"),
            "no invented fallback title may be emitted, got: {episodes:?}"
        );
        assert!(
            episodes.is_empty(),
            "an untitled media is not an episode, got: {episodes:?}"
        );
    }

    /// The preview filter keeps an episode only when the title is
    /// non-empty (after trim) AND a non-empty audio url is present:
    /// untitled or audio-less elements never reach the preview.
    #[test]
    fn test_keep_valid_episodes_rejects_untitled_and_audioless_items() {
        let episodes = vec![
            WebEpisode {
                title: "Valide".to_owned(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/valide.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: String::new(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/sans-titre.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: "   ".to_owned(),
                summary: String::new(),
                audio_url: Some("https://fixture.example.org/media/blanc.m4a".to_owned()),
                image_url: None,
            },
            WebEpisode {
                title: "Sans audio".to_owned(),
                summary: String::new(),
                audio_url: None,
                image_url: None,
            },
        ];
        let kept = keep_valid_episodes(episodes);
        assert_eq!(
            kept.len(),
            1,
            "only the honest episode survives, got: {kept:?}"
        );
        assert_eq!(kept[0].title, "Valide");
    }

    /// Property (architect-audit): the honesty filter is an ORDER-
    /// PRESERVING SUBSEQUENCE — every kept episode passed the predicate
    /// (non-empty trimmed title AND non-empty trimmed audio url) at its
    /// input position, and every passing input episode is kept. The
    /// titles and audio urls are generated (absent, empty, blank,
    /// valid), not only the four hand-picked examples.
    #[test]
    fn test_keep_valid_episodes_property_order_preserving_subsequence() {
        let titles = ["", "   ", "Titre", "  Espacé  "];
        let audios: [Option<&str>; 4] = [
            None,
            Some(""),
            Some("   "),
            Some("https://fixture.example.org/x.m4a"),
        ];
        for title in titles {
            for audio in audios {
                let episode = WebEpisode {
                    title: title.to_owned(),
                    summary: String::new(),
                    audio_url: audio.map(str::to_owned),
                    image_url: None,
                };
                let expected = !episode.title.trim().is_empty()
                    && episode
                        .audio_url
                        .as_deref()
                        .is_some_and(|url| !url.trim().is_empty());
                let kept = keep_valid_episodes(vec![episode]);
                assert_eq!(
                    kept.len(),
                    usize::from(expected),
                    "single-episode predicate: title={title:?} audio={audio:?}"
                );
            }
        }
        // Order preservation over a mixed generated set.
        let generated: Vec<WebEpisode> = (0..16)
            .map(|index| match index % 4 {
                0 => WebEpisode {
                    title: String::new(),
                    summary: String::new(),
                    audio_url: None,
                    image_url: None,
                },
                1 => WebEpisode {
                    title: "Valide".to_owned(),
                    summary: String::new(),
                    audio_url: Some(format!("https://fixture.example.org/{index}.m4a")),
                    image_url: None,
                },
                2 => WebEpisode {
                    title: "   ".to_owned(),
                    summary: String::new(),
                    audio_url: Some("   ".to_owned()),
                    image_url: None,
                },
                _ => WebEpisode {
                    title: format!("T{index}"),
                    summary: String::new(),
                    audio_url: Some(String::new()),
                    image_url: None,
                },
            })
            .collect();
        let kept = keep_valid_episodes(generated.clone());
        let expected_kept = generated
            .iter()
            .filter(|episode| {
                !episode.title.trim().is_empty()
                    && episode
                        .audio_url
                        .as_deref()
                        .is_some_and(|url| !url.trim().is_empty())
            })
            .map(|episode| (episode.title.as_str(), episode.audio_url.as_deref()))
            .collect::<Vec<_>>();
        let kept_pairs = kept
            .iter()
            .map(|episode| (episode.title.as_str(), episode.audio_url.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            kept_pairs, expected_kept,
            "the kept set must be exactly the passing subsequence, in input order"
        );
    }

    // ===== Preview fingerprint (architect-audit) =====
    //
    // The page checksum is the PREVIEW POINTER the accept re-proves
    // against: two DIFFERENT episode sets must never share it, whatever
    // bytes the page's fields carry.

    /// RED (architect-audit): a JSON-LD `name` or url may carry U+0000
    /// (serde_json accepts `\u0000`). The preview pointer must still
    /// distinguish every episode set: a NUL inside one field must never
    /// shift the next field's boundary into the same digest.
    ///
    /// Set A: episode 1's image ENDS with a NUL (`i\0`). Set B: the
    /// image keeps only `i` and episode 2's title STARTS with that
    /// NUL (`\0t2`). Under the OLD NUL-separated framing both sets
    /// fingerprinted to the byte-identical stream `…/i NUL NUL t2 …`
    /// (separator and carried NUL are the same byte 0x00) — two
    /// genuinely DIFFERENT episode sets, both passing `keep_valid`,
    /// which the length-prefixed framing separates.
    #[test]
    fn test_page_checksum_distinguishes_sets_with_nul_carried_fields() {
        let base = "https://fixture.example.org/page";
        let set_a = vec![
            WebEpisode {
                title: "t1".to_owned(),
                summary: String::new(),
                audio_url: Some("a1".to_owned()),
                image_url: Some("i\0".to_owned()),
            },
            WebEpisode {
                title: "t2".to_owned(),
                summary: String::new(),
                audio_url: Some("a2".to_owned()),
                image_url: None,
            },
        ];
        let set_b = vec![
            WebEpisode {
                title: "t1".to_owned(),
                summary: String::new(),
                audio_url: Some("a1".to_owned()),
                image_url: Some("i".to_owned()),
            },
            WebEpisode {
                title: "\0t2".to_owned(),
                summary: String::new(),
                audio_url: Some("a2".to_owned()),
                image_url: None,
            },
        ];
        assert_ne!(
            page_checksum_of(&set_a, base),
            page_checksum_of(&set_b, base),
            "a NUL inside a field must not shift the next episode's field boundary into the same checksum: {base:?}"
        );
    }

    /// Inverse of [`framed_episode_set`] — the honest decoder the
    /// round-trip property is asserted against: a big-endian u32
    /// count, then per episode three (len u32 BE, bytes) fields.
    fn decode_episode_set_frame(frame: &[u8]) -> Option<Vec<(String, String, String)>> {
        let mut pos = 0usize;
        let read_u32 = |pos: &mut usize| -> Option<u32> {
            let end = pos.checked_add(4)?;
            if end > frame.len() {
                return None;
            }
            let value = u32::from_be_bytes(frame[*pos..end].try_into().ok()?);
            *pos = end;
            Some(value)
        };
        let count = read_u32(&mut pos)? as usize;
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            let mut fields = Vec::with_capacity(3);
            for _ in 0..3 {
                let length = read_u32(&mut pos)? as usize;
                let end = pos.checked_add(length)?;
                if end > frame.len() {
                    return None;
                }
                fields.push(String::from_utf8(frame[pos..end].to_vec()).ok()?);
                pos = end;
            }
            decoded.push((fields.remove(0), fields.remove(0), fields.remove(0)));
        }
        (pos == frame.len()).then_some(decoded)
    }

    /// GREEN (architect-audit): the frame is SELF-DELIMITING — over a
    /// GENERATED pool of episode sets (titles/urls carrying NUL,
    /// blankness, unicode, absent/empty values, relative, root-relative
    /// and absolute shapes, 0..=2 episodes) every frame decodes back to
    /// exactly its own (title, resolved audio, resolved image) triples.
    /// A round-trip over the whole generated domain implies INJECTIVITY
    /// there: two different sets sharing a frame would both decode
    /// from it, which is impossible.
    #[test]
    fn test_framed_episode_set_property_roundtrip_injective_over_generated_sets() {
        let base = "https://fixture.example.org/page";
        let titles = ["t1", "Époqué", "  spaces  ", "nul\0inside", ""];
        let audios: [Option<&str>; 5] = [
            None,
            Some(""),
            Some("   "),
            Some("a1"),
            Some("//cdn.example.org/track.m4a"),
        ];
        let images: [Option<&str>; 4] = [
            None,
            Some("img\0"),
            Some("/root-relative.png"),
            Some("https://other.example.org/abs.png"),
        ];

        let mut sets: Vec<Vec<WebEpisode>> = Vec::new();
        sets.push(Vec::new());
        for title in titles {
            for audio in audios {
                for image in images {
                    let episode = WebEpisode {
                        title: title.to_owned(),
                        summary: String::new(),
                        audio_url: audio.map(str::to_owned),
                        image_url: image.map(str::to_owned),
                    };
                    sets.push(vec![episode.clone()]);
                    sets.push(vec![
                        episode,
                        WebEpisode {
                            title: "t2".to_owned(),
                            summary: String::new(),
                            audio_url: Some("a2".to_owned()),
                            image_url: None,
                        },
                    ]);
                }
            }
        }

        for set in &sets {
            let frame = framed_episode_set(set, base);
            let decoded = decode_episode_set_frame(&frame)
                .unwrap_or_else(|| panic!("the frame must always decode: {set:?}"));
            let expected: Vec<(String, String, String)> = set
                .iter()
                .map(|episode| {
                    (
                        episode.title.clone(),
                        resolved_media_field(episode.audio_url.as_deref(), base),
                        resolved_media_field(episode.image_url.as_deref(), base),
                    )
                })
                .collect();
            assert_eq!(
                decoded, expected,
                "the frame must round-trip to exactly one episode set: {set:?}"
            );
        }
    }

    /// The exact WIRE FORMAT of the frame is pinned byte for byte: a
    /// big-endian u32 count, then per episode the title, resolved audio
    /// and resolved image — each prefixed by its own big-endian u32
    /// byte length, an absent reference contributing an empty field. A
    /// silent re-layout would change the preview pointer the accept
    /// re-proves against.
    #[test]
    fn test_framed_episode_set_pins_count_and_length_prefix_wire_format() {
        let base = "https://fixture.example.org/page";
        let set = vec![WebEpisode {
            title: "a".to_owned(),
            summary: String::new(),
            audio_url: Some("https://fixture.example.org/x.m4a".to_owned()),
            image_url: None,
        }];
        let frame = framed_episode_set(&set, base);
        let expected: [u8; 50] = [
            0, 0, 0, 1, // count = 1
            0, 0, 0, 1, b'a', // title
            0, 0, 0, 33, // resolved audio length
            b'h', b't', b't', b'p', b's', b':', b'/', b'/', b'f', b'i', b'x', b't', b'u', b'r',
            b'e', b'.', b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'o', b'r', b'g', b'/',
            b'x', b'.', b'm', b'4', b'a', 0, 0, 0, 0, // absent image -> empty field
        ];
        assert_eq!(
            frame,
            expected.to_vec(),
            "the frame layout is count u32 BE + per-episode len-prefixed title/audio/image"
        );
    }
}
