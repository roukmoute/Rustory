//! Official-catalog parsing + caching (Phase C).
//!
//! Turns the RAW Lunii `/v2/packs` JSON (fetched on an explicit user action,
//! or imported from a file for the 100%-offline path) into validated
//! [`OfficialCatalogEntry`] rows, then replaces the disposable `official`
//! cache. The catalog is UNTRUSTED input: every title is normalized +
//! validated with the same rules as a local story title (NFC + trim +
//! denylist + ≤120), every UUID must be canonical, and cover references are
//! accepted ONLY as CDN-relative paths resolved under `COVER_BASE` (no
//! arbitrary host → no SSRF, no `..` traversal); anything that fails is
//! skipped, never aborts the whole import. No execution, no implicit network.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::application::device::title::{replace_official_catalog, OfficialCatalogEntry};
use crate::domain::device::is_canonical_pack_uuid;
use crate::domain::shared::AppError;
use crate::domain::story::{normalize_title, validate_title};
use crate::infrastructure::db::DbHandle;
use crate::infrastructure::device::OfficialCatalogSource;
use crate::infrastructure::filesystem::{clear_catalog_covers, write_catalog_cover};

/// Upper bound on an imported catalog file (defensive; the real catalog is
/// a few MB). Larger inputs are refused before parsing.
pub const MAX_CATALOG_BYTES: usize = 32 * 1024 * 1024;
/// Upper bound on a parsed cover URL.
const MAX_THUMBNAIL_CHARS: usize = 2048;
/// Default catalog locale (the app ships in French).
pub const DEFAULT_CATALOG_LOCALE: &str = "fr_FR";
/// Host that serves the cover images referenced by the catalog as relative
/// paths (verified against the live data, 2026-06-16).
const COVER_BASE: &str = "https://storage.googleapis.com/lunii-data-prod";
/// Per-cover download budget — bounds one slow image without stalling the
/// whole refresh.
const COVER_BUDGET_PER_IMAGE: Duration = Duration::from_secs(10);

/// Fetch the official catalog over the network (EXPLICIT action), parse +
/// validate it, and replace the cache. Returns the number of recognized
/// entries cached.
///
/// The DB mutex is locked ONLY for the final replace — never across the
/// network fetch — mirroring the import service's lock discipline.
pub fn refresh_official_catalog(
    db: &Mutex<DbHandle>,
    source: &dyn OfficialCatalogSource,
    covers_dir: &Path,
    locale: &str,
    budget: Duration,
) -> Result<u32, AppError> {
    let started = Instant::now();
    let raw = source.fetch(locale, budget)?;
    let mut entries = parse_official_catalog(&raw, locale)?;
    // Refuse to overwrite a good cache with nothing: a parseable-but-wrong
    // response (e.g. `{}`, an unexpected envelope, or a server blip) yields
    // zero entries — replacing would silently wipe the cached titles. Fail
    // loudly instead and leave the previous cache intact.
    if entries.is_empty() {
        return Err(empty_catalog_error());
    }

    // Eager cover caching — offline-first: covers are downloaded HERE, on the
    // explicit refresh, never on a device read. Best-effort and bounded: a
    // cover that fails, times out, or is not a real image simply leaves that
    // pack cover-less (the title still stands). The parser put the remote URL
    // in `thumbnail`; we replace it with the LOCAL cache file name so the DB
    // (and the UI) only ever reference the offline cache. The disposable
    // cache is wiped before being re-filled.
    clear_catalog_covers(covers_dir);
    for entry in entries.iter_mut() {
        let Some(url) = entry.thumbnail.take() else {
            continue;
        };
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            continue; // out of budget — keep the title, skip the cover
        }
        let per_cover = remaining.min(COVER_BUDGET_PER_IMAGE);
        if let Ok(file_name) = source
            .fetch_cover(&url, per_cover)
            .and_then(|bytes| write_catalog_cover(covers_dir, &entry.pack_uuid, &bytes))
        {
            entry.thumbnail = Some(file_name);
        }
    }

    let mut guard = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    replace_official_catalog(&mut guard, &entries)
}

/// Import the catalog from an in-memory file payload (100% offline path),
/// parse + validate it, and replace the cache. Returns the number cached.
pub fn import_official_catalog_from_bytes(
    db: &mut DbHandle,
    bytes: &[u8],
    locale: &str,
) -> Result<u32, AppError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(parse_error("oversize"));
    }
    let raw = std::str::from_utf8(bytes).map_err(|_| parse_error("utf8"))?;
    let mut entries = parse_official_catalog(raw, locale)?;
    // Same guard as the network refresh: an imported file that parses to zero
    // recognized entries must not wipe an existing cache.
    if entries.is_empty() {
        return Err(empty_catalog_error());
    }
    // Offline path: never download covers (that would be network during an
    // explicitly-offline import). Drop the parsed cover URLs so no remote
    // reference is stored.
    for entry in entries.iter_mut() {
        entry.thumbnail = None;
    }
    replace_official_catalog(db, &entries)
}

/// Parse the raw catalog JSON into validated entries for `locale`.
///
/// Tolerant of the documented Lunii/STUdio shapes: an optional `response`
/// envelope wrapping either an object keyed by pack UUID or an array of pack
/// objects. Each entry's title is resolved from `localized_infos[locale]`,
/// falling back to any other locale, then a top-level `title`. Invalid /
/// untitled entries are skipped (never fatal); a non-JSON / wrong-shaped
/// root is an error.
pub fn parse_official_catalog(
    json: &str,
    locale: &str,
) -> Result<Vec<OfficialCatalogEntry>, AppError> {
    let root: Value = serde_json::from_str(json).map_err(|_| parse_error("json"))?;
    let packs = root.get("response").unwrap_or(&root);

    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    match packs {
        Value::Object(map) => {
            for (key, value) in map {
                if let Some(entry) = extract_entry(Some(key), value, locale) {
                    if seen.insert(entry.pack_uuid.clone()) {
                        entries.push(entry);
                    }
                }
            }
        }
        Value::Array(items) => {
            for value in items {
                if let Some(entry) = extract_entry(None, value, locale) {
                    if seen.insert(entry.pack_uuid.clone()) {
                        entries.push(entry);
                    }
                }
            }
        }
        _ => return Err(parse_error("shape")),
    }
    Ok(entries)
}

/// Build one validated entry, or `None` to skip a malformed / untitled pack.
fn extract_entry(key: Option<&str>, value: &Value, locale: &str) -> Option<OfficialCatalogEntry> {
    let raw_uuid = value
        .get("uuid")
        .and_then(Value::as_str)
        .or(key)?
        .to_ascii_lowercase();
    if !is_canonical_pack_uuid(&raw_uuid) {
        return None;
    }

    let title = resolve_title(value, locale)?;
    let normalized = normalize_title(&title);
    if validate_title(&normalized).is_err() {
        return None;
    }

    let thumbnail = resolve_thumbnail(value, locale);

    Some(OfficialCatalogEntry {
        pack_uuid: raw_uuid,
        title: normalized,
        thumbnail,
    })
}

/// Resolve a title: requested locale first, then any locale in
/// `localized_infos`, then a top-level `title`.
fn resolve_title(value: &Value, locale: &str) -> Option<String> {
    let infos = value.get("localized_infos");
    if let Some(localized) = infos.and_then(|i| i.get(locale)) {
        if let Some(title) = localized.get("title").and_then(Value::as_str) {
            if !title.trim().is_empty() {
                return Some(title.to_string());
            }
        }
    }
    if let Some(Value::Object(by_locale)) = infos {
        for localized in by_locale.values() {
            if let Some(title) = localized.get("title").and_then(Value::as_str) {
                if !title.trim().is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }
    value
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string)
}

/// Resolve a cover URL (requested locale, else any). Accepts only bounded
/// http(s) URLs; anything else is dropped (the title still stands).
fn resolve_thumbnail(value: &Value, locale: &str) -> Option<String> {
    let infos = value.get("localized_infos")?;
    let from = |localized: &Value| -> Option<String> {
        let image = localized.get("image")?;
        let url = image
            .get("image_url")
            .or_else(|| image.get("thumbnail_url"))
            .and_then(Value::as_str)?;
        accept_thumbnail(url)
    };
    if let Some(found) = infos.get(locale).and_then(from) {
        return Some(found);
    }
    if let Value::Object(by_locale) = infos {
        for localized in by_locale.values() {
            if let Some(found) = from(localized) {
                return Some(found);
            }
        }
    }
    None
}

/// Normalize a catalog cover reference into an absolute URL **always under
/// [`COVER_BASE`]**. The catalog is UNTRUSTED input, so the downloader must
/// never be steerable: only a CDN-relative path (`/public/images/packs/…`)
/// is accepted, resolved under `COVER_BASE`. Absolute URLs (arbitrary host →
/// SSRF), protocol-relative URLs (`//host`), and any `..` traversal are
/// rejected outright.
fn accept_thumbnail(url: &str) -> Option<String> {
    let trimmed = url.trim();
    // Must be a CDN-relative path: a single leading '/', not '//host'.
    let rest = trimmed.strip_prefix('/')?;
    if rest.starts_with('/') || trimmed.contains("..") {
        return None;
    }
    let absolute = format!("{COVER_BASE}/{rest}");
    if absolute.len() > MAX_THUMBNAIL_CHARS {
        return None;
    }
    Some(absolute)
}

fn parse_error(stage: &'static str) -> AppError {
    AppError::official_catalog_unavailable(
        "Catalogue officiel illisible: le fichier ou la réponse n'a pas le format attendu.",
        "Vérifie que le fichier est bien un catalogue officiel Lunii puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "parse",
        "stage": stage,
    }))
}

/// A parseable response that yielded zero recognized entries. Surfaced
/// instead of replacing — the previous cache is left untouched.
fn empty_catalog_error() -> AppError {
    AppError::official_catalog_unavailable(
        "Catalogue officiel vide ou non reconnu: aucune correspondance trouvée.",
        "Le cache existant est conservé. Vérifie la source du catalogue puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "parse",
        "stage": "empty",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;
    use crate::infrastructure::device::MockOfficialCatalogSource;
    use crate::infrastructure::filesystem::read_catalog_cover;
    use tempfile::TempDir;

    const UUID_A: &str = "11111111-1111-1111-1111-1111111111aa";
    const UUID_B: &str = "22222222-2222-2222-2222-2222222222bb";

    /// Seed one cached official row directly, to assert it survives a failed
    /// refresh/import.
    fn insert_official(db: &DbHandle, uuid: &str, title: &str) {
        db.conn()
            .execute(
                "INSERT INTO pack_metadata (pack_uuid, source, title, thumbnail, updated_at) \
                 VALUES (?1, 'official', ?2, NULL, '2026-06-16T00:00:00.000Z')",
                rusqlite::params![uuid, title],
            )
            .expect("insert official");
    }

    fn studio_envelope() -> String {
        format!(
            r#"{{
              "response": {{
                "{UUID_A}": {{
                  "uuid": "{UUID_A}",
                  "localized_infos": {{
                    "fr_FR": {{ "title": "Suzanne et Gaston", "image": {{ "image_url": "/public/images/packs/cover-a.png" }} }},
                    "en_GB": {{ "title": "Suzanne and Gaston" }}
                  }}
                }},
                "{UUID_B}": {{
                  "uuid": "{UUID_B}",
                  "localized_infos": {{
                    "en_GB": {{ "title": "Only English" }}
                  }}
                }}
              }}
            }}"#
        )
    }

    #[test]
    fn parses_localized_titles_and_cover_for_the_requested_locale() {
        let entries = parse_official_catalog(&studio_envelope(), "fr_FR").expect("parse");
        let a = entries.iter().find(|e| e.pack_uuid == UUID_A).expect("a");
        assert_eq!(a.title, "Suzanne et Gaston");
        // The CDN-relative cover path is resolved under COVER_BASE.
        assert_eq!(
            a.thumbnail.as_deref(),
            Some("https://storage.googleapis.com/lunii-data-prod/public/images/packs/cover-a.png")
        );
        // B has no fr_FR title → falls back to any available locale.
        let b = entries.iter().find(|e| e.pack_uuid == UUID_B).expect("b");
        assert_eq!(b.title, "Only English");
        assert!(b.thumbnail.is_none());
    }

    #[test]
    fn rejects_absolute_and_traversal_cover_urls_ssrf_guard() {
        // The catalog is untrusted: a cover must resolve under COVER_BASE
        // only. Absolute hosts (SSRF), protocol-relative URLs and `..`
        // traversal are dropped (the title still stands).
        for bad in [
            "https://evil.example/x.png",
            "http://169.254.169.254/latest/meta-data",
            "//evil.example/x.png",
            "/public/../../etc/passwd",
            "file:///etc/passwd",
        ] {
            let json = serde_json::json!({
                UUID_A: { "localized_infos": { "fr_FR": { "title": "T", "image": { "image_url": bad } } } },
            })
            .to_string();
            let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
            assert_eq!(entries.len(), 1, "{bad}: title kept");
            assert!(
                entries[0].thumbnail.is_none(),
                "{bad}: cover must be dropped"
            );
        }
    }

    #[test]
    fn parses_the_live_v2_packs_shape_top_level_title_fallback() {
        // A `response` object keyed by an OPAQUE id (not the UUID); this entry
        // carries `uuid` + `title` at the top level with NO `localized_infos`
        // — exercises the top-level title fallback (and no cover).
        let json = format!(
            r#"{{
              "code": "0.0",
              "response": {{
                "2y66qye9lwakniwvxq7asym3q": {{
                  "uuid": "{UUID_A}",
                  "reference": "ALB_fr_FR_PETITS_REVES_AU_PIANO",
                  "title": "Petits rêves au piano",
                  "subtitle": "15 morceaux de musique classique",
                  "hidden": false
                }}
              }}
            }}"#
        );
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries.len(), 1);
        // The opaque key is ignored; the real UUID comes from the `uuid` field.
        assert_eq!(entries[0].pack_uuid, UUID_A);
        assert_eq!(entries[0].title, "Petits rêves au piano");
        assert!(entries[0].thumbnail.is_none());
    }

    #[test]
    fn parses_the_live_v2_packs_shape_with_localized_cover() {
        // The REAL production entry (verified 2026-06-16): opaque key + `uuid`,
        // and a `localized_infos.fr_FR` block with `title` + `image.image_url`
        // as a CDN-relative path. Proves title + cover resolution on the
        // production shape.
        let json = format!(
            r#"{{
              "code": "0.0",
              "response": {{
                "2y66qye9lwakniwvxq7asym3q": {{
                  "uuid": "{UUID_A}",
                  "reference": "ALB_fr_FR_PETITS_REVES_AU_PIANO",
                  "title": "Petits rêves au piano",
                  "localized_infos": {{
                    "fr_FR": {{
                      "title": "Petits rêves au piano",
                      "subtitle": "15 morceaux de musique classique",
                      "image": {{ "image_url": "/public/images/packs/2y66qye9lwakniwvxq7asym3q.fr_FR.1.png" }}
                    }}
                  }},
                  "previews": ["/public/previews/x.mp3"]
                }}
              }}
            }}"#
        );
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pack_uuid, UUID_A);
        assert_eq!(entries[0].title, "Petits rêves au piano");
        assert_eq!(
            entries[0].thumbnail.as_deref(),
            Some("https://storage.googleapis.com/lunii-data-prod/public/images/packs/2y66qye9lwakniwvxq7asym3q.fr_FR.1.png")
        );
    }

    #[test]
    fn parses_a_top_level_array_shape_without_response_envelope() {
        let json = format!(r#"[{{"uuid":"{UUID_A}","title":"Direct"}}]"#);
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Direct");
    }

    #[test]
    fn uses_the_object_key_as_uuid_when_the_value_has_none() {
        let json = format!(r#"{{"{UUID_A}": {{ "title": "Keyed" }}}}"#);
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pack_uuid, UUID_A);
    }

    #[test]
    fn lowercases_uppercase_uuids_from_the_catalog() {
        let json = format!(
            r#"{{"response":{{"{up}": {{ "title": "Up" }}}}}}"#,
            up = UUID_A.to_uppercase()
        );
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries[0].pack_uuid, UUID_A);
    }

    #[test]
    fn skips_invalid_uuids_untitled_and_unsafe_titles_without_failing() {
        // A title carrying a bidi-override (U+202E) — built via `json!` so
        // the dangerous codepoint never appears as a source literal.
        let bidi_title = format!("Bidi{}attack", '\u{202E}');
        let json = serde_json::json!({
            "not-a-uuid": { "title": "Bad UUID" },
            UUID_A: { "title": "" },
            UUID_B: { "title": bidi_title },
        })
        .to_string();
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        // All three are dropped (bad uuid, empty title, denylisted char) —
        // but the parse as a whole still succeeds.
        assert!(entries.is_empty());
    }

    #[test]
    fn drops_non_http_thumbnails_but_keeps_the_title() {
        let json = format!(
            r#"{{"{UUID_A}": {{ "localized_infos": {{ "fr_FR": {{ "title": "T", "image": {{ "image_url": "file:///etc/passwd" }} }} }} }}}}"#
        );
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].thumbnail.is_none());
    }

    #[test]
    fn builds_an_absolute_cover_url_from_the_live_relative_path() {
        // Mirrors the real shape: cover as a relative path under the data CDN.
        let json = format!(
            r#"{{"{UUID_A}": {{ "localized_infos": {{ "fr_FR": {{ "title": "T", "image": {{ "image_url": "/public/images/packs/abc.fr_FR.1.png" }} }} }} }}}}"#
        );
        let entries = parse_official_catalog(&json, "fr_FR").expect("parse");
        assert_eq!(
            entries[0].thumbnail.as_deref(),
            Some("https://storage.googleapis.com/lunii-data-prod/public/images/packs/abc.fr_FR.1.png")
        );
    }

    #[test]
    fn rejects_non_json_and_wrong_shaped_roots() {
        assert!(parse_official_catalog("not json", "fr_FR").is_err());
        assert!(parse_official_catalog("123", "fr_FR").is_err());
        assert!(parse_official_catalog("\"a string\"", "fr_FR").is_err());
    }

    #[test]
    fn refresh_pulls_from_the_source_parses_caches_and_stores_covers_locally() {
        let mut handle = db::open_in_memory().expect("db");
        db::run_migrations(&mut handle).expect("migrate");
        let db = Mutex::new(handle);
        let covers = TempDir::new().expect("covers");
        let source = MockOfficialCatalogSource::new();
        source.enqueue_body(studio_envelope());

        let count = refresh_official_catalog(
            &db,
            &source,
            covers.path(),
            DEFAULT_CATALOG_LOCALE,
            Duration::from_secs(5),
        )
        .expect("refresh");
        assert_eq!(count, 2);
        assert_eq!(source.fetch_count(), 1);
        // UUID_A has a cover URL → one cover fetched + cached locally; UUID_B
        // has none → no cover fetch.
        assert_eq!(source.cover_fetch_count(), 1);

        let guard = db.lock().expect("lock");
        assert_eq!(
            crate::application::device::title::count_official_catalog(&guard).expect("count"),
            2
        );
        // The stored cover is a LOCAL file name, never the remote URL, and the
        // bytes are readable from the offline cache.
        let truth =
            crate::application::device::title::resolve_local_truth(&guard, &[UUID_A.to_string()])
                .expect("resolve");
        let file_name = truth.titles[UUID_A].thumbnail.clone().expect("cover file");
        assert!(!file_name.starts_with("http"), "must store a local name");
        read_catalog_cover(covers.path(), &file_name).expect("cover readable offline");
    }

    #[test]
    fn refresh_is_best_effort_when_a_cover_download_fails() {
        let mut handle = db::open_in_memory().expect("db");
        db::run_migrations(&mut handle).expect("migrate");
        let db = Mutex::new(handle);
        let covers = TempDir::new().expect("covers");
        let source = MockOfficialCatalogSource::new();
        source.enqueue_body(studio_envelope());
        source.fail_all_covers();

        // The catalog (titles) still caches; the failing cover just leaves
        // that pack cover-less.
        let count = refresh_official_catalog(
            &db,
            &source,
            covers.path(),
            DEFAULT_CATALOG_LOCALE,
            Duration::from_secs(5),
        )
        .expect("refresh succeeds despite cover failure");
        assert_eq!(count, 2);
        let guard = db.lock().expect("lock");
        let truth =
            crate::application::device::title::resolve_local_truth(&guard, &[UUID_A.to_string()])
                .expect("resolve");
        assert!(truth.titles[UUID_A].thumbnail.is_none());
    }

    #[test]
    fn refresh_propagates_a_source_network_failure() {
        let mut handle = db::open_in_memory().expect("db");
        db::run_migrations(&mut handle).expect("migrate");
        let db = Mutex::new(handle);
        let covers = TempDir::new().expect("covers");
        let source = MockOfficialCatalogSource::new();
        source.enqueue_failure(AppError::official_catalog_unavailable("offline", "retry"));

        let err = refresh_official_catalog(
            &db,
            &source,
            covers.path(),
            DEFAULT_CATALOG_LOCALE,
            Duration::from_secs(5),
        )
        .expect_err("must propagate");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::OfficialCatalogUnavailable
        );
    }

    #[test]
    fn refresh_with_an_empty_or_wrong_shaped_response_keeps_the_existing_cache() {
        let mut handle = db::open_in_memory().expect("db");
        db::run_migrations(&mut handle).expect("migrate");
        insert_official(&handle, UUID_A, "Titre déjà en cache");
        let db = Mutex::new(handle);
        let covers = TempDir::new().expect("covers");
        let source = MockOfficialCatalogSource::new();
        // A parseable-but-empty envelope (server blip / wrong shape).
        source.enqueue_body("{}");

        let err = refresh_official_catalog(
            &db,
            &source,
            covers.path(),
            DEFAULT_CATALOG_LOCALE,
            Duration::from_secs(5),
        )
        .expect_err("empty response must not replace");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "empty");
        // The previous cache survived.
        let guard = db.lock().expect("lock");
        assert_eq!(
            crate::application::device::title::count_official_catalog(&guard).expect("count"),
            1
        );
    }

    #[test]
    fn import_with_zero_recognized_entries_keeps_the_existing_cache() {
        let mut db = db::open_in_memory().expect("db");
        db::run_migrations(&mut db).expect("migrate");
        insert_official(&db, UUID_A, "Titre déjà en cache");

        // A JSON array of only-invalid entries → parses to zero.
        let err = import_official_catalog_from_bytes(
            &mut db,
            br#"[{"uuid":"not-a-uuid","title":"x"}]"#,
            DEFAULT_CATALOG_LOCALE,
        )
        .expect_err("zero recognized entries must not replace");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::OfficialCatalogUnavailable
        );
        assert_eq!(
            crate::application::device::title::count_official_catalog(&db).expect("count"),
            1
        );
    }

    #[test]
    fn import_from_bytes_caches_and_rejects_oversize() {
        let mut handle = db::open_in_memory().expect("db");
        db::run_migrations(&mut handle).expect("migrate");

        let count = import_official_catalog_from_bytes(
            &mut handle,
            studio_envelope().as_bytes(),
            DEFAULT_CATALOG_LOCALE,
        )
        .expect("import");
        assert_eq!(count, 2);

        let oversize = vec![b'x'; MAX_CATALOG_BYTES + 1];
        let err =
            import_official_catalog_from_bytes(&mut handle, &oversize, DEFAULT_CATALOG_LOCALE)
                .expect_err("oversize must fail");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "oversize");
    }
}
