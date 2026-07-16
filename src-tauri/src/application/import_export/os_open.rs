//! OS-open intent state — the application-side receiver of "open this file
//! with Rustory" gestures forwarded by the operating system (cold-start
//! argv, the single-instance second-launch relay, macOS `RunEvent::Opened`).
//!
//! Design (see `ui-states.md#OS Open Contract`):
//!
//! - The intent (an absolute `PathBuf`) is held HERE, Rust-side. It never
//!   crosses the IPC boundary — the frontend pulls a typed VERDICT through
//!   [`analyze_pending_intent`] and only ever sees the file's basename.
//! - ONE pending intent at a time; a newer offer REPLACES it (the user's
//!   last gesture wins). Several files in ONE gesture become a single
//!   `MultipleFiles` intent — never a partial processing.
//! - Every offer stamps a monotonic GENERATION, and consumption is a
//!   compare-and-take ([`OsOpenState::take_if`]): a settling analysis
//!   claims exactly the generation it read, so a read that finishes AFTER
//!   a newer gesture landed can never consume that newer intent — the
//!   analysis loop drops its stale verdict and serves the newest one.
//!   One-shot by construction: the frontend's StrictMode double-effect is
//!   harmless (the second analysis answers `none`).
//! - Arguments travel as [`OsString`]s end to end (a Unix filename is a
//!   byte sequence, not UTF-8): filtering inspects raw bytes and the
//!   basename falls back to a sober placeholder — a legal non-UTF-8 path
//!   can never panic the channel.
//! - NO Tauri dependency in this module: every decision (filtering,
//!   replacement, generations, verdicts) is a pure function over an
//!   instantiable [`OsOpenState`], fully unit-testable. The `lib.rs`
//!   frontier stays a thin glue (URL→path conversion, focus, emit).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::shared::AppError;
use crate::ipc::dto::import_export::OsOpenAnalysisDto;

use super::import::{analyze_artifact, read_artifact_bounded};

/// One pending OS-open intent, as filtered from the raw OS arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsOpenIntent {
    /// Exactly one file candidate — the artifact to route into the
    /// existing import review.
    Artifact(PathBuf),
    /// Several file candidates in ONE gesture — a calm named limit;
    /// nothing is partially processed. The count names the gesture's size
    /// (asserted by the filtering tests) — the wire verdict carries the
    /// frozen copy alone.
    MultipleFiles(usize),
}

/// The (at most one) pending intent plus the monotonic generation of the
/// LAST offer — the identity a settling analysis must present to consume.
#[derive(Debug, Default)]
struct PendingSlot {
    generation: u64,
    intent: Option<OsOpenIntent>,
}

/// The Rust-owned holder of the (at most one) pending OS-open intent.
///
/// Instantiable so unit tests work on LOCAL instances — the process-wide
/// [`OS_OPEN_STATE`] global is consumed only by the Tauri wiring and the
/// commands. The mutex is only ever held for the duration of a field
/// swap — never across an `.await`, never around file I/O.
#[derive(Debug, Default)]
pub struct OsOpenState {
    pending: Mutex<PendingSlot>,
}

/// Process-wide intent state consumed by the Tauri wiring (`lib.rs`) and
/// the `analyze_os_open_request` / `discard_os_open_request` commands.
pub static OS_OPEN_STATE: OsOpenState = OsOpenState::new();

impl OsOpenState {
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(PendingSlot {
                generation: 0,
                intent: None,
            }),
        }
    }

    /// Filter raw OS-provided arguments into at most one intent and, when
    /// one results, store it — REPLACING any pending intent (the user's
    /// last gesture is the true intent) under a FRESH generation. Returns
    /// `true` iff an intent resulted (the wiring then wakes the window and
    /// signals the front).
    ///
    /// Pure filtering rules, one decision per test:
    /// - empty arguments and `-`-prefixed flags are discarded (raw-byte
    ///   inspection — a non-UTF-8 argument is a legal path candidate);
    /// - RELATIVE paths resolve against the PROVIDED `cwd` (the real trap:
    ///   a second instance has its own cwd — the callback's, never the
    ///   living process's);
    /// - 0 candidates → no-op (a pending intent survives);
    /// - 1 candidate → [`OsOpenIntent::Artifact`];
    /// - ≥ 2 candidates → [`OsOpenIntent::MultipleFiles`] (no partial
    ///   processing).
    pub fn offer(&self, args: &[OsString], cwd: &Path) -> bool {
        let mut candidates: Vec<PathBuf> = args
            .iter()
            .filter(|arg| !arg.is_empty() && !arg.as_encoded_bytes().starts_with(b"-"))
            .map(|arg| {
                let path = Path::new(arg);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    cwd.join(path)
                }
            })
            .collect();

        let intent = match candidates.len() {
            0 => return false,
            1 => OsOpenIntent::Artifact(candidates.remove(0)),
            n => OsOpenIntent::MultipleFiles(n),
        };
        let mut slot = self.lock();
        slot.generation += 1;
        slot.intent = Some(intent);
        true
    }

    /// Clone the pending intent (with its generation) without consuming it.
    pub fn peek(&self) -> Option<(u64, OsOpenIntent)> {
        let slot = self.lock();
        slot.intent.clone().map(|intent| (slot.generation, intent))
    }

    /// Consume the pending intent IFF it is still the given generation —
    /// the compare-and-take that keeps "the last gesture wins" true under
    /// the real race: a settlement for generation N can never consume a
    /// newer generation offered while N was being read. Returns `true`
    /// when the intent was claimed (one-shot: a second claim of the same
    /// generation yields `false`).
    pub fn take_if(&self, generation: u64) -> bool {
        let mut slot = self.lock();
        if slot.generation == generation && slot.intent.is_some() {
            slot.intent = None;
            true
        } else {
            false
        }
    }

    /// Drop the pending intent, if any. Idempotent.
    pub fn discard(&self) {
        self.lock().intent = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PendingSlot> {
        // Same poisoning posture as the DB handle: the protected value is
        // a plain slot swap, always left coherent — recover the guard.
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Analyze the pending OS-open intent into its typed wire verdict — the
/// WHOLE decision logic of the `analyze_os_open_request` command, kept out
/// of the Tauri handler so it stays unit-testable on a local state.
///
/// Synchronous by design (the command drives it on a `spawn_blocking`
/// worker): the bounded file read happens here, and no `MutexGuard` ever
/// crosses an `.await` (the state locks are inside `peek`/`take_if`).
pub fn analyze_pending_intent(state: &OsOpenState) -> Result<OsOpenAnalysisDto, AppError> {
    analyze_pending_intent_with(state, read_artifact_bounded)
}

/// [`analyze_pending_intent`] parameterized by the file reader, so the
/// mid-read race (a newer gesture landing while a file is being read) is
/// DETERMINISTICALLY testable: the test's reader offers a second intent
/// before returning.
///
/// The loop settles on the NEWEST intent, never a stale one:
/// - No pending intent → [`OsOpenAnalysisDto::None`] (total silent no-op).
/// - `MultipleFiles` → claimed by generation + the frozen calm-limit copy.
/// - `Artifact` → bounded read then the UNCHANGED [`analyze_artifact`]
///   pipeline (same authority, same findings). If a NEWER intent replaced
///   this one while its file was being read, the stale outcome (verdict OR
///   read failure) is DROPPED and the loop re-analyzes the newest intent —
///   the user's last gesture wins. A read failure with the intent still
///   current LEAVES IT PENDING and rejects with the existing `file_read`
///   transport error — `Réessayer` replays the same intent. Success claims
///   the intent before returning (one-shot).
fn analyze_pending_intent_with(
    state: &OsOpenState,
    read: impl Fn(&Path) -> Result<Vec<u8>, AppError>,
) -> Result<OsOpenAnalysisDto, AppError> {
    loop {
        let Some((generation, intent)) = state.peek() else {
            return Ok(OsOpenAnalysisDto::None);
        };
        match intent {
            OsOpenIntent::MultipleFiles(_) => {
                if state.take_if(generation) {
                    return Ok(OsOpenAnalysisDto::multiple_files());
                }
                // A newer gesture replaced it between peek and claim —
                // analyze THAT one instead.
                continue;
            }
            OsOpenIntent::Artifact(path) => {
                // Carry the BASENAME only across the boundary — never the
                // absolute path (PII). Falls back to a sober placeholder
                // for a non-UTF-8 / unnameable path (the dialog-import
                // pattern) — never a forced conversion, never a panic.
                let source_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("artefact")
                    .to_string();
                match read(&path) {
                    Ok(bytes) => {
                        if !state.take_if(generation) {
                            // The user's newer gesture wins — drop this
                            // stale verdict and serve the newest intent.
                            continue;
                        }
                        let analysis = analyze_artifact(&bytes, source_name);
                        return Ok(OsOpenAnalysisDto::analyzed(
                            &analysis.analysis,
                            analysis.source_name,
                            analysis.artifact_checksum,
                        ));
                    }
                    Err(err) => {
                        let still_current = matches!(
                            state.peek(),
                            Some((current, _)) if current == generation
                        );
                        if !still_current {
                            // The failure belongs to a superseded gesture
                            // (replaced or discarded mid-read) — it is not
                            // the answer; settle on the current state.
                            continue;
                        }
                        // The intent SURVIVES a transport failure so
                        // `Réessayer` can replay it.
                        return Err(err);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    fn strings(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn offer_discards_flags_and_empty_arguments() {
        let state = OsOpenState::new();
        let offered = state.offer(&strings(&["--flag", "-v", ""]), Path::new("/tmp"));
        assert!(!offered, "flags/empties alone must yield no intent");
        assert_eq!(state.peek(), None);
    }

    #[test]
    fn offer_with_zero_candidates_is_a_no_op_that_keeps_a_pending_intent() {
        let state = OsOpenState::new();
        state.offer(&strings(&["/tmp/a.rustory"]), Path::new("/tmp"));
        // A second launch with no file argument (a plain app relaunch)
        // must NOT wipe the still-unconsumed intent.
        let offered = state.offer(&strings(&["--verbose"]), Path::new("/elsewhere"));
        assert!(!offered);
        let (_, intent) = state.peek().expect("intent survives");
        assert_eq!(
            intent,
            OsOpenIntent::Artifact(PathBuf::from("/tmp/a.rustory"))
        );
    }

    #[test]
    fn offer_with_one_candidate_yields_an_artifact_intent() {
        let state = OsOpenState::new();
        let offered = state.offer(
            &strings(&["--verbose", "/home/user/histoire.rustory"]),
            Path::new("/tmp"),
        );
        assert!(offered);
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(
            intent,
            OsOpenIntent::Artifact(PathBuf::from("/home/user/histoire.rustory"))
        );
    }

    #[test]
    fn offer_with_several_candidates_yields_multiple_files_with_the_count() {
        let state = OsOpenState::new();
        let offered = state.offer(
            &strings(&["/tmp/a.rustory", "/tmp/b.rustory", "/tmp/c.rustory"]),
            Path::new("/tmp"),
        );
        assert!(offered);
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(intent, OsOpenIntent::MultipleFiles(3));
    }

    #[test]
    fn offer_resolves_a_relative_path_against_the_provided_cwd() {
        let state = OsOpenState::new();
        // The second instance's OWN cwd — never this process's.
        state.offer(
            &strings(&["histoire.rustory"]),
            Path::new("/home/user/Documents"),
        );
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(
            intent,
            OsOpenIntent::Artifact(PathBuf::from("/home/user/Documents/histoire.rustory"))
        );
    }

    #[test]
    fn offer_replaces_a_pending_intent_under_a_fresh_generation() {
        let state = OsOpenState::new();
        state.offer(&strings(&["/tmp/first.rustory"]), Path::new("/tmp"));
        let (first_generation, _) = state.peek().expect("first intent");
        // The user's LAST gesture wins — the newer intent replaces, and its
        // generation moves forward (the stale settlement can no longer claim).
        state.offer(&strings(&["/tmp/second.rustory"]), Path::new("/tmp"));
        let (second_generation, intent) = state.peek().expect("second intent");
        assert_eq!(
            intent,
            OsOpenIntent::Artifact(PathBuf::from("/tmp/second.rustory"))
        );
        assert!(second_generation > first_generation);
    }

    #[cfg(unix)]
    #[test]
    fn offer_accepts_a_non_utf8_argument_without_panicking() {
        use std::os::unix::ffi::OsStringExt;
        let state = OsOpenState::new();
        // A legal Unix filename that is NOT UTF-8 (raw 0xFF byte).
        let raw = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, b'.', b'r']);
        let offered = state.offer(std::slice::from_ref(&raw), Path::new("/tmp"));
        assert!(offered, "a non-UTF-8 path is a candidate, never a panic");
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(intent, OsOpenIntent::Artifact(PathBuf::from(raw)));
    }

    #[test]
    fn peek_clones_without_consuming_and_take_if_is_one_shot() {
        let state = OsOpenState::new();
        state.offer(&strings(&["/tmp/a.rustory"]), Path::new("/tmp"));
        let (generation, _) = state.peek().expect("peek never consumes");
        assert!(state.peek().is_some(), "peek never consumes");
        assert!(state.take_if(generation), "the first claim wins");
        assert!(!state.take_if(generation), "the second claim yields false");
        assert_eq!(state.peek(), None);
    }

    #[test]
    fn take_if_refuses_a_stale_generation_and_keeps_the_newer_intent() {
        let state = OsOpenState::new();
        state.offer(&strings(&["/tmp/a.rustory"]), Path::new("/tmp"));
        let (stale_generation, _) = state.peek().expect("first intent");
        // The user opens B while A's settlement is still in flight.
        state.offer(&strings(&["/tmp/b.rustory"]), Path::new("/tmp"));
        // A's settlement finishes: its claim MUST fail — B survives intact.
        assert!(!state.take_if(stale_generation));
        let (_, intent) = state.peek().expect("the newer intent survives");
        assert_eq!(
            intent,
            OsOpenIntent::Artifact(PathBuf::from("/tmp/b.rustory"))
        );
    }

    #[test]
    fn discard_is_idempotent() {
        let state = OsOpenState::new();
        state.offer(&strings(&["/tmp/a.rustory"]), Path::new("/tmp"));
        state.discard();
        assert_eq!(state.peek(), None);
        // A second discard on an already-empty state is a silent no-op.
        state.discard();
        assert_eq!(state.peek(), None);
    }

    // ---------------- analyze_pending_intent ----------------

    #[test]
    fn analyze_answers_none_when_nothing_is_pending() {
        let state = OsOpenState::new();
        let dto = analyze_pending_intent(&state).expect("analyze");
        assert_eq!(dto, OsOpenAnalysisDto::None);
    }

    #[test]
    fn analyze_consumes_a_multiple_files_intent_into_the_frozen_copy() {
        let state = OsOpenState::new();
        state.offer(
            &strings(&["/tmp/a.rustory", "/tmp/b.rustory"]),
            Path::new("/tmp"),
        );
        let dto = analyze_pending_intent(&state).expect("analyze");
        assert_eq!(dto, OsOpenAnalysisDto::multiple_files());
        // Consumed: the next analysis is the silent no-op.
        assert_eq!(
            analyze_pending_intent(&state).expect("analyze"),
            OsOpenAnalysisDto::None
        );
    }

    #[test]
    fn analyze_takes_the_artifact_intent_after_a_successful_read() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("histoire.rustory");
        std::fs::write(&path, b"{ not a valid artifact").expect("seed");

        let state = OsOpenState::new();
        state.offer(&strings(&[path.to_str().expect("utf8")]), tmp.path());
        let dto = analyze_pending_intent(&state).expect("analyze");
        // Invalid content is a calm CONTENT VERDICT (blocked findings
        // envelope), never an error — and the source is named by basename.
        match &dto {
            OsOpenAnalysisDto::Analyzed {
                source_name,
                importable_content,
                ..
            } => {
                assert_eq!(source_name, "histoire.rustory");
                assert!(importable_content.is_none(), "blocked ⇒ no content");
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
        // take-after-success: the second call answers none (StrictMode-safe).
        assert_eq!(
            analyze_pending_intent(&state).expect("analyze"),
            OsOpenAnalysisDto::None
        );
    }

    /// THE central race of the channel, deterministically staged through
    /// the injected reader: the user opens B while A's file is being read.
    /// A's settlement must NEVER consume B — the loop drops A's stale
    /// verdict and serves B, and nothing is lost.
    #[test]
    fn a_read_settling_after_a_newer_gesture_serves_the_newest_intent() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path_a = tmp.path().join("a.rustory");
        let path_b = tmp.path().join("b.rustory");
        std::fs::write(&path_a, b"{ contenu A").expect("seed a");
        std::fs::write(&path_b, b"{ contenu B").expect("seed b");

        let state = OsOpenState::new();
        state.offer(&strings(&[path_a.to_str().expect("utf8")]), tmp.path());

        // The reader plays the barrier: WHILE A is being read, B lands.
        let b_arg = strings(&[path_b.to_str().expect("utf8")]);
        let dto = analyze_pending_intent_with(&state, |path| {
            if path == path_a {
                state.offer(&b_arg, tmp.path());
            }
            read_artifact_bounded(path)
        })
        .expect("analyze");

        match &dto {
            OsOpenAnalysisDto::Analyzed { source_name, .. } => {
                assert_eq!(
                    source_name, "b.rustory",
                    "the settlement serves the NEWEST gesture, never the stale one"
                );
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
        // B was consumed by its own settlement — nothing is pending, and A's
        // stale verdict was dropped, never surfaced.
        assert_eq!(
            analyze_pending_intent(&state).expect("analyze"),
            OsOpenAnalysisDto::None
        );
    }

    /// The failure sibling of the race above: A's read FAILS while B has
    /// already replaced it — the stale failure is not the answer, B is.
    #[test]
    fn a_stale_read_failure_is_dropped_when_a_newer_gesture_landed_mid_read() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let missing_a = tmp.path().join("disparue.rustory");
        let path_b = tmp.path().join("b.rustory");
        std::fs::write(&path_b, b"{ contenu B").expect("seed b");

        let state = OsOpenState::new();
        state.offer(&strings(&[missing_a.to_str().expect("utf8")]), tmp.path());

        let b_arg = strings(&[path_b.to_str().expect("utf8")]);
        let dto = analyze_pending_intent_with(&state, |path| {
            if path == missing_a {
                state.offer(&b_arg, tmp.path());
            }
            read_artifact_bounded(path)
        })
        .expect("the stale failure must not surface — B analyzes");

        match &dto {
            OsOpenAnalysisDto::Analyzed { source_name, .. } => {
                assert_eq!(source_name, "b.rustory");
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
    }

    #[test]
    fn analyze_keeps_the_intent_pending_on_a_read_failure() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let missing = tmp.path().join("disparue.rustory");

        let state = OsOpenState::new();
        state.offer(&strings(&[missing.to_str().expect("utf8")]), tmp.path());
        let err = analyze_pending_intent(&state).expect_err("read must fail");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        // The intent SURVIVES the transport failure — `Réessayer` replays it.
        let (_, intent) = state.peek().expect("intent still pending");
        assert_eq!(intent, OsOpenIntent::Artifact(missing.clone()));

        // Replay after the file reappears: the SAME intent now analyzes.
        std::fs::write(&missing, b"{}").expect("seed");
        let dto = analyze_pending_intent(&state).expect("analyze");
        assert!(matches!(dto, OsOpenAnalysisDto::Analyzed { .. }));
        assert_eq!(state.peek(), None, "consumed after the successful replay");
    }

    /// A discard landing while the file is being read (the user closed the
    /// failure and abandoned): the settlement finds the slot empty and
    /// resolves `none` — never a resurrected verdict from the Rust side.
    #[test]
    fn a_discard_landing_mid_read_settles_as_none() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("a.rustory");
        std::fs::write(&path, b"{ contenu").expect("seed");

        let state = OsOpenState::new();
        state.offer(&strings(&[path.to_str().expect("utf8")]), tmp.path());
        let dto = analyze_pending_intent_with(&state, |p| {
            state.discard();
            read_artifact_bounded(p)
        })
        .expect("analyze");
        assert_eq!(dto, OsOpenAnalysisDto::None);
    }

    #[cfg(unix)]
    #[test]
    fn analyze_serves_a_non_utf8_path_with_the_placeholder_basename() {
        use std::os::unix::ffi::OsStringExt;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        // A legal Unix filename that is NOT UTF-8 — creatable and readable.
        let mut raw_name = tmp.path().as_os_str().to_os_string();
        raw_name.push(OsString::from_vec(vec![b'/', 0xff, 0xfe, b'.', b'r']));
        let path = PathBuf::from(&raw_name);
        std::fs::write(&path, b"{}").expect("seed non-utf8 file");

        let state = OsOpenState::new();
        state.offer(&[raw_name], Path::new("/"));
        let dto = analyze_pending_intent(&state).expect("no panic, a calm verdict");
        match &dto {
            OsOpenAnalysisDto::Analyzed {
                source_name,
                importable_content,
                ..
            } => {
                // The unnameable basename degrades to the sober placeholder
                // (never a lossy leak), and the non-`.rustory` name is a
                // blocked verdict — AC2's clear message, not a crash.
                assert_eq!(source_name, "artefact");
                assert!(importable_content.is_none());
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
    }

    #[test]
    fn analyze_recognizes_a_clean_artifact_end_to_end() {
        use crate::domain::export::{
            ArtifactEnvelopeV1, ExportedStoryV1, RustoryArtifactV1, RUSTORY_ARTIFACT_FORMAT_VERSION,
        };
        use crate::domain::story::content_checksum;

        const CANONICAL_STRUCTURE: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";
        let artifact = RustoryArtifactV1 {
            rustory_artifact: ArtifactEnvelopeV1 {
                format_version: RUSTORY_ARTIFACT_FORMAT_VERSION,
                exported_at: "2026-06-27T10:00:00.000Z".into(),
                exported_by: "rustory/0.1.0".into(),
            },
            story: ExportedStoryV1 {
                schema_version: 3,
                title: "Le Soleil".into(),
                structure_json: CANONICAL_STRUCTURE.into(),
                content_checksum: content_checksum(CANONICAL_STRUCTURE),
                created_at: "2026-06-20T10:00:00.000Z".into(),
                updated_at: "2026-06-24T14:15:00.000Z".into(),
            },
        };
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let path = tmp.path().join("soleil.rustory");
        std::fs::write(&path, artifact.to_canonical_json().expect("ser")).expect("seed");

        let state = OsOpenState::new();
        state.offer(&strings(&[path.to_str().expect("utf8")]), tmp.path());
        let dto = analyze_pending_intent(&state).expect("analyze");
        match dto {
            OsOpenAnalysisDto::Analyzed {
                importable_content,
                source_name,
                artifact_checksum,
                ..
            } => {
                assert_eq!(source_name, "soleil.rustory");
                assert_eq!(artifact_checksum.len(), 64);
                let content = importable_content.expect("clean ⇒ importable");
                assert_eq!(content.title, "Le Soleil");
            }
            other => panic!("expected analyzed, got {other:?}"),
        }
    }
}
