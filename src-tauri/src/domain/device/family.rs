/// Closed set of device families Rustory officially knows about. New
/// families must add a variant here AND a profile registry entry — both
/// or none, never one without the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceFamily {
    /// Lunii device family. The MVP target.
    Lunii,
    // Flam, Tonies, etc. — Post-MVP only.
}

/// Cohort of firmware versions sharing the same metadata format and the
/// same authorized operation set. Granularity is intentionally coarse —
/// a per-patch-version matrix would explode without product value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LuniiFirmwareCohort {
    /// Lunii Origine, firmware family 1.x / 2.x — metadata format v3.
    OrigineV1,
    /// Lunii (mid-gen), firmware family 3.0–3.1 — metadata format v6.
    MidGenV2,
    /// Lunii V3, firmware family 3.2.x+ — metadata format v7.
    V3,
}

impl LuniiFirmwareCohort {
    /// Stable, log-friendly tag that never changes between releases. Used
    /// by `infrastructure::diagnostics::device_log` so a grep stays valid
    /// even if user-facing copy is reworded.
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::OrigineV1 => "origine_v1",
            Self::MidGenV2 => "mid_gen_v2",
            Self::V3 => "v3",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_tag_is_stable_for_origine_v1() {
        assert_eq!(
            LuniiFirmwareCohort::OrigineV1.diagnostic_tag(),
            "origine_v1"
        );
    }

    #[test]
    fn diagnostic_tag_is_stable_for_mid_gen_v2() {
        assert_eq!(LuniiFirmwareCohort::MidGenV2.diagnostic_tag(), "mid_gen_v2");
    }

    #[test]
    fn diagnostic_tag_is_stable_for_v3() {
        assert_eq!(LuniiFirmwareCohort::V3.diagnostic_tag(), "v3");
    }

    #[test]
    fn device_family_round_trips_via_clone_and_eq() {
        let f = DeviceFamily::Lunii;
        let g = f;
        assert_eq!(f, g);
    }

    #[test]
    fn cohort_round_trips_via_clone_and_eq() {
        let c = LuniiFirmwareCohort::OrigineV1;
        let d = c;
        assert_eq!(c, d);
    }
}
