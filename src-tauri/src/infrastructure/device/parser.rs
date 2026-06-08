use sha2::{Digest, Sha256};

/// Errors produced by [`parse_metadata_version`]. The application layer
/// maps these into `UnsupportedReason::MetadataCorrupt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataParseError {
    /// The `.md` payload was empty — no version byte to read.
    Empty,
    /// The first byte sits in a reserved/sentinel range (>127). Even if
    /// `.md` ever extended its format byte to 8 bits, values above 127
    /// have never been observed in the wild and would more likely
    /// indicate a corrupted read.
    OutOfRange(u8),
}

/// Read the metadata format version (3, 6, 7, …) from the first byte of
/// a `.md` payload.
///
/// Format reference (cross-checked across public OSS implementations):
/// - marian-m12l/studio (Java) parses the leading byte as the format
///   version number for both `device.md` (V1/V2) and the per-pack
///   `.md` files.
/// - o-daneel/Lunii.QT (Python) follows the same convention.
pub fn parse_metadata_version(payload: &[u8]) -> Result<u8, MetadataParseError> {
    let first = payload.first().copied().ok_or(MetadataParseError::Empty)?;
    if first > 127 {
        return Err(MetadataParseError::OutOfRange(first));
    }
    Ok(first)
}

/// Build the opaque device identifier: SHA-256 of the `.pi` payload
/// concatenated with the optional volume serial, hex-encoded lowercase
/// and truncated to 32 characters.
///
/// Why not BLAKE2: Rustory already pins `sha2 = =0.10` (used by
/// `domain::story::integrity` for content checksums). Adding a second
/// crypto crate for an opaque identifier — non-secret, non-adversarial
/// — would inflate the dependency surface without product value.
///
/// Why 32 chars: enough to make collisions astronomical for the
/// realistic domain (handful of devices per household, dozens per
/// professional support session) while keeping log lines readable.
pub fn compute_device_identifier(pi_payload: &[u8], volume_serial: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pi_payload);
    if let Some(serial) = volume_serial {
        hasher.update(serial.as_bytes());
    }
    let digest = hasher.finalize();
    let hex = digest.iter().fold(String::with_capacity(64), |mut acc, b| {
        acc.push_str(&format!("{b:02x}"));
        acc
    });
    hex[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metadata_version_returns_empty_when_payload_is_empty() {
        assert_eq!(parse_metadata_version(&[]), Err(MetadataParseError::Empty));
    }

    #[test]
    fn parse_metadata_version_returns_3_for_origine_v1() {
        assert_eq!(parse_metadata_version(&[3]), Ok(3));
    }

    #[test]
    fn parse_metadata_version_returns_6_for_midgen_v2() {
        assert_eq!(parse_metadata_version(&[6]), Ok(6));
    }

    #[test]
    fn parse_metadata_version_returns_7_for_v3() {
        assert_eq!(parse_metadata_version(&[7]), Ok(7));
    }

    #[test]
    fn parse_metadata_version_returns_unknown_byte_4() {
        assert_eq!(parse_metadata_version(&[4]), Ok(4));
    }

    #[test]
    fn parse_metadata_version_returns_unknown_byte_99() {
        assert_eq!(parse_metadata_version(&[99]), Ok(99));
    }

    #[test]
    fn parse_metadata_version_returns_out_of_range_for_128() {
        assert_eq!(
            parse_metadata_version(&[128]),
            Err(MetadataParseError::OutOfRange(128))
        );
    }

    #[test]
    fn parse_metadata_version_returns_out_of_range_for_255() {
        assert_eq!(
            parse_metadata_version(&[255]),
            Err(MetadataParseError::OutOfRange(255))
        );
    }

    #[test]
    fn parse_metadata_version_ignores_trailing_bytes() {
        // The format byte is the FIRST byte; the rest of `.md` carries
        // device-specific metadata that the scan does not interpret.
        assert_eq!(parse_metadata_version(&[3, 0xff, 0x42, 0x10]), Ok(3));
    }

    #[test]
    fn compute_device_identifier_is_deterministic_for_same_inputs() {
        let a = compute_device_identifier(b"PI_PAYLOAD", Some("SERIAL"));
        let b = compute_device_identifier(b"PI_PAYLOAD", Some("SERIAL"));
        assert_eq!(a, b);
    }

    #[test]
    fn compute_device_identifier_changes_when_serial_changes() {
        let a = compute_device_identifier(b"PI_PAYLOAD", Some("SERIAL_A"));
        let b = compute_device_identifier(b"PI_PAYLOAD", Some("SERIAL_B"));
        assert_ne!(a, b);
    }

    #[test]
    fn compute_device_identifier_changes_when_pi_changes() {
        let a = compute_device_identifier(b"PI_A", Some("SERIAL"));
        let b = compute_device_identifier(b"PI_B", Some("SERIAL"));
        assert_ne!(a, b);
    }

    #[test]
    fn compute_device_identifier_handles_missing_serial() {
        let a = compute_device_identifier(b"PI_PAYLOAD", None);
        let b = compute_device_identifier(b"PI_PAYLOAD", None);
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn compute_device_identifier_returns_32_hex_chars() {
        let id = compute_device_identifier(b"PI", Some("SERIAL"));
        assert_eq!(id.len(), 32);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn compute_device_identifier_does_not_leak_pi_payload_in_output() {
        let pi = b"VERY_SECRET_HARDWARE_SERIAL_NUMBER_42";
        let id = compute_device_identifier(pi, Some("SERIAL"));
        // The identifier must NOT be a substring of the raw PI payload
        // (impossible by construction — it's a digest — but assert it
        // explicitly so a future refactor that breaks the contract
        // fails loudly).
        let pi_str = String::from_utf8_lossy(pi);
        assert!(!pi_str.contains(&id));
        assert!(!id.contains("SERIAL"));
        assert!(!id.contains("HARDWARE"));
    }
}
