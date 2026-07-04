//! Read-only assembly of the transfer-artifact descriptor.
//!
//! Given a story's preparation plan, enumerates and RE-CHECKSUMS the artifacts a
//! transfer would need, producing an ephemeral [`TransferArtifactDescriptor`].
//! Strictly READ-ONLY: it never writes, never decrypts, never duplicates bytes
//! onto the device. The only inputs are the LOCAL canonical structure (a native
//! minimal story) or the LOCAL promoted pack under
//! `{app_data_dir}/imports/<story_id>/` (an imported raw pack from story 2.x).
//!
//! In MVP there is NO media transcoding to perform (the available stories are
//! raw imported packs already in device format, or minimal native stories with
//! an empty `nodes`). A future media transformer would slot into the assembly
//! HERE — declared, not implemented (no false coverage). If it ever produces a
//! derived file, it must follow staging → validation → promotion like the
//! import path; in MVP the descriptor stays in memory, so there is no derived
//! write and no promotion (hence no boot sweep is needed for it yet).
//!
//! Integrity baseline: the aggregate checksum is recomputed with the EXACT same
//! algorithm the import recorded (`rel_path` + `\0` + bytes, in the
//! [`validate_pack_inventory`] manifest order), so a later
//! [`verify_aggregate`](crate::domain::transfer::verify_aggregate) against the
//! stored `pack_checksum` detects silent on-disk corruption. A trait keeps the
//! application layer testable without real files.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::domain::device::{
    validate_pack_inventory, PackEntry, PackEntryKind, PackValidationIssue, LUNII_CONTENT_DIR,
    MAX_IMPORT_PACK_FILES, MAX_PACK_ASSET_DEPTH,
};
use crate::domain::story::content_checksum;
use crate::domain::transfer::{
    PreparationFailureCause, PreparedArtifact, PreparedArtifactKind, TransferArtifactDescriptor,
    PREPARATION_PIPELINE_VERSION,
};

use super::resolve_import_story_dir;

/// Streaming buffer — matches the import copier so large packs never hold more
/// than a fixed slice in memory.
const COPY_BUF_BYTES: usize = 64 * 1024;

/// What a story needs assembled. Pure data the application layer composes from
/// the canonical facts read under the scoped DB lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyPlan {
    pub story_id: String,
    pub target_cohort: String,
    pub source: AssemblySource,
}

/// The kind of local source the artifacts come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblySource {
    /// A native minimal story — the canonical structure is the only artifact.
    Native { structure_json: String },
    /// An imported raw pack under `{app_data_dir}/imports/<story_id>/`.
    ImportedPack,
}

/// The integrity authority for transfer artifacts. Read-only by contract; MUST
/// respect the wall-clock `budget` so a stalled disk cannot keep a
/// `spawn_blocking` worker alive past the command budget.
///
/// Beyond assembling the LOCAL descriptor, it also re-checksums the DEVICE copy
/// of an imported pack ([`reaggregate_device_pack`]) — the `verify` phase reads
/// the bytes that landed on the Lunii and reproduces the same aggregate to prove
/// byte fidelity against the prepared baseline.
///
/// [`reaggregate_device_pack`]: TransferArtifactSource::reaggregate_device_pack
pub trait TransferArtifactSource: Send + Sync + 'static {
    fn assemble(
        &self,
        app_data_dir: &Path,
        plan: &AssemblyPlan,
        budget: Duration,
    ) -> Result<TransferArtifactDescriptor, PreparationFailureCause>;

    /// Re-checksum the pack written under `.content/<SHORT_ID>` on the device
    /// `mount_path`, returning its aggregate hex. Uses the EXACT import
    /// aggregation, so the caller can compare it to the prepared
    /// `aggregate_checksum` to confirm byte fidelity (`verify` phase). Read-only:
    /// never copies bytes off the device, never decrypts.
    fn reaggregate_device_pack(
        &self,
        mount_path: &Path,
        short_id: &str,
        budget: Duration,
    ) -> Result<String, PreparationFailureCause>;
}

/// Production assembler: stdlib filesystem reads + SHA-256.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTransferArtifactSource;

impl TransferArtifactSource for SystemTransferArtifactSource {
    fn assemble(
        &self,
        app_data_dir: &Path,
        plan: &AssemblyPlan,
        budget: Duration,
    ) -> Result<TransferArtifactDescriptor, PreparationFailureCause> {
        match &plan.source {
            AssemblySource::Native { structure_json } => Ok(assemble_native(plan, structure_json)),
            AssemblySource::ImportedPack => assemble_imported_pack(app_data_dir, plan, budget),
        }
    }

    fn reaggregate_device_pack(
        &self,
        mount_path: &Path,
        short_id: &str,
        budget: Duration,
    ) -> Result<String, PreparationFailureCause> {
        let pack_dir = mount_path.join(LUNII_CONTENT_DIR).join(short_id);
        aggregate_pack_dir(&pack_dir, budget).map(|(aggregate, _)| aggregate)
    }
}

/// A native minimal story: the canonical structure is the single artifact, and
/// the aggregate IS its `content_checksum` (the same value
/// `verify_aggregate` compares against `stories.content_checksum`). Cannot fail
/// — the structure JSON came from the DB and already passed the preflight.
fn assemble_native(plan: &AssemblyPlan, structure_json: &str) -> TransferArtifactDescriptor {
    let checksum = content_checksum(structure_json);
    let artifact = PreparedArtifact {
        kind: PreparedArtifactKind::CanonicalStructure,
        relative_ref: "structure.json".to_string(),
        byte_len: structure_json.len() as u64,
        checksum: checksum.clone(),
    };
    TransferArtifactDescriptor {
        story_id: plan.story_id.clone(),
        target_cohort: plan.target_cohort.clone(),
        pipeline_version: PREPARATION_PIPELINE_VERSION,
        artifacts: vec![artifact],
        aggregate_checksum: checksum,
    }
}

/// An imported raw pack: re-walk the promoted tree, re-validate it against the
/// declared subset, then re-checksum every file in manifest order. The aggregate
/// reproduces the import's `pack_checksum` byte-for-byte so a later
/// `verify_aggregate` catches a flipped byte as `ArtifactCorrupt`.
fn assemble_imported_pack(
    app_data_dir: &Path,
    plan: &AssemblyPlan,
    budget: Duration,
) -> Result<TransferArtifactDescriptor, PreparationFailureCause> {
    let pack_dir = resolve_import_story_dir(app_data_dir, &plan.story_id);
    let (aggregate_checksum, artifacts) = aggregate_pack_dir(&pack_dir, budget)?;
    Ok(TransferArtifactDescriptor {
        story_id: plan.story_id.clone(),
        target_cohort: plan.target_cohort.clone(),
        pipeline_version: PREPARATION_PIPELINE_VERSION,
        artifacts,
        aggregate_checksum,
    })
}

/// Read-only walk + structural re-validation + aggregate re-checksum of a pack
/// directory, in the import's EXACT aggregation (`rel_path` + NUL + bytes, in
/// [`validate_pack_inventory`] manifest order). Shared by the LOCAL imports
/// assembler and the DEVICE re-checksum the `verify` phase runs, so both
/// reproduce the import's `pack_checksum` byte-for-byte. Strictly read-only:
/// never copies, never decrypts, never writes. Returns the aggregate hex + the
/// per-file artifacts (the device verify ignores the latter).
fn aggregate_pack_dir(
    pack_dir: &Path,
    budget: Duration,
) -> Result<(String, Vec<PreparedArtifact>), PreparationFailureCause> {
    let started = Instant::now();

    // The pack folder must still be a real directory. Absence is the recoverable
    // "the artifacts went missing" branch (a verify reads this as "not present").
    match fs::symlink_metadata(pack_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(PreparationFailureCause::ArtifactCorrupt),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(PreparationFailureCause::ArtifactMissing);
        }
        Err(err) => return Err(map_io(&err)),
    }

    let mut entries: Vec<PackEntry> = Vec::new();
    walk(pack_dir, &mut Vec::new(), &mut entries, started, budget)?;

    // Reuse the SAME deterministic ordering + structural rules as the import, so
    // the re-hash lines up with the stored checksum and a missing required file
    // is reported honestly rather than producing a different (lower) aggregate.
    let manifest = validate_pack_inventory(&entries).map_err(|issue| map_issue(&issue))?;

    let mut aggregate = Sha256::new();
    let mut artifacts = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        if started.elapsed() >= budget {
            return Err(PreparationFailureCause::Interrupted);
        }
        // Aggregate domain-separator: `rel_path` + NUL, then the bytes — the
        // exact import algorithm (`infrastructure/device/pack_reader.rs`).
        aggregate.update(file.rel_path.as_bytes());
        aggregate.update([0u8]);

        let src = join_rel_path(pack_dir, &file.rel_path);
        let (size, per_file_checksum) = stream_file(&src, &mut aggregate, started, budget)?;
        artifacts.push(PreparedArtifact {
            kind: PreparedArtifactKind::PackFile,
            relative_ref: file.rel_path.clone(),
            byte_len: size,
            checksum: per_file_checksum,
        });
    }

    Ok((format!("{:x}", aggregate.finalize()), artifacts))
}

/// Recursive bounded read-only walk, mirroring the import enumerator so the
/// validated manifest matches. Never follows symlinks; stops recursing at the
/// asset depth the domain would refuse anyway.
fn walk(
    dir: &Path,
    rel_components: &mut Vec<String>,
    out: &mut Vec<PackEntry>,
    started: Instant,
    budget: Duration,
) -> Result<(), PreparationFailureCause> {
    let read_dir = fs::read_dir(dir).map_err(|err| map_io(&err))?;
    for dir_entry in read_dir {
        if started.elapsed() >= budget {
            return Err(PreparationFailureCause::Interrupted);
        }
        // Defense in depth: a corrupted tree with an absurd file count is
        // treated as corruption rather than walked unbounded.
        if out.len() > MAX_IMPORT_PACK_FILES.saturating_mul(2) {
            return Err(PreparationFailureCause::ArtifactCorrupt);
        }
        let dir_entry = dir_entry.map_err(|err| map_io(&err))?;
        let name = match dir_entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => return Err(PreparationFailureCause::ArtifactCorrupt),
        };
        let meta = fs::symlink_metadata(dir_entry.path()).map_err(|err| map_io(&err))?;
        let kind = if meta.file_type().is_symlink() {
            PackEntryKind::Symlink
        } else if meta.is_dir() {
            PackEntryKind::Dir
        } else if meta.is_file() {
            PackEntryKind::File
        } else {
            PackEntryKind::Other
        };

        rel_components.push(name);
        let rel_path = rel_components.join("/");
        let depth = rel_components.len();
        out.push(PackEntry {
            rel_path,
            kind,
            size: if kind == PackEntryKind::File {
                meta.len()
            } else {
                0
            },
        });
        if kind == PackEntryKind::Dir && depth <= MAX_PACK_ASSET_DEPTH {
            walk(&dir_entry.path(), rel_components, out, started, budget)?;
        }
        rel_components.pop();
    }
    Ok(())
}

/// Stream a file through `aggregate` AND a fresh per-file hasher, with deadline
/// checks between chunks. Returns the byte count and the per-file SHA-256 hex.
///
/// Closes the enumerate→read TOCTOU window (mirrors the import copier): the path
/// is `lstat`'d just before opening and the OPEN HANDLE is `fstat`'d after — a
/// file replaced by a symlink or another inode between the inventory walk and
/// this read is refused as `ArtifactCorrupt`, never followed outside the managed
/// `imports/<story_id>/` folder.
fn stream_file(
    src: &Path,
    aggregate: &mut Sha256,
    started: Instant,
    budget: Duration,
) -> Result<(u64, String), PreparationFailureCause> {
    let expected = fs::symlink_metadata(src).map_err(|err| map_io(&err))?;
    if !expected.is_file() {
        return Err(PreparationFailureCause::ArtifactCorrupt);
    }
    let reader = File::open(src).map_err(|err| map_io(&err))?;
    let opened = reader.metadata().map_err(|err| map_io(&err))?;
    if !opened.is_file() {
        return Err(PreparationFailureCause::ArtifactCorrupt);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != expected.dev() || opened.ino() != expected.ino() {
            return Err(PreparationFailureCause::ArtifactCorrupt);
        }
    }
    let mut reader = reader;
    let mut per_file = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    let mut size: u64 = 0;
    loop {
        if started.elapsed() >= budget {
            return Err(PreparationFailureCause::Interrupted);
        }
        let read = reader.read(&mut buf).map_err(|err| map_io(&err))?;
        if read == 0 {
            break;
        }
        aggregate.update(&buf[..read]);
        per_file.update(&buf[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok((size, format!("{:x}", per_file.finalize())))
}

/// Join a validated forward-slash `rel_path` under `base`, component by
/// component. The components come from our own enumeration (never `..`, never
/// absolute).
fn join_rel_path(base: &Path, rel_path: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for component in rel_path.split('/') {
        out.push(component);
    }
    out
}

/// A read failure is `ArtifactMissing` when the artifact is gone, otherwise
/// `ArtifactCorrupt` (permission, I/O error, swapped to a non-regular file).
fn map_io(err: &std::io::Error) -> PreparationFailureCause {
    if err.kind() == std::io::ErrorKind::NotFound {
        PreparationFailureCause::ArtifactMissing
    } else {
        PreparationFailureCause::ArtifactCorrupt
    }
}

/// A required file absent is `ArtifactMissing`; any other structural violation
/// (empty required, unknown entry, symlink, too deep, oversize) is corruption.
fn map_issue(issue: &PackValidationIssue) -> PreparationFailureCause {
    match issue {
        PackValidationIssue::MissingRequired { .. } => PreparationFailureCause::ArtifactMissing,
        _ => PreparationFailureCause::ArtifactCorrupt,
    }
}

/// Test double scripting assembly + device re-checksum results without touching
/// the filesystem.
#[cfg(test)]
#[derive(Default)]
pub struct MockTransferArtifactSource {
    responses: std::sync::Mutex<
        std::collections::VecDeque<Result<TransferArtifactDescriptor, PreparationFailureCause>>,
    >,
    reaggregations:
        std::sync::Mutex<std::collections::VecDeque<Result<String, PreparationFailureCause>>>,
}

#[cfg(test)]
impl MockTransferArtifactSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&self, response: Result<TransferArtifactDescriptor, PreparationFailureCause>) {
        self.responses
            .lock()
            .expect("mock lock")
            .push_back(response);
    }

    /// Script the next [`reaggregate_device_pack`] result (the device-side
    /// re-checksum the `verify` phase compares to the prepared baseline).
    ///
    /// [`reaggregate_device_pack`]: TransferArtifactSource::reaggregate_device_pack
    pub fn enqueue_reaggregate(&self, response: Result<String, PreparationFailureCause>) {
        self.reaggregations
            .lock()
            .expect("mock lock")
            .push_back(response);
    }
}

#[cfg(test)]
impl TransferArtifactSource for MockTransferArtifactSource {
    fn assemble(
        &self,
        _app_data_dir: &Path,
        _plan: &AssemblyPlan,
        _budget: Duration,
    ) -> Result<TransferArtifactDescriptor, PreparationFailureCause> {
        self.responses
            .lock()
            .expect("mock lock")
            .pop_front()
            .expect("MockTransferArtifactSource: no scripted response enqueued")
    }

    fn reaggregate_device_pack(
        &self,
        _mount_path: &Path,
        _short_id: &str,
        _budget: Duration,
    ) -> Result<String, PreparationFailureCause> {
        self.reaggregations
            .lock()
            .expect("mock lock")
            .pop_front()
            .expect("MockTransferArtifactSource: no scripted reaggregation enqueued")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::transfer::verify_aggregate;

    const HEALTHY_JSON: &str = "{\"schemaVersion\":3,\"startNodeId\":\"n1\",\"nodes\":[{\"id\":\"n1\",\"text\":\"\",\"label\":\"\",\"imageAssetId\":null,\"audioAssetId\":null,\"options\":[]}]}";

    fn budget() -> Duration {
        Duration::from_secs(30)
    }

    /// Write a complete plausible pack (declared subset) into `pack_dir`.
    fn write_pack(pack_dir: &Path) {
        std::fs::create_dir_all(pack_dir).expect("mkdir pack");
        std::fs::write(pack_dir.join("ni"), vec![0x4E; 512]).expect("ni");
        std::fs::write(pack_dir.join("li"), vec![0x4C; 256]).expect("li");
        std::fs::write(pack_dir.join("ri"), vec![0x52; 128]).expect("ri");
        std::fs::write(pack_dir.join("si"), vec![0x53; 128]).expect("si");
        std::fs::write(pack_dir.join("nm"), vec![0x6E; 32]).expect("nm");
        let rf = pack_dir.join("rf").join("000");
        std::fs::create_dir_all(&rf).expect("rf/000");
        std::fs::write(rf.join("AAAAAAAA"), vec![0xAA; 2048]).expect("rf asset");
        let sf = pack_dir.join("sf").join("000");
        std::fs::create_dir_all(&sf).expect("sf/000");
        std::fs::write(sf.join("BBBBBBBB"), vec![0xBB; 4096]).expect("sf asset");
    }

    fn native_plan() -> AssemblyPlan {
        AssemblyPlan {
            story_id: "0197a5d0-0000-7000-8000-000000000000".into(),
            target_cohort: "origine_v1".into(),
            source: AssemblySource::Native {
                structure_json: HEALTHY_JSON.into(),
            },
        }
    }

    fn imported_plan(story_id: &str) -> AssemblyPlan {
        AssemblyPlan {
            story_id: story_id.into(),
            target_cohort: "origine_v1".into(),
            source: AssemblySource::ImportedPack,
        }
    }

    fn seed_pack(story_id: &str) -> tempfile::TempDir {
        let app_data = tempfile::tempdir().expect("app data");
        let pack_dir = resolve_import_story_dir(app_data.path(), story_id);
        write_pack(&pack_dir);
        app_data
    }

    #[test]
    fn native_story_assembles_a_single_structure_artifact() {
        let descriptor = SystemTransferArtifactSource
            .assemble(Path::new("/unused"), &native_plan(), budget())
            .expect("assemble native");
        assert_eq!(descriptor.pipeline_version, PREPARATION_PIPELINE_VERSION);
        assert_eq!(descriptor.artifacts.len(), 1);
        assert_eq!(
            descriptor.artifacts[0].kind,
            PreparedArtifactKind::CanonicalStructure
        );
        assert_eq!(
            descriptor.aggregate_checksum,
            content_checksum(HEALTHY_JSON)
        );
        // A native story always matches its own canonical checksum.
        assert!(verify_aggregate(&descriptor, &content_checksum(HEALTHY_JSON)).is_ok());
    }

    #[test]
    fn imported_pack_assembles_every_declared_file_deterministically() {
        let story_id = "11111111-1111-7111-8111-111111111111";
        let app_data = seed_pack(story_id);

        let descriptor = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("assemble imported");
        // ni li ri si nm + rf/000/AAAAAAAA + sf/000/BBBBBBBB = 7 files.
        assert_eq!(descriptor.artifacts.len(), 7);
        assert!(descriptor
            .artifacts
            .iter()
            .all(|a| a.kind == PreparedArtifactKind::PackFile));
        assert_eq!(descriptor.aggregate_checksum.len(), 64);
        assert!(descriptor
            .aggregate_checksum
            .chars()
            .all(|c| c.is_ascii_hexdigit()));

        // Deterministic: a second assembly yields the same aggregate.
        let again = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("assemble again");
        assert_eq!(descriptor.aggregate_checksum, again.aggregate_checksum);
    }

    #[test]
    fn missing_pack_folder_is_artifact_missing() {
        let app_data = tempfile::tempdir().expect("app data");
        let err = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan("does-not-exist"), budget())
            .expect_err("absent pack must fail");
        assert_eq!(err, PreparationFailureCause::ArtifactMissing);
    }

    #[test]
    fn missing_required_file_is_artifact_missing() {
        let story_id = "22222222-2222-7222-8222-222222222222";
        let app_data = seed_pack(story_id);
        std::fs::remove_file(resolve_import_story_dir(app_data.path(), story_id).join("si"))
            .expect("drop required file");
        let err = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect_err("missing required must fail");
        assert_eq!(err, PreparationFailureCause::ArtifactMissing);
    }

    #[test]
    fn a_flipped_byte_makes_the_aggregate_diverge_so_corruption_is_detectable() {
        let story_id = "33333333-3333-7333-8333-333333333333";
        let app_data = seed_pack(story_id);
        let healthy = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("healthy assemble");

        // Flip a byte in a pack file: the re-assembled aggregate diverges, so
        // `verify_aggregate` against the original baseline returns ArtifactCorrupt.
        std::fs::write(
            resolve_import_story_dir(app_data.path(), story_id).join("ni"),
            vec![0x00; 512],
        )
        .expect("tamper ni");
        let tampered = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("tampered assemble");

        assert_ne!(healthy.aggregate_checksum, tampered.aggregate_checksum);
        assert_eq!(
            verify_aggregate(&tampered, &healthy.aggregate_checksum)
                .expect_err("a flipped byte must be detected"),
            PreparationFailureCause::ArtifactCorrupt
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_pack_entry_is_refused_as_corrupt_never_followed() {
        // A pack entry replaced by a symlink must be refused (never read outside
        // the managed folder): the read-only walk classifies it via
        // `symlink_metadata` and the stream guard re-checks at open time.
        let story_id = "55555555-5555-7555-8555-555555555555";
        let app_data = seed_pack(story_id);
        let pack_dir = resolve_import_story_dir(app_data.path(), story_id);
        let outside = app_data.path().join("outside-secret");
        std::fs::write(&outside, b"SECRET").expect("seed outside target");
        std::fs::remove_file(pack_dir.join("ni")).expect("rm ni");
        std::os::unix::fs::symlink(&outside, pack_dir.join("ni")).expect("symlink ni");
        let err = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect_err("a symlinked entry must be refused");
        assert_eq!(err, PreparationFailureCause::ArtifactCorrupt);
    }

    #[test]
    fn zero_budget_aborts_with_interrupted() {
        let story_id = "44444444-4444-7444-8444-444444444444";
        let app_data = seed_pack(story_id);
        let err = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), Duration::ZERO)
            .expect_err("zero budget must abort");
        assert_eq!(err, PreparationFailureCause::Interrupted);
    }

    #[test]
    fn reaggregate_device_pack_reproduces_the_import_aggregate() {
        // The device re-checksum the `verify` phase runs must reproduce the EXACT
        // aggregate the local import/assembly recorded — same algorithm, same
        // files, same order — so a faithful round-trip compares equal.
        let story_id = "66666666-6666-7666-8666-666666666666";
        let app_data = seed_pack(story_id);
        let local = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("assemble local");

        // Lay the SAME pack under a device-style `.content/<SHORT_ID>` mount.
        let mount = tempfile::tempdir().expect("mount");
        let short_id = "FAC5562D";
        write_pack(&mount.path().join(LUNII_CONTENT_DIR).join(short_id));

        let device_aggregate = SystemTransferArtifactSource
            .reaggregate_device_pack(mount.path(), short_id, budget())
            .expect("reaggregate device pack");
        assert_eq!(
            device_aggregate, local.aggregate_checksum,
            "the device re-checksum matches the import aggregate byte-for-byte"
        );
    }

    #[test]
    fn reaggregate_device_pack_diverges_when_a_device_byte_is_flipped() {
        let story_id = "77777777-7777-7777-8777-777777777777";
        let app_data = seed_pack(story_id);
        let baseline = SystemTransferArtifactSource
            .assemble(app_data.path(), &imported_plan(story_id), budget())
            .expect("assemble local")
            .aggregate_checksum;

        let mount = tempfile::tempdir().expect("mount");
        let short_id = "FAC5562D";
        let pack_dir = mount.path().join(LUNII_CONTENT_DIR).join(short_id);
        write_pack(&pack_dir);
        std::fs::write(pack_dir.join("ni"), vec![0x00; 512]).expect("tamper ni on device");

        let device_aggregate = SystemTransferArtifactSource
            .reaggregate_device_pack(mount.path(), short_id, budget())
            .expect("reaggregate tampered device pack");
        assert_ne!(
            device_aggregate, baseline,
            "a flipped device byte makes the re-checksum diverge — caught by verify"
        );
    }

    #[test]
    fn reaggregate_device_pack_reports_missing_when_absent() {
        let mount = tempfile::tempdir().expect("mount");
        let err = SystemTransferArtifactSource
            .reaggregate_device_pack(mount.path(), "DEADBEEF", budget())
            .expect_err("absent device pack must fail");
        assert_eq!(err, PreparationFailureCause::ArtifactMissing);
    }
}
