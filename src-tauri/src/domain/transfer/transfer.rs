//! Story-transfer domain (pure, framework-free).
//!
//! The write counterpart of the preparation model: preparation (story 3.x)
//! assembles LOCALLY "what a transfer would need"; this module owns the PURE
//! rules of writing it back to the device — deriving the `.content/<SHORT_ID>`
//! folder name, turning a prepared descriptor into a write plan, the idempotent
//! `.pi` index mutation, cohort coherence, and the closed set of functional
//! transfer-failure causes. No I/O, no `infrastructure/`, no `tauri::*`: the
//! infrastructure writer performs the safe/atomic write, the application layer
//! orchestrates the job, and the IPC layer maps these types to wire DTOs.
//!
//! Decision reminders (see `docs/architecture/ui-states.md#Story Transfer
//! Contract`): the MVP write is the round-trip of an IMPORTED story — the opaque
//! pack bytes are re-written verbatim, never decrypted, never invented. A native
//! story (canonical structure only, no device-format pack) is NOT transferable.
//! A functional failure is the terminal `retryable` state of the job (NOT an
//! `AppError`); each cause maps to one canonical FR `message` + `userAction`.

use crate::domain::device::{
    is_canonical_pack_uuid, parse_pack_index, DeviceFamily, LUNII_PACK_UUID_BYTES,
};
use crate::domain::story::Severity;

use super::{PreparedArtifactKind, TransferArtifactDescriptor};

/// Closed set of functional transfer-failure causes. A functional failure is the
/// terminal `retryable` state of the job (NOT an `AppError`); each cause maps to
/// one canonical FR `message` + `userAction` at the IPC layer, and to a fixed
/// severity below. Transport failures (mount/`app_data_dir` unreachable) stay
/// `AppError::TransferFailed` and are deliberately absent here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFailureCause {
    /// The connected profile is not authorized to be written (V3 / unsupported).
    /// The capability gate refuses BEFORE any device mutation (fail-closed).
    WriteNotAuthorized,
    /// The story has no fresh prepared descriptor — preparation must run first.
    NotPrepared,
    /// The story has no device-format pack (a native story, or a descriptor
    /// without any pack file) — nothing to write back in MVP.
    NotTransferable,
    /// The live re-scan no longer resolves to the requested device (unplugged /
    /// swapped / unreadable), or the prepared cohort no longer matches.
    DeviceChanged,
    /// The device refused the write (no space, I/O error, read-only volume).
    WriteRejected,
    /// The pack already present under the target `.content/<SHORT_ID>` is in a
    /// state the write-job proof cannot vouch for: a non-directory or symlinked
    /// target root; an irregular entry inside (symlink, unplanned EMPTY
    /// directory, special file); an entry whose bytes could not be read (or any
    /// unreadable I/O during the proof); or a divergent folder that cannot be
    /// ATTRIBUTED to the target UUID (not referenced by the device index, or
    /// another indexed UUID shares the target SHORT_ID). RUSTORY refuses
    /// protectively, zero device byte modified — never worded as the device
    /// refusing (that copy stays with [`WriteRejected`], which keeps the pure
    /// write-I/O failures).
    DevicePackUnprovable,
    /// The wall-clock budget was exhausted, or the operation was interrupted
    /// (device yanked mid-write, window close). The local draft is preserved.
    Interrupted,
}

impl TransferFailureCause {
    /// Stable snake_case wire/log tag — the closed identifier support greps on,
    /// never a localized message.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::WriteNotAuthorized => "write_not_authorized",
            Self::NotPrepared => "not_prepared",
            Self::NotTransferable => "not_transferable",
            Self::DeviceChanged => "device_changed",
            Self::WriteRejected => "write_rejected",
            Self::DevicePackUnprovable => "device_pack_unprovable",
            Self::Interrupted => "interrupted",
        }
    }

    /// Stable camelCase wire discriminant carried on the `job:failed` event so the
    /// UI keeps the structured cause (AC3) alongside the message + next gesture.
    /// MUST match `TransferCauseDto`'s serde representation (the IPC mirror).
    pub const fn wire_cause(self) -> &'static str {
        match self {
            Self::WriteNotAuthorized => "writeNotAuthorized",
            Self::NotPrepared => "notPrepared",
            Self::NotTransferable => "notTransferable",
            Self::DeviceChanged => "deviceChanged",
            Self::WriteRejected => "writeRejected",
            Self::DevicePackUnprovable => "devicePackUnprovable",
            Self::Interrupted => "interrupted",
        }
    }

    /// Parse the camelCase [`wire_cause`](Self::wire_cause) tag back into the
    /// closed cause — the inverse used when re-hydrating a persisted terminal from
    /// the durable `transfer_jobs` memory. `None` for any value outside the set
    /// (a drifted / corrupt stored tag the read path treats as "no memory").
    pub fn from_wire_cause(tag: &str) -> Option<Self> {
        match tag {
            "writeNotAuthorized" => Some(Self::WriteNotAuthorized),
            "notPrepared" => Some(Self::NotPrepared),
            "notTransferable" => Some(Self::NotTransferable),
            "deviceChanged" => Some(Self::DeviceChanged),
            "writeRejected" => Some(Self::WriteRejected),
            "devicePackUnprovable" => Some(Self::DevicePackUnprovable),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }

    /// Frozen severity per cause (reuses the canonical-validity vocabulary). It
    /// does NOT change the UI rendering — every transfer failure surfaces as
    /// `échec récupérable` with `Relancer` — but it labels the cause for traces
    /// and keeps the cause→severity mapping under test. `Blocking` marks a
    /// structural limit or integrity problem (a fresh transfer is needed once
    /// the cause is cleared); `Fixable` marks a problem the user can clear with
    /// a direct gesture (prepare first, re-plug the device, retry).
    pub const fn severity(self) -> Severity {
        match self {
            Self::NotPrepared | Self::DeviceChanged | Self::Interrupted => Severity::Fixable,
            // `DevicePackUnprovable` is an integrity problem of the on-device
            // content (an irregular entry a re-plug does not clear), extracted
            // from the `WriteRejected` branch — it inherits its `Blocking`.
            Self::WriteNotAuthorized
            | Self::NotTransferable
            | Self::WriteRejected
            | Self::DevicePackUnprovable => Severity::Blocking,
        }
    }

    /// Single canonical FR copy per cause: `(message, userAction)`. The SAME
    /// pair feeds the `job:failed` event (application layer) AND the `retryable`
    /// transfer DTO (IPC layer) — never two wordings for one cause. The UI
    /// renders both strings verbatim and adds the `Relancer` gesture. No
    /// technical jargon leaks (no `write`, `job`, `staging`, `payload`).
    pub const fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::WriteNotAuthorized => (
                "Envoi impossible : ce modèle de Lunii n'accepte pas encore l'envoi d'histoires.",
                "Branche une Lunii compatible puis relance l'envoi.",
            ),
            Self::NotPrepared => (
                "Envoi impossible : l'histoire n'est pas encore prête.",
                "Prépare l'histoire puis relance l'envoi.",
            ),
            Self::NotTransferable => (
                "Envoi impossible : cette histoire n'a pas de version compatible avec l'appareil.",
                "Importe une histoire depuis une Lunii pour pouvoir l'y renvoyer.",
            ),
            Self::DeviceChanged => (
                "Envoi interrompu : l'appareil connecté a changé.",
                "Rebranche la Lunii souhaitée puis relance l'envoi.",
            ),
            Self::WriteRejected => (
                "Envoi interrompu : la Lunii a refusé l'enregistrement de l'histoire.",
                "Vérifie l'espace disponible sur la Lunii puis relance l'envoi.",
            ),
            // The honest wording for a PROTECTIVE refusal: Rustory declines to
            // touch an on-device state it cannot vouch for — never "the device
            // refused" (frozen in product-language.md, Change Control).
            Self::DevicePackUnprovable => (
                "Envoi interrompu : la copie présente sur l'appareil est dans un état que Rustory ne reconnaît pas, rien n'a été modifié.",
                "Vérifie l'appareil, débranche-le puis rebranche-le, puis relance l'envoi.",
            ),
            Self::Interrupted => ("Envoi interrompu avant la fin.", "Relance l'envoi."),
        }
    }
}

/// Whether a failed transfer left the DEVICE untouched or mid-mutation — the
/// honest distinction story 3.5 surfaces. It is a property of the DEVICE (did the
/// write reach the content promotion?), ORTHOGONAL to [`TransferFailureCause`]:
/// the SAME cause can be `Failed` (refused before any byte hit the device) or
/// `Incomplete` (an I/O failure AFTER the content was promoted but before it was
/// indexed). Distinct from `état partiel` (a `verify` verdict, a later story).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferCompleteness {
    /// The device was NEVER mutated — the write was refused/interrupted before the
    /// content promotion. The existing content is intact; surfaced as the
    /// canonical `échec récupérable`.
    Failed,
    /// The write began mutating the device (content promoted) then was interrupted
    /// before the index update: the device may hold an unreferenced partial copy.
    /// Surfaced as `transfert incomplet`; a FRESH relaunch (never a hidden partial
    /// resume) restores a safe state.
    Incomplete,
}

impl TransferCompleteness {
    /// Stable snake_case wire/log tag — the closed identifier the UI branches on
    /// and traces record, never a localized message.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Incomplete => "incomplete",
        }
    }

    /// Parse the [`diagnostic_tag`](Self::diagnostic_tag) back into the closed
    /// completeness — the inverse used when re-hydrating a persisted terminal.
    /// `None` for any value outside the set.
    pub fn from_diagnostic_tag(tag: &str) -> Option<Self> {
        match tag {
            "failed" => Some(Self::Failed),
            "incomplete" => Some(Self::Incomplete),
            _ => None,
        }
    }
}

/// Classify a transfer failure by whether the DEVICE write reached its mutation
/// point. Orthogonal to the cause: `reached_device_mutation` (reported by the
/// writer — `true` only once the content promotion succeeded) is the ONLY input.
/// `cause` is accepted for call-site clarity and future-proofing but never alters
/// the result.
pub fn classify(
    _cause: TransferFailureCause,
    reached_device_mutation: bool,
) -> TransferCompleteness {
    if reached_device_mutation {
        TransferCompleteness::Incomplete
    } else {
        TransferCompleteness::Failed
    }
}

/// The verdict of the `verify` phase — the read-only re-read that PROVES what a
/// successful write CLAIMS (the NFR "no success without explicit verification").
/// Orthogonal to both [`TransferFailureCause`] (a write-phase functional cause)
/// and [`TransferCompleteness`] (whether the WRITE mutated the device): a verdict
/// describes what the RE-READ found, not how the write ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyVerdict {
    /// Presence (uuid indexed) + content present + byte fidelity (re-checksum)
    /// all confirm the write. The legitimate success: `transférée et vérifiée`.
    Verified,
    /// The device was mutated and the pack is present but NOT fully coherent
    /// (content promoted but not indexed, or a divergent re-checksum): the honest
    /// non-success `état partiel` — never a silent success. DISTINCT from
    /// [`TransferCompleteness::Incomplete`] (`transfert incomplet`, a `transfer`
    /// phase interruption).
    Partial,
    /// The re-read PROVES the write did not land (pack absent) OR verification
    /// cannot run/confirm (device gone / unreadable during `verify`): the
    /// recoverable `échec récupérable`. A reconnected relaunch re-verifies.
    Failed,
}

impl VerifyVerdict {
    /// Stable snake_case wire/log tag — the closed identifier traces record and
    /// the `job:failed` event carries, never a localized message.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }

    /// Parse the [`diagnostic_tag`](Self::diagnostic_tag) back into the closed
    /// verdict — the inverse used when re-hydrating a persisted terminal. `None`
    /// for any value outside the set.
    pub fn from_diagnostic_tag(tag: &str) -> Option<Self> {
        match tag {
            "verified" => Some(Self::Verified),
            "partial" => Some(Self::Partial),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    /// Canonical FR state label (product-language). Internal mapping anchor — the
    /// UI renders these exact words; the wire tag never reaches the user.
    pub const fn state_label(self) -> &'static str {
        match self {
            Self::Verified => "transférée et vérifiée",
            Self::Partial => "état partiel",
            Self::Failed => "échec récupérable",
        }
    }

    /// Single canonical FR `(message, userAction)` for a NON-success verdict.
    /// `Verified` carries `None` — its confirmation summary (what changed / stayed
    /// unchanged / final state) is composed from the comparison facts, not a fixed
    /// failure copy. The UI renders both strings verbatim and adds the
    /// `Relancer` / `Abandonner` gestures. No technical jargon leaks.
    pub const fn copy(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Verified => None,
            Self::Partial => Some((
                "Envoi dans un état partiel : certains éléments n'ont pas pu être confirmés sur la Lunii.",
                "Relance l'envoi pour rétablir un état sûr.",
            )),
            Self::Failed => Some((
                "La vérification de l'envoi n'a pas pu être confirmée.",
                "Rebranche la Lunii puis relance l'envoi pour vérifier le résultat.",
            )),
        }
    }
}

/// Outcome of the device-pack re-checksum during `verify`. Distinguishes a
/// READABLE divergence (the bytes were read and disagree) from an IMPOSSIBLE
/// re-checksum (the content is absent or could not be re-read) — the two must NOT
/// collapse to one "checksum failed" bool: a readable divergence is `Partial`
/// (present but incoherent), an impossible re-checksum is `Failed` (unconfirmable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumProbe {
    /// The device bytes re-checksum to the prepared baseline (byte fidelity).
    Match,
    /// The device bytes were read but disagree with the baseline.
    Diverged,
    /// The re-checksum could not be computed (content absent, or unreadable /
    /// structurally invalid) — verification is unconfirmable.
    Unavailable,
}

/// Classify the `verify` verdict from the facts the read-only re-read produces.
/// Cardinal rule (NFR): NO `Verified` without proof, and a `Partial` / `Failed`
/// is never dressed up as a success.
///
/// - `readable == false` (device gone / unreadable / not the written device) ⇒
///   cannot confirm ⇒ [`VerifyVerdict::Failed`] (a reconnected relaunch re-verifies).
/// - `content_present == false` (pack absent — incl. an index entry without its
///   content) ⇒ the write provably did not land ⇒ [`VerifyVerdict::Failed`].
/// - `checksum == Unavailable` (content present but the bytes could not be
///   re-read) ⇒ unconfirmable ⇒ [`VerifyVerdict::Failed`].
/// - `checksum == Diverged` (bytes read but disagree) ⇒ present but incoherent ⇒
///   [`VerifyVerdict::Partial`].
/// - `checksum == Match` AND `indexed` ⇒ [`VerifyVerdict::Verified`].
/// - `checksum == Match` but NOT `indexed` (content promoted, UUID absent from
///   `.pi`) ⇒ present but incoherent ⇒ [`VerifyVerdict::Partial`].
pub const fn classify_verify(
    indexed: bool,
    content_present: bool,
    checksum: ChecksumProbe,
    readable: bool,
) -> VerifyVerdict {
    if !readable {
        return VerifyVerdict::Failed;
    }
    if !content_present {
        return VerifyVerdict::Failed;
    }
    match checksum {
        ChecksumProbe::Unavailable => VerifyVerdict::Failed,
        ChecksumProbe::Diverged => VerifyVerdict::Partial,
        ChecksumProbe::Match => {
            if indexed {
                VerifyVerdict::Verified
            } else {
                VerifyVerdict::Partial
            }
        }
    }
}

/// The outcome the writer CONSTATED on the target `.content/<SHORT_ID>` — a fact
/// of the write itself, never deduced from a pre-write state (FR23). Orthogonal
/// to [`VerifyVerdict`]: every outcome — including [`ReusedIdentical`] — still
/// passes the same full `verify` phase before any success is claimed.
///
/// [`ReusedIdentical`]: WriteOutcome::ReusedIdentical
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// No pack existed under the target folder — the write created it (the
    /// historical first-send path).
    CreatedNew,
    /// The existing pack was PROVEN byte-identical to the plan (every planned
    /// file present, exact size + checksum, no extra entry): idempotent
    /// re-index only, zero content byte rewritten.
    ReusedIdentical,
    /// The existing pack diverged (missing / differing / extra files, all
    /// readable regular entries) and was atomically REPLACED by the plan's
    /// bytes — the FR23 update.
    ReplacedDivergent,
}

impl WriteOutcome {
    /// Stable snake_case diagnostics tag (`transfer.jsonl`) — never a localized
    /// message, never rendered by the UI.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::CreatedNew => "created_new",
            Self::ReusedIdentical => "reused_identical",
            Self::ReplacedDivergent => "replaced_divergent",
        }
    }
}

/// The `verified` confirmation summary (AC2/FR15/FR23), COMPOSED in Rust and
/// rendered VERBATIM by the panel (no frontend reinterpretation): what CHANGED
/// (the write outcome, named) and what stayed UNCHANGED (the other device
/// stories).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSummary {
    /// "« <Titre> » est maintenant sur la Lunii." / "… a été mise à jour sur…" /
    /// "… était déjà à jour sur…" — what changed + the final state.
    pub changed: String,
    /// "N autres histoires de l'appareil restent inchangées." — what stayed.
    pub unchanged: String,
}

/// Compose the `verified` summary lines in Rust (AC2/FR15/FR23).
/// `unchanged_count` reuses the pre-send comparison count; `family` is the FRESH
/// preflight profile's family (never a cached value) and `write_outcome` the
/// fact the writer constated. The `changed` line bifurcates: the Lunii wording
/// is the frozen historical literal for [`WriteOutcome::CreatedNew`] and the
/// FR23 update/already-up-to-date sentences otherwise; the generic "l'appareil"
/// variants are declared-without-a-live-emitter until a non-Lunii write exists
/// (the `formatSendCtaLabel` pattern). The panel renders both strings verbatim.
pub fn compose_verified_summary(
    story_title: &str,
    unchanged_count: u32,
    family: DeviceFamily,
    write_outcome: WriteOutcome,
) -> VerifiedSummary {
    let lunii = matches!(family, DeviceFamily::Lunii);
    let changed = match (write_outcome, lunii) {
        (WriteOutcome::CreatedNew, true) => {
            format!("« {story_title} » est maintenant sur la Lunii.")
        }
        (WriteOutcome::CreatedNew, false) => {
            format!("« {story_title} » est maintenant sur l'appareil.")
        }
        (WriteOutcome::ReplacedDivergent, true) => {
            format!("« {story_title} » a été mise à jour sur la Lunii.")
        }
        (WriteOutcome::ReplacedDivergent, false) => {
            format!("« {story_title} » a été mise à jour sur l'appareil.")
        }
        (WriteOutcome::ReusedIdentical, true) => {
            format!("« {story_title} » était déjà à jour sur la Lunii.")
        }
        (WriteOutcome::ReusedIdentical, false) => {
            format!("« {story_title} » était déjà à jour sur l'appareil.")
        }
    };
    let unchanged = match unchanged_count {
        0 => "Aucune autre histoire de l'appareil n'a été modifiée.".to_string(),
        1 => "1 autre histoire de l'appareil reste inchangée.".to_string(),
        n => format!("{n} autres histoires de l'appareil restent inchangées."),
    };
    VerifiedSummary { changed, unchanged }
}

/// The single canonical FR `(message, userAction)` for a failure terminal,
/// combining the cause with the device completeness. An `Incomplete` outcome
/// carries the device-state nuance (a partial copy may exist; relaunching is
/// safe) regardless of the precise cause; a `Failed` outcome defers to the
/// cause's own copy (the device is intact — no second wording for one cause). No
/// technical jargon leaks.
pub fn failure_copy(
    cause: TransferFailureCause,
    completeness: TransferCompleteness,
) -> (&'static str, &'static str) {
    match completeness {
        TransferCompleteness::Failed => cause.copy(),
        TransferCompleteness::Incomplete => (
            "Envoi incomplet : l'appareil peut contenir une copie partielle de l'histoire.",
            "Relance l'envoi pour rétablir un état sûr.",
        ),
    }
}

/// Derive the `.content/<SHORT_ID>` folder name from a canonical pack UUID: the
/// UPPERCASE last 8 hex characters (= the last four UUID bytes), mirroring the
/// device's own folder-naming convention and [`pack_short_id`]. Returns `None`
/// for a non-canonical value — a programming-error guard, since callers pass the
/// value the import recorded, which the schema keeps canonical.
///
/// [`pack_short_id`]: crate::domain::device::pack_short_id
pub fn short_id_from_pack_uuid(pack_uuid: &str) -> Option<String> {
    if !is_canonical_pack_uuid(pack_uuid) {
        return None;
    }
    Some(pack_uuid[pack_uuid.len() - 8..].to_ascii_uppercase())
}

/// Parse a canonical lowercase hyphenated UUID into its 16 raw bytes — the
/// on-device `.pi` representation that [`append_pack_uuid`] writes. `None` for a
/// non-canonical value (the same fail-closed guard as [`short_id_from_pack_uuid`]).
pub fn pack_uuid_bytes(pack_uuid: &str) -> Option<[u8; LUNII_PACK_UUID_BYTES]> {
    if !is_canonical_pack_uuid(pack_uuid) {
        return None;
    }
    let hex: Vec<u8> = pack_uuid.bytes().filter(|b| *b != b'-').collect();
    if hex.len() != LUNII_PACK_UUID_BYTES * 2 {
        return None;
    }
    let mut bytes = [0u8; LUNII_PACK_UUID_BYTES];
    for (i, slot) in bytes.iter_mut().enumerate() {
        let pair = std::str::from_utf8(&hex[i * 2..i * 2 + 2]).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

/// One file the device write must reproduce, in its pack-relative location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackWriteFile {
    pub rel_path: String,
    pub byte_len: u64,
    pub checksum: String,
}

/// The plan a device write executes: the target `.content/<SHORT_ID>` folder
/// name plus the files (references + integrity values) to reproduce there. Built
/// purely from a prepared descriptor — never the duplicated bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackWritePlan {
    pub short_id: String,
    pub files: Vec<PackWriteFile>,
}

/// Turn a prepared [`TransferArtifactDescriptor`] into a [`PackWritePlan`] for
/// the `.content/<SHORT_ID>` folder named by `short_id`.
///
/// Only [`PreparedArtifactKind::PackFile`] artifacts are written (the opaque
/// imported pack bytes). A descriptor with NO pack file — a native minimal story
/// carries only its [`PreparedArtifactKind::CanonicalStructure`] — is
/// [`TransferFailureCause::NotTransferable`]: there is no device-format pack to
/// round-trip, and MVP has no media transcoder to synthesize one.
pub fn build_write_plan(
    descriptor: &TransferArtifactDescriptor,
    short_id: &str,
) -> Result<PackWritePlan, TransferFailureCause> {
    let files: Vec<PackWriteFile> = descriptor
        .artifacts
        .iter()
        .filter(|a| a.kind == PreparedArtifactKind::PackFile)
        .map(|a| PackWriteFile {
            rel_path: a.relative_ref.clone(),
            byte_len: a.byte_len,
            checksum: a.checksum.clone(),
        })
        .collect();
    if files.is_empty() {
        return Err(TransferFailureCause::NotTransferable);
    }
    Ok(PackWritePlan {
        short_id: short_id.to_string(),
        files,
    })
}

/// Append a pack UUID's 16 bytes to a `.pi` index payload, IDEMPOTENTLY: a UUID
/// already present (as a clean 16-byte chunk) yields the payload unchanged; an
/// absent one is appended at EOF — the device's own "list of installed packs,
/// 16 bytes each, in reading order" convention. A trailing partial chunk of an
/// already-corrupt index is left untouched (we never rewrite bytes we did not
/// author). Pure: the infrastructure writer persists the returned bytes
/// atomically (temp + rename) only AFTER the pack content is safely promoted.
pub fn append_pack_uuid(pi_bytes: &[u8], uuid_bytes: &[u8; LUNII_PACK_UUID_BYTES]) -> Vec<u8> {
    let index = parse_pack_index(pi_bytes);
    if index.uuids.iter().any(|existing| existing == uuid_bytes) {
        return pi_bytes.to_vec();
    }
    let mut out = pi_bytes.to_vec();
    out.extend_from_slice(uuid_bytes);
    out
}

/// Remove a pack UUID's 16-byte chunk(s) from a `.pi` index payload — the exact
/// inverse of [`append_pack_uuid`]. A UUID present as one or more clean 16-byte
/// chunks is removed and the remaining entries keep their reading order; an
/// absent UUID yields the payload unchanged (idempotent, so a double delete is a
/// no-op). A trailing partial chunk of an already-corrupt index is preserved
/// verbatim — like the append path, we never rewrite bytes we did not author.
///
/// Pure: the infrastructure deleter persists the returned bytes atomically
/// (temp + rename) and delists the pack in the `.pi` BEFORE removing its content
/// folder — the safe inverse of the write path's "files first, index second", so
/// an interrupted delete can only orphan a content folder (harmless, sweepable),
/// never leave a `.pi` entry pointing at deleted content.
pub fn remove_pack_uuid(pi_bytes: &[u8], uuid_bytes: &[u8; LUNII_PACK_UUID_BYTES]) -> Vec<u8> {
    let index = parse_pack_index(pi_bytes);
    if !index.uuids.iter().any(|existing| existing == uuid_bytes) {
        return pi_bytes.to_vec();
    }
    let mut out = Vec::with_capacity(pi_bytes.len());
    for existing in &index.uuids {
        if existing != uuid_bytes {
            out.extend_from_slice(existing);
        }
    }
    // Preserve any trailing partial bytes we did not author (an already-corrupt
    // tail): reslice them from the ORIGINAL payload at its full-chunk boundary.
    let full_chunks_len = (pi_bytes.len() / LUNII_PACK_UUID_BYTES) * LUNII_PACK_UUID_BYTES;
    out.extend_from_slice(&pi_bytes[full_chunks_len..]);
    out
}

/// Ensure the prepared descriptor targets the cohort of the connected device.
///
/// A mismatch means the artifacts were prepared for a DIFFERENT device than the
/// one now connected (e.g. a v3-metadata pack about to land on a v6 device).
/// Treated as [`TransferFailureCause::DeviceChanged`] — the write target is not
/// the one the preparation was pinned to, so it is refused rather than written
/// blindly.
pub fn ensure_cohort_coherent(
    descriptor_cohort: &str,
    device_cohort: &str,
) -> Result<(), TransferFailureCause> {
    if descriptor_cohort == device_cohort {
        Ok(())
    } else {
        Err(TransferFailureCause::DeviceChanged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device::{format_pack_uuid, pack_short_id};
    use crate::domain::transfer::{PreparedArtifact, PREPARATION_PIPELINE_VERSION};

    const PACK_UUID: &str = "abababab-abab-abab-abab-ababfac5562d";

    fn pack_file_artifact(rel: &str, size: u64) -> PreparedArtifact {
        PreparedArtifact {
            kind: PreparedArtifactKind::PackFile,
            relative_ref: rel.into(),
            byte_len: size,
            checksum: "a".repeat(64),
        }
    }

    fn descriptor(artifacts: Vec<PreparedArtifact>) -> TransferArtifactDescriptor {
        TransferArtifactDescriptor {
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            target_cohort: "origine_v1".into(),
            pipeline_version: PREPARATION_PIPELINE_VERSION,
            artifacts,
            aggregate_checksum: "a".repeat(64),
        }
    }

    #[test]
    fn short_id_is_uppercase_last_eight_hex_of_canonical_uuid() {
        assert_eq!(
            short_id_from_pack_uuid(PACK_UUID).as_deref(),
            Some("FAC5562D")
        );
        // Matches the byte-based derivation used everywhere else.
        let bytes = pack_uuid_bytes(PACK_UUID).expect("canonical");
        assert_eq!(
            short_id_from_pack_uuid(PACK_UUID).unwrap(),
            pack_short_id(&bytes)
        );
    }

    #[test]
    fn short_id_refuses_a_non_canonical_uuid() {
        assert!(short_id_from_pack_uuid("not-a-uuid").is_none());
        assert!(short_id_from_pack_uuid("ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D").is_none());
        assert!(short_id_from_pack_uuid("").is_none());
    }

    #[test]
    fn pack_uuid_bytes_round_trips_with_format_pack_uuid() {
        let bytes = pack_uuid_bytes(PACK_UUID).expect("canonical");
        assert_eq!(format_pack_uuid(&bytes), PACK_UUID);
    }

    #[test]
    fn pack_uuid_bytes_refuses_a_non_canonical_uuid() {
        assert!(pack_uuid_bytes("nope").is_none());
        assert!(pack_uuid_bytes("ABABABAB-ABAB-ABAB-ABAB-ABABFAC5562D").is_none());
    }

    #[test]
    fn append_pack_uuid_adds_an_absent_uuid_at_eof() {
        let uuid = pack_uuid_bytes(PACK_UUID).unwrap();
        let out = append_pack_uuid(&[], &uuid);
        assert_eq!(out, uuid.to_vec());

        let existing = pack_uuid_bytes("11111111-1111-1111-1111-111111111111").unwrap();
        let out = append_pack_uuid(&existing, &uuid);
        assert_eq!(
            out.len(),
            32,
            "an absent uuid is appended after the existing one"
        );
        assert_eq!(&out[..16], &existing);
        assert_eq!(&out[16..], &uuid);
    }

    #[test]
    fn append_pack_uuid_is_idempotent_when_already_present() {
        let uuid = pack_uuid_bytes(PACK_UUID).unwrap();
        let pi = uuid.to_vec();
        assert_eq!(
            append_pack_uuid(&pi, &uuid),
            pi,
            "a present uuid is a no-op"
        );

        // Present among several entries → still unchanged.
        let other = pack_uuid_bytes("22222222-2222-2222-2222-222222222222").unwrap();
        let mut multi = other.to_vec();
        multi.extend_from_slice(&uuid);
        assert_eq!(append_pack_uuid(&multi, &uuid), multi);
    }

    #[test]
    fn remove_pack_uuid_deletes_the_target_and_keeps_the_others_in_order() {
        let a = pack_uuid_bytes("11111111-1111-1111-1111-111111111111").unwrap();
        let target = pack_uuid_bytes(PACK_UUID).unwrap();
        let c = pack_uuid_bytes("33333333-3333-3333-3333-333333333333").unwrap();
        let mut pi = a.to_vec();
        pi.extend_from_slice(&target);
        pi.extend_from_slice(&c);

        let out = remove_pack_uuid(&pi, &target);
        assert_eq!(out.len(), 32, "one 16-byte entry removed");
        assert_eq!(&out[..16], &a, "the entry before keeps its place");
        assert_eq!(&out[16..], &c, "the entry after keeps its reading order");
    }

    #[test]
    fn remove_pack_uuid_is_idempotent_when_absent() {
        let present = pack_uuid_bytes("11111111-1111-1111-1111-111111111111").unwrap();
        let absent = pack_uuid_bytes(PACK_UUID).unwrap();
        let pi = present.to_vec();
        assert_eq!(
            remove_pack_uuid(&pi, &absent),
            pi,
            "removing an absent uuid is a no-op (double delete is safe)"
        );
        // Removing the last remaining entry yields an empty (validly wiped) index.
        assert!(remove_pack_uuid(&pi, &present).is_empty());
    }

    #[test]
    fn remove_pack_uuid_purges_every_duplicate_of_a_corrupt_index() {
        let target = pack_uuid_bytes(PACK_UUID).unwrap();
        let keep = pack_uuid_bytes("44444444-4444-4444-4444-444444444444").unwrap();
        let mut pi = target.to_vec();
        pi.extend_from_slice(&keep);
        pi.extend_from_slice(&target); // duplicate of the target
        let out = remove_pack_uuid(&pi, &target);
        assert_eq!(
            out,
            keep.to_vec(),
            "all duplicates of the target are delisted"
        );
    }

    #[test]
    fn remove_pack_uuid_preserves_a_trailing_partial_chunk() {
        let target = pack_uuid_bytes(PACK_UUID).unwrap();
        let keep = pack_uuid_bytes("44444444-4444-4444-4444-444444444444").unwrap();
        let mut pi = target.to_vec();
        pi.extend_from_slice(&keep);
        pi.extend_from_slice(&[0xAB, 0xCD, 0xEF]); // 3 trailing bytes we did not author
        let out = remove_pack_uuid(&pi, &target);
        let mut expected = keep.to_vec();
        expected.extend_from_slice(&[0xAB, 0xCD, 0xEF]);
        assert_eq!(
            out, expected,
            "the unauthored trailing tail is left untouched"
        );
    }

    #[test]
    fn build_write_plan_keeps_pack_files_for_an_imported_story() {
        let d = descriptor(vec![
            pack_file_artifact("ni", 4),
            pack_file_artifact("rf/000/AAAAAAAA", 8),
        ]);
        let plan = build_write_plan(&d, "FAC5562D").expect("imported is transferable");
        assert_eq!(plan.short_id, "FAC5562D");
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.files[0].rel_path, "ni");
        assert_eq!(plan.files[1].rel_path, "rf/000/AAAAAAAA");
    }

    #[test]
    fn build_write_plan_refuses_a_native_story_as_not_transferable() {
        let d = descriptor(vec![PreparedArtifact {
            kind: PreparedArtifactKind::CanonicalStructure,
            relative_ref: "structure.json".into(),
            byte_len: 30,
            checksum: "a".repeat(64),
        }]);
        assert_eq!(
            build_write_plan(&d, "FAC5562D").expect_err("native must refuse"),
            TransferFailureCause::NotTransferable
        );
    }

    #[test]
    fn build_write_plan_refuses_a_native_multi_node_story_too() {
        // A multi-node native story still carries ONLY its canonical
        // structure (bigger bytes, same kind — the story transcoder stays
        // unimplemented), so the write-plan gate keeps refusing it: never a
        // false "transferable" because the graph grew. Orthogonal to the
        // local preflight verdict, which may legitimately say `présumée
        // transférable` on the same story.
        let d = descriptor(vec![PreparedArtifact {
            kind: PreparedArtifactKind::CanonicalStructure,
            relative_ref: "structure.json".into(),
            byte_len: 4096,
            checksum: "b".repeat(64),
        }]);
        assert_eq!(
            build_write_plan(&d, "FAC5562D").expect_err("native multi-node must refuse"),
            TransferFailureCause::NotTransferable
        );
    }

    #[test]
    fn cohort_coherence_passes_on_match_and_fails_device_changed_on_mismatch() {
        assert!(ensure_cohort_coherent("origine_v1", "origine_v1").is_ok());
        assert_eq!(
            ensure_cohort_coherent("origine_v1", "mid_gen_v2").expect_err("mismatch must refuse"),
            TransferFailureCause::DeviceChanged
        );
    }

    /// Every closed cause, in declaration order — the one list every
    /// closed-set test iterates. Nothing ties an array literal to the enum by
    /// itself: the REAL guard is [`all_causes_list_is_exhaustive`], whose
    /// exhaustive `match` breaks the compilation when a variant is added,
    /// forcing this list (whose length it asserts) to grow with it.
    const ALL_CAUSES: [TransferFailureCause; 7] = [
        TransferFailureCause::WriteNotAuthorized,
        TransferFailureCause::NotPrepared,
        TransferFailureCause::NotTransferable,
        TransferFailureCause::DeviceChanged,
        TransferFailureCause::WriteRejected,
        TransferFailureCause::DevicePackUnprovable,
        TransferFailureCause::Interrupted,
    ];

    #[test]
    fn all_causes_list_is_exhaustive() {
        // Compile-time tripwire: this match refuses to compile when the enum
        // gains a variant — the fix necessarily revisits this test, whose
        // assertions then force ALL_CAUSES to cover the new variant too.
        let assert_listed = |cause: TransferFailureCause| match cause {
            TransferFailureCause::WriteNotAuthorized
            | TransferFailureCause::NotPrepared
            | TransferFailureCause::NotTransferable
            | TransferFailureCause::DeviceChanged
            | TransferFailureCause::WriteRejected
            | TransferFailureCause::DevicePackUnprovable
            | TransferFailureCause::Interrupted => (),
        };
        for cause in ALL_CAUSES {
            assert_listed(cause);
        }
        // 7 DISTINCT entries (tags are pairwise distinct, asserted below) out
        // of a 7-variant enum = every variant is present exactly once.
        let mut tags = ALL_CAUSES
            .map(TransferFailureCause::diagnostic_tag)
            .to_vec();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), ALL_CAUSES.len(), "no duplicate in ALL_CAUSES");
    }

    #[test]
    fn failure_cause_severity_mapping_is_frozen() {
        assert_eq!(
            TransferFailureCause::NotPrepared.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::DeviceChanged.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::Interrupted.severity(),
            Severity::Fixable
        );
        assert_eq!(
            TransferFailureCause::WriteNotAuthorized.severity(),
            Severity::Blocking
        );
        assert_eq!(
            TransferFailureCause::NotTransferable.severity(),
            Severity::Blocking
        );
        assert_eq!(
            TransferFailureCause::WriteRejected.severity(),
            Severity::Blocking
        );
        // The unprovable-state refusal is an on-device integrity problem (a
        // re-plug does not clear an irregular entry) — Blocking, inherited from
        // the WriteRejected branch it was extracted from.
        assert_eq!(
            TransferFailureCause::DevicePackUnprovable.severity(),
            Severity::Blocking
        );
    }

    #[test]
    fn every_failure_cause_has_non_empty_copy() {
        for cause in ALL_CAUSES {
            let (message, action) = cause.copy();
            assert!(!message.is_empty(), "{cause:?} message empty");
            assert!(!action.is_empty(), "{cause:?} userAction empty");
        }
    }

    #[test]
    fn failure_cause_diagnostic_tags_are_stable_and_distinct() {
        let tags = ALL_CAUSES.map(TransferFailureCause::diagnostic_tag);
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len(), "tags must be distinct");
        assert!(tags.iter().all(|t| !t.is_empty()));
        assert_eq!(
            TransferFailureCause::DevicePackUnprovable.diagnostic_tag(),
            "device_pack_unprovable"
        );
    }

    #[test]
    fn every_failure_cause_wire_tag_round_trips() {
        for cause in ALL_CAUSES {
            assert_eq!(
                TransferFailureCause::from_wire_cause(cause.wire_cause()),
                Some(cause),
                "{cause:?} must round-trip"
            );
        }
        assert_eq!(
            TransferFailureCause::DevicePackUnprovable.wire_cause(),
            "devicePackUnprovable"
        );
        assert_eq!(TransferFailureCause::from_wire_cause("unknown"), None);
    }

    #[test]
    fn device_pack_unprovable_copy_is_honest_about_who_protects() {
        // The protective refusal must say RUSTORY declines and nothing was
        // modified — never re-serve the "device refused / check free space"
        // wording that stays with the pure write-I/O `WriteRejected`.
        let (message, action) = TransferFailureCause::DevicePackUnprovable.copy();
        assert_eq!(
            message,
            "Envoi interrompu : la copie présente sur l'appareil est dans un état que Rustory ne reconnaît pas, rien n'a été modifié."
        );
        assert_eq!(
            action,
            "Vérifie l'appareil, débranche-le puis rebranche-le, puis relance l'envoi."
        );
        let (rejected_message, rejected_action) = TransferFailureCause::WriteRejected.copy();
        assert_ne!(message, rejected_message);
        assert_ne!(action, rejected_action);
        assert!(
            !message.contains("a refusé"),
            "never worded as the device refusing"
        );
    }

    #[test]
    fn classify_is_failed_before_mutation_and_incomplete_after() {
        use TransferFailureCause::*;
        // The realistic writer outcomes: Interrupted is always pre-promotion, a
        // staging WriteRejected is pre-promotion → Failed.
        assert_eq!(classify(Interrupted, false), TransferCompleteness::Failed);
        assert_eq!(classify(WriteRejected, false), TransferCompleteness::Failed);
        // A durability/index I/O failure AFTER a successful promote → Incomplete.
        assert_eq!(
            classify(WriteRejected, true),
            TransferCompleteness::Incomplete
        );
        // Orthogonal to the cause: the result folds purely on the mutation flag.
        for cause in ALL_CAUSES {
            assert_eq!(classify(cause, false), TransferCompleteness::Failed);
            assert_eq!(classify(cause, true), TransferCompleteness::Incomplete);
        }
    }

    #[test]
    fn completeness_diagnostic_tags_are_stable() {
        assert_eq!(TransferCompleteness::Failed.diagnostic_tag(), "failed");
        assert_eq!(
            TransferCompleteness::Incomplete.diagnostic_tag(),
            "incomplete"
        );
    }

    #[test]
    fn failure_copy_defers_to_cause_when_failed_and_is_device_aware_when_incomplete() {
        // Failed → the cause's own copy verbatim (the device is intact).
        assert_eq!(
            failure_copy(
                TransferFailureCause::Interrupted,
                TransferCompleteness::Failed
            ),
            TransferFailureCause::Interrupted.copy()
        );
        // Incomplete → the device-nuance copy, identical regardless of the cause,
        // non-empty, and free of any technical jargon.
        let (m_a, a_a) = failure_copy(
            TransferFailureCause::WriteRejected,
            TransferCompleteness::Incomplete,
        );
        let (m_b, a_b) = failure_copy(
            TransferFailureCause::Interrupted,
            TransferCompleteness::Incomplete,
        );
        assert_eq!(
            (m_a, a_a),
            (m_b, a_b),
            "Incomplete copy is cause-independent"
        );
        assert!(!m_a.is_empty() && !a_a.is_empty());
        for bad in [
            "write", "staging", "promote", "index", ".pi", "job", "payload", "stage",
        ] {
            assert!(!m_a.contains(bad), "no jargon: {bad}");
            assert!(!a_a.contains(bad), "no jargon: {bad}");
        }
    }

    #[test]
    fn classify_verify_truth_table() {
        use ChecksumProbe::*;
        use VerifyVerdict::*;
        // Indexed + content present + byte-faithful ⇒ the only Verified case.
        assert_eq!(classify_verify(true, true, Match, true), Verified);
        // Not readable / not the written device ⇒ Failed regardless of the rest.
        assert_eq!(classify_verify(true, true, Match, false), Failed);
        assert_eq!(classify_verify(false, false, Unavailable, false), Failed);
        // Pack absent (content not present) ⇒ Failed, even if an index entry lingers.
        assert_eq!(
            classify_verify(false, false, Unavailable, true),
            Failed,
            "pack absent"
        );
        assert_eq!(
            classify_verify(true, false, Unavailable, true),
            Failed,
            "uuid indexed but content folder missing ⇒ Failed (pack absent)"
        );
        // Content present but the re-checksum could not run ⇒ Failed (unconfirmable).
        assert_eq!(
            classify_verify(true, true, Unavailable, true),
            Failed,
            "present but unconfirmable"
        );
        // Content present + readable divergence ⇒ Partial, regardless of `indexed`
        // — a safety-critical classifier ("no success without proof"): `Diverged`
        // must never become `indexed`-dependent, so BOTH index states are locked.
        assert_eq!(
            classify_verify(true, true, Diverged, true),
            Partial,
            "present + indexed but the bytes diverge"
        );
        assert_eq!(
            classify_verify(false, true, Diverged, true),
            Partial,
            "diverging bytes on a promoted-but-unindexed content ⇒ Partial"
        );
        // Content present + byte-faithful but NOT indexed ⇒ Partial.
        assert_eq!(
            classify_verify(false, true, Match, true),
            Partial,
            "content promoted but uuid not indexed"
        );
    }

    #[test]
    fn compose_verified_summary_composes_changed_and_unchanged_lines() {
        let zero = compose_verified_summary(
            "Mon histoire",
            0,
            DeviceFamily::Lunii,
            WriteOutcome::CreatedNew,
        );
        assert!(zero.changed.contains("Mon histoire"));
        assert!(zero.changed.contains("sur la Lunii"));
        assert!(zero.unchanged.to_lowercase().contains("aucune autre"));

        let one = compose_verified_summary("T", 1, DeviceFamily::Lunii, WriteOutcome::CreatedNew);
        assert!(one.unchanged.starts_with("1 autre histoire"));
        assert!(one.unchanged.contains("reste inchangée"));

        let many = compose_verified_summary("T", 3, DeviceFamily::Lunii, WriteOutcome::CreatedNew);
        assert!(many.unchanged.starts_with("3 autres histoires"));
        assert!(many.unchanged.contains("restent inchangées"));
    }

    #[test]
    fn compose_verified_summary_first_send_lunii_literal_is_frozen() {
        // Invariance guard: the historical first-send Lunii literal must not
        // change by a byte (the existing tests and persisted rows rely on it).
        let summary = compose_verified_summary(
            "Mon histoire",
            2,
            DeviceFamily::Lunii,
            WriteOutcome::CreatedNew,
        );
        assert_eq!(
            summary.changed,
            "« Mon histoire » est maintenant sur la Lunii."
        );
    }

    #[test]
    fn compose_verified_summary_bifurcates_by_outcome_and_family() {
        // One decision = one test: every (outcome, family) pair yields its exact
        // frozen literal (product-language.md, Change Control). The generic
        // variants are declared-without-a-live-emitter (no non-Lunii write
        // exists) but frozen and tested in both directions.
        for (outcome, family, expected) in [
            (
                WriteOutcome::CreatedNew,
                DeviceFamily::Lunii,
                "« T » est maintenant sur la Lunii.",
            ),
            (
                WriteOutcome::CreatedNew,
                DeviceFamily::Flam,
                "« T » est maintenant sur l'appareil.",
            ),
            (
                WriteOutcome::ReplacedDivergent,
                DeviceFamily::Lunii,
                "« T » a été mise à jour sur la Lunii.",
            ),
            (
                WriteOutcome::ReplacedDivergent,
                DeviceFamily::Flam,
                "« T » a été mise à jour sur l'appareil.",
            ),
            (
                WriteOutcome::ReusedIdentical,
                DeviceFamily::Lunii,
                "« T » était déjà à jour sur la Lunii.",
            ),
            (
                WriteOutcome::ReusedIdentical,
                DeviceFamily::Flam,
                "« T » était déjà à jour sur l'appareil.",
            ),
        ] {
            let summary = compose_verified_summary("T", 0, family, outcome);
            assert_eq!(summary.changed, expected, "{outcome:?}/{family:?}");
        }
    }

    #[test]
    fn write_outcome_diagnostic_tags_are_stable_and_distinct() {
        let tags = [
            WriteOutcome::CreatedNew.diagnostic_tag(),
            WriteOutcome::ReusedIdentical.diagnostic_tag(),
            WriteOutcome::ReplacedDivergent.diagnostic_tag(),
        ];
        assert_eq!(
            tags,
            ["created_new", "reused_identical", "replaced_divergent"]
        );
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len());
    }

    #[test]
    fn verify_verdict_tags_and_labels_are_stable_and_distinct() {
        let tags = [
            VerifyVerdict::Verified.diagnostic_tag(),
            VerifyVerdict::Partial.diagnostic_tag(),
            VerifyVerdict::Failed.diagnostic_tag(),
        ];
        assert_eq!(tags, ["verified", "partial", "failed"]);
        // The canonical state labels are the promoted/new ones.
        assert_eq!(
            VerifyVerdict::Verified.state_label(),
            "transférée et vérifiée"
        );
        assert_eq!(VerifyVerdict::Partial.state_label(), "état partiel");
        assert_eq!(VerifyVerdict::Failed.state_label(), "échec récupérable");
    }

    #[test]
    fn verify_partial_is_not_the_transfer_incomplete_wording() {
        // `état partiel` (a verify verdict) must never be confused with
        // `transfert incomplet` (a `transfer`-phase interruption, 3.5). Distinct
        // labels AND distinct copy.
        assert_ne!(VerifyVerdict::Partial.state_label(), "transfert incomplet");
        let (partial_msg, _) = VerifyVerdict::Partial.copy().expect("partial has copy");
        let (incomplete_msg, _) = failure_copy(
            TransferFailureCause::WriteRejected,
            TransferCompleteness::Incomplete,
        );
        assert_ne!(
            partial_msg, incomplete_msg,
            "partial and incomplete must read differently"
        );
    }

    #[test]
    fn verify_verdict_copy_is_present_for_non_success_only_and_jargon_free() {
        assert!(
            VerifyVerdict::Verified.copy().is_none(),
            "a verified success carries no failure copy"
        );
        for verdict in [VerifyVerdict::Partial, VerifyVerdict::Failed] {
            let (message, action) = verdict.copy().expect("non-success verdict has copy");
            assert!(!message.is_empty(), "{verdict:?} message empty");
            assert!(!action.is_empty(), "{verdict:?} userAction empty");
            for bad in [
                "checksum", "promote", "index", ".pi", ".content", "write", "job", "payload",
            ] {
                assert!(!message.contains(bad), "{verdict:?} jargon: {bad}");
                assert!(!action.contains(bad), "{verdict:?} jargon: {bad}");
            }
        }
    }
}
