/// Closed set of device families Rustory officially knows about. New
/// families must add a variant here AND a profile registry entry — both
/// or none, never one without the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceFamily {
    /// Lunii device family. The MVP target.
    Lunii,
    /// FLAM device family. Recognized with zero activated capability —
    /// the support matrix keeps every operation ❌ until support
    /// activates them line by line.
    Flam,
    // Tonies, etc. — future families.
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

/// FLAM firmware cohorts. A single conservative bucket: the internal
/// structure of `.mdf` is not publicly documented, so Rustory refuses
/// to invent a version byte to split cohorts. Real cohorts will come
/// from physical-hardware confirmation ("Adding a new cohort" in
/// `docs/architecture/device-support-profile.md` absorbs the split).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlamFirmwareCohort {
    /// Every recognized FLAM until the `.mdf` format is established.
    Gen1,
}

impl FlamFirmwareCohort {
    /// Stable, log-friendly tag — same contract as
    /// [`LuniiFirmwareCohort::diagnostic_tag`].
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Gen1 => "flam_gen1",
        }
    }
}

/// Family-tagged firmware cohort. The sum makes an impossible pairing
/// (a Lunii profile with a FLAM cohort or vice versa) unrepresentable,
/// and every exhaustive `match` on it breaks loudly when a family is
/// added — the "both or none" invariant outilled by the type system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirmwareCohort {
    Lunii(LuniiFirmwareCohort),
    Flam(FlamFirmwareCohort),
}

impl FirmwareCohort {
    /// Delegated stable tag — one namespace across families
    /// (`origine_v1` / `mid_gen_v2` / `v3` / `flam_gen1`).
    pub const fn diagnostic_tag(self) -> &'static str {
        match self {
            Self::Lunii(c) => c.diagnostic_tag(),
            Self::Flam(c) => c.diagnostic_tag(),
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
    fn diagnostic_tag_is_stable_for_flam_gen1() {
        assert_eq!(FlamFirmwareCohort::Gen1.diagnostic_tag(), "flam_gen1");
    }

    #[test]
    fn firmware_cohort_sum_delegates_diagnostic_tag_per_family() {
        assert_eq!(
            FirmwareCohort::Lunii(LuniiFirmwareCohort::OrigineV1).diagnostic_tag(),
            "origine_v1"
        );
        assert_eq!(
            FirmwareCohort::Lunii(LuniiFirmwareCohort::MidGenV2).diagnostic_tag(),
            "mid_gen_v2"
        );
        assert_eq!(
            FirmwareCohort::Lunii(LuniiFirmwareCohort::V3).diagnostic_tag(),
            "v3"
        );
        assert_eq!(
            FirmwareCohort::Flam(FlamFirmwareCohort::Gen1).diagnostic_tag(),
            "flam_gen1"
        );
    }

    #[test]
    fn device_family_round_trips_via_clone_and_eq() {
        let f = DeviceFamily::Lunii;
        let g = f;
        assert_eq!(f, g);
        let h = DeviceFamily::Flam;
        let i = h;
        assert_eq!(h, i);
        assert_ne!(f, h);
    }

    #[test]
    fn cohort_round_trips_via_clone_and_eq() {
        let c = LuniiFirmwareCohort::OrigineV1;
        let d = c;
        assert_eq!(c, d);
    }
}
