use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::domain::device::{
    FLAM_CONFIG_DIR, FLAM_PRIMARY_MARKER, FLAM_STORY_DIR, LUNII_BINARY_TOKEN_MARKER,
    LUNII_DEVICE_ID_MARKER, LUNII_PRIMARY_MARKER, MAX_METADATA_FILE_BYTES,
};
use crate::domain::shared::AppError;

use super::scanner::{CandidateFacts, DeviceCandidate, DeviceScanReport, DeviceScanner};

/// Env var consumed by the system scanner to inject additional mount
/// roots beyond what `sysinfo::Disks` enumerates. Useful when the
/// process runs in a mount namespace (Docker / chroot) that does not
/// see the host's USB auto-mount tree. The value is a path list using
/// the OS path separator (`:` on Linux/macOS, `;` on Windows); each
/// non-empty entry is probed in addition to the sysinfo result. Empty
/// or unset → sysinfo only.
pub const EXTRA_MOUNT_ROOTS_ENV: &str = "RUSTORY_DEVICE_MOUNT_ROOTS";

/// Default sentinel reused by tests that do not need a custom mount root
/// list. Production callers go through the `Default` impl which queries
/// `sysinfo::Disks::new_with_refreshed_list()`.
pub const SYSTEM_SCANNER_DEFAULT: &str = "<sysinfo>";

/// Production [`DeviceScanner`] backed by `sysinfo` for cross-platform
/// disk enumeration plus stdlib for marker reads.
///
/// In test contexts the scanner can be constructed with explicit mount
/// roots via [`SystemDeviceScanner::with_explicit_mount_roots`]. The
/// production code path uses `with_sysinfo_enumeration` which probes the
/// real OS — that path is exercised only at runtime (CI containers
/// rarely expose a Lunii mount).
pub struct SystemDeviceScanner {
    source: ScannerSource,
}

enum ScannerSource {
    Sysinfo,
    ExplicitRoots(Vec<PathBuf>),
}

impl Default for SystemDeviceScanner {
    fn default() -> Self {
        Self::with_sysinfo_enumeration()
    }
}

impl SystemDeviceScanner {
    pub fn with_sysinfo_enumeration() -> Self {
        Self {
            source: ScannerSource::Sysinfo,
        }
    }

    /// Build a scanner that probes ONLY the supplied roots. Used by
    /// integration tests so a `TempDir` mount can be exercised without
    /// requiring real USB hardware.
    pub fn with_explicit_mount_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            source: ScannerSource::ExplicitRoots(roots),
        }
    }

    fn enumerate_roots(&self) -> Vec<PathBuf> {
        match &self.source {
            ScannerSource::Sysinfo => {
                let disks = sysinfo::Disks::new_with_refreshed_list();
                // Restrict the sysinfo-discovered roots to removable
                // volumes: a Lunii is a USB Mass Storage device, so
                // probing every system mount (/, /home, /boot/efi, …)
                // would waste budget AND risk surfacing a system
                // partition that just happens to ship vfat. Honest
                // for production; tests inject roots via the
                // `with_explicit_mount_roots` path and bypass this
                // filter.
                let mut roots: Vec<PathBuf> = disks
                    .iter()
                    .filter(|d| d.is_removable())
                    .map(|d| d.mount_point().to_path_buf())
                    .collect();
                roots.extend(extra_mount_roots_from_env());
                deduplicate_paths(roots)
            }
            ScannerSource::ExplicitRoots(roots) => deduplicate_paths(roots.clone()),
        }
    }
}

/// Order-preserving dedup. We want the first occurrence of each
/// canonical path to survive (the order matters for the truncated-
/// scan budget guarantee: removable disks from sysinfo come first,
/// env-injected roots come after).
fn deduplicate_paths(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::with_capacity(roots.len());
    let mut out: Vec<PathBuf> = Vec::with_capacity(roots.len());
    for p in roots {
        let canonical = std::fs::canonicalize(&p).unwrap_or(p);
        if seen.insert(canonical.clone()) {
            out.push(canonical);
        }
    }
    out
}

/// Parse the `RUSTORY_DEVICE_MOUNT_ROOTS` env var, expanding each
/// non-empty path entry to ITSELF + its first TWO levels of children.
/// Two levels are required because udisks2 nests USB volumes one
/// level deeper than the env-injected root: a bind of `/media` is
/// followed by `/media/$USER` (user's auto-mount dir) which then
/// contains `/media/$USER/$VOLUME` (the actual mountpoint with the
/// Lunii marker files). Walking only one level would miss those
/// mountpoints inside Docker, exactly the case this env var exists
/// to fix.
///
/// Symlink handling: we only follow real directories that are NOT
/// symlinks (`symlink_metadata` is_dir without dereferencing). This
/// prevents a `/media/foo -> /media` loop from re-feeding the same
/// root infinitely. `deduplicate_paths` (post-canonicalize) then
/// catches any residual aliasing.
fn extra_mount_roots_from_env() -> Vec<PathBuf> {
    let Ok(raw) = std::env::var(EXTRA_MOUNT_ROOTS_ENV) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in std::env::split_paths(&raw) {
        if entry.as_os_str().is_empty() {
            continue;
        }
        out.push(entry.clone());
        // Two levels of children — bounded to keep the scan budget
        // honest. The user can still pass each mountpoint explicitly
        // if they need finer control.
        if let Ok(children) = std::fs::read_dir(&entry) {
            for c in children.flatten() {
                let p = c.path();
                if is_real_directory(&p) {
                    out.push(p.clone());
                    if let Ok(grandchildren) = std::fs::read_dir(&p) {
                        for g in grandchildren.flatten() {
                            let gp = g.path();
                            if is_real_directory(&gp) {
                                out.push(gp);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// True for an actual directory entry — NOT for symlinks (even
/// symlinks pointing to a directory). Used by `extra_mount_roots_from_env`
/// to refuse following symlinks during the recursive expansion: a
/// `/media/foo -> /media` loop must NOT feed itself.
fn is_real_directory(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) => m.is_dir(),
        Err(_) => false,
    }
}

impl DeviceScanner for SystemDeviceScanner {
    fn scan(&self, budget: Duration) -> Result<DeviceScanReport, AppError> {
        let started = Instant::now();
        let roots = self.enumerate_roots();
        // Per-scan deadline shared with read_bounded so a hung mount
        // cannot keep the spawn_blocking worker alive past the budget.
        // The flag is set once `started.elapsed() >= budget` is
        // observed by the per-root loop; the next read inside
        // read_bounded notices it and aborts the I/O with an Interrupted
        // error that the soft-error branch surfaces as `kind = timeout`.
        let deadline_exceeded = Arc::new(AtomicBool::new(false));

        let mut candidates: Vec<DeviceCandidate> = Vec::new();
        let mut truncated = false;
        // Track per-root soft errors so we can fail-loud when EVERY
        // root either was permission-denied or vanished before we
        // could read it — silently returning "no device" in that case
        // makes the AC #3 "scan transport failure" branch (the panel's
        // `Détection indisponible`) dead in production.
        let mut first_soft_error_kind: Option<&'static str> = None;

        for root in roots {
            if started.elapsed() >= budget {
                truncated = true;
                deadline_exceeded.store(true, Ordering::Relaxed);
                break;
            }
            match probe_root(&root, &deadline_exceeded) {
                Ok(Some(candidate)) => candidates.push(candidate),
                Ok(None) => continue,
                Err(ProbeError::FatalScan(io_err)) => {
                    return Err(map_fatal_scan_error(io_err));
                }
                Err(ProbeError::Soft(io_err)) => {
                    if first_soft_error_kind.is_none() {
                        first_soft_error_kind = Some(io_kind_label(io_err.kind()));
                    }
                }
            }
        }

        if candidates.is_empty() && !truncated {
            if let Some(kind) = first_soft_error_kind {
                return Err(soft_scan_error(kind));
            }
        }

        Ok(DeviceScanReport {
            candidates,
            elapsed: started.elapsed(),
            truncated_due_to_timeout: truncated,
        })
    }
}

#[derive(Debug)]
enum ProbeError {
    /// Per-root error that should not halt the whole scan. The inner
    /// `io::Error` is consumed by the scan loop to derive a closed-set
    /// `details.kind` for the diagnostic event when every root fails.
    Soft(std::io::Error),
    /// Whole-scan fatal error (e.g. OS enumeration broken). Currently
    /// not produced by `probe_root`; reserved for future expansion.
    #[allow(dead_code)]
    FatalScan(std::io::Error),
}

fn probe_root(
    root: &Path,
    deadline_exceeded: &Arc<AtomicBool>,
) -> Result<Option<DeviceCandidate>, ProbeError> {
    // Fixed family precedence: a volume carrying `.md` is a LUNII
    // candidate — even when `.mdf` coexists — and the historical Lunii
    // probe applies verbatim. Only a volume WITHOUT `.md` enters the
    // FLAM probe. The observable Lunii behavior never changes by a
    // byte (device-support-profile.md → FLAM recognition markers).
    let md_path = root.join(LUNII_PRIMARY_MARKER);
    if md_path.is_file() {
        return probe_lunii_root(root, &md_path, deadline_exceeded);
    }
    // A `.md` entry present under ANY other shape (directory, broken
    // symlink, special file) keeps the volume OUT of the FLAM probe:
    // such a volume was ignored before FLAM recognition existed, and
    // "without `.md`" means without the ENTRY — not "without a regular
    // `.md` file". The Lunii gate above stays `is_file()` VERBATIM so
    // historical symlinked-regular `.md` volumes keep probing as Lunii.
    if std::fs::symlink_metadata(&md_path).is_ok() {
        return Ok(None);
    }
    probe_flam_root(root, deadline_exceeded)
}

fn probe_lunii_root(
    root: &Path,
    md_path: &Path,
    deadline_exceeded: &Arc<AtomicBool>,
) -> Result<Option<DeviceCandidate>, ProbeError> {
    let pi_path = root.join(LUNII_DEVICE_ID_MARKER);
    let bt_path = root.join(LUNII_BINARY_TOKEN_MARKER);

    // An oversized marker is the strongest "this is NOT a Lunii"
    // signal we have: real `.md` is < 200 B, real `.pi` is 32 B. A
    // file that overflows the 4 KB cap was either corrupted on
    // disk or planted by a different family of device. Reject the
    // candidate entirely rather than truncating and feeding a
    // partial payload into the classifier (which would happily read
    // a fake version byte from the first byte).
    let metadata_payload = match read_bounded(md_path, deadline_exceeded) {
        Ok(p) => p,
        Err(ReadBoundedError::Oversize) => return Ok(None),
        Err(ReadBoundedError::Io(e)) => return Err(ProbeError::Soft(e)),
    };
    // A `.md` present without `.pi` is NOT a healthy Lunii — but it
    // is also NOT "this is not a Lunii". The user plugged in a
    // device whose primary marker matches; surface the candidate
    // with an empty `pi_payload` so the application classifier
    // produces `MetadataCorrupt` and the panel renders the
    // "marqueurs appareil incomplets" copy. Silently dropping this
    // case here would hide the symptom from the user (the previous
    // behavior would skip the volume and look like "no device").
    let pi_payload = if pi_path.is_file() {
        match read_bounded(&pi_path, deadline_exceeded) {
            Ok(p) => p,
            Err(ReadBoundedError::Oversize) => return Ok(None),
            Err(ReadBoundedError::Io(e)) => return Err(ProbeError::Soft(e)),
        }
    } else {
        Vec::new()
    };

    Ok(Some(DeviceCandidate {
        mount_path: root.to_path_buf(),
        // `sysinfo` exposes per-disk metadata on Windows but the cross-
        // platform serial story is uneven on Linux/macOS. Returning
        // `None` here keeps the scanner honest and the device_identifier
        // computed downstream remains stable across reboots because the
        // `.pi` payload is itself stable.
        volume_serial: None,
        facts: CandidateFacts::Lunii {
            metadata_payload,
            pi_payload,
            has_bt: bt_path.is_file(),
        },
    }))
}

/// FLAM probe — born hardened. The `.mdf` read is no-follow end to end
/// ([`read_bounded_no_follow`]); the required `str/` and `etc/` entries
/// must be REAL directories (`is_real_directory`, no-follow). An
/// empty `.mdf` still surfaces the candidate so a broken FLAM is SEEN
/// and explained by the classifier (`metadataCorrupt`), never silently
/// skipped. The historical Lunii probe is deliberately NOT retrofitted
/// here — its no-follow parity is a separate, deferred hardening of
/// the most sensitive path of the product.
fn probe_flam_root(
    root: &Path,
    deadline_exceeded: &Arc<AtomicBool>,
) -> Result<Option<DeviceCandidate>, ProbeError> {
    let mdf_path = root.join(FLAM_PRIMARY_MARKER);
    let mdf_payload = match read_bounded_no_follow(&mdf_path, deadline_exceeded) {
        Ok(Some(p)) => p,
        // Absent / symlink / irregular / oversize / swapped-in entry /
        // per-volume I/O failure: not a readable FLAM marker — the
        // volume is ignored and the scan continues (never a scan-level
        // `AppError` on the FLAM path).
        Ok(None) => return Ok(None),
        // Only the shared scan deadline reaches here; the soft error
        // lets the scan loop surface the honest timeout outcome.
        Err(e) => return Err(ProbeError::Soft(e)),
    };
    Ok(Some(DeviceCandidate {
        mount_path: root.to_path_buf(),
        // Same rationale as the Lunii probe: the serial story is uneven
        // across platforms; the `.mdf` payload keeps the identifier
        // stable across reboots.
        volume_serial: None,
        facts: CandidateFacts::Flam {
            mdf_payload,
            has_str_dir: is_real_directory(&root.join(FLAM_STORY_DIR)),
            has_etc_dir: is_real_directory(&root.join(FLAM_CONFIG_DIR)),
        },
    }))
}

/// Failure modes from [`read_bounded`]. `Oversize` is a typed signal
/// that the file on disk exceeded [`MAX_METADATA_FILE_BYTES`] — a
/// strong "this is not a Lunii marker" indicator that the caller
/// must surface (and NOT confuse with a transient I/O hiccup).
enum ReadBoundedError {
    Oversize,
    Io(std::io::Error),
}

fn read_bounded(
    path: &Path,
    deadline_exceeded: &Arc<AtomicBool>,
) -> Result<Vec<u8>, ReadBoundedError> {
    if deadline_exceeded.load(Ordering::Relaxed) {
        return Err(ReadBoundedError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "scan deadline exceeded before open",
        )));
    }
    let file = File::open(path).map_err(ReadBoundedError::Io)?;
    let mut buf = Vec::new();
    // Read MAX + 1 bytes so we can detect the overflow case without
    // a separate `metadata.len()` round-trip (which would race with
    // a file growing during the scan).
    file.take(MAX_METADATA_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(ReadBoundedError::Io)?;
    if deadline_exceeded.load(Ordering::Relaxed) {
        return Err(ReadBoundedError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "scan deadline exceeded during read",
        )));
    }
    if buf.len() as u64 > MAX_METADATA_FILE_BYTES {
        return Err(ReadBoundedError::Oversize);
    }
    Ok(buf)
}

/// Open the file WITHOUT following a symlink at its final component and
/// WITHOUT blocking on a special file. On Unix the open itself carries
/// `O_NOFOLLOW | O_NONBLOCK` (ABI-frozen per-OS/ARCH flag values —
/// `libc` is not a direct dependency of this crate): a symlink swapped
/// in after the lstat FAILS the open (`ELOOP`) instead of being
/// followed, and a swapped-in FIFO opens non-blocking (then fails the
/// handle re-check) instead of suspending the blocking worker forever.
/// On other platforms the lstat + handle re-check in the caller remain
/// the only guard — the swap window stays theoretical there
/// (documented limit).
///
/// The numeric flags are only valid for the OS/arch couples listed
/// below; any other Unix target fails the BUILD (`compile_error!`)
/// rather than silently opening with wrong bits (on powerpc Linux for
/// instance, `0o400000` is `O_DIRECT` — reads could fail `EINVAL`).
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Linux asm-generic ABI (x86, x86_64, arm, aarch64, riscv,
        // loongarch): O_NOFOLLOW = 0o400000, O_NONBLOCK = 0o4000.
        #[cfg(all(
            target_os = "linux",
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv32",
                target_arch = "riscv64",
                target_arch = "loongarch64",
            )
        ))]
        const NO_FOLLOW_NON_BLOCK: i32 = 0o400000 | 0o4000;
        #[cfg(all(
            target_os = "linux",
            not(any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv32",
                target_arch = "riscv64",
                target_arch = "loongarch64",
            ))
        ))]
        compile_error!(
            "O_NOFOLLOW/O_NONBLOCK values are arch-specific on Linux (mips/sparc/alpha/parisc/powerpc differ); add the validated constants for this architecture"
        );
        // BSD-lineage ABI (macOS, iOS, FreeBSD, OpenBSD, NetBSD):
        // O_NOFOLLOW = 0x0100, O_NONBLOCK = 0x0004.
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
        ))]
        const NO_FOLLOW_NON_BLOCK: i32 = 0x0100 | 0x0004;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
        )))]
        compile_error!(
            "O_NOFOLLOW/O_NONBLOCK values are OS-specific (illumos/Solaris/AIX differ); add the validated constants for this Unix target"
        );
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW_NON_BLOCK)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        File::open(path)
    }
}

/// Bounded no-follow read of a candidate FLAM marker. Same deadline
/// checks and byte bound as [`read_bounded`], hardened end to end
/// (the `open_bounded_regular` pattern): `symlink_metadata` gates the
/// common case up-front, [`open_no_follow`] closes the swap window at
/// the open itself (Unix), then the OPENED handle is re-checked — it
/// must still be a regular in-bound file whose `(dev, ino)` matches
/// the lstat'ed entry, so a TOCTOU swap is refused instead of read.
///
/// Returns:
/// - `Ok(Some(payload))` — the marker is a regular, in-bound file
///   (an EMPTY payload is a valid, VISIBLE outcome);
/// - `Ok(None)` — absent, symlink, irregular, oversize, swapped
///   entry, OR any per-volume I/O failure (open/fstat/read): "not a
///   readable FLAM marker", the volume is IGNORED and the scan
///   continues. Unlike the Lunii probe, a failing FLAM volume never
///   escalates to a scan-level `AppError` — it must not mask a
///   healthy candidate sitting on another mount;
/// - `Err(io)` — the shared scan DEADLINE only (budget exhausted),
///   so a hostile volume cannot eat the other candidates' budget.
fn read_bounded_no_follow(
    path: &Path,
    deadline_exceeded: &Arc<AtomicBool>,
) -> Result<Option<Vec<u8>>, std::io::Error> {
    if deadline_exceeded.load(Ordering::Relaxed) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "scan deadline exceeded before open",
        ));
    }
    let Ok(pre) = std::fs::symlink_metadata(path) else {
        // Absent marker: not a FLAM candidate. A transient lstat
        // failure folds into the same "no candidate" outcome — the
        // 3 s polling re-converges on the next pass.
        return Ok(None);
    };
    if pre.file_type().is_symlink() || !pre.is_file() {
        return Ok(None);
    }
    if pre.len() > MAX_METADATA_FILE_BYTES {
        return Ok(None);
    }
    let Ok(file) = open_no_follow(path) else {
        return Ok(None);
    };
    let Ok(meta) = file.metadata() else {
        return Ok(None);
    };
    if !meta.is_file() || meta.len() > MAX_METADATA_FILE_BYTES {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // The handle must be the very entry the lstat classified — a
        // symlink/file swapped in between changes (dev, ino).
        if meta.dev() != pre.dev() || meta.ino() != pre.ino() {
            return Ok(None);
        }
    }
    let mut buf = Vec::new();
    // MAX + 1 so a file GROWN past the bound between the fstat and the
    // read is still caught without a second metadata round-trip.
    if file
        .take(MAX_METADATA_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Ok(None);
    }
    if deadline_exceeded.load(Ordering::Relaxed) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "scan deadline exceeded during read",
        ));
    }
    if buf.len() as u64 > MAX_METADATA_FILE_BYTES {
        return Ok(None);
    }
    Ok(Some(buf))
}

/// Whole-scan fatal error mapping. Only `os_enum` reaches this path.
fn map_fatal_scan_error(io_err: std::io::Error) -> AppError {
    AppError::device_scan_failed(
        "Détection indisponible: vérifie que la Lunii est branchée et réessaie.",
        "Réessaie la détection ; si le problème persiste, consulte le profil de support.",
    )
    .with_details(serde_json::json!({
        "source": "os_enum",
        "kind": io_kind_label(io_err.kind()),
    }))
}

/// "Every root failed before producing a candidate" mapping. The
/// closed-set `kind` mirrors `io_kind_label` and is documented in
/// `docs/architecture/ui-states.md#Device Detection Contract`.
fn soft_scan_error(kind: &'static str) -> AppError {
    AppError::device_scan_failed(
        "Détection indisponible: vérifie que la Lunii est branchée et réessaie.",
        "Réessaie la détection ; si le problème persiste, consulte le profil de support.",
    )
    .with_details(serde_json::json!({
        "source": "fs_read",
        "kind": kind,
    }))
}

#[allow(dead_code)] // legacy entry point kept so external callers compile cleanly
fn map_fatal_probe_error(err: ProbeError) -> AppError {
    let (source, kind) = match err {
        ProbeError::Soft(_) => ("fs_read", "soft"),
        ProbeError::FatalScan(io_err) => ("os_enum", io_kind_label(io_err.kind())),
    };
    AppError::device_scan_failed(
        "Détection indisponible: vérifie que la Lunii est branchée et réessaie.",
        "Réessaie la détection ; si le problème persiste, consulte le profil de support.",
    )
    .with_details(serde_json::json!({
        "source": source,
        "kind": kind,
    }))
}

fn io_kind_label(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::TimedOut => "timeout",
        std::io::ErrorKind::Interrupted => "interrupted",
        _ => "io_other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::fixtures::{
        temp_flam_mount, temp_flam_mount_corrupt, FlamCorruptKind,
    };

    fn no_deadline() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn probe(root: &Path) -> Option<DeviceCandidate> {
        probe_root(root, &no_deadline()).expect("probe must not error")
    }

    fn flam_facts(candidate: &DeviceCandidate) -> (&Vec<u8>, bool, bool) {
        match &candidate.facts {
            CandidateFacts::Flam {
                mdf_payload,
                has_str_dir,
                has_etc_dir,
            } => (mdf_payload, *has_str_dir, *has_etc_dir),
            other => panic!("expected Flam facts, got {other:?}"),
        }
    }

    #[test]
    fn probe_root_recognizes_conforming_flam_fixture() {
        let (_g, root) = temp_flam_mount();
        let candidate = probe(&root).expect("conforming FLAM must surface");
        let (mdf_payload, has_str_dir, has_etc_dir) = flam_facts(&candidate);
        assert!(!mdf_payload.is_empty());
        assert!(has_str_dir);
        assert!(has_etc_dir);
    }

    #[test]
    fn probe_root_surfaces_empty_mdf_fixture_as_visible_candidate() {
        // A broken FLAM must be SEEN (classified corrupt downstream),
        // never silently skipped.
        let (_g, root) = temp_flam_mount_corrupt(FlamCorruptKind::EmptyMdf);
        let candidate = probe(&root).expect("empty .mdf must stay visible");
        let (mdf_payload, _, _) = flam_facts(&candidate);
        assert!(mdf_payload.is_empty());
    }

    #[test]
    fn probe_root_ignores_oversize_mdf_fixture() {
        let (_g, root) = temp_flam_mount_corrupt(FlamCorruptKind::OversizeMdf);
        assert!(probe(&root).is_none());
    }

    #[test]
    fn probe_root_flags_missing_str_dir_fixture() {
        let (_g, root) = temp_flam_mount_corrupt(FlamCorruptKind::MissingStrDir);
        let candidate = probe(&root).expect("incomplete FLAM must stay visible");
        let (_, has_str_dir, has_etc_dir) = flam_facts(&candidate);
        assert!(!has_str_dir);
        assert!(has_etc_dir);
    }

    #[test]
    fn probe_root_flags_missing_etc_dir_fixture() {
        let (_g, root) = temp_flam_mount_corrupt(FlamCorruptKind::MissingEtcDir);
        let candidate = probe(&root).expect("incomplete FLAM must stay visible");
        let (_, has_str_dir, has_etc_dir) = flam_facts(&candidate);
        assert!(has_str_dir);
        assert!(!has_etc_dir);
    }

    #[test]
    fn probe_root_ignores_volume_whose_md_entry_is_a_directory() {
        // Family precedence, historical shape: a `.md` DIRECTORY is not
        // a Lunii marker, and its presence keeps the volume out of the
        // FLAM probe too — ignored, the pre-FLAM behavior.
        let (_g, root) = temp_flam_mount();
        std::fs::create_dir(root.join(LUNII_PRIMARY_MARKER)).expect("mkdir .md");
        assert!(probe(&root).is_none());
    }
}
