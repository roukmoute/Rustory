use sha2::{Digest, Sha256};

use crate::domain::story::schema::CanonicalStructure;

/// Serialize a [`CanonicalStructure`] to its canonical JSON form. The
/// result is the exact byte sequence written to `stories.structure_json`
/// and fed to [`content_checksum`]; any drift (whitespace, key order,
/// field renaming) would invalidate every previously-stored checksum.
pub fn canonical_structure_json(structure: &CanonicalStructure) -> String {
    serde_json::to_string(structure).expect("canonical structure never fails to serialize")
}

/// Compute the SHA-256 hex digest of the canonical JSON bytes. Lower-case
/// hex, 64 characters exactly — the stored value is the ground truth the
/// UI would eventually use to detect silent on-disk corruption.
pub fn content_checksum(json: &str) -> String {
    let digest = Sha256::digest(json.as_bytes());
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_shape_is_stable() {
        let structure = CanonicalStructure::minimal();
        assert_eq!(
            canonical_structure_json(&structure),
            "{\"schemaVersion\":1,\"nodes\":[]}",
        );
    }

    #[test]
    fn content_checksum_is_64_hex_chars_lowercase() {
        let checksum = content_checksum("{\"schemaVersion\":1,\"nodes\":[]}");
        assert_eq!(checksum.len(), 64);
        assert!(
            checksum
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "checksum must be lowercase hex: {checksum}"
        );
    }

    #[test]
    fn content_checksum_is_deterministic() {
        let a = content_checksum("abc");
        let b = content_checksum("abc");
        assert_eq!(a, b);
    }

    #[test]
    fn content_checksum_detects_single_byte_change() {
        assert_ne!(content_checksum("a"), content_checksum("b"));
        assert_ne!(content_checksum(""), content_checksum("a"));
    }

    #[test]
    fn content_checksum_matches_known_sha256_of_empty_string() {
        // Reference value from the SHA-256 test vectors so a drift in the
        // implementation (e.g., accidental prefix/padding) is noticed.
        assert_eq!(
            content_checksum(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
    }
}
