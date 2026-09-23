//! Does the connected device have room for a pack? — the PURE decision and
//! the user-facing size wording behind the pre-send free-space check.
//!
//! The device write stages the WHOLE pack on the volume, promotes it by
//! `rename`, and only THEN removes the pack it replaces (see the V3 pack
//! writer). So the peak need is the full NEW pack, whatever the old one
//! weighed — the check never discounts a replacement.
//!
//! No I/O here: the caller supplies the two numbers.

/// The outcome of comparing what a send needs against what the device has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSpaceVerdict {
    /// The device has at least `required` bytes free.
    Fits,
    /// `missing` bytes are lacking — always strictly positive.
    Short { missing: u64 },
}

/// Compare a pack's byte need against the device's free bytes. A pack that
/// EXACTLY fills the free space still fits: the writer needs no byte beyond
/// the files themselves.
pub fn check_device_space(required: u64, available: u64) -> DeviceSpaceVerdict {
    match required.checked_sub(available) {
        Some(missing) if missing > 0 => DeviceSpaceVerdict::Short { missing },
        _ => DeviceSpaceVerdict::Fits,
    }
}

/// Render a byte count the way the product speaks of storage: decimal units
/// (`Mo` = 10⁶, `Go` = 10⁹ — the units printed on the device's packaging and
/// already used by the voice-download line), French decimal comma, one
/// decimal for gigabytes. Below a megabyte the exact figure tells the user
/// nothing actionable, so it reads `moins de 1 Mo`.
pub fn format_device_bytes(bytes: u64) -> String {
    let megabytes = (bytes as f64 / 1_000_000.0).round();
    if megabytes < 1.0 {
        return "moins de 1 Mo".to_string();
    }
    if megabytes < 1_000.0 {
        return format!("{megabytes:.0} Mo");
    }
    let gigabytes = (bytes as f64 / 1_000_000_000.0 * 10.0).round() / 10.0;
    format!("{} Go", format!("{gigabytes:.1}").replace('.', ","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_that_fits_exactly_is_not_short() {
        assert_eq!(check_device_space(0, 0), DeviceSpaceVerdict::Fits);
        assert_eq!(check_device_space(1_000, 1_000), DeviceSpaceVerdict::Fits);
        assert_eq!(check_device_space(1_000, 1_001), DeviceSpaceVerdict::Fits);
    }

    #[test]
    fn a_pack_over_the_free_space_names_exactly_what_is_missing() {
        assert_eq!(
            check_device_space(1_001, 1_000),
            DeviceSpaceVerdict::Short { missing: 1 }
        );
        assert_eq!(
            check_device_space(3_000_000_000, 1_000_000_000),
            DeviceSpaceVerdict::Short {
                missing: 2_000_000_000
            }
        );
        // An empty device: the whole pack is missing, no underflow.
        assert_eq!(
            check_device_space(u64::MAX, 0),
            DeviceSpaceVerdict::Short { missing: u64::MAX }
        );
    }

    #[test]
    fn sizes_are_spelled_in_decimal_units_with_a_french_comma() {
        assert_eq!(format_device_bytes(0), "moins de 1 Mo");
        assert_eq!(format_device_bytes(499_999), "moins de 1 Mo");
        assert_eq!(format_device_bytes(500_000), "1 Mo");
        assert_eq!(format_device_bytes(250_000_000), "250 Mo");
        assert_eq!(format_device_bytes(999_400_000), "999 Mo");
        // The megabyte rounding tips over a thousand: promoted to gigabytes
        // rather than printed as "1000 Mo".
        assert_eq!(format_device_bytes(999_500_000), "1,0 Go");
        assert_eq!(format_device_bytes(1_400_000_000), "1,4 Go");
        assert_eq!(format_device_bytes(8_000_000_000), "8,0 Go");
        // Never a dot: the copy is French.
        assert!(!format_device_bytes(1_400_000_000).contains('.'));
    }
}
