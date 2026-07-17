//! Fetches the latest PUBLISHED official release from the repository's
//! public releases API.
//!
//! The THIRD networked code path in Rustory (`catalog_source.rs` and
//! `rss_source.rs` are the first two). It runs AT MOST ONCE per launch
//! behind the [`UpdateReleaseSource`] trait, on a `spawn_blocking`
//! worker, gated by the pure per-launch decision (`Update Availability
//! Contract`) — never during any user flow, never required by the core
//! flow. The trait lets the application layer resolve and memoize
//! without any network in tests.
//!
//! DELIBERATELY a neighbor of the two existing clients, not a shared
//! abstraction: their named condition for a common "network source"
//! refactor (a third real consumer) is now MET, but that refactor is a
//! dedicated chore — here the DISCIPLINES are reproduced verbatim (one
//! keep-alive client, a wall-clock budget, a cap+1 bounded read,
//! PII-free stage-token errors — never the URL, never a raw network
//! message) without touching either existing client.
//!
//! The response is UNTRUSTED and minimal: HTTP 200 yields the raw
//! `tag_name` (the strict domain parser decides what it means), HTTP 404
//! yields the REAL "no published release" state of the world — never a
//! failure. Everything else is a closed-set transport stage.

use std::io::Read;
use std::time::Duration;

use crate::domain::update::ReleaseProbe;

/// The canonical public endpoint answering the latest PUBLISHED release
/// (drafts and pre-releases excluded by the API contract itself). The
/// production constant — locked by a contract test.
pub const GITHUB_LATEST_RELEASE_ENDPOINT: &str =
    "https://api.github.com/repos/roukmoute/Rustory/releases/latest";

/// Environment override of the endpoint — a smoke/local tool (the
/// product precedent: the simulated device mount roots), read ONCE at
/// source construction. The production constant stays the wire truth.
pub const UPDATE_CHECK_ENDPOINT_ENV: &str = "RUSTORY_UPDATE_CHECK_ENDPOINT";

/// Hard ceiling on the response body. The real "latest release" answer
/// is a few kB of JSON; 1 MiB bounds a hostile/runaway response without
/// truncating real data.
pub const MAX_UPDATE_RESPONSE_BYTES: u64 = 1024 * 1024;

/// Closed set of PII-free failure stages of the consultation (the
/// `catalog_source::fetch_error` discipline): a stable token, NEVER the
/// URL, never a raw network message. The application layer maps every
/// stage to the calm `checkUnavailable` verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFetchStage {
    /// The HTTP client could not be built (TLS backend initialization).
    Client,
    /// The request itself failed (connection, DNS, timeout, spent
    /// budget).
    Request,
    /// A hostile terminal status (403/429/5xx, any redirect status…) —
    /// everything that is neither 200 nor the meaningful 404. The
    /// endpoint is canonical: `Policy::none` never follows a 3xx, so a
    /// redirect answer lands HERE as its terminal status.
    Status,
    /// Reading the response body failed mid-stream.
    Read,
    /// The response body exceeded [`MAX_UPDATE_RESPONSE_BYTES`].
    Oversize,
    /// A 200 answer whose body is not the expected shape (unparsable
    /// JSON, absent or empty `tag_name`).
    Malformed,
}

impl UpdateFetchStage {
    /// Stable snake-free token for the diagnostics log (closed set).
    pub const fn token(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Request => "request",
            Self::Status => "status",
            Self::Read => "read",
            Self::Oversize => "oversize",
            Self::Malformed => "malformed",
        }
    }
}

/// One consultation of the latest published release. Returns the raw
/// probe (untrusted — the strict domain parser decides) or a closed-set
/// transport stage. The budget caps the WHOLE request, connection to
/// last body byte.
pub trait UpdateReleaseSource: Send + Sync {
    fn fetch_latest(&self, budget: Duration) -> Result<ReleaseProbe, UpdateFetchStage>;
}

/// Partial view of the "latest release" JSON answer: `tag_name` is the
/// ONLY field this product reads. DELIBERATELY no `deny_unknown_fields`
/// — the inverse of the artifact-parsing pattern: the API answers with
/// dozens of fields that are none of our business, and refusing them
/// would turn every additive API evolution into a false `malformed`.
#[derive(serde::Deserialize)]
struct LatestReleaseBody {
    #[serde(default)]
    tag_name: Option<String>,
}

/// Production source: blocking HTTPS (system OpenSSL via `native-tls`).
/// Holds ONE keep-alive client (the sibling clients' discipline — the
/// check is single-shot per launch, so the reuse is structural rather
/// than hot, and the named bounds hold on every client actually used).
#[derive(Debug, Clone)]
pub struct GithubHttpReleaseSource {
    /// `None` when the TLS backend could not initialize. DELIBERATE
    /// inverse of the RSS client's `expect`: those clients serve an
    /// EXPLICIT user action, this one serves a background consultation —
    /// panicking the boot for optional information would contradict the
    /// contract ("never disturbs the core flow"), so a broken client
    /// degrades to the calm `client` stage at fetch time.
    client: Option<reqwest::blocking::Client>,
    endpoint: String,
}

impl Default for GithubHttpReleaseSource {
    fn default() -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("Rustory/", env!("CARGO_PKG_VERSION")))
            // The endpoint is canonical: any redirect is a transport
            // failure, never followed.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok();
        let endpoint = std::env::var(UPDATE_CHECK_ENDPOINT_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| GITHUB_LATEST_RELEASE_ENDPOINT.to_string());
        Self { client, endpoint }
    }
}

impl GithubHttpReleaseSource {
    /// Endpoint-direct constructor for the hermetic loopback tests —
    /// production always goes through `Default` (constant + env
    /// override).
    #[cfg(test)]
    fn with_endpoint(endpoint: String) -> Self {
        Self {
            endpoint,
            ..Self::default()
        }
    }
}

impl UpdateReleaseSource for GithubHttpReleaseSource {
    fn fetch_latest(&self, budget: Duration) -> Result<ReleaseProbe, UpdateFetchStage> {
        let client = self.client.as_ref().ok_or(UpdateFetchStage::Client)?;
        // An already-spent budget refuses up-front rather than issuing
        // an instant-timeout call (the sibling clients' pattern).
        if budget.is_zero() {
            return Err(UpdateFetchStage::Request);
        }
        // The blocking client applies the timeout from connection start
        // to the END of the body read, so the capped read below stays
        // under the same wall-clock budget.
        let resp = client
            .get(&self.endpoint)
            .timeout(budget)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|_| UpdateFetchStage::Request)?;
        match resp.status() {
            reqwest::StatusCode::OK => {}
            // The REAL state of the world while no official release is
            // published: an honest answer, never a failure.
            reqwest::StatusCode::NOT_FOUND => return Ok(ReleaseProbe::NoPublishedRelease),
            // Everything else — 403/429 rate limits, 5xx, a redirect
            // status surfaced by `Policy::none` — is a transport stage.
            _ => return Err(UpdateFetchStage::Status),
        }
        let body = read_bytes_capped(resp)?;
        let latest: LatestReleaseBody =
            serde_json::from_slice(&body).map_err(|_| UpdateFetchStage::Malformed)?;
        match latest.tag_name {
            Some(tag) if !tag.is_empty() => Ok(ReleaseProbe::Latest { tag }),
            _ => Err(UpdateFetchStage::Malformed),
        }
    }
}

/// Read the response body bounded to [`MAX_UPDATE_RESPONSE_BYTES`]:
/// reads one byte past the cap so an overflow is detectable, then
/// refuses it. Keeps a hostile or runaway response from buffering
/// unboundedly.
fn read_bytes_capped(resp: reqwest::blocking::Response) -> Result<Vec<u8>, UpdateFetchStage> {
    let mut buf = Vec::new();
    resp.take(MAX_UPDATE_RESPONSE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| UpdateFetchStage::Read)?;
    if buf.len() as u64 > MAX_UPDATE_RESPONSE_BYTES {
        return Err(UpdateFetchStage::Oversize);
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;

    const BUDGET: Duration = Duration::from_secs(5);

    /// Serve ONE canned HTTP response on a loopback listener and hand
    /// back the URL to fetch — a REAL local HTTP exchange, zero external
    /// network (the sibling integration tests' pattern, reduced to its
    /// unit form).
    fn serve_once(
        status_line: &'static str,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            // Drain the request head (best effort — the client sends a
            // small GET).
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf);
            let head = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).expect("head");
            // Stream in chunks so an oversize body can be interrupted by
            // the client hanging up right at the cap.
            for chunk in body.chunks(64 * 1024) {
                if socket.write_all(chunk).is_err() {
                    break;
                }
            }
        });
        (format!("http://127.0.0.1:{port}/latest"), handle)
    }

    #[test]
    fn a_200_with_a_tag_yields_the_raw_probe() {
        let (url, serve) = serve_once(
            "200 OK",
            br#"{"tag_name":"v9.9.9","name":"ignored","assets":[]}"#.to_vec(),
        );
        let probe = GithubHttpReleaseSource::with_endpoint(url)
            .fetch_latest(BUDGET)
            .expect("probe");
        assert_eq!(
            probe,
            ReleaseProbe::Latest {
                tag: "v9.9.9".to_string()
            }
        );
        let _ = serve.join();
    }

    #[test]
    fn a_404_is_the_honest_no_published_release_state() {
        let (url, serve) = serve_once("404 Not Found", br#"{"message":"Not Found"}"#.to_vec());
        let probe = GithubHttpReleaseSource::with_endpoint(url)
            .fetch_latest(BUDGET)
            .expect("a 404 is a state of the world, never a failure");
        assert_eq!(probe, ReleaseProbe::NoPublishedRelease);
        let _ = serve.join();
    }

    #[test]
    fn a_hostile_status_maps_to_the_status_stage() {
        let (url, serve) = serve_once(
            "403 Forbidden",
            br#"{"message":"API rate limit exceeded"}"#.to_vec(),
        );
        let stage = GithubHttpReleaseSource::with_endpoint(url)
            .fetch_latest(BUDGET)
            .expect_err("a rate limit is a transport failure, never an alarm");
        assert_eq!(stage, UpdateFetchStage::Status);
        let _ = serve.join();
    }

    #[test]
    fn rotten_json_maps_to_the_malformed_stage() {
        let (url, serve) = serve_once("200 OK", b"not json at all".to_vec());
        let stage = GithubHttpReleaseSource::with_endpoint(url)
            .fetch_latest(BUDGET)
            .expect_err("rotten JSON must refuse");
        assert_eq!(stage, UpdateFetchStage::Malformed);
        let _ = serve.join();
    }

    #[test]
    fn an_absent_or_empty_tag_name_maps_to_the_malformed_stage() {
        for body in [&br#"{"name":"no tag"}"#[..], &br#"{"tag_name":""}"#[..]] {
            let (url, serve) = serve_once("200 OK", body.to_vec());
            let stage = GithubHttpReleaseSource::with_endpoint(url)
                .fetch_latest(BUDGET)
                .expect_err("a tagless answer must refuse");
            assert_eq!(stage, UpdateFetchStage::Malformed);
            let _ = serve.join();
        }
    }

    #[test]
    fn an_oversize_response_refuses_at_cap_plus_one() {
        let body_len = (MAX_UPDATE_RESPONSE_BYTES + 1) as usize;
        let (url, serve) = serve_once("200 OK", vec![b'a'; body_len]);
        let stage = GithubHttpReleaseSource::with_endpoint(url)
            .fetch_latest(BUDGET)
            .expect_err("the cap+1 read must refuse the oversize body");
        assert_eq!(stage, UpdateFetchStage::Oversize);
        let _ = serve.join();
    }

    #[test]
    fn a_spent_budget_refuses_before_any_request() {
        let source = GithubHttpReleaseSource::with_endpoint("http://127.0.0.1:9/never".to_string());
        let stage = source
            .fetch_latest(Duration::ZERO)
            .expect_err("a zero budget must refuse up-front");
        assert_eq!(stage, UpdateFetchStage::Request);
    }

    #[test]
    fn stage_tokens_are_stable_and_pairwise_distinct() {
        let stages = [
            UpdateFetchStage::Client,
            UpdateFetchStage::Request,
            UpdateFetchStage::Status,
            UpdateFetchStage::Read,
            UpdateFetchStage::Oversize,
            UpdateFetchStage::Malformed,
        ];
        let tokens: Vec<&str> = stages.iter().map(|stage| stage.token()).collect();
        assert_eq!(
            tokens,
            vec![
                "client",
                "request",
                "status",
                "read",
                "oversize",
                "malformed"
            ]
        );
        for (index, a) in tokens.iter().enumerate() {
            for b in tokens.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }
}
