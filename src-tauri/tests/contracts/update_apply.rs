//! Wire contracts of the update-apply gesture (`Update Apply
//! Contract`): the exact camelCase serialization of the plan, session
//! state and start DTOs (strict omission disciplines), the dedicated
//! `update:*` event payloads, the frozen tags/tokens, the byte-for-byte
//! frozen copies per mode/phase/stage, and the engraved constants (feed
//! endpoint, env override, event names). The three-manifest
//! version-alignment lock lives in the availability contracts and stays
//! green untouched — this gesture bumps no version.

use rustory_lib::application::update::{StartUpdateApplyOutcome, UpdateApplySessionSnapshot};
use rustory_lib::domain::update::{
    ManualUpdateReason, UpdateApplyFailureStage, UpdateApplyMode, UpdateApplyPhase,
    UpdateApplyState,
};
use rustory_lib::infrastructure::updates::{
    UPDATE_FEED_CHECK_BUDGET, UPDATE_FEED_ENDPOINT, UPDATE_FEED_ENDPOINT_ENV,
};
use rustory_lib::ipc::dto::settings::{
    StartUpdateApplyDto, UpdateApplyPlanDto, UpdateApplyStateDto,
};
use rustory_lib::ipc::events::{
    UpdateApplyCompletedEvent, UpdateApplyFailedEvent, UpdateApplyProgressEvent,
    EVENT_UPDATE_COMPLETED, EVENT_UPDATE_FAILED, EVENT_UPDATE_PROGRESS,
};

/// Build one authoritative snapshot for the state-DTO contracts.
fn snapshot(state: UpdateApplyState, job_id: Option<&str>) -> UpdateApplySessionSnapshot {
    UpdateApplySessionSnapshot {
        state,
        job_id: job_id.map(str::to_string),
    }
}

// ===== Plan DTO — exact serialization, omission discipline =====

#[test]
fn the_integrated_plan_serializes_without_a_reason_key() {
    let dto = UpdateApplyPlanDto::from_mode(UpdateApplyMode::Integrated);
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "mode": "integrated",
            "headline": "Cette copie peut installer les mises à jour de Rustory.",
            "guidance": "Le téléchargement vérifie l'authenticité de la mise à jour avant de \
                         l'installer.",
        })
    );
    // Omission discipline: the key is ABSENT, never `null`.
    assert!(v.get("reason").is_none());
}

#[test]
fn every_manual_plan_serializes_its_frozen_reason_and_couple() {
    let expected: [(ManualUpdateReason, &str, &str, &str); 5] = [
        (
            ManualUpdateReason::PackageManagerOwned,
            "package_manager_owned",
            "La mise à jour de Rustory passe par ton gestionnaire de paquets.",
            "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil de \
             ton système, puis relance Rustory.",
        ),
        (
            ManualUpdateReason::UnofficialInstall,
            "unofficial_install",
            "La mise à jour intégrée n'est pas disponible pour cette copie.",
            "Cette copie n'est pas passée par un canal de distribution officiel. La page \
             officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
        ),
        (
            ManualUpdateReason::ChannelUnproven,
            "channel_unproven",
            "La mise à jour intégrée n'est pas encore disponible pour cette installation.",
            "Rustory ne peut pas confirmer le canal de cette copie. La page officielle des \
             versions reste disponible : github.com/roukmoute/Rustory/releases.",
        ),
        (
            ManualUpdateReason::TrustChainNotConfigured,
            "trust_chain_not_configured",
            "La mise à jour intégrée n'est pas encore activée pour cette copie.",
            "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page \
             officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
        ),
        (
            ManualUpdateReason::DevelopmentBuild,
            "development_build",
            "La mise à jour intégrée n'est pas disponible pour un build de développement.",
            "Reconstruis Rustory depuis les sources pour obtenir la dernière version.",
        ),
    ];
    for (reason, token, headline, guidance) in expected {
        let dto = UpdateApplyPlanDto::from_mode(UpdateApplyMode::Manual { reason });
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(
            v,
            serde_json::json!({
                "mode": "manual",
                "reason": token,
                "headline": headline,
                "guidance": guidance,
            }),
            "manual plan {reason:?} must serialize its frozen couple"
        );
    }
}

// ===== State DTO — exact serialization of the four sealed states =====

#[test]
fn the_idle_state_serializes_as_the_bare_status() {
    let dto = UpdateApplyStateDto::from_snapshot(snapshot(UpdateApplyState::Idle, None));
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(v, serde_json::json!({ "status": "idle" }));
}

#[test]
fn the_running_state_serializes_its_phase_known_percent_and_correlation_id() {
    let dto = UpdateApplyStateDto::from_snapshot(snapshot(
        UpdateApplyState::Running {
            phase: UpdateApplyPhase::Downloading,
            percent: Some(42),
        },
        Some("j1"),
    ));
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "running",
            "jobId": "j1",
            "phase": "downloading",
            "percent": 42,
            "headline": "Téléchargement de la mise à jour en cours…",
            "notice": "Tu peux continuer à utiliser Rustory pendant cette opération.",
        })
    );
}

#[test]
fn the_running_state_omits_an_unknown_percent() {
    for (phase, tag, headline) in [
        (
            UpdateApplyPhase::Checking,
            "checking",
            "Vérification de la mise à jour en cours…",
        ),
        (
            UpdateApplyPhase::Downloading,
            "downloading",
            "Téléchargement de la mise à jour en cours…",
        ),
        (
            UpdateApplyPhase::Installing,
            "installing",
            "Installation de la mise à jour en cours…",
        ),
    ] {
        let dto = UpdateApplyStateDto::from_snapshot(snapshot(
            UpdateApplyState::Running {
                phase,
                percent: None,
            },
            Some("j1"),
        ));
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(
            v,
            serde_json::json!({
                "status": "running",
                "jobId": "j1",
                "phase": tag,
                "headline": headline,
                "notice": "Tu peux continuer à utiliser Rustory pendant cette opération.",
            }),
            "running {tag} must omit the unknown percent (never null)"
        );
        assert!(v.get("percent").is_none());
    }
}

#[test]
fn the_ready_to_restart_state_serializes_its_frozen_couple() {
    // The session keeps the finished job's id, but the wire correlation
    // rides the RUNNING face only — a terminal needs no re-attachment.
    let dto =
        UpdateApplyStateDto::from_snapshot(snapshot(UpdateApplyState::ReadyToRestart, Some("j1")));
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "readyToRestart",
            "headline": "La mise à jour de Rustory est prête.",
            "notice": "Redémarre Rustory pour terminer l'installation. Ton travail local reste \
                       en place.",
        })
    );
    assert!(v.get("jobId").is_none());
}

#[test]
fn every_failed_state_serializes_its_stage_and_frozen_couple() {
    let expected: [(UpdateApplyFailureStage, &str, &str, &str); 5] = [
        (
            UpdateApplyFailureStage::Feed,
            "feed",
            "Le canal de mise à jour n'a pas répondu.",
            "Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des \
             versions reste disponible : github.com/roukmoute/Rustory/releases.",
        ),
        (
            UpdateApplyFailureStage::NotApplicable,
            "not_applicable",
            "La mise à jour n'est pas encore proposée pour cette installation.",
            "La nouvelle version n'est pas encore publiée sur le canal de mise à jour de cette \
             copie. La page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases.",
        ),
        (
            UpdateApplyFailureStage::Download,
            "download",
            "Le téléchargement de la mise à jour n'a pas abouti.",
            "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
        ),
        (
            UpdateApplyFailureStage::Verification,
            "verification",
            "L'authenticité de la mise à jour n'a pas pu être confirmée.",
            "Rien n'a été installé : Rustory reste sur sa version actuelle. Réessaie plus tard ; \
             la page officielle des versions reste disponible : \
             github.com/roukmoute/Rustory/releases.",
        ),
        (
            UpdateApplyFailureStage::Install,
            "install",
            "L'installation de la mise à jour n'a pas abouti.",
            "Ta version actuelle de Rustory reste en place et utilisable. Réessaie, ou passe par \
             la page officielle des versions : github.com/roukmoute/Rustory/releases.",
        ),
    ];
    for (stage, token, headline, notice) in expected {
        let dto = UpdateApplyStateDto::from_snapshot(snapshot(
            UpdateApplyState::Failed { stage },
            Some("j1"),
        ));
        let v = serde_json::to_value(&dto).expect("ser");
        assert_eq!(
            v,
            serde_json::json!({
                "status": "failed",
                "stage": token,
                "headline": headline,
                "notice": notice,
            }),
            "failed stage {stage:?} must serialize its frozen couple"
        );
        assert!(v.get("phase").is_none());
        assert!(v.get("percent").is_none());
        assert!(v.get("jobId").is_none());
    }
}

// ===== Start DTO — outcome states, jobId discipline =====

#[test]
fn an_accepted_start_serializes_its_job_id() {
    let dto = StartUpdateApplyDto::from_outcome(StartUpdateApplyOutcome::Started {
        job_id: "0190a0b0-c0d0-7000-8000-000000000000".to_string(),
    });
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "outcome": "started",
            "jobId": "0190a0b0-c0d0-7000-8000-000000000000",
        })
    );
}

#[test]
fn the_two_refusals_serialize_without_a_job_id() {
    let already = StartUpdateApplyDto::from_outcome(StartUpdateApplyOutcome::AlreadyInFlight);
    let v = serde_json::to_value(&already).expect("ser");
    assert_eq!(v, serde_json::json!({ "outcome": "alreadyRunning" }));
    assert!(v.get("jobId").is_none());

    let not_eligible = StartUpdateApplyDto::from_outcome(StartUpdateApplyOutcome::NotEligible);
    let v = serde_json::to_value(&not_eligible).expect("ser");
    assert_eq!(v, serde_json::json!({ "outcome": "notEligible" }));
    assert!(v.get("jobId").is_none());
}

// ===== Dedicated update:* events — exact payloads =====

#[test]
fn update_progress_serializes_exactly_with_and_without_percent() {
    let with = UpdateApplyProgressEvent {
        job_id: "j1".to_string(),
        phase: "downloading".to_string(),
        percent: Some(7),
        sequence: 3,
    };
    assert_eq!(
        serde_json::to_value(&with).expect("ser"),
        serde_json::json!({
            "jobId": "j1",
            "phase": "downloading",
            "percent": 7,
            "sequence": 3,
        })
    );
    let without = UpdateApplyProgressEvent {
        job_id: "j1".to_string(),
        phase: "checking".to_string(),
        percent: None,
        sequence: 0,
    };
    let v = serde_json::to_value(&without).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "jobId": "j1",
            "phase": "checking",
            "sequence": 0,
        })
    );
    assert!(v.get("percent").is_none(), "omitted, never null");
}

#[test]
fn update_completed_serializes_exactly() {
    let ev = UpdateApplyCompletedEvent {
        job_id: "j1".to_string(),
        sequence: 5,
    };
    assert_eq!(
        serde_json::to_value(&ev).expect("ser"),
        serde_json::json!({
            "jobId": "j1",
            "sequence": 5,
        })
    );
}

#[test]
fn update_failed_serializes_exactly_with_the_frozen_copies() {
    let ev = UpdateApplyFailedEvent {
        job_id: "j1".to_string(),
        sequence: 4,
        stage: "download".to_string(),
        headline: "Le téléchargement de la mise à jour n'a pas abouti.".to_string(),
        notice: "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie."
            .to_string(),
    };
    assert_eq!(
        serde_json::to_value(&ev).expect("ser"),
        serde_json::json!({
            "jobId": "j1",
            "sequence": 4,
            "stage": "download",
            "headline": "Le téléchargement de la mise à jour n'a pas abouti.",
            "notice": "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis \
                       réessaie.",
        })
    );
}

// ===== Engraved constants =====

#[test]
fn the_feed_endpoint_its_override_and_the_event_names_are_locked() {
    // The env override (`RUSTORY_UPDATER_FEED_ENDPOINT`) is a smoke
    // tool; THIS constant is what a distributed copy consults — the
    // `stable` channel's signed feed, public IFF a release is PUBLISHED.
    assert_eq!(
        UPDATE_FEED_ENDPOINT,
        "https://github.com/roukmoute/Rustory/releases/latest/download/latest.json"
    );
    assert_eq!(UPDATE_FEED_ENDPOINT_ENV, "RUSTORY_UPDATER_FEED_ENDPOINT");
    assert_eq!(UPDATE_FEED_CHECK_BUDGET.as_secs(), 10);
    assert_eq!(EVENT_UPDATE_PROGRESS, "update:progress");
    assert_eq!(EVENT_UPDATE_COMPLETED, "update:completed");
    assert_eq!(EVENT_UPDATE_FAILED, "update:failed");
}

// ===== The neutral plugins.updater shell (documented exception) =====

#[test]
fn the_committed_updater_config_block_stays_a_neutral_shell() {
    // The crate REQUIRES a deserializable `plugins.updater` block to
    // register (a required `pubkey` field — an absent block fails the
    // boot), so the committed file carries a NEUTRAL SHELL: an empty
    // pubkey and nothing else. The effective configuration is 100%
    // runtime (gateway endpoint + compile-time public key); this lock
    // trips if anyone turns the shell into a real static config — a
    // static endpoint, a dangerous flag or a committed key never ships.
    // And `createUpdaterArtifacts` stays UNCOMMITTED: a local build must
    // never require a signing key (the release workflow's overlay alone
    // enables it).
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
    )
    .expect("read tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");

    assert_eq!(
        conf["plugins"]["updater"],
        serde_json::json!({ "pubkey": "" }),
        "plugins.updater must stay the neutral shell — the real endpoint and \
         public key are runtime facts, never static config"
    );
    assert!(
        conf["bundle"].get("createUpdaterArtifacts").is_none(),
        "createUpdaterArtifacts must never be committed — the release \
         workflow's overlay alone enables it (a local build needs no key)"
    );
}

#[test]
fn the_release_overlay_never_exists_in_the_repo() {
    // build-release.yml materializes `updater-overlay.json` on the
    // RUNNER only (the overlay enabling `createUpdaterArtifacts`, also
    // gitignored): a committed overlay would silently turn every local
    // build into a key-demanding release build — and the neutral-shell
    // lock above cannot catch it (the overlay is a separate file).
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("updater-overlay.json")
            .exists(),
        "src-tauri/updater-overlay.json must never exist in the repo — the \
         release workflow materializes it on the runner only"
    );
}
