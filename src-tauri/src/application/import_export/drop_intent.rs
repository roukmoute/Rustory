//! Drop intent state — the application-side receiver of "file or folder
//! dropped on the window" gestures forwarded by the native drag-drop
//! handler (`WindowEvent::DragDrop`, captured at the `lib.rs` frontier).
//!
//! Design (see `ui-states.md#Drop Intent Contract`):
//!
//! - The intent (an absolute `PathBuf`) is held HERE, Rust-side, in ITS
//!   OWN slot — never shared with the OS-open intent (`os_open.rs`): a
//!   drop never replaces a pending open intent and vice versa. The slot
//!   DUPLICATES the `PendingSlot` + monotonic-generation + compare-and-take
//!   pattern locally (the generic extraction waits for a third channel,
//!   per project rule).
//! - Offering has NO argv semantics: the native drag-drop hands absolute,
//!   clean paths — no cwd resolution, no `-`-prefix filtering (a dropped
//!   file named `-histoire.rustory` is a legitimate candidate), no
//!   `file://` conversion. Only EMPTY paths are discarded.
//! - ONE pending intent at a time; a newer offer REPLACES it (the user's
//!   last gesture wins — within THIS channel). Several elements in ONE
//!   gesture become a single `MultipleItems` intent — NOTHING is
//!   processed, neither the first element nor the rest.
//! - Classification happens at ANALYSIS time via `fs::metadata` (which
//!   follows symlinks, like a picker — the no-follow hardening joins the
//!   known "no-follow parity" workstream): a regular FILE routes into the
//!   unchanged artifact-import analysis, a FOLDER into the unchanged
//!   structured-folder analysis. No new pipeline, no new verdict.
//! - NO Tauri dependency in this module: every decision (filtering,
//!   replacement, generations, classification, verdicts) is a pure
//!   function over an instantiable [`DropIntentState`], fully
//!   unit-testable. The `lib.rs` frontier stays a thin glue (emit/offer).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::domain::shared::AppError;
use crate::ipc::dto::import_export::DropAnalysisDto;

use super::import::{analyze_artifact, file_read_error, read_artifact_bounded};
use super::structured_creation::{
    analyze_structured_folder, non_filesystem_path_error, StructuredCreationOutcome,
};

/// One pending drop intent, as filtered from the dropped paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropIntent {
    /// Exactly one dropped element — classified (file vs folder) at
    /// analysis time, never at drop time.
    Item(PathBuf),
    /// Several elements in ONE gesture — a calm named limit; nothing is
    /// partially processed. The count names the gesture's size (asserted
    /// by the filtering tests) — the wire verdict carries the frozen copy
    /// alone.
    MultipleItems(usize),
}

/// The (at most one) pending intent plus the monotonic generation of the
/// LAST offer — the identity a settling analysis must present to consume.
#[derive(Debug, Default)]
struct PendingSlot {
    generation: u64,
    intent: Option<DropIntent>,
}

/// The Rust-owned holder of the (at most one) pending drop intent.
///
/// Instantiable so unit tests work on LOCAL instances — the process-wide
/// [`DROP_INTENT_STATE`] global is consumed only by the Tauri wiring and
/// the commands. The mutex is only ever held for the duration of a field
/// swap — never across an `.await`, never around file I/O.
#[derive(Debug, Default)]
pub struct DropIntentState {
    pending: Mutex<PendingSlot>,
}

/// Process-wide intent state consumed by the Tauri wiring (`lib.rs`) and
/// the `analyze_drop_request` / `discard_drop_request` commands.
pub static DROP_INTENT_STATE: DropIntentState = DropIntentState::new();

impl DropIntentState {
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(PendingSlot {
                generation: 0,
                intent: None,
            }),
        }
    }

    /// Filter the dropped paths into at most one intent and, when one
    /// results, store it — REPLACING any pending intent (the user's last
    /// gesture is the true intent) under a FRESH generation. Returns
    /// `true` iff an intent resulted (the wiring then signals the front).
    ///
    /// NO argv semantics (the real difference with the OS-open `offer`):
    /// the native drag-drop hands absolute, clean paths — only EMPTY
    /// paths are discarded (a `-`-prefixed name is a legitimate
    /// candidate); 0 candidates → no-op (a pending intent survives);
    /// 1 candidate → [`DropIntent::Item`]; ≥ 2 candidates →
    /// [`DropIntent::MultipleItems`] (no partial processing).
    pub fn offer_dropped(&self, paths: Vec<PathBuf>) -> bool {
        let mut candidates: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| !path.as_os_str().is_empty())
            .collect();

        let intent = match candidates.len() {
            0 => return false,
            1 => DropIntent::Item(candidates.remove(0)),
            n => DropIntent::MultipleItems(n),
        };
        let mut slot = self.lock();
        slot.generation += 1;
        slot.intent = Some(intent);
        true
    }

    /// Clone the pending intent (with its generation) without consuming it.
    pub fn peek(&self) -> Option<(u64, DropIntent)> {
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

/// Analyze the pending drop intent into its typed wire verdict — the WHOLE
/// decision logic of the `analyze_drop_request` command, kept out of the
/// Tauri handler so it stays unit-testable on a local state.
///
/// Synchronous by design (the command drives it on a `spawn_blocking`
/// worker): the classification and the bounded reads happen here, and no
/// `MutexGuard` ever crosses an `.await` (the state locks are inside
/// `peek`/`take_if`).
pub fn analyze_pending_drop(state: &DropIntentState) -> Result<DropAnalysisDto, AppError> {
    analyze_pending_drop_with(state, read_artifact_bounded, analyze_structured_folder)
}

/// [`analyze_pending_drop`] parameterized by the file reader AND the
/// folder analyzer, so the mid-read races (a newer gesture landing while
/// an element is being read, a folder vanishing mid-analysis) are
/// DETERMINISTICALLY testable: the test's reader/analyzer stages the race
/// before delegating.
///
/// The loop settles on the NEWEST intent, never a stale one:
/// - No pending intent → [`DropAnalysisDto::None`] (total silent no-op).
/// - `MultipleItems` → claimed by generation + the frozen calm-limit copy.
/// - `Item` → classified by `fs::metadata` (follows symlinks, like a
///   picker): a regular FILE goes through the bounded read then the
///   UNCHANGED [`analyze_artifact`] pipeline (same authority, same
///   findings — the `artifact` settlement); a FOLDER requires a UTF-8
///   path (the wire round-trip to the accept — otherwise the honest
///   `non_filesystem_path` refusal of the folder flow, the picker's exact
///   error) then the UNCHANGED [`analyze_structured_folder`] (the
///   `folder` settlement); anything else (missing, FIFO, device) is the
///   existing `file_read` transport regime. If a NEWER intent replaced
///   this one mid-read, the stale outcome (verdict OR failure) is DROPPED
///   and the loop re-analyzes the newest intent — the user's last gesture
///   wins. A failure with the intent still current LEAVES IT PENDING and
///   rejects — `Réessayer` replays the same intent. A verdict claims the
///   intent before returning (one-shot).
fn analyze_pending_drop_with(
    state: &DropIntentState,
    read: impl Fn(&Path) -> Result<Vec<u8>, AppError>,
    analyze_folder: impl Fn(&Path) -> Result<StructuredCreationOutcome, AppError>,
) -> Result<DropAnalysisDto, AppError> {
    loop {
        let Some((generation, intent)) = state.peek() else {
            return Ok(DropAnalysisDto::None);
        };
        let path = match intent {
            DropIntent::MultipleItems(_) => {
                if state.take_if(generation) {
                    return Ok(DropAnalysisDto::multiple_items());
                }
                // A newer gesture replaced it between peek and claim —
                // analyze THAT one instead.
                continue;
            }
            DropIntent::Item(path) => path,
        };

        // Classification at ANALYSIS time. `fs::metadata` FOLLOWS symlinks
        // (picker parity — the no-follow hardening is a named deferred
        // workstream); its failure (element gone, permissions) is the
        // exact transport regime `read_artifact_bounded` would have
        // produced, so `Réessayer` replays the same intent either way.
        let outcome: Result<DropAnalysisDto, AppError> = match std::fs::metadata(&path) {
            Err(_) => Err(file_read_error("metadata")),
            Ok(meta) if meta.is_dir() => {
                analyze_dropped_folder(state, generation, &path, &analyze_folder)
            }
            Ok(meta) if meta.is_file() => analyze_dropped_file(state, generation, &path, &read),
            // Neither file nor folder (FIFO, device, socket): the same
            // refusal the bounded read enforces, decided BEFORE any open.
            Ok(_) => Err(file_read_error("not_regular_file")),
        };
        match outcome {
            Ok(DropAnalysisDto::None) => continue,
            Ok(settled) => return Ok(settled),
            Err(err) => {
                let still_current = matches!(
                    state.peek(),
                    Some((current, _)) if current == generation
                );
                if !still_current {
                    // The failure belongs to a superseded gesture
                    // (replaced or discarded mid-read) — it is not the
                    // answer; settle on the current state.
                    continue;
                }
                // The intent SURVIVES a transport failure so `Réessayer`
                // can replay it.
                return Err(err);
            }
        }
    }
}

/// Settle a dropped regular FILE: bounded read + the unchanged artifact
/// analysis. Returns `Ok(None)` when the claim lost to a newer gesture
/// (the caller's loop then re-serves the newest intent).
fn analyze_dropped_file(
    state: &DropIntentState,
    generation: u64,
    path: &Path,
    read: &impl Fn(&Path) -> Result<Vec<u8>, AppError>,
) -> Result<DropAnalysisDto, AppError> {
    // Carry the BASENAME only across the boundary — never the absolute
    // path (PII). Falls back to a sober placeholder for a non-UTF-8 /
    // unnameable path (the dialog-import pattern) — never a forced
    // conversion, never a panic.
    let source_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artefact")
        .to_string();
    let bytes = read(path)?;
    if !state.take_if(generation) {
        // The user's newer gesture wins — drop this stale verdict and let
        // the loop serve the newest intent.
        return Ok(DropAnalysisDto::None);
    }
    let analysis = analyze_artifact(&bytes, source_name);
    Ok(DropAnalysisDto::artifact(
        &analysis.analysis,
        analysis.source_name,
        analysis.artifact_checksum,
    ))
}

/// Settle a dropped FOLDER: UTF-8 wire path + the unchanged structured
/// folder analysis. Returns `Ok(None)` when the claim lost to a newer
/// gesture (the caller's loop then re-serves the newest intent).
fn analyze_dropped_folder(
    state: &DropIntentState,
    generation: u64,
    path: &Path,
    analyze_folder: &impl Fn(&Path) -> Result<StructuredCreationOutcome, AppError>,
) -> Result<DropAnalysisDto, AppError> {
    // The wire is UTF-8 JSON: a non-UTF-8 path cannot round-trip VERBATIM
    // to the accept phase (a lossy conversion would re-analyze a DIFFERENT
    // folder). Refused at the boundary rather than silently altered — the
    // picker's exact error.
    let folder_path = path
        .to_str()
        .map(str::to_string)
        .ok_or_else(non_filesystem_path_error)?;
    let outcome = analyze_folder(path)?;
    // TOCTOU requalification: a folder that VANISHED (or changed type)
    // between the classification and this point folds into a calm
    // "no readable manifest" envelope verdict — which would then be
    // CONSUMED, lying about a folder that no longer exists. Re-check the
    // classification fact and requalify into the replayable transport
    // regime instead (the intent stays pending, `Réessayer` re-reads the
    // real disk). A folder still present keeps its verdict — a genuinely
    // manifestless folder stays the calm content verdict it is.
    match std::fs::metadata(path) {
        Err(_) => return Err(file_read_error("metadata")),
        Ok(meta) if !meta.is_dir() => return Err(file_read_error("not_regular_file")),
        Ok(_) => {}
    }
    if !state.take_if(generation) {
        return Ok(DropAnalysisDto::None);
    }
    Ok(DropAnalysisDto::folder(
        &outcome.analysis,
        outcome.folder_name,
        folder_path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;
    use tempfile::TempDir;

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    // ---------------- offer_dropped ----------------

    #[test]
    fn offer_discards_empty_paths() {
        let state = DropIntentState::new();
        let offered = state.offer_dropped(vec![PathBuf::new(), PathBuf::new()]);
        assert!(!offered, "empty paths alone must yield no intent");
        assert_eq!(state.peek(), None);
    }

    #[test]
    fn offer_with_zero_candidates_is_a_no_op_that_keeps_a_pending_intent() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/a.rustory"]));
        // An exotic empty drop must NOT wipe the still-unconsumed intent.
        let offered = state.offer_dropped(vec![PathBuf::new()]);
        assert!(!offered);
        let (_, intent) = state.peek().expect("intent survives");
        assert_eq!(intent, DropIntent::Item(PathBuf::from("/tmp/a.rustory")));
    }

    #[test]
    fn offer_with_one_candidate_yields_an_item_intent() {
        let state = DropIntentState::new();
        let offered = state.offer_dropped(paths(&["/home/user/histoire.rustory"]));
        assert!(offered);
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(
            intent,
            DropIntent::Item(PathBuf::from("/home/user/histoire.rustory"))
        );
    }

    #[test]
    fn offer_accepts_a_dash_prefixed_name_as_a_legitimate_candidate() {
        // NO argv semantics: a dropped file named like a flag is a real
        // file the user pointed at — never filtered.
        let state = DropIntentState::new();
        let offered = state.offer_dropped(paths(&["/tmp/-histoire.rustory"]));
        assert!(offered, "a dash-prefixed dropped path is a candidate");
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(
            intent,
            DropIntent::Item(PathBuf::from("/tmp/-histoire.rustory"))
        );
    }

    #[test]
    fn offer_with_several_candidates_yields_multiple_items_with_the_count() {
        let state = DropIntentState::new();
        let offered = state.offer_dropped(paths(&["/tmp/a.rustory", "/tmp/b", "/tmp/c.png"]));
        assert!(offered);
        let (_, intent) = state.peek().expect("intent present");
        assert_eq!(intent, DropIntent::MultipleItems(3));
    }

    #[test]
    fn offer_replaces_a_pending_intent_under_a_fresh_generation() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/first.rustory"]));
        let (first_generation, _) = state.peek().expect("first intent");
        // The user's LAST gesture wins — the newer intent replaces, and its
        // generation moves forward (the stale settlement can no longer claim).
        state.offer_dropped(paths(&["/tmp/second.rustory"]));
        let (second_generation, intent) = state.peek().expect("second intent");
        assert_eq!(
            intent,
            DropIntent::Item(PathBuf::from("/tmp/second.rustory"))
        );
        assert!(second_generation > first_generation);
    }

    #[test]
    fn peek_clones_without_consuming_and_take_if_is_one_shot() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/a.rustory"]));
        let (generation, _) = state.peek().expect("peek never consumes");
        assert!(state.peek().is_some(), "peek never consumes");
        assert!(state.take_if(generation), "the first claim wins");
        assert!(!state.take_if(generation), "the second claim yields false");
        assert_eq!(state.peek(), None);
    }

    #[test]
    fn take_if_refuses_a_stale_generation_and_keeps_the_newer_intent() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/a.rustory"]));
        let (stale_generation, _) = state.peek().expect("first intent");
        // The user drops B while A's settlement is still in flight.
        state.offer_dropped(paths(&["/tmp/b.rustory"]));
        // A's settlement finishes: its claim MUST fail — B survives intact.
        assert!(!state.take_if(stale_generation));
        let (_, intent) = state.peek().expect("the newer intent survives");
        assert_eq!(intent, DropIntent::Item(PathBuf::from("/tmp/b.rustory")));
    }

    #[test]
    fn discard_is_idempotent() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/a.rustory"]));
        state.discard();
        assert_eq!(state.peek(), None);
        // A second discard on an already-empty state is a silent no-op.
        state.discard();
        assert_eq!(state.peek(), None);
    }

    // ---------------- analyze_pending_drop ----------------

    #[test]
    fn analyze_answers_none_when_nothing_is_pending() {
        let state = DropIntentState::new();
        let dto = analyze_pending_drop(&state).expect("analyze");
        assert_eq!(dto, DropAnalysisDto::None);
    }

    #[test]
    fn analyze_consumes_a_multiple_items_intent_into_the_frozen_copy() {
        let state = DropIntentState::new();
        state.offer_dropped(paths(&["/tmp/a.rustory", "/tmp/b.rustory"]));
        let dto = analyze_pending_drop(&state).expect("analyze");
        assert_eq!(dto, DropAnalysisDto::multiple_items());
        // Consumed: the next analysis is the silent no-op — NOTHING was
        // processed (neither the first element nor the rest).
        assert_eq!(
            analyze_pending_drop(&state).expect("analyze"),
            DropAnalysisDto::None
        );
    }

    #[test]
    fn analyze_classifies_a_regular_file_into_the_artifact_settlement() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("histoire.rustory");
        std::fs::write(&path, b"{ not a valid artifact").expect("seed");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path.clone()]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        // Invalid content is a calm CONTENT VERDICT (blocked findings
        // envelope), never an error — and the source is named by basename.
        match &dto {
            DropAnalysisDto::Artifact {
                source_name,
                importable_content,
                ..
            } => {
                assert_eq!(source_name, "histoire.rustory");
                assert!(importable_content.is_none(), "blocked ⇒ no content");
            }
            other => panic!("expected artifact, got {other:?}"),
        }
        // take-after-success: the second call answers none (StrictMode-safe).
        assert_eq!(
            analyze_pending_drop(&state).expect("analyze"),
            DropAnalysisDto::None
        );
    }

    #[test]
    fn analyze_serves_the_calm_envelope_verdict_for_an_unsupported_extension() {
        // A dropped media (or any non-`.rustory` file) is a calm content
        // verdict through the EXISTING findings — never a half-treatment.
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("photo.png");
        std::fs::write(&path, [0x89, b'P', b'N', b'G']).expect("seed");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        match &dto {
            DropAnalysisDto::Artifact {
                source_name,
                importable_content,
                findings,
                ..
            } => {
                assert_eq!(source_name, "photo.png");
                assert!(importable_content.is_none(), "blocked ⇒ no content");
                assert!(
                    findings
                        .iter()
                        .any(|f| f.message.contains("n'est pas un artefact Rustory valide")),
                    "the existing envelope copy names the refusal"
                );
            }
            other => panic!("expected artifact, got {other:?}"),
        }
    }

    #[test]
    fn analyze_classifies_a_folder_into_the_folder_settlement() {
        let tmp = TempDir::new().expect("tempdir");
        let folder = tmp.path().join("mon-histoire");
        std::fs::create_dir(&folder).expect("mkdir");
        std::fs::write(
            folder.join("histoire.json"),
            r#"{ "formatVersion": 1, "title": "Le voyage", "nodes": [ { "id": "n1", "text": "Début" } ] }"#,
        )
        .expect("manifest");

        let state = DropIntentState::new();
        state.offer_dropped(vec![folder.clone()]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        match &dto {
            DropAnalysisDto::Folder {
                folder_name,
                folder_path,
                creatable_summary,
                ..
            } => {
                assert_eq!(folder_name, "mon-histoire");
                assert_eq!(
                    folder_path,
                    folder.to_str().expect("utf8"),
                    "the folderPath round-trips VERBATIM to the accept"
                );
                let summary = creatable_summary.as_ref().expect("creatable");
                assert_eq!(summary.title, "Le voyage");
            }
            other => panic!("expected folder, got {other:?}"),
        }
        // Consumed one-shot, like every settled verdict.
        assert_eq!(
            analyze_pending_drop(&state).expect("analyze"),
            DropAnalysisDto::None
        );
    }

    #[test]
    fn analyze_serves_the_folder_envelope_verdict_for_a_manifestless_folder() {
        let tmp = TempDir::new().expect("tempdir");
        let folder = tmp.path().join("dossier-vide");
        std::fs::create_dir(&folder).expect("mkdir");

        let state = DropIntentState::new();
        state.offer_dropped(vec![folder]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        match &dto {
            DropAnalysisDto::Folder {
                creatable_summary,
                findings,
                ..
            } => {
                assert!(creatable_summary.is_none(), "blocked ⇒ nothing creatable");
                assert!(
                    findings
                        .iter()
                        .any(|f| f.message.contains("manifest histoire.json")),
                    "the existing folder envelope copy names the refusal"
                );
            }
            other => panic!("expected folder, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn analyze_follows_a_symlink_to_a_file_into_the_artifact_settlement() {
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("cible.rustory");
        std::fs::write(&target, b"{").expect("seed");
        let link = tmp.path().join("lien.rustory");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        let state = DropIntentState::new();
        state.offer_dropped(vec![link]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        match &dto {
            DropAnalysisDto::Artifact { source_name, .. } => {
                // Named by the LINK's basename (what the user dropped).
                assert_eq!(source_name, "lien.rustory");
            }
            other => panic!("expected artifact, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn analyze_follows_a_symlink_to_a_folder_into_the_folder_settlement() {
        let tmp = TempDir::new().expect("tempdir");
        let target = tmp.path().join("vrai-dossier");
        std::fs::create_dir(&target).expect("mkdir");
        let link = tmp.path().join("lien-dossier");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        let state = DropIntentState::new();
        state.offer_dropped(vec![link]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        // The manifestless folder envelope verdict proves the FOLDER route
        // was taken through the symlink (metadata follows it).
        assert!(
            matches!(&dto, DropAnalysisDto::Folder { .. }),
            "expected folder, got {dto:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn analyze_refuses_a_non_utf8_folder_path_honestly() {
        use std::os::unix::ffi::OsStringExt;
        let tmp = TempDir::new().expect("tempdir");
        let mut raw_name = tmp.path().as_os_str().to_os_string();
        raw_name.push(std::ffi::OsString::from_vec(vec![b'/', 0xff, 0xfe]));
        let folder = PathBuf::from(&raw_name);
        std::fs::create_dir(&folder).expect("mkdir non-utf8 folder");

        let state = DropIntentState::new();
        state.offer_dropped(vec![folder]);
        // The wire is UTF-8 JSON: the folderPath cannot round-trip — the
        // honest refusal of the folder flow, the picker's exact error.
        let err = analyze_pending_drop(&state).expect_err("non-UTF-8 must refuse");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], "non_filesystem_path");
    }

    #[test]
    fn analyze_keeps_the_intent_pending_on_a_read_failure() {
        let tmp = TempDir::new().expect("tempdir");
        let missing = tmp.path().join("disparue.rustory");

        let state = DropIntentState::new();
        state.offer_dropped(vec![missing.clone()]);
        let err = analyze_pending_drop(&state).expect_err("read must fail");
        assert_eq!(err.code, AppErrorCode::ImportFailed);
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        // The intent SURVIVES the transport failure — `Réessayer` replays it.
        let (_, intent) = state.peek().expect("intent still pending");
        assert_eq!(intent, DropIntent::Item(missing.clone()));

        // Replay after the file reappears: the SAME intent now analyzes.
        std::fs::write(&missing, b"{}").expect("seed");
        let dto = analyze_pending_drop(&state).expect("analyze");
        assert!(matches!(dto, DropAnalysisDto::Artifact { .. }));
        assert_eq!(state.peek(), None, "consumed after the successful replay");
    }

    /// THE central race of the channel, deterministically staged through
    /// the injected reader: the user drops B while A's file is being read.
    /// A's settlement must NEVER consume B — the loop drops A's stale
    /// verdict and serves B, and nothing is lost.
    #[test]
    fn a_read_settling_after_a_newer_gesture_serves_the_newest_intent() {
        let tmp = TempDir::new().expect("tempdir");
        let path_a = tmp.path().join("a.rustory");
        let path_b = tmp.path().join("b.rustory");
        std::fs::write(&path_a, b"{ contenu A").expect("seed a");
        std::fs::write(&path_b, b"{ contenu B").expect("seed b");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path_a.clone()]);

        // The reader plays the barrier: WHILE A is being read, B lands.
        let dto = analyze_pending_drop_with(
            &state,
            |path| {
                if path == path_a {
                    state.offer_dropped(vec![path_b.clone()]);
                }
                read_artifact_bounded(path)
            },
            analyze_structured_folder,
        )
        .expect("analyze");

        match &dto {
            DropAnalysisDto::Artifact { source_name, .. } => {
                assert_eq!(
                    source_name, "b.rustory",
                    "the settlement serves the NEWEST gesture, never the stale one"
                );
            }
            other => panic!("expected artifact, got {other:?}"),
        }
        // B was consumed by its own settlement — nothing is pending, and A's
        // stale verdict was dropped, never surfaced.
        assert_eq!(
            analyze_pending_drop(&state).expect("analyze"),
            DropAnalysisDto::None
        );
    }

    /// The failure sibling of the race above: A's read FAILS while B has
    /// already replaced it — the stale failure is not the answer, B is.
    /// A exists (the classification passes) so the race is staged in the
    /// injected reader: B lands mid-read, then A's read fails.
    #[test]
    fn a_stale_read_failure_is_dropped_when_a_newer_gesture_landed_mid_read() {
        let tmp = TempDir::new().expect("tempdir");
        let path_a = tmp.path().join("a.rustory");
        let path_b = tmp.path().join("b.rustory");
        std::fs::write(&path_a, b"{ contenu A").expect("seed a");
        std::fs::write(&path_b, b"{ contenu B").expect("seed b");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path_a.clone()]);

        let dto = analyze_pending_drop_with(
            &state,
            |path| {
                if path == path_a {
                    state.offer_dropped(vec![path_b.clone()]);
                    return Err(file_read_error("read"));
                }
                read_artifact_bounded(path)
            },
            analyze_structured_folder,
        )
        .expect("the stale failure must not surface — B analyzes");

        match &dto {
            DropAnalysisDto::Artifact { source_name, .. } => {
                assert_eq!(source_name, "b.rustory");
            }
            other => panic!("expected artifact, got {other:?}"),
        }
    }

    /// A discard landing while the file is being read (the user closed the
    /// failure and abandoned): the settlement finds the slot empty and
    /// resolves `none` — never a resurrected verdict from the Rust side.
    #[test]
    fn a_discard_landing_mid_read_settles_as_none() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("a.rustory");
        std::fs::write(&path, b"{ contenu").expect("seed");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path]);
        let dto = analyze_pending_drop_with(
            &state,
            |p| {
                state.discard();
                read_artifact_bounded(p)
            },
            analyze_structured_folder,
        )
        .expect("analyze");
        assert_eq!(dto, DropAnalysisDto::None);
    }

    /// The folder sibling of the TOCTOU defenses: a folder that VANISHES
    /// between the classification and the analysis must be the replayable
    /// TRANSPORT regime — never a calm "no readable manifest" verdict
    /// (which would be CONSUMED, lying about a folder that no longer
    /// exists). Staged deterministically through the injected analyzer.
    #[test]
    fn a_folder_vanishing_mid_analysis_requalifies_into_replayable_transport() {
        let tmp = TempDir::new().expect("tempdir");
        let folder = tmp.path().join("ephemere");
        std::fs::create_dir(&folder).expect("mkdir");
        std::fs::write(
            folder.join("histoire.json"),
            r#"{ "formatVersion": 1, "title": "Éphémère", "nodes": [ { "id": "n1" } ] }"#,
        )
        .expect("manifest");

        let state = DropIntentState::new();
        state.offer_dropped(vec![folder.clone()]);

        // The analyzer stages the race: the folder vanishes mid-analysis
        // (the real analyzer then reads it as envelope-blocked).
        let err = analyze_pending_drop_with(&state, read_artifact_bounded, |p| {
            std::fs::remove_dir_all(p).expect("vanish");
            analyze_structured_folder(p)
        })
        .expect_err("a vanished folder is transport, never a calm verdict");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], "metadata");
        // The intent SURVIVES — `Réessayer` re-reads the real disk.
        let (_, intent) = state.peek().expect("intent still pending");
        assert_eq!(intent, DropIntent::Item(folder));
    }

    /// The type-swap sibling: the folder is REPLACED BY A FILE
    /// mid-analysis — same requalification (transport, intent pending);
    /// the replay then re-classifies the path as the file it now is.
    #[test]
    fn a_folder_swapped_for_a_file_mid_analysis_requalifies_into_transport() {
        let tmp = TempDir::new().expect("tempdir");
        let folder = tmp.path().join("mue.rustory");
        std::fs::create_dir(&folder).expect("mkdir");

        let state = DropIntentState::new();
        state.offer_dropped(vec![folder.clone()]);

        let err = analyze_pending_drop_with(&state, read_artifact_bounded, |p| {
            let outcome = analyze_structured_folder(p);
            std::fs::remove_dir_all(p).expect("remove");
            std::fs::write(p, b"{}").expect("swap for a file");
            outcome
        })
        .expect_err("a type-swapped folder is transport, never a calm verdict");
        let v = serde_json::to_value(&err).expect("ser");
        assert_eq!(v["details"]["source"], "file_read");
        assert_eq!(v["details"]["stage"], "not_regular_file");
        // The intent SURVIVES — the replay re-classifies the NEW disk
        // state (a regular file → the artifact settlement).
        let dto = analyze_pending_drop(&state).expect("replay analyzes the file");
        assert!(
            matches!(&dto, DropAnalysisDto::Artifact { .. }),
            "expected artifact after the swap, got {dto:?}"
        );
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
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("soleil.rustory");
        std::fs::write(&path, artifact.to_canonical_json().expect("ser")).expect("seed");

        let state = DropIntentState::new();
        state.offer_dropped(vec![path]);
        let dto = analyze_pending_drop(&state).expect("analyze");
        match dto {
            DropAnalysisDto::Artifact {
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
            other => panic!("expected artifact, got {other:?}"),
        }
    }
}
