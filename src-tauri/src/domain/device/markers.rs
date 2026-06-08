/// Filename markers Rustory looks for at the root of a candidate volume
/// to decide it might be a Lunii. The set is closed by intent: any
/// future family adds its own marker enum.
///
/// **Required for confirmed Lunii**: `.md` (primary identifier carrying
/// the metadata format version) + `.pi` (device-id payload hashed into
/// the opaque `device_identifier`). These two markers are universal
/// across observed V1 / V2 / V3 generations in 2026.
///
/// **Informational only** (presence enriches diagnostics but does NOT
/// gate classification): `.bt`, `.ri`, `.li`. Notably, real-world V3
/// firmware 3.3.2 ships without `.bt`; gating on it would produce a
/// false-negative for working hardware.
///
/// References (cross-checked against public OSS reverse-engineering
/// AND validated against a physical Lunii V3 sample):
/// - marian-m12l/studio (Java reference impl, supports metadata v3/v6/v7)
/// - o-daneel/Lunii.QT (READMEs document `.md/.mdf/.pi/.bt/.ri/.li`
///   markers at volume root for V1/V2/V3 distinction)
/// - physical Lunii V3 fw 3.3.2 sample (2026-04-26): exposes `.md`
///   (128 B, first byte 0x07), `.pi` (32 B), `.pi.hidden`, `.cfg`,
///   `.content/`, `.logo`, `etc/` — no `.bt` present.
pub const LUNII_PRIMARY_MARKER: &str = ".md";
pub const LUNII_DEVICE_ID_MARKER: &str = ".pi";
pub const LUNII_BINARY_TOKEN_MARKER: &str = ".bt";
pub const LUNII_ROM_INFO_MARKER: &str = ".ri";
pub const LUNII_LIB_INFO_MARKER: &str = ".li";

/// Tight upper bound on the `.md` (and other marker) file size we are
/// willing to read into memory during the scan. A genuine `.md` is < 1 KB;
/// anything bigger is treated as `metadata_corrupt`. Bounded I/O on the
/// scan path keeps the NFR4 5-second budget honest even on adversarial
/// mounts.
pub const MAX_METADATA_FILE_BYTES: u64 = 4 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lunii_primary_marker_is_dot_md() {
        assert_eq!(LUNII_PRIMARY_MARKER, ".md");
    }

    #[test]
    fn lunii_device_id_marker_is_dot_pi() {
        assert_eq!(LUNII_DEVICE_ID_MARKER, ".pi");
    }

    #[test]
    fn lunii_binary_token_marker_is_dot_bt() {
        assert_eq!(LUNII_BINARY_TOKEN_MARKER, ".bt");
    }

    #[test]
    fn lunii_required_marker_set_is_md_and_pi() {
        // The required set for a confirmed Lunii is exactly two
        // markers: `.md` for the primary identifier carrying the
        // metadata format version, and `.pi` for the opaque device id.
        // `.bt`, `.ri` and `.li` are informational only — surfaced for
        // diagnostics but never gate the classification (a real V3 fw
        // 3.3.2 was observed without `.bt`).
        let required = [LUNII_PRIMARY_MARKER, LUNII_DEVICE_ID_MARKER];
        assert_eq!(required.len(), 2);
        assert!(required.contains(&".md"));
        assert!(required.contains(&".pi"));
    }

    #[test]
    fn max_metadata_file_bytes_is_4_kb() {
        assert_eq!(MAX_METADATA_FILE_BYTES, 4096);
    }
}
