//! Pure update-availability domain (`Update Availability Contract`):
//! the strict release-version convention, the per-launch check decision,
//! the sealed verdict states and their frozen composed copies. Zero I/O,
//! zero framework — the exact pattern of `domain::import::file_association`.
//!
//! The check is a CONSULTATION of public information, never an updater:
//! everything here decides or names, nothing here fetches (the network
//! client lives in `infrastructure::updates`, the orchestration in
//! `application::update`).

use crate::domain::import::LinuxInstallKind;

/// A parsed official release version under the strict `MAJOR.MINOR.PATCH`
/// convention. Field order gives the derived `Ord` the lexicographic
/// comparison the convention means (major, then minor, then patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// STRICT, fail-closed parse of an official version literal: an optional
/// `v` prefix, then EXACTLY three dot-separated components of pure ASCII
/// digits, leading zeros refused (except the bare `0` component). Any
/// deviation — pre-release suffix, build metadata, whitespace, emptiness,
/// a fourth component, a u64 overflow — yields `None`: a tag outside the
/// convention NEVER produces a verdict, it is reported as not parsable.
pub fn parse_release_version(raw: &str) -> Option<ReleaseVersion> {
    let body = raw.strip_prefix('v').unwrap_or(raw);
    let mut components = body.split('.');
    let major = parse_version_component(components.next()?)?;
    let minor = parse_version_component(components.next()?)?;
    let patch = parse_version_component(components.next()?)?;
    if components.next().is_some() {
        return None;
    }
    Some(ReleaseVersion {
        major,
        minor,
        patch,
    })
}

/// One strict component: non-empty, ASCII digits only, no leading zero
/// (except `0` itself), within u64.
fn parse_version_component(component: &str) -> Option<u64> {
    if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if component.len() > 1 && component.starts_with('0') {
        return None;
    }
    component.parse::<u64>().ok()
}

/// The user-facing rendering of a version: `MAJOR.MINOR.PATCH`, never a
/// `v` prefix (`product-language.md`).
pub fn format_release_version(version: ReleaseVersion) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

/// Why THIS copy does not check — the closed motive set of the skip
/// decision. The motive lives in the diagnostics log only; the wire
/// carries the single `checkNotRun` state (one copy couple).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckSkipReason {
    /// A development build (`debug_assertions`): a workstation must work
    /// offline without noise.
    DevelopmentBuild,
    /// A PROVEN unofficial install (`localBuild` probe verdict): no
    /// current distribution channel exists to inform about.
    UnofficialInstall,
}

impl UpdateCheckSkipReason {
    /// Stable snake_case token for the diagnostics log (closed set).
    pub const fn log_token(self) -> &'static str {
        match self {
            Self::DevelopmentBuild => "development_build",
            Self::UnofficialInstall => "unofficial_install",
        }
    }
}

/// The per-launch gate verdict: run the consultation, or skip it with
/// its frozen motive. Decided BEFORE any network dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckDecision {
    Run,
    Skip { reason: UpdateCheckSkipReason },
}

/// PURE decision of whether this copy checks (the decision table of the
/// documented contract). A development build never checks; a PROVEN
/// local build never checks (no current channel — a frozen reason, never
/// an error state); every distributed copy checks — INCLUDING a silent
/// probe (Windows/macOS release, an indeterminable Linux executable):
/// consulting a public page claims nothing about the channel, so the
/// no-unprovable-claims rule does not gate the read.
pub fn decide_update_check(
    is_debug_build: bool,
    install: Option<LinuxInstallKind>,
) -> UpdateCheckDecision {
    if is_debug_build {
        return UpdateCheckDecision::Skip {
            reason: UpdateCheckSkipReason::DevelopmentBuild,
        };
    }
    match install {
        Some(LinuxInstallKind::LocalBuild) => UpdateCheckDecision::Skip {
            reason: UpdateCheckSkipReason::UnofficialInstall,
        },
        Some(LinuxInstallKind::AppImage) | Some(LinuxInstallKind::SystemPackage) | None => {
            UpdateCheckDecision::Run
        }
    }
}

/// What the release source observed on the official releases page: the
/// latest published tag, or the REAL absence of any published release
/// (a state of the world, never a failure). Returned by the
/// `infrastructure::updates` source and resolved purely below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseProbe {
    /// The latest published release, as its RAW tag (untrusted — the
    /// strict parser decides what it means).
    Latest { tag: String },
    /// The repository has no published release at all.
    NoPublishedRelease,
}

/// The sealed verdict states of one launch's consultation — the
/// `verdict ≠ transport ≠ policy` regime applied to the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAvailability {
    /// A STRICTLY newer official version is published.
    UpdateAvailable { latest: ReleaseVersion },
    /// No newer version is published — including the no-release world.
    UpToDate,
    /// The consultation could not be done (transport failure, hostile
    /// status, unparsable answer). Retried at the next launch.
    CheckUnavailable,
    /// This copy does not check (the decision table's skip states).
    CheckNotRun,
}

impl UpdateAvailability {
    /// Stable camelCase wire tag (settings DTO). Must stay
    /// byte-identical to the TS mirror's closed set. Exhaustive match —
    /// adding a state without deciding its tag is a compile error (the
    /// DTO tripwire pattern).
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::UpdateAvailable { .. } => "updateAvailable",
            Self::UpToDate => "upToDate",
            Self::CheckUnavailable => "checkUnavailable",
            Self::CheckNotRun => "checkNotRun",
        }
    }
}

/// PURE resolution of a fetched probe against the running version:
/// `NoPublishedRelease` → `UpToDate` ("no newer version is published" is
/// TRUE when nothing is published — never presented as an error); a
/// parsed tag STRICTLY newer → `UpdateAvailable`; equal or older →
/// `UpToDate` (a downgrade is never signaled); a tag outside the
/// convention → `CheckUnavailable` (fail-closed — never a best-effort
/// guess). Transport failures never reach this function: the
/// application layer maps them to `CheckUnavailable` directly.
pub fn resolve_availability(current: ReleaseVersion, probe: &ReleaseProbe) -> UpdateAvailability {
    match probe {
        ReleaseProbe::NoPublishedRelease => UpdateAvailability::UpToDate,
        ReleaseProbe::Latest { tag } => match parse_release_version(tag) {
            Some(latest) if latest > current => UpdateAvailability::UpdateAvailable { latest },
            Some(_) => UpdateAvailability::UpToDate,
            None => UpdateAvailability::CheckUnavailable,
        },
    }
}

/// The frozen headline of a verdict (`product-language.md` — the
/// calm-information regime, NEW literals: nothing here recycles the
/// runtime-error or durable-limit vocabularies). Composed purely; the
/// contract tests lock every literal byte-for-byte.
pub fn update_headline(availability: &UpdateAvailability) -> String {
    match availability {
        UpdateAvailability::UpdateAvailable { latest } => {
            format!(
                "Nouvelle version disponible : {}.",
                format_release_version(*latest)
            )
        }
        UpdateAvailability::UpToDate => "Aucune version plus récente n'est publiée.".to_string(),
        UpdateAvailability::CheckUnavailable => {
            "La vérification de version n'a pas pu être faite.".to_string()
        }
        UpdateAvailability::CheckNotRun => {
            "La vérification de version n'est pas exécutée pour cette copie.".to_string()
        }
    }
}

/// The frozen notice of a verdict. `current` composes only the
/// `updateAvailable` notice ("your version / the published version" —
/// never an ambiguity about the installed version); the releases-page
/// address is plain TEXT (the product ships no external-browser opener).
pub fn update_notice(availability: &UpdateAvailability, current: ReleaseVersion) -> String {
    match availability {
        UpdateAvailability::UpdateAvailable { .. } => format!(
            "Ta version actuelle est {}. Récupère la nouvelle version depuis la page \
             officielle des versions : github.com/roukmoute/Rustory/releases.",
            format_release_version(current)
        ),
        UpdateAvailability::UpToDate => "Aucune action n'est nécessaire.".to_string(),
        UpdateAvailability::CheckUnavailable => {
            "Rustory reste pleinement utilisable. La vérification réessaiera au prochain \
             lancement."
                .to_string()
        }
        UpdateAvailability::CheckNotRun => {
            "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : \
             aucune vérification réseau n'est effectuée."
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u64, minor: u64, patch: u64) -> ReleaseVersion {
        ReleaseVersion {
            major,
            minor,
            patch,
        }
    }

    // ===== Strict parsing =====

    #[test]
    fn parses_a_nominal_version() {
        assert_eq!(parse_release_version("1.2.3"), Some(version(1, 2, 3)));
        assert_eq!(parse_release_version("0.1.0"), Some(version(0, 1, 0)));
    }

    #[test]
    fn parses_the_optional_v_prefix() {
        assert_eq!(parse_release_version("v1.2.3"), Some(version(1, 2, 3)));
        assert_eq!(parse_release_version("v0.0.0"), Some(version(0, 0, 0)));
    }

    #[test]
    fn refuses_a_pre_release_or_build_metadata_suffix() {
        assert_eq!(parse_release_version("1.2.3-beta.1"), None);
        assert_eq!(parse_release_version("1.2.3+build.5"), None);
        assert_eq!(parse_release_version("v1.2.3-rc1"), None);
    }

    #[test]
    fn refuses_whitespace_anywhere() {
        assert_eq!(parse_release_version(" 1.2.3"), None);
        assert_eq!(parse_release_version("1.2.3 "), None);
        assert_eq!(parse_release_version("1. 2.3"), None);
    }

    #[test]
    fn refuses_emptiness_and_missing_components() {
        assert_eq!(parse_release_version(""), None);
        assert_eq!(parse_release_version("v"), None);
        assert_eq!(parse_release_version("1"), None);
        assert_eq!(parse_release_version("1.2"), None);
        assert_eq!(parse_release_version("1..3"), None);
        assert_eq!(parse_release_version("1.2."), None);
    }

    #[test]
    fn refuses_a_fourth_component() {
        assert_eq!(parse_release_version("1.2.3.4"), None);
    }

    #[test]
    fn refuses_leading_zeros_but_accepts_the_bare_zero() {
        assert_eq!(parse_release_version("01.2.3"), None);
        assert_eq!(parse_release_version("1.02.3"), None);
        assert_eq!(parse_release_version("1.2.03"), None);
        assert_eq!(parse_release_version("0.0.0"), Some(version(0, 0, 0)));
    }

    #[test]
    fn refuses_non_ascii_digits_and_signs() {
        assert_eq!(parse_release_version("１.2.3"), None);
        assert_eq!(parse_release_version("+1.2.3"), None);
        assert_eq!(parse_release_version("1.-2.3"), None);
        assert_eq!(parse_release_version("V1.2.3"), None);
    }

    #[test]
    fn refuses_a_u64_overflow() {
        assert_eq!(parse_release_version("99999999999999999999.0.0"), None);
        // The exact u64::MAX still parses — the bound is the type, not a guess.
        assert_eq!(
            parse_release_version("18446744073709551615.0.0"),
            Some(version(18446744073709551615, 0, 0))
        );
    }

    // ===== Convention tripwire =====

    #[test]
    fn the_binary_version_respects_the_release_convention() {
        // TRIPWIRE: engraving a version outside the strict convention in
        // Cargo.toml must fail here — the check would otherwise silently
        // lose the ability to compare against published releases.
        assert!(parse_release_version(env!("CARGO_PKG_VERSION")).is_some());
    }

    // ===== Lexicographic comparison =====

    #[test]
    fn compares_major_then_minor_then_patch() {
        assert!(version(2, 0, 0) > version(1, 9, 9));
        assert!(version(1, 3, 0) > version(1, 2, 9));
        assert!(version(1, 2, 4) > version(1, 2, 3));
        assert!(version(1, 2, 3) == version(1, 2, 3));
        assert!(version(0, 9, 9) < version(1, 0, 0));
    }

    // ===== Check decision =====

    #[test]
    fn a_debug_build_skips_before_consulting_the_probe() {
        assert_eq!(
            decide_update_check(true, Some(LinuxInstallKind::SystemPackage)),
            UpdateCheckDecision::Skip {
                reason: UpdateCheckSkipReason::DevelopmentBuild
            }
        );
    }

    #[test]
    fn a_proven_local_build_skips_as_unofficial() {
        assert_eq!(
            decide_update_check(false, Some(LinuxInstallKind::LocalBuild)),
            UpdateCheckDecision::Skip {
                reason: UpdateCheckSkipReason::UnofficialInstall
            }
        );
    }

    #[test]
    fn an_appimage_copy_runs_the_check() {
        assert_eq!(
            decide_update_check(false, Some(LinuxInstallKind::AppImage)),
            UpdateCheckDecision::Run
        );
    }

    #[test]
    fn a_system_package_copy_runs_the_check() {
        assert_eq!(
            decide_update_check(false, Some(LinuxInstallKind::SystemPackage)),
            UpdateCheckDecision::Run
        );
    }

    #[test]
    fn a_silent_probe_runs_the_check_as_a_distributed_copy() {
        // Windows/macOS release, an indeterminable Linux executable: the
        // consultation is a public read — it claims nothing about the
        // channel, so the silent probe never blocks it.
        assert_eq!(decide_update_check(false, None), UpdateCheckDecision::Run);
    }

    #[test]
    fn skip_reason_log_tokens_are_stable() {
        assert_eq!(
            UpdateCheckSkipReason::DevelopmentBuild.log_token(),
            "development_build"
        );
        assert_eq!(
            UpdateCheckSkipReason::UnofficialInstall.log_token(),
            "unofficial_install"
        );
    }

    // ===== Pure resolution =====

    #[test]
    fn no_published_release_resolves_to_up_to_date() {
        // The REAL state of the world today: no official release exists.
        // "No newer version is published" is true — never an error.
        assert_eq!(
            resolve_availability(version(0, 1, 0), &ReleaseProbe::NoPublishedRelease),
            UpdateAvailability::UpToDate
        );
    }

    #[test]
    fn a_strictly_newer_tag_resolves_to_update_available() {
        assert_eq!(
            resolve_availability(
                version(0, 1, 0),
                &ReleaseProbe::Latest {
                    tag: "v9.9.9".to_string()
                }
            ),
            UpdateAvailability::UpdateAvailable {
                latest: version(9, 9, 9)
            }
        );
    }

    #[test]
    fn an_equal_tag_resolves_to_up_to_date() {
        assert_eq!(
            resolve_availability(
                version(0, 1, 0),
                &ReleaseProbe::Latest {
                    tag: "v0.1.0".to_string()
                }
            ),
            UpdateAvailability::UpToDate
        );
    }

    #[test]
    fn an_older_tag_resolves_to_up_to_date_never_a_downgrade() {
        assert_eq!(
            resolve_availability(
                version(2, 0, 0),
                &ReleaseProbe::Latest {
                    tag: "v1.9.9".to_string()
                }
            ),
            UpdateAvailability::UpToDate
        );
    }

    #[test]
    fn an_unparsable_tag_resolves_to_check_unavailable() {
        // Fail-closed: a tag outside the convention never produces a
        // verdict — the check reports it as not doable, never a guess.
        for rotten in ["nightly", "v1.2.3-beta", "1.2", "", "v1.2.3.4"] {
            assert_eq!(
                resolve_availability(
                    version(0, 1, 0),
                    &ReleaseProbe::Latest {
                        tag: rotten.to_string()
                    }
                ),
                UpdateAvailability::CheckUnavailable,
                "tag {rotten:?} must yield no verdict"
            );
        }
    }

    // ===== Frozen copies (byte-for-byte) =====

    #[test]
    fn update_available_copies_compose_both_versions() {
        let availability = UpdateAvailability::UpdateAvailable {
            latest: version(9, 9, 9),
        };
        assert_eq!(
            update_headline(&availability),
            "Nouvelle version disponible : 9.9.9."
        );
        assert_eq!(
            update_notice(&availability, version(0, 1, 0)),
            "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page \
             officielle des versions : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn up_to_date_copies_are_frozen() {
        let availability = UpdateAvailability::UpToDate;
        assert_eq!(
            update_headline(&availability),
            "Aucune version plus récente n'est publiée."
        );
        assert_eq!(
            update_notice(&availability, version(0, 1, 0)),
            "Aucune action n'est nécessaire."
        );
    }

    #[test]
    fn check_unavailable_copies_are_frozen() {
        let availability = UpdateAvailability::CheckUnavailable;
        assert_eq!(
            update_headline(&availability),
            "La vérification de version n'a pas pu être faite."
        );
        assert_eq!(
            update_notice(&availability, version(0, 1, 0)),
            "Rustory reste pleinement utilisable. La vérification réessaiera au prochain \
             lancement."
        );
    }

    #[test]
    fn check_not_run_copies_are_frozen() {
        let availability = UpdateAvailability::CheckNotRun;
        assert_eq!(
            update_headline(&availability),
            "La vérification de version n'est pas exécutée pour cette copie."
        );
        assert_eq!(
            update_notice(&availability, version(0, 1, 0)),
            "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : \
             aucune vérification réseau n'est effectuée."
        );
    }

    #[test]
    fn version_formatting_never_carries_a_v_prefix() {
        assert_eq!(format_release_version(version(1, 2, 3)), "1.2.3");
        assert_eq!(format_release_version(version(0, 1, 0)), "0.1.0");
    }

    // ===== Wire tags (tripwire) =====

    #[test]
    fn wire_tags_are_stable_and_pairwise_distinct() {
        // The exhaustive match in `wire_tag` is the compile-time
        // tripwire; this locks the emitted bytes and their distinctness.
        let states = [
            UpdateAvailability::UpdateAvailable {
                latest: version(9, 9, 9),
            },
            UpdateAvailability::UpToDate,
            UpdateAvailability::CheckUnavailable,
            UpdateAvailability::CheckNotRun,
        ];
        let tags: Vec<&str> = states.iter().map(|state| state.wire_tag()).collect();
        assert_eq!(
            tags,
            vec![
                "updateAvailable",
                "upToDate",
                "checkUnavailable",
                "checkNotRun"
            ]
        );
        for (index, a) in tags.iter().enumerate() {
            for b in tags.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }
}
