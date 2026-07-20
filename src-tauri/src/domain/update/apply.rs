//! Pure update-apply domain (`Update Apply Contract`): the per-copy plan
//! decision of the update GESTURE, its closed manual reasons, its session
//! state machine (phases, failure stages) and their frozen composed
//! copies. Zero I/O, zero framework — the exact pattern of
//! `domain::update::availability`.
//!
//! The gesture is a user-triggered MUTATION of the installed copy —
//! everything here decides or names, nothing here downloads (the updater
//! gateway lives in `infrastructure::updates`, the orchestration in
//! `application::update::apply`).

use crate::domain::import::LinuxInstallKind;

/// Why THIS copy gets manual guidance instead of the integrated gesture —
/// the closed reason set of the plan decision. Unlike the availability
/// skip motives, the reason DOES reach the wire (each carries its own
/// frozen guidance couple); the same stable token serves the diagnostics
/// log and the DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualUpdateReason {
    /// A development build (`debug_assertions`): defensive only — the
    /// gesture surface never exists there (no verdict exists in a dev
    /// build), decided in SHORT-CIRCUIT before any probe.
    DevelopmentBuild,
    /// A PROVEN unofficial install (`localBuild` probe verdict): no
    /// official distribution channel exists for this copy.
    UnofficialInstall,
    /// A PROVEN system package: the package manager owns the
    /// installation — the Tauri updater updates NO deb/rpm (Linux
    /// integrated updates are AppImage-only).
    PackageManagerOwned,
    /// A SILENT probe (Windows/macOS release, an indeterminable Linux
    /// executable): the channel cannot be proven, so the mutation is
    /// refused — the documented inverse of the availability read's
    /// public-consultation rule.
    ChannelUnproven,
    /// A PROVEN AppImage WITHOUT an embedded public key: the copy cannot
    /// verify update authenticity — fail-closed, never permissive.
    TrustChainNotConfigured,
}

impl ManualUpdateReason {
    /// Stable snake_case token (closed set) — one value for BOTH the
    /// diagnostics log and the wire `reason` field.
    pub const fn log_token(self) -> &'static str {
        match self {
            Self::DevelopmentBuild => "development_build",
            Self::UnofficialInstall => "unofficial_install",
            Self::PackageManagerOwned => "package_manager_owned",
            Self::ChannelUnproven => "channel_unproven",
            Self::TrustChainNotConfigured => "trust_chain_not_configured",
        }
    }
}

/// The per-copy plan of the gesture: integrated (this copy may install
/// updates) or manual with its frozen reason. Decided purely, re-decided
/// Rust-side at every start — never trusted from the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateApplyMode {
    Integrated,
    Manual { reason: ManualUpdateReason },
}

impl UpdateApplyMode {
    /// Stable camelCase-free wire tag of the mode (settings DTO).
    /// Exhaustive match — adding a mode without deciding its tag is a
    /// compile error (the DTO tripwire pattern).
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::Integrated => "integrated",
            Self::Manual { .. } => "manual",
        }
    }
}

/// PURE decision of this copy's gesture plan (the decision table of the
/// documented contract). STRICTER than the availability gate — a
/// MUTATION requires a PROVEN channel: a development build never gets
/// the gesture (short-circuited before any probe — the caller must not
/// even consult the probe, see the command-side resolver); a proven
/// local build has no official channel; a proven system package belongs
/// to the package manager; a silent probe is an unproven channel; a
/// proven AppImage gets the gesture IFF the trust chain (embedded
/// public key) is configured — fail-closed otherwise.
pub fn decide_update_apply(
    is_debug_build: bool,
    install: Option<LinuxInstallKind>,
    trust_chain_configured: bool,
) -> UpdateApplyMode {
    if is_debug_build {
        return UpdateApplyMode::Manual {
            reason: ManualUpdateReason::DevelopmentBuild,
        };
    }
    match install {
        Some(LinuxInstallKind::LocalBuild) => UpdateApplyMode::Manual {
            reason: ManualUpdateReason::UnofficialInstall,
        },
        Some(LinuxInstallKind::SystemPackage) => UpdateApplyMode::Manual {
            reason: ManualUpdateReason::PackageManagerOwned,
        },
        None => UpdateApplyMode::Manual {
            reason: ManualUpdateReason::ChannelUnproven,
        },
        Some(LinuxInstallKind::AppImage) => {
            if trust_chain_configured {
                UpdateApplyMode::Integrated
            } else {
                UpdateApplyMode::Manual {
                    reason: ManualUpdateReason::TrustChainNotConfigured,
                }
            }
        }
    }
}

/// The named phases of a gesture in flight, in their canonical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateApplyPhase {
    Checking,
    Downloading,
    Installing,
}

impl UpdateApplyPhase {
    /// Stable camelCase-free wire tag (events + state DTO). Exhaustive
    /// match (tripwire).
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Downloading => "downloading",
            Self::Installing => "installing",
        }
    }
}

/// The CLOSED, PII-free failure stages of the gesture — where the
/// attempt stopped, never why in raw transport words. Every stage leaves
/// the current installation INTACT (the mechanism applies nothing
/// without a complete verified artifact).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateApplyFailureStage {
    /// The signed feed could not be consulted (network, hostile status,
    /// unreadable manifest).
    Feed,
    /// The feed WAS consulted but no update applies to this
    /// target/version (the check answered "nothing", or the manifest
    /// does not cover this target) — the honest "not yet offered for
    /// this installation" state, never a coverage lie.
    NotApplicable,
    /// Downloading the artifact failed mid-way.
    Download,
    /// The artifact's signature/authenticity was NOT confirmed — nothing
    /// was applied, there is no "install anyway" path.
    Verification,
    /// Applying the verified artifact failed — the current installation
    /// stays in place and usable.
    Install,
}

impl UpdateApplyFailureStage {
    /// Stable snake_case token (closed set) — one value for BOTH the
    /// diagnostics log and the wire `stage` field.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::NotApplicable => "not_applicable",
            Self::Download => "download",
            Self::Verification => "verification",
            Self::Install => "install",
        }
    }
}

/// The SESSION state of the gesture (no persistence — after a restart,
/// the running version IS the proof). `Running.percent` is an integer
/// 0..=100 present IFF a reliable fraction is known — never invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateApplyState {
    #[default]
    Idle,
    Running {
        phase: UpdateApplyPhase,
        percent: Option<u8>,
    },
    ReadyToRestart,
    Failed {
        stage: UpdateApplyFailureStage,
    },
}

impl UpdateApplyState {
    /// Stable camelCase wire tag (state DTO). Exhaustive match
    /// (tripwire).
    pub const fn wire_tag(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running { .. } => "running",
            Self::ReadyToRestart => "readyToRestart",
            Self::Failed { .. } => "failed",
        }
    }
}

/// The frozen headline of a gesture plan (`product-language.md` — the
/// calm-gesture regime, NEW literals: nothing here recycles the
/// availability couples nor the transfer vocabulary). Composed purely;
/// the contract tests lock every literal byte-for-byte.
pub fn update_apply_plan_headline(mode: &UpdateApplyMode) -> &'static str {
    match mode {
        UpdateApplyMode::Integrated => "Cette copie peut installer les mises à jour de Rustory.",
        UpdateApplyMode::Manual { reason } => match reason {
            ManualUpdateReason::DevelopmentBuild => {
                "La mise à jour intégrée n'est pas disponible pour un build de développement."
            }
            ManualUpdateReason::UnofficialInstall => {
                "La mise à jour intégrée n'est pas disponible pour cette copie."
            }
            ManualUpdateReason::PackageManagerOwned => {
                "La mise à jour de Rustory passe par ton gestionnaire de paquets."
            }
            ManualUpdateReason::ChannelUnproven => {
                "La mise à jour intégrée n'est pas encore disponible pour cette installation."
            }
            ManualUpdateReason::TrustChainNotConfigured => {
                "La mise à jour intégrée n'est pas encore activée pour cette copie."
            }
        },
    }
}

/// The frozen guidance of a gesture plan. The releases-page address is
/// plain TEXT (the product ships no external-browser opener) and the
/// formula (`La page officielle des versions reste disponible : …`) is
/// the gesture's OWN — never the availability notice's exclusive
/// wording.
pub fn update_apply_plan_guidance(mode: &UpdateApplyMode) -> &'static str {
    match mode {
        UpdateApplyMode::Integrated => {
            "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer."
        }
        UpdateApplyMode::Manual { reason } => match reason {
            ManualUpdateReason::DevelopmentBuild => {
                "Reconstruis Rustory depuis les sources pour obtenir la dernière version."
            }
            ManualUpdateReason::UnofficialInstall => {
                "Cette copie n'est pas passée par un canal de distribution officiel. La page \
                 officielle des versions reste disponible : github.com/roukmoute/Rustory/releases."
            }
            ManualUpdateReason::PackageManagerOwned => {
                "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil \
                 de ton système, puis relance Rustory."
            }
            ManualUpdateReason::ChannelUnproven => {
                "Rustory ne peut pas confirmer le canal de cette copie. La page officielle des \
                 versions reste disponible : github.com/roukmoute/Rustory/releases."
            }
            ManualUpdateReason::TrustChainNotConfigured => {
                "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page \
                 officielle des versions reste disponible : github.com/roukmoute/Rustory/releases."
            }
        },
    }
}

/// The frozen headline of a gesture in flight, by phase.
pub fn update_apply_running_headline(phase: UpdateApplyPhase) -> &'static str {
    match phase {
        UpdateApplyPhase::Checking => "Vérification de la mise à jour en cours…",
        UpdateApplyPhase::Downloading => "Téléchargement de la mise à jour en cours…",
        UpdateApplyPhase::Installing => "Installation de la mise à jour en cours…",
    }
}

/// The frozen COMMON notice of a gesture in flight — the non-tunnel
/// promise (the app stays usable).
pub fn update_apply_running_notice() -> &'static str {
    "Tu peux continuer à utiliser Rustory pendant cette opération."
}

/// The frozen headline of the ready-to-restart state.
pub fn update_apply_ready_headline() -> &'static str {
    "La mise à jour de Rustory est prête."
}

/// The frozen notice of the ready-to-restart state — names the needed
/// gesture AND the local-first promise.
pub fn update_apply_ready_notice() -> &'static str {
    "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place."
}

/// The frozen headline of a failed gesture, by stage — the probable
/// cause, calm, never accusatory.
pub fn update_apply_failed_headline(stage: UpdateApplyFailureStage) -> &'static str {
    match stage {
        UpdateApplyFailureStage::Feed => "Le canal de mise à jour n'a pas répondu.",
        UpdateApplyFailureStage::NotApplicable => {
            "La mise à jour n'est pas encore proposée pour cette installation."
        }
        UpdateApplyFailureStage::Download => "Le téléchargement de la mise à jour n'a pas abouti.",
        UpdateApplyFailureStage::Verification => {
            "L'authenticité de la mise à jour n'a pas pu être confirmée."
        }
        UpdateApplyFailureStage::Install => "L'installation de la mise à jour n'a pas abouti.",
    }
}

/// The frozen notice of a failed gesture, by stage — impact (the intact
/// installation) + next gesture; the releases-page address travels as
/// plain text.
pub fn update_apply_failed_notice(stage: UpdateApplyFailureStage) -> &'static str {
    match stage {
        UpdateApplyFailureStage::Feed => {
            "Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des \
             versions reste disponible : github.com/roukmoute/Rustory/releases."
        }
        UpdateApplyFailureStage::NotApplicable => {
            "La nouvelle version n'est pas encore publiée sur le canal de mise à jour de cette \
             copie. La page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases."
        }
        UpdateApplyFailureStage::Download => {
            "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie."
        }
        UpdateApplyFailureStage::Verification => {
            "Rien n'a été installé : Rustory reste sur sa version actuelle. Réessaie plus tard ; \
             la page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases."
        }
        UpdateApplyFailureStage::Install => {
            "Ta version actuelle de Rustory reste en place et utilisable. Réessaie, ou passe par \
             la page officielle des versions : github.com/roukmoute/Rustory/releases."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Plan decision (the documented table, exhaustively) =====

    #[test]
    fn a_debug_build_is_manual_before_any_probe_or_trust_fact() {
        // The short-circuit path: whatever the probe would say and
        // whatever the trust chain claims, a dev build never gets the
        // gesture (the command-side resolver additionally proves the
        // probe closure never even runs).
        for install in [
            Some(LinuxInstallKind::AppImage),
            Some(LinuxInstallKind::SystemPackage),
            Some(LinuxInstallKind::LocalBuild),
            None,
        ] {
            for trust in [true, false] {
                assert_eq!(
                    decide_update_apply(true, install, trust),
                    UpdateApplyMode::Manual {
                        reason: ManualUpdateReason::DevelopmentBuild
                    },
                    "debug must short-circuit for {install:?}, trust={trust}"
                );
            }
        }
    }

    #[test]
    fn a_proven_local_build_is_manual_as_unofficial_even_with_a_trust_chain() {
        // The trust chain never rescues an unproven/unofficial channel.
        for trust in [true, false] {
            assert_eq!(
                decide_update_apply(false, Some(LinuxInstallKind::LocalBuild), trust),
                UpdateApplyMode::Manual {
                    reason: ManualUpdateReason::UnofficialInstall
                }
            );
        }
    }

    #[test]
    fn a_proven_system_package_is_manual_as_package_manager_owned() {
        // The Tauri updater updates NO deb/rpm — the package manager
        // owns the installation, trust chain or not.
        for trust in [true, false] {
            assert_eq!(
                decide_update_apply(false, Some(LinuxInstallKind::SystemPackage), trust),
                UpdateApplyMode::Manual {
                    reason: ManualUpdateReason::PackageManagerOwned
                }
            );
        }
    }

    #[test]
    fn a_silent_probe_is_manual_as_channel_unproven() {
        // Windows/macOS release, an indeterminable Linux executable: the
        // STRICTER-than-information rule — a mutation requires a PROVEN
        // channel, so the silent probe refuses the gesture (the exact
        // inverse of the availability read, which runs on a silent
        // probe).
        for trust in [true, false] {
            assert_eq!(
                decide_update_apply(false, None, trust),
                UpdateApplyMode::Manual {
                    reason: ManualUpdateReason::ChannelUnproven
                }
            );
        }
    }

    #[test]
    fn a_proven_appimage_without_a_trust_chain_is_manual_fail_closed() {
        assert_eq!(
            decide_update_apply(false, Some(LinuxInstallKind::AppImage), false),
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::TrustChainNotConfigured
            }
        );
    }

    #[test]
    fn a_proven_appimage_with_a_trust_chain_is_the_only_integrated_path() {
        assert_eq!(
            decide_update_apply(false, Some(LinuxInstallKind::AppImage), true),
            UpdateApplyMode::Integrated
        );
    }

    // ===== Tokens and wire tags (tripwires) =====

    #[test]
    fn manual_reason_tokens_are_stable_and_pairwise_distinct() {
        let reasons = [
            ManualUpdateReason::DevelopmentBuild,
            ManualUpdateReason::UnofficialInstall,
            ManualUpdateReason::PackageManagerOwned,
            ManualUpdateReason::ChannelUnproven,
            ManualUpdateReason::TrustChainNotConfigured,
        ];
        let tokens: Vec<&str> = reasons.iter().map(|reason| reason.log_token()).collect();
        assert_eq!(
            tokens,
            vec![
                "development_build",
                "unofficial_install",
                "package_manager_owned",
                "channel_unproven",
                "trust_chain_not_configured"
            ]
        );
        for (index, a) in tokens.iter().enumerate() {
            for b in tokens.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn mode_wire_tags_are_stable() {
        assert_eq!(UpdateApplyMode::Integrated.wire_tag(), "integrated");
        assert_eq!(
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::ChannelUnproven
            }
            .wire_tag(),
            "manual"
        );
    }

    #[test]
    fn phase_wire_tags_are_stable_and_pairwise_distinct() {
        let phases = [
            UpdateApplyPhase::Checking,
            UpdateApplyPhase::Downloading,
            UpdateApplyPhase::Installing,
        ];
        let tags: Vec<&str> = phases.iter().map(|phase| phase.wire_tag()).collect();
        assert_eq!(tags, vec!["checking", "downloading", "installing"]);
        for (index, a) in tags.iter().enumerate() {
            for b in tags.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn failure_stage_tokens_are_stable_and_pairwise_distinct() {
        let stages = [
            UpdateApplyFailureStage::Feed,
            UpdateApplyFailureStage::NotApplicable,
            UpdateApplyFailureStage::Download,
            UpdateApplyFailureStage::Verification,
            UpdateApplyFailureStage::Install,
        ];
        let tokens: Vec<&str> = stages.iter().map(|stage| stage.token()).collect();
        assert_eq!(
            tokens,
            vec![
                "feed",
                "not_applicable",
                "download",
                "verification",
                "install"
            ]
        );
        for (index, a) in tokens.iter().enumerate() {
            for b in tokens.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn state_wire_tags_are_stable_and_pairwise_distinct() {
        let states = [
            UpdateApplyState::Idle,
            UpdateApplyState::Running {
                phase: UpdateApplyPhase::Checking,
                percent: None,
            },
            UpdateApplyState::ReadyToRestart,
            UpdateApplyState::Failed {
                stage: UpdateApplyFailureStage::Feed,
            },
        ];
        let tags: Vec<&str> = states.iter().map(|state| state.wire_tag()).collect();
        assert_eq!(tags, vec!["idle", "running", "readyToRestart", "failed"]);
        for (index, a) in tags.iter().enumerate() {
            for b in tags.iter().skip(index + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_default_session_state_is_idle() {
        assert_eq!(UpdateApplyState::default(), UpdateApplyState::Idle);
    }

    // ===== Frozen copies (byte-for-byte) =====

    #[test]
    fn integrated_plan_copies_are_frozen() {
        let mode = UpdateApplyMode::Integrated;
        assert_eq!(
            update_apply_plan_headline(&mode),
            "Cette copie peut installer les mises à jour de Rustory."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer."
        );
    }

    #[test]
    fn package_manager_owned_plan_copies_are_frozen() {
        let mode = UpdateApplyMode::Manual {
            reason: ManualUpdateReason::PackageManagerOwned,
        };
        assert_eq!(
            update_apply_plan_headline(&mode),
            "La mise à jour de Rustory passe par ton gestionnaire de paquets."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil de \
             ton système, puis relance Rustory."
        );
    }

    #[test]
    fn unofficial_install_plan_copies_are_frozen() {
        let mode = UpdateApplyMode::Manual {
            reason: ManualUpdateReason::UnofficialInstall,
        };
        assert_eq!(
            update_apply_plan_headline(&mode),
            "La mise à jour intégrée n'est pas disponible pour cette copie."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Cette copie n'est pas passée par un canal de distribution officiel. La page \
             officielle des versions reste disponible : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn channel_unproven_plan_copies_are_frozen() {
        let mode = UpdateApplyMode::Manual {
            reason: ManualUpdateReason::ChannelUnproven,
        };
        assert_eq!(
            update_apply_plan_headline(&mode),
            "La mise à jour intégrée n'est pas encore disponible pour cette installation."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Rustory ne peut pas confirmer le canal de cette copie. La page officielle des \
             versions reste disponible : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn trust_chain_not_configured_plan_copies_are_frozen() {
        let mode = UpdateApplyMode::Manual {
            reason: ManualUpdateReason::TrustChainNotConfigured,
        };
        assert_eq!(
            update_apply_plan_headline(&mode),
            "La mise à jour intégrée n'est pas encore activée pour cette copie."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page \
             officielle des versions reste disponible : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn development_build_plan_copies_are_frozen() {
        // Defensive couple only: the zone never renders in a dev build
        // (the verdict does not exist there) — but the copy exists so a
        // drifted wire can never render an empty plan.
        let mode = UpdateApplyMode::Manual {
            reason: ManualUpdateReason::DevelopmentBuild,
        };
        assert_eq!(
            update_apply_plan_headline(&mode),
            "La mise à jour intégrée n'est pas disponible pour un build de développement."
        );
        assert_eq!(
            update_apply_plan_guidance(&mode),
            "Reconstruis Rustory depuis les sources pour obtenir la dernière version."
        );
    }

    #[test]
    fn running_copies_are_frozen_per_phase_with_one_common_notice() {
        assert_eq!(
            update_apply_running_headline(UpdateApplyPhase::Checking),
            "Vérification de la mise à jour en cours…"
        );
        assert_eq!(
            update_apply_running_headline(UpdateApplyPhase::Downloading),
            "Téléchargement de la mise à jour en cours…"
        );
        assert_eq!(
            update_apply_running_headline(UpdateApplyPhase::Installing),
            "Installation de la mise à jour en cours…"
        );
        assert_eq!(
            update_apply_running_notice(),
            "Tu peux continuer à utiliser Rustory pendant cette opération."
        );
    }

    #[test]
    fn ready_to_restart_copies_are_frozen() {
        assert_eq!(
            update_apply_ready_headline(),
            "La mise à jour de Rustory est prête."
        );
        assert_eq!(
            update_apply_ready_notice(),
            "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place."
        );
    }

    #[test]
    fn failed_feed_copies_are_frozen() {
        assert_eq!(
            update_apply_failed_headline(UpdateApplyFailureStage::Feed),
            "Le canal de mise à jour n'a pas répondu."
        );
        assert_eq!(
            update_apply_failed_notice(UpdateApplyFailureStage::Feed),
            "Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des \
             versions reste disponible : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn failed_not_applicable_copies_are_frozen() {
        assert_eq!(
            update_apply_failed_headline(UpdateApplyFailureStage::NotApplicable),
            "La mise à jour n'est pas encore proposée pour cette installation."
        );
        assert_eq!(
            update_apply_failed_notice(UpdateApplyFailureStage::NotApplicable),
            "La nouvelle version n'est pas encore publiée sur le canal de mise à jour de cette \
             copie. La page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn failed_download_copies_are_frozen() {
        assert_eq!(
            update_apply_failed_headline(UpdateApplyFailureStage::Download),
            "Le téléchargement de la mise à jour n'a pas abouti."
        );
        assert_eq!(
            update_apply_failed_notice(UpdateApplyFailureStage::Download),
            "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie."
        );
    }

    #[test]
    fn failed_verification_copies_are_frozen() {
        assert_eq!(
            update_apply_failed_headline(UpdateApplyFailureStage::Verification),
            "L'authenticité de la mise à jour n'a pas pu être confirmée."
        );
        assert_eq!(
            update_apply_failed_notice(UpdateApplyFailureStage::Verification),
            "Rien n'a été installé : Rustory reste sur sa version actuelle. Réessaie plus tard ; \
             la page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn failed_install_copies_are_frozen() {
        assert_eq!(
            update_apply_failed_headline(UpdateApplyFailureStage::Install),
            "L'installation de la mise à jour n'a pas abouti."
        );
        assert_eq!(
            update_apply_failed_notice(UpdateApplyFailureStage::Install),
            "Ta version actuelle de Rustory reste en place et utilisable. Réessaie, ou passe par \
             la page officielle des versions : github.com/roukmoute/Rustory/releases."
        );
    }

    #[test]
    fn no_gesture_copy_recycles_the_availability_notice_formula() {
        // IDENTITY ≠ COPY: `Récupère la nouvelle version depuis la page
        // officielle des versions : …` is the EXCLUSIVE property of the
        // availability contract's `updateAvailable` notice — the gesture
        // copies use their own formula (`La page officielle des versions
        // reste disponible : …`).
        let all_copies: Vec<&str> = [
            UpdateApplyMode::Integrated,
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::DevelopmentBuild,
            },
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::UnofficialInstall,
            },
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::PackageManagerOwned,
            },
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::ChannelUnproven,
            },
            UpdateApplyMode::Manual {
                reason: ManualUpdateReason::TrustChainNotConfigured,
            },
        ]
        .iter()
        .flat_map(|mode| {
            [
                update_apply_plan_headline(mode),
                update_apply_plan_guidance(mode),
            ]
        })
        .chain(
            [
                UpdateApplyFailureStage::Feed,
                UpdateApplyFailureStage::NotApplicable,
                UpdateApplyFailureStage::Download,
                UpdateApplyFailureStage::Verification,
                UpdateApplyFailureStage::Install,
            ]
            .iter()
            .flat_map(|&stage| {
                [
                    update_apply_failed_headline(stage),
                    update_apply_failed_notice(stage),
                ]
            }),
        )
        .chain([
            update_apply_running_headline(UpdateApplyPhase::Checking),
            update_apply_running_headline(UpdateApplyPhase::Downloading),
            update_apply_running_headline(UpdateApplyPhase::Installing),
            update_apply_running_notice(),
            update_apply_ready_headline(),
            update_apply_ready_notice(),
        ])
        .collect();
        for copy in all_copies {
            assert!(
                !copy.contains("Récupère la nouvelle version"),
                "the availability notice formula must never leak into a gesture copy: {copy:?}"
            );
        }
    }
}
