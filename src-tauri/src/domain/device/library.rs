//! Device-side library inventory model.
//!
//! Reinterprets the `.pi` payload — already hashed into the opaque
//! `device_identifier` by the detection path — as what it actually is on
//! the wire: an ORDERED list of installed pack UUIDs, 16 bytes each, read
//! back to back until EOF. The order is the device's reading order.
//!
//! Scope is INVENTORY only (FR2 / FR33): enumerate the packs by their
//! on-device identifier. No title, cover or content quality is derived
//! here — the device stores none of that for official packs; surfacing a
//! title would require an external catalog that the MVP deliberately does
//! not consult (anti-catalog + offline-first). Each entry is therefore an
//! opaque, "non reconnue" pack identity.
//!
//! Framework-free: pure byte parsing + hex formatting, no `infrastructure`
//! and no `tauri::*`. The infrastructure reader supplies the raw `.pi` /
//! `.pi.hidden` bytes and the per-pack `.content/<SHORT_ID>` presence; the
//! IPC layer maps [`DeviceLibrary`] to a wire DTO.

/// Bytes per pack UUID inside `.pi` / `.pi.hidden`.
pub const LUNII_PACK_UUID_BYTES: usize = 16;

/// Upper bound on the `.pi` (and `.pi.hidden`) payload the library reader
/// is willing to load. DELIBERATELY larger than
/// [`MAX_METADATA_FILE_BYTES`](super::markers::MAX_METADATA_FILE_BYTES)
/// (4 KB): that cap is sized for the detection path which only hashes a
/// short `.pi`, but a real library of more than 256 packs has a `.pi`
/// bigger than 4 KB (256 × 16 = 4096). Reusing the detection cap here
/// would silently truncate the inventory of a well-stocked device. 64 KB
/// covers 4096 packs — far beyond any realistic household library — while
/// still bounding adversarial reads.
pub const MAX_PACK_INDEX_BYTES: u64 = 64 * 1024;

// Compile-time guard: the inventory bound MUST exceed the detection cap
// (`.md` / `.pi` capped at 4 KB), otherwise a well-stocked device's `.pi`
// would be truncated before its inventory is even parsed.
const _: () = assert!(MAX_PACK_INDEX_BYTES > super::markers::MAX_METADATA_FILE_BYTES);

/// Parsed `.pi` / `.pi.hidden` payload: the ordered pack UUIDs plus a
/// flag telling whether the payload length was a clean multiple of 16.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackIndex {
    /// Pack UUIDs in device-reading order.
    pub uuids: Vec<[u8; LUNII_PACK_UUID_BYTES]>,
    /// True when the payload had a trailing partial chunk (< 16 bytes)
    /// that was ignored. A healthy index is an exact multiple of 16; a
    /// remainder hints at corruption or a format we do not fully model.
    pub had_trailing_bytes: bool,
}

/// One device-resident story as surfaced by the inventory read. Carries
/// only opaque identity + structural flags — never an asserted title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStoryEntry {
    /// Canonical lowercase hyphenated pack UUID. Public content identity,
    /// stable across devices; safe to surface (NOT the device serial).
    pub uuid: String,
    /// Uppercase last 8 hex characters of the UUID — the `.content`
    /// sub-folder name and the opaque label shown to the user.
    pub short_id: String,
    /// Listed in `.pi.hidden` rather than `.pi`.
    pub hidden: bool,
    /// A `.content/<short_id>` folder exists on the volume. `false` flags
    /// an orphan/ambiguous entry (referenced but absent) — surfaced, not
    /// hidden (FR33).
    pub content_present: bool,
}

/// Whole device-side inventory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceLibrary {
    pub entries: Vec<DeviceStoryEntry>,
    /// True when any consumed index payload carried trailing partial
    /// bytes (see [`PackIndex::had_trailing_bytes`]).
    pub had_trailing_bytes: bool,
}

/// Parse a `.pi` / `.pi.hidden` payload into its ordered pack UUIDs.
///
/// Each UUID is exactly [`LUNII_PACK_UUID_BYTES`]; a trailing partial
/// chunk is ignored and flagged via [`PackIndex::had_trailing_bytes`]
/// rather than panicking. An empty payload yields zero packs — a valid
/// state for an empty (freshly wiped) Lunii, NOT an error.
pub fn parse_pack_index(payload: &[u8]) -> PackIndex {
    let mut chunks = payload.chunks_exact(LUNII_PACK_UUID_BYTES);
    let mut uuids = Vec::with_capacity(payload.len() / LUNII_PACK_UUID_BYTES);
    for chunk in chunks.by_ref() {
        let mut bytes = [0u8; LUNII_PACK_UUID_BYTES];
        bytes.copy_from_slice(chunk);
        uuids.push(bytes);
    }
    PackIndex {
        uuids,
        had_trailing_bytes: !chunks.remainder().is_empty(),
    }
}

/// Format the 16 UUID bytes as the canonical lowercase hyphenated form
/// (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`). Manual formatting keeps the
/// domain free of any UUID crate dependency.
pub fn format_pack_uuid(bytes: &[u8; LUNII_PACK_UUID_BYTES]) -> String {
    let mut out = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// True when `value` is a canonical lowercase hyphenated UUID (8-4-4-4-12),
/// the exact shape [`format_pack_uuid`] emits. The single source of truth
/// for "is this a well-formed pack UUID?" at every boundary that accepts
/// one (import input, manual-title input), so the rule never drifts.
pub fn is_canonical_pack_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !(b.is_ascii_digit() || (b'a'..=b'f').contains(b)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Derive the `.content` sub-folder name: the uppercase hex of the last
/// four UUID bytes (= the last 8 characters of the canonical string).
/// This mirrors the device's own folder-naming convention.
pub fn pack_short_id(bytes: &[u8; LUNII_PACK_UUID_BYTES]) -> String {
    bytes[12..16]
        .iter()
        .fold(String::with_capacity(8), |mut acc, b| {
            acc.push_str(&format!("{b:02X}"));
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid_bytes(tail: [u8; 4]) -> [u8; 16] {
        let mut b = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x00, 0x00,
            0x00, 0x00,
        ];
        b[12..16].copy_from_slice(&tail);
        b
    }

    #[test]
    fn parse_pack_index_empty_payload_yields_zero_packs() {
        let index = parse_pack_index(&[]);
        assert!(index.uuids.is_empty());
        assert!(!index.had_trailing_bytes);
    }

    #[test]
    fn parse_pack_index_reads_one_uuid_per_16_bytes_in_order() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&uuid_bytes([0xAA, 0xBB, 0xCC, 0xDD]));
        payload.extend_from_slice(&uuid_bytes([0x01, 0x02, 0x03, 0x04]));
        let index = parse_pack_index(&payload);
        assert_eq!(index.uuids.len(), 2);
        assert_eq!(index.uuids[0], uuid_bytes([0xAA, 0xBB, 0xCC, 0xDD]));
        assert_eq!(index.uuids[1], uuid_bytes([0x01, 0x02, 0x03, 0x04]));
        assert!(!index.had_trailing_bytes);
    }

    #[test]
    fn parse_pack_index_flags_trailing_partial_chunk() {
        let mut payload = uuid_bytes([1, 2, 3, 4]).to_vec();
        payload.extend_from_slice(&[0xFF, 0xEE, 0xDD]); // 3 dangling bytes
        let index = parse_pack_index(&payload);
        assert_eq!(index.uuids.len(), 1);
        assert!(index.had_trailing_bytes);
    }

    #[test]
    fn parse_pack_index_handles_256_packs_without_loss() {
        // 256 packs × 16 = 4096 bytes — the exact size that would be
        // truncated by the 4 KB detection cap. The dedicated bound keeps
        // them all.
        let payload = vec![0u8; 256 * LUNII_PACK_UUID_BYTES];
        let index = parse_pack_index(&payload);
        assert_eq!(index.uuids.len(), 256);
        assert!(!index.had_trailing_bytes);
        assert!(payload.len() as u64 <= MAX_PACK_INDEX_BYTES);
    }

    #[test]
    fn max_pack_index_bytes_is_64_kb() {
        // The ordering vs the detection cap is enforced at compile time
        // by the `const _: () = assert!(..)` guard in the module body.
        assert_eq!(MAX_PACK_INDEX_BYTES, 64 * 1024);
    }

    #[test]
    fn format_pack_uuid_emits_canonical_lowercase_hyphenated_form() {
        let bytes = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x77, 0x88,
        ];
        assert_eq!(
            format_pack_uuid(&bytes),
            "12345678-9abc-def0-1122-334455667788"
        );
    }

    #[test]
    fn pack_short_id_is_uppercase_hex_of_last_four_bytes() {
        let bytes = uuid_bytes([0xab, 0xcd, 0x12, 0x34]);
        assert_eq!(pack_short_id(&bytes), "ABCD1234");
        // Matches the tail of the canonical string, uppercased.
        let canonical = format_pack_uuid(&bytes).to_uppercase();
        assert_eq!(pack_short_id(&bytes), &canonical[canonical.len() - 8..]);
    }

    #[test]
    fn device_library_default_is_empty() {
        let lib = DeviceLibrary::default();
        assert!(lib.entries.is_empty());
        assert!(!lib.had_trailing_bytes);
    }

    #[test]
    fn is_canonical_pack_uuid_accepts_the_format_pack_uuid_output() {
        let bytes = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
            0x77, 0x88,
        ];
        assert!(is_canonical_pack_uuid(&format_pack_uuid(&bytes)));
    }

    #[test]
    fn is_canonical_pack_uuid_rejects_malformed_shapes() {
        assert!(!is_canonical_pack_uuid("")); // empty
        assert!(!is_canonical_pack_uuid(
            "12345678-9abc-def0-1122-33445566778"
        )); // too short
        assert!(!is_canonical_pack_uuid(
            "12345678-9abc-def0-1122-3344556677889"
        )); // too long
        assert!(!is_canonical_pack_uuid(
            "12345678-9ABC-def0-1122-334455667788"
        )); // uppercase
        assert!(!is_canonical_pack_uuid(
            "123456789abcdef0112233445566778800"
        )); // no hyphens
        assert!(!is_canonical_pack_uuid(
            "12345678_9abc_def0_1122_334455667788"
        )); // wrong sep
        assert!(!is_canonical_pack_uuid(
            "g2345678-9abc-def0-1122-334455667788"
        )); // non-hex
    }
}
