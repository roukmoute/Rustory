//! Fetches the RAW official-catalog JSON from Lunii's servers.
//!
//! This is the ONLY networked code path in Rustory. It runs exclusively on
//! an explicit user action ("Récupérer le catalogue") behind the
//! [`OfficialCatalogSource`] trait, on a `spawn_blocking` worker — never
//! implicitly, never during a device read (offline-first / anti-catalog
//! guardrail, NFR14 / FR19). The trait lets the application layer parse and
//! cache without any network in tests.
//!
//! Wire contract (verified against the live endpoints, 2026-06-16):
//! - `GET https://server-auth-prod.lunii.com/guest/create` →
//!   `{"code":"0.0","response":{"token":{"server":"<JWT>","studio":"<JWT>"}}}`;
//!   the guest token lives at `response.token.server`.
//! - `GET https://server-data-prod.lunii.com/v2/packs` with header
//!   `X-AUTH-TOKEN: <token>` → `{"response":{"<id>":{…}}}`, one entry per
//!   OPAQUE key (not the UUID). Each entry carries `uuid` and `title` at the
//!   top level AND a `localized_infos[locale]` block holding `title`,
//!   `subtitle` and `image.image_url` — the cover, served as a CDN-RELATIVE
//!   path (e.g. `/public/images/packs/…png`). The parser resolves the title
//!   from `localized_infos[locale]` (then any locale, then the top-level
//!   `title`) and the cover from `localized_infos[locale].image`.
//!
//! The round-trip itself cannot be exercised offline / in CI; the parser
//! ([`crate::application::device::catalog`]) is fully tested against fixtures
//! mirroring this shape (with and without the localized cover). The response
//! is treated as UNTRUSTED and validated downstream; this layer only
//! transports bytes.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::domain::shared::AppError;

/// Guest-authenticated fetch of the official catalog. Returns the raw
/// `/v2/packs` JSON body (untrusted — validated by the parser). [`fetch_cover`]
/// downloads a single cover image during the SAME explicit refresh (never on
/// a device read), bounded; the bytes are untrusted and validated before
/// caching.
pub trait OfficialCatalogSource: Send + Sync + 'static {
    fn fetch(&self, locale: &str, budget: Duration) -> Result<String, AppError>;
    fn fetch_cover(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError>;
}

/// Lunii guest auth endpoint — issues a short-lived token with no account.
const AUTH_URL: &str = "https://server-auth-prod.lunii.com/guest/create";
/// Lunii commercial catalog endpoint (`UUID → localized_infos`).
const PACKS_URL: &str = "https://server-data-prod.lunii.com/v2/packs";

/// Hard ceiling on the catalog JSON body. The real catalog is ~2.3 MB;
/// 32 MB bounds a hostile/runaway response without truncating real data.
const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;
/// Hard ceiling on a single cover download. Kept equal to the cover cache's
/// write cap ([`crate::infrastructure::filesystem::MAX_COVER_BYTES`], 4 MiB)
/// so a too-large cover is refused at transport, not downloaded in full only
/// to be rejected at write time.
const MAX_COVER_RESPONSE_BYTES: u64 = crate::infrastructure::filesystem::MAX_COVER_BYTES as u64;

/// Production source: blocking HTTPS (system OpenSSL via `native-tls`). Holds
/// ONE client so the ~574 cover downloads reuse the keep-alive connection
/// instead of paying a TLS handshake each.
#[derive(Debug, Clone)]
pub struct LuniiHttpCatalogSource {
    client: reqwest::blocking::Client,
}

impl Default for LuniiHttpCatalogSource {
    fn default() -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Rustory/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client }
    }
}

impl OfficialCatalogSource for LuniiHttpCatalogSource {
    fn fetch(&self, _locale: &str, budget: Duration) -> Result<String, AppError> {
        // The budget covers the WHOLE auth + packs cycle, not each request:
        // a per-request timeout would let the two halves take `budget` each.
        // We give each request only the time left on the shared deadline.
        let started = Instant::now();

        // 1. Guest token. A GET with no credentials — an anonymous,
        //    throwaway session. Verified against the live endpoint: it
        //    answers `{"response":{"token":{"server":"<JWT>", ...}}}`.
        let auth_resp = self
            .client
            .get(AUTH_URL)
            .timeout(remaining(budget, started, "auth_request")?)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| fetch_error("auth_request"))?;
        let auth_body = read_body_capped(auth_resp, "auth_body")?;
        let token = extract_token(&auth_body).ok_or_else(|| fetch_error("auth_token"))?;

        // 2. Catalog, authorized with the guest token.
        let packs_resp = self
            .client
            .get(PACKS_URL)
            .timeout(remaining(budget, started, "packs_request")?)
            .header("X-AUTH-TOKEN", &token)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| fetch_error("packs_request"))?;
        read_body_capped(packs_resp, "packs_body")
    }

    fn fetch_cover(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError> {
        if budget.is_zero() {
            return Err(fetch_error("cover_request"));
        }
        let resp = self
            .client
            .get(url)
            .timeout(budget)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| fetch_error("cover_request"))?;
        read_bytes_capped(resp, MAX_COVER_RESPONSE_BYTES, "cover_body")
    }
}

/// Time left on the shared deadline; a recoverable timeout error when the
/// budget is already spent (rather than issuing a zero/instant-timeout call).
fn remaining(
    budget: Duration,
    started: Instant,
    stage: &'static str,
) -> Result<Duration, AppError> {
    let left = budget.saturating_sub(started.elapsed());
    if left.is_zero() {
        return Err(fetch_error(stage));
    }
    Ok(left)
}

/// Read a response body to bytes, bounded to `cap`: reads one byte past the
/// cap so an overflow is detectable, then refuses it. Keeps a hostile or
/// runaway response from buffering unboundedly.
fn read_bytes_capped(
    resp: reqwest::blocking::Response,
    cap: u64,
    stage: &'static str,
) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    resp.take(cap + 1)
        .read_to_end(&mut buf)
        .map_err(|_| fetch_error(stage))?;
    if buf.len() as u64 > cap {
        return Err(fetch_error("response_oversize"));
    }
    Ok(buf)
}

/// Read a response body as UTF-8 text, bounded to [`MAX_RESPONSE_BYTES`].
fn read_body_capped(
    resp: reqwest::blocking::Response,
    stage: &'static str,
) -> Result<String, AppError> {
    let buf = read_bytes_capped(resp, MAX_RESPONSE_BYTES, stage)?;
    String::from_utf8(buf).map_err(|_| fetch_error(stage))
}

/// Pull the guest token out of the auth response. Tolerant of a few shapes
/// (`response.token.server`, `token.server`, `response.token`, `token`) so
/// a minor envelope change does not silently break recognition.
fn extract_token(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    for pointer in [
        "/response/token/server",
        "/token/server",
        "/response/token",
        "/token",
    ] {
        if let Some(token) = value.pointer(pointer).and_then(|v| v.as_str()) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// User-facing catalog-fetch failure. PII-free: a stable `stage` token, no
/// raw network message, no URL.
fn fetch_error(stage: &'static str) -> AppError {
    AppError::official_catalog_unavailable(
        "Récupération du catalogue officiel impossible: le service est injoignable.",
        "Vérifie ta connexion puis réessaie ; tu peux aussi importer un fichier de catalogue hors-ligne.",
    )
    .with_details(serde_json::json!({
        "source": "network",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_reads_the_studio_envelope() {
        let json = r#"{"response":{"token":{"server":"abc123"}}}"#;
        assert_eq!(extract_token(json).as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_token_tolerates_a_flat_token() {
        assert_eq!(
            extract_token(r#"{"token":"flat"}"#).as_deref(),
            Some("flat")
        );
    }

    #[test]
    fn extract_token_returns_none_for_missing_or_empty() {
        assert_eq!(extract_token(r#"{"response":{}}"#), None);
        assert_eq!(extract_token(r#"{"token":{"server":""}}"#), None);
        assert_eq!(extract_token("not json"), None);
    }

    #[test]
    fn fetch_error_is_actionable_and_offline_friendly() {
        let err = fetch_error("auth_request");
        assert_eq!(
            err.code,
            crate::domain::shared::AppErrorCode::OfficialCatalogUnavailable
        );
        assert!(err.user_action.as_deref().unwrap_or("").contains("fichier"));
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "network");
        assert_eq!(v["details"]["stage"], "auth_request");
    }
}
