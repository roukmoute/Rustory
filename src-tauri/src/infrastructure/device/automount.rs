//! Best-effort auto-mount for plugged Lunii volumes the desktop session
//! did not auto-mount.
//!
//! Linux only — on macOS / Windows the OS mounts USB Mass Storage
//! volumes by default and this module is a no-op. On minimal Linux
//! desktops (no GNOME/KDE session daemon, locked-down polkit) udisks2
//! may not trigger an automatic mount when a Lunii is plugged in. To
//! keep the panel responsive without asking the user to run
//! `udisksctl mount` manually, the scanner asks udisks2 (over D-Bus,
//! via zbus) to mount the candidates that match a tight Lunii
//! signature.
//!
//! Filtering policy (Linux):
//! - the candidate block device must be a partition whose `Drive` path
//!   contains "STM" — Lunii devices report through STM32-based USB
//!   bridges and udisks2 surfaces "STM_Product_*" as the drive id.
//!   This excludes generic USB sticks (SanDisk, Kingston, …) from the
//!   auto-mount path so we never mutate unrelated media without the
//!   user's intent.
//! - `IdType` must be "vfat" and `IdUsage` must be "filesystem".
//! - `MountPoints` must be empty (already-mounted volumes are left
//!   alone — `read_connected_lunii` will pick them up via the regular
//!   scan).
//!
//! Opt-out: set `RUSTORY_DEVICE_AUTOMOUNT=0` to disable the auto-mount
//! path entirely. Useful for tests and for users who want to keep
//! every mount under their explicit control.

use std::path::PathBuf;

/// Outcome of a single auto-mount attempt. Returned verbatim by the
/// scan layer so it can be surfaced in the device diagnostics log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountAttempt {
    /// Device path observed (`/dev/sda1`, etc.).
    pub device: String,
    pub outcome: MountOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountOutcome {
    /// udisks2 accepted the mount request and reported the resulting
    /// mount point.
    Mounted { mountpoint: PathBuf },
    /// Volume was already mounted before this attempt — skipped.
    AlreadyMounted,
    /// Filtering policy rejected this candidate (not a Lunii volume,
    /// wrong filesystem, etc.). Reason carries a closed-set tag.
    Skipped { reason: &'static str },
    /// udisks2 refused or D-Bus call failed. `reason` is a stable
    /// short tag for diagnostics; never the raw error message.
    Failed { reason: &'static str },
}

/// Decide whether a candidate volume should be auto-mounted by
/// Rustory. Pure function — exposed for unit tests so the policy can
/// be exercised without a live udisks2.
pub fn looks_like_lunii_candidate(
    drive_path: &str,
    id_type: &str,
    id_usage: &str,
    mount_points: &[String],
) -> bool {
    if !drive_path.contains("STM") {
        return false;
    }
    if id_type != "vfat" {
        return false;
    }
    if id_usage != "filesystem" {
        return false;
    }
    if !mount_points.is_empty() {
        return false;
    }
    true
}

const ENV_OPT_OUT: &str = "RUSTORY_DEVICE_AUTOMOUNT";

fn opt_out() -> bool {
    parse_opt_out(std::env::var(ENV_OPT_OUT).ok().as_deref())
}

/// Pure parser exposed for tests so the opt-out policy can be
/// exercised without ever touching the process-wide env (which would
/// race other tests in parallel: `set_var` is `unsafe` precisely
/// because it is not thread-safe).
fn parse_opt_out(value: Option<&str>) -> bool {
    matches!(value, Some("0"))
}

/// Entry point: best-effort auto-mount of every plugged Lunii volume
/// that udisks2 has not already mounted. On non-Linux platforms or
/// when the opt-out env var is set, returns an empty list silently.
pub fn try_automount_lunii_candidates() -> Vec<MountAttempt> {
    if opt_out() {
        return Vec::new();
    }
    platform::try_automount()
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use zbus::blocking::{Connection, Proxy};
    use zbus::names::InterfaceName;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};

    use super::{looks_like_lunii_candidate, MountAttempt, MountOutcome};

    /// Per-call timeout for every D-Bus round-trip we make to
    /// udisks2. zbus 5's blocking API has no built-in deadline so we
    /// run the call in a dedicated thread and recv_timeout — a hung
    /// daemon leaks ONE thread per stuck call (bounded to a handful
    /// per scan) instead of pinning the `spawn_blocking` worker
    /// forever.
    const DBUS_CALL_TIMEOUT: Duration = Duration::from_millis(1500);

    /// Run a D-Bus call in a worker thread with a wall-clock deadline.
    /// Returns the call result if it landed within `DBUS_CALL_TIMEOUT`,
    /// or `Err("dbus_timeout")` if not. The worker thread itself is
    /// left to drain on its own; we never join it because zbus 5 has
    /// no cancellation primitive.
    fn call_with_timeout<F, T>(label: &'static str, f: F) -> Result<T, &'static str>
    where
        F: FnOnce() -> Result<T, &'static str> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Result<T, &'static str>>();
        thread::spawn(move || {
            let _ = tx.send(f());
        });
        match rx.recv_timeout(DBUS_CALL_TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                let _ = label; // kept for future debug logging
                Err("dbus_timeout")
            }
        }
    }

    const UDISKS2_BUS: &str = "org.freedesktop.UDisks2";
    const MANAGER_PATH: &str = "/org/freedesktop/UDisks2/Manager";
    const MANAGER_IFACE: &str = "org.freedesktop.UDisks2.Manager";
    const BLOCK_IFACE: &str = "org.freedesktop.UDisks2.Block";
    const FS_IFACE: &str = "org.freedesktop.UDisks2.Filesystem";
    const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

    // Mirror of the domain markers so this module stays free of any
    // dependency cycle with `crate::domain::device`. If the canonical
    // markers ever diverge, both lists must be updated in lockstep.
    const LUNII_PRIMARY_MARKER: &str = ".md";
    const LUNII_DEVICE_ID_MARKER: &str = ".pi";

    pub fn try_automount() -> Vec<MountAttempt> {
        let conn = match call_with_timeout("system_bus", || {
            Connection::system().map_err(|_| "system_bus_unreachable")
        }) {
            Ok(c) => c,
            // No system bus / no udisks2 / timeout → silently skip.
            // The regular sysinfo-based scan still runs and the user
            // can manually mount if needed.
            Err(_) => return Vec::new(),
        };

        let conn_clone = conn.clone();
        let block_paths = match call_with_timeout("get_block_devices", move || {
            list_block_devices(&conn_clone).map_err(|_| "list_blocks_failed")
        }) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

        let mut attempts: Vec<MountAttempt> = Vec::new();
        for path in block_paths {
            let device_label = path.as_str().rsplit('/').next().unwrap_or("?").to_string();
            match inspect_and_maybe_mount(&conn, &path) {
                Ok(Some(outcome)) => attempts.push(MountAttempt {
                    device: format!("/dev/{device_label}"),
                    outcome,
                }),
                Ok(None) => {} // not even a candidate, no attempt recorded
                Err(reason) => attempts.push(MountAttempt {
                    device: format!("/dev/{device_label}"),
                    outcome: MountOutcome::Failed { reason },
                }),
            }
        }
        attempts
    }

    fn list_block_devices(conn: &Connection) -> zbus::Result<Vec<OwnedObjectPath>> {
        let proxy = Proxy::new(
            conn,
            UDISKS2_BUS,
            MANAGER_PATH,
            InterfaceName::try_from(MANAGER_IFACE).expect("static iface name"),
        )?;
        // Options dict is required by the API (can be empty).
        let options: HashMap<String, OwnedValue> = HashMap::new();
        let paths: Vec<OwnedObjectPath> = proxy.call("GetBlockDevices", &(options,))?;
        Ok(paths)
    }

    fn inspect_and_maybe_mount(
        conn: &Connection,
        block_path: &OwnedObjectPath,
    ) -> Result<Option<MountOutcome>, &'static str> {
        let block_props =
            block_properties(conn, block_path).map_err(|_| "block_props_unreachable")?;
        let drive_path = block_props
            .get("Drive")
            .and_then(owned_value_as_object_path)
            .ok_or("block_drive_missing")?;
        let id_type = block_props
            .get("IdType")
            .and_then(owned_value_as_string)
            .unwrap_or_default();
        let id_usage = block_props
            .get("IdUsage")
            .and_then(owned_value_as_string)
            .unwrap_or_default();

        // MountPoints is reported on the Filesystem interface; if the
        // Block has no Filesystem (e.g. partition-table block), the
        // call returns an error — treat that as "no mount points" so
        // looks_like_lunii_candidate evaluates the rest of the
        // criteria and we still bail out cleanly on `Skipped`.
        let mount_points = filesystem_mount_points(conn, block_path).unwrap_or_default();

        if !looks_like_lunii_candidate(drive_path.as_str(), &id_type, &id_usage, &mount_points) {
            if !mount_points.is_empty() {
                return Ok(Some(MountOutcome::AlreadyMounted));
            }
            return Ok(Some(MountOutcome::Skipped {
                reason: filter_skip_reason(drive_path.as_str(), &id_type, &id_usage),
            }));
        }

        // Candidate matches the tight Lunii signature → ask udisks2
        // to mount it. Pass an empty options dict; udisks2 picks safe
        // defaults (read-write, user-private mountpoint).
        let proxy = Proxy::new(
            conn,
            UDISKS2_BUS,
            block_path.as_str(),
            InterfaceName::try_from(FS_IFACE).expect("static iface name"),
        )
        .map_err(|_| "fs_proxy_unreachable")?;
        let options: HashMap<String, OwnedValue> = HashMap::new();
        // Note: zbus 5 has no per-method timeout on a Proxy built
        // from a blocking::Connection; udisks Mount is a fast call
        // (<1s on real hardware) so we accept the theoretical hang
        // risk. The scanner-level budget still bounds the surrounding
        // probe loop, and a misbehaving udisks would surface in the
        // device.jsonl log on the next iteration.
        let mountpoint: String = proxy
            .call("Mount", &(options,))
            .map_err(|_| "udisks_mount_refused")?;
        let mountpoint = PathBuf::from(mountpoint);

        // Defense-in-depth: the pre-mount filter accepts every STM /
        // vfat / unmounted volume. A future block device family that
        // also surfaces a drive id containing "STM" would pass the
        // policy. Confirm the volume is actually Lunii by probing the
        // canonical marker pair at the freshly-resolved mountpoint;
        // if either marker is missing, undo the mount immediately so
        // we never leave a stranger media mounted on the user's
        // behalf. An Unmount failure here is NOT silent: the caller
        // gets a typed `Failed` outcome so support can spot media
        // left mounted by Rustory's auto-mount path.
        if !is_validated_lunii_volume(&mountpoint) {
            let unmount_result =
                proxy.call::<_, _, ()>("Unmount", &(HashMap::<String, OwnedValue>::new(),));
            if unmount_result.is_err() {
                return Ok(Some(MountOutcome::Failed {
                    reason: "unmount_after_validation_failed",
                }));
            }
            return Ok(Some(MountOutcome::Skipped {
                reason: "validated_not_lunii",
            }));
        }
        Ok(Some(MountOutcome::Mounted { mountpoint }))
    }

    fn is_validated_lunii_volume(root: &std::path::Path) -> bool {
        root.join(LUNII_PRIMARY_MARKER).is_file() && root.join(LUNII_DEVICE_ID_MARKER).is_file()
    }

    fn block_properties(
        conn: &Connection,
        block_path: &OwnedObjectPath,
    ) -> zbus::Result<HashMap<String, OwnedValue>> {
        let proxy = Proxy::new(
            conn,
            UDISKS2_BUS,
            block_path.as_str(),
            InterfaceName::try_from(PROPS_IFACE).expect("static iface name"),
        )?;
        let props: HashMap<String, OwnedValue> = proxy.call("GetAll", &(BLOCK_IFACE,))?;
        Ok(props)
    }

    fn filesystem_mount_points(
        conn: &Connection,
        block_path: &OwnedObjectPath,
    ) -> zbus::Result<Vec<String>> {
        let proxy = Proxy::new(
            conn,
            UDISKS2_BUS,
            block_path.as_str(),
            InterfaceName::try_from(PROPS_IFACE).expect("static iface name"),
        )?;
        let mp: OwnedValue = proxy.call("Get", &(FS_IFACE, "MountPoints"))?;
        Ok(mount_points_from_value(&mp))
    }

    fn mount_points_from_value(value: &OwnedValue) -> Vec<String> {
        // MountPoints is `aay` — array of NUL-terminated byte arrays
        // of paths. zbus deserializes it as Vec<Vec<u8>>.
        let Ok(raw): Result<Vec<Vec<u8>>, _> = value.try_clone().and_then(Vec::try_from) else {
            return Vec::new();
        };
        raw.into_iter()
            .map(|mut bytes| {
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                String::from_utf8_lossy(&bytes).into_owned()
            })
            .collect()
    }

    fn owned_value_as_string(value: &OwnedValue) -> Option<String> {
        value
            .try_clone()
            .ok()
            .and_then(|v| String::try_from(v).ok())
    }

    fn owned_value_as_object_path(value: &OwnedValue) -> Option<OwnedObjectPath> {
        value
            .try_clone()
            .ok()
            .and_then(|v| OwnedObjectPath::try_from(v).ok())
    }

    fn filter_skip_reason(drive_path: &str, id_type: &str, id_usage: &str) -> &'static str {
        if !drive_path.contains("STM") {
            return "drive_not_stm";
        }
        if id_type != "vfat" {
            return "id_type_not_vfat";
        }
        if id_usage != "filesystem" {
            return "id_usage_not_filesystem";
        }
        "unknown"
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::MountAttempt;
    pub fn try_automount() -> Vec<MountAttempt> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_lunii_accepts_stm_vfat_unmounted_filesystem() {
        assert!(looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/STM_Product_335A364C3335",
            "vfat",
            "filesystem",
            &[],
        ));
    }

    #[test]
    fn looks_like_lunii_rejects_when_drive_path_misses_stm() {
        assert!(!looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/SanDisk_USB_Drive",
            "vfat",
            "filesystem",
            &[],
        ));
    }

    #[test]
    fn looks_like_lunii_rejects_non_vfat_filesystem() {
        assert!(!looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/STM_Product_xxx",
            "ntfs",
            "filesystem",
            &[],
        ));
    }

    #[test]
    fn looks_like_lunii_rejects_non_filesystem_usage() {
        assert!(!looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/STM_Product_xxx",
            "vfat",
            "raid",
            &[],
        ));
    }

    #[test]
    fn looks_like_lunii_rejects_already_mounted_volume() {
        assert!(!looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/STM_Product_xxx",
            "vfat",
            "filesystem",
            &["/media/roukmoute/D2C9-7A59".to_string()],
        ));
    }

    #[test]
    fn looks_like_lunii_rejects_empty_id_type() {
        assert!(!looks_like_lunii_candidate(
            "/org/freedesktop/UDisks2/drives/STM_Product_xxx",
            "",
            "filesystem",
            &[],
        ));
    }

    #[test]
    fn parse_opt_out_only_triggers_on_literal_zero() {
        // Pure parser path — no env-var mutation, safe in parallel.
        // The env-driven `opt_out()` is a thin wrapper around this
        // function and is exercised end-to-end via the
        // try_automount_lunii_candidates smoke test (which is
        // serialized to its own `examples/automount_smoke.rs`
        // binary, not run inside the unit-test process).
        assert!(parse_opt_out(Some("0")));
        assert!(!parse_opt_out(Some("1")));
        assert!(!parse_opt_out(Some("false")));
        assert!(!parse_opt_out(Some("")));
        assert!(!parse_opt_out(None));
    }
}
