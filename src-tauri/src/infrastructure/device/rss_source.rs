//! Fetches the RAW bytes of a user-provided RSS feed.
//!
//! The SECOND networked code path in Rustory (the official-catalog fetch
//! is the first — see `catalog_source.rs`). It runs exclusively on an
//! explicit user action (`Récupérer le flux`, or the accept's re-fetch of
//! the same address) behind the [`RssFeedSource`] trait, on a
//! `spawn_blocking` worker — never implicitly, never during any other
//! flow (offline-first guardrail). The trait lets the application layer
//! parse and commit without any network in tests.
//!
//! DELIBERATELY a neighbor of `LuniiHttpCatalogSource`, not a shared
//! abstraction: the catalog talks to hard-coded Lunii endpoints while
//! this client follows an arbitrary user-provided address (hence its own
//! bounded redirect policy). The DISCIPLINES are the same — one
//! keep-alive client, a shared wall-clock budget, a cap+1 bounded read,
//! PII-free stage-token errors (never the URL, never the host, never a
//! raw network message). A generic "network source" abstraction will be
//! born with a second real consumer, not speculatively.
//!
//! The response bytes are UNTRUSTED and validated downstream by the
//! bounded domain parser (`domain::import::rss`); this layer only
//! transports bytes.

use std::io::Read;
use std::time::Duration;

use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::MAX_MEDIA_BYTES;

/// Hard ceiling on the fetched feed body. A real RSS document is a few
/// hundred kB; 8 MiB bounds a hostile/runaway response without truncating
/// real data (and matches the local-artifact import ceiling).
pub const MAX_RSS_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// A user-provided feed can sit behind a couple of redirects (feed
/// proxies, `www.` canonicalization); five hops cover the legitimate
/// cases while refusing a redirect loop.
const MAX_RSS_REDIRECTS: usize = 5;

/// Explicit fetch of a user-provided RSS feed. Returns the raw response
/// bytes (untrusted — validated by the bounded domain parser). The URL is
/// already validated by the caller (`is_supported_feed_url`); the budget
/// caps the WHOLE request, connection to last body byte.
pub trait RssFeedSource: Send + Sync + 'static {
    fn fetch(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError>;

    /// Explicit fetch of ONE enclosure referenced by an ACCEPTED item —
    /// same disciplines as [`Self::fetch`] (whole-request budget, cap+1
    /// bounded read, PII-free stage tokens) with the MEDIA ceiling
    /// ([`MAX_MEDIA_BYTES`]) : un podcast légitime dépasse largement les
    /// 8 MiB du flux. The bytes are UNTRUSTED — validated downstream by
    /// the media sniff/transcode (`store_media`). The default refuses:
    /// a source that never learned enclosures degrades honestly to the
    /// « média distant non récupéré » verdict instead of lying.
    fn fetch_enclosure(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError> {
        let _ = (url, budget);
        Err(fetch_error("enclosure_unsupported"))
    }
}

/// Production source: blocking HTTPS (system OpenSSL via `native-tls`).
/// Holds ONE dedicated client so the preview fetch and the accept's
/// re-fetch reuse the keep-alive connection instead of paying a second
/// TLS handshake.
#[derive(Debug, Clone)]
pub struct HttpRssFeedSource {
    client: reqwest::blocking::Client,
}

impl Default for HttpRssFeedSource {
    fn default() -> Self {
        // NO silent fallback: a builder failure here means the TLS backend
        // could not initialize — an unusable environment. A bare
        // `Client::new()` repli would silently drop the named bounds (the
        // 5-redirect policy, the UA) and panics on the same failure mode
        // anyway, so it would only disguise the crash behind a
        // non-conforming client. The bounds hold on EVERY client actually
        // used, by construction.
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Rustory/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(MAX_RSS_REDIRECTS))
            .build()
            .expect("RSS HTTP client: TLS backend initialization failed");
        Self { client }
    }
}

impl RssFeedSource for HttpRssFeedSource {
    fn fetch(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError> {
        // An already-spent budget refuses up-front rather than issuing an
        // instant-timeout call (the catalog source's `remaining` pattern,
        // reduced to its single-request form).
        if budget.is_zero() {
            return Err(fetch_error("budget"));
        }
        // The blocking client applies the timeout from connection start to
        // the END of the body read, so the capped read below stays under
        // the same wall-clock budget.
        let resp = self
            .client
            .get(url)
            .timeout(budget)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| fetch_error("request"))?;
        read_bytes_capped(resp, MAX_RSS_RESPONSE_BYTES)
    }

    fn fetch_enclosure(&self, url: &str, budget: Duration) -> Result<Vec<u8>, AppError> {
        if budget.is_zero() {
            return Err(fetch_error("budget"));
        }
        let resp = self
            .client
            .get(url)
            .timeout(budget)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| fetch_error("request"))?;
        read_bytes_capped(resp, MAX_MEDIA_BYTES as u64)
    }
}

/// Read the response body bounded to `cap` bytes: reads one byte past the
/// cap so an overflow is detectable, then refuses it. Keeps a hostile or
/// runaway response from buffering unboundedly.
fn read_bytes_capped(resp: reqwest::blocking::Response, cap: u64) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    resp.take(cap + 1)
        .read_to_end(&mut buf)
        .map_err(|_| fetch_error("read"))?;
    if buf.len() as u64 > cap {
        return Err(fetch_error("response_oversize"));
    }
    Ok(buf)
}

/// User-facing feed-fetch failure. PII-free: a stable `stage` token, no
/// raw network message, no URL, no host.
pub fn fetch_error(stage: &'static str) -> AppError {
    AppError::rss_source_unreachable(
        "Récupération du flux impossible: la source est injoignable.",
        "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
    )
    .with_details(serde_json::json!({
        "source": "network",
        "stage": stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn fetch_error_is_actionable_and_pii_free() {
        let err = fetch_error("request");
        assert_eq!(err.code, AppErrorCode::RssSourceUnreachable);
        assert_eq!(
            err.message,
            "Récupération du flux impossible: la source est injoignable."
        );
        assert_eq!(
            err.user_action.as_deref(),
            Some("Vérifie l'adresse du flux et ta connexion, puis réessaie.")
        );
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["code"], "RSS_SOURCE_UNREACHABLE");
        assert_eq!(v["details"]["source"], "network");
        assert_eq!(v["details"]["stage"], "request");
    }

    #[test]
    fn a_spent_budget_refuses_before_any_request() {
        let source = HttpRssFeedSource::default();
        let err = source
            .fetch("http://127.0.0.1:9/never", Duration::ZERO)
            .expect_err("a zero budget must refuse up-front");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["stage"], "budget");
    }
}
