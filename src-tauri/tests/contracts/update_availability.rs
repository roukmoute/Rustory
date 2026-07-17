//! Wire contracts of the update-availability read (`Update Availability
//! Contract`): the exact camelCase serialization of the FOUR sealed
//! states (omission discipline on `latestVersion`), the frozen
//! Rust-carried copies (byte-for-byte, with example versions for the
//! composed ones), the frozen wire tags, the production endpoint
//! constant — and the version-alignment lock (the binary, the bundle
//! and the npm manifest cannot silently drift apart).

use std::path::{Path, PathBuf};

use rustory_lib::domain::update::{parse_release_version, ReleaseVersion, UpdateAvailability};
use rustory_lib::infrastructure::updates::{
    GITHUB_LATEST_RELEASE_ENDPOINT, MAX_UPDATE_RESPONSE_BYTES,
};
use rustory_lib::ipc::dto::settings::UpdateAvailabilityDto;

fn version(major: u64, minor: u64, patch: u64) -> ReleaseVersion {
    ReleaseVersion {
        major,
        minor,
        patch,
    }
}

// ===== Exact serialization of the four sealed states =====

#[test]
fn update_available_serializes_in_camel_case_with_the_latest_version() {
    let dto = UpdateAvailabilityDto::from_availability(
        UpdateAvailability::UpdateAvailable {
            latest: version(9, 9, 9),
        },
        version(0, 1, 0),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "updateAvailable",
            "headline": "Nouvelle version disponible : 9.9.9.",
            "notice": "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis \
                       la page officielle des versions : github.com/roukmoute/Rustory/releases.",
            "currentVersion": "0.1.0",
            "latestVersion": "9.9.9",
        })
    );
}

#[test]
fn up_to_date_serializes_without_the_latest_version_key() {
    let dto =
        UpdateAvailabilityDto::from_availability(UpdateAvailability::UpToDate, version(0, 1, 0));
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "upToDate",
            "headline": "Aucune version plus récente n'est publiée.",
            "notice": "Aucune action n'est nécessaire.",
            "currentVersion": "0.1.0",
        })
    );
    // Omission discipline: the key is ABSENT, never `null`.
    assert!(v.get("latestVersion").is_none());
}

#[test]
fn check_unavailable_serializes_the_calm_transport_state() {
    let dto = UpdateAvailabilityDto::from_availability(
        UpdateAvailability::CheckUnavailable,
        version(0, 1, 0),
    );
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "checkUnavailable",
            "headline": "La vérification de version n'a pas pu être faite.",
            "notice": "Rustory reste pleinement utilisable. La vérification réessaiera au \
                       prochain lancement.",
            "currentVersion": "0.1.0",
        })
    );
    assert!(v.get("latestVersion").is_none());
}

#[test]
fn check_not_run_serializes_the_single_copy_couple() {
    // ONE couple whatever the internal skip motive — the motive lives in
    // the diagnostics log, never on the wire.
    let dto =
        UpdateAvailabilityDto::from_availability(UpdateAvailability::CheckNotRun, version(0, 1, 0));
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "checkNotRun",
            "headline": "La vérification de version n'est pas exécutée pour cette copie.",
            "notice": "Cette copie de Rustory ne provient pas d'un canal de distribution \
                       officiel : aucune vérification réseau n'est effectuée.",
            "currentVersion": "0.1.0",
        })
    );
    assert!(v.get("latestVersion").is_none());
}

#[test]
fn an_out_of_convention_binary_version_serializes_the_calm_fallback() {
    // A binary whose OWN version escapes the strict convention (a
    // locally-built semver pre-release) degrades to the same calm
    // couple, `currentVersion` carrying the RAW string — the TS guard
    // refuses this out-of-convention world (drift-silence regime),
    // and no panic ever poisons the responder.
    let dto = UpdateAvailabilityDto::check_unavailable_with_raw_version("0.2.0-rc.1");
    let v = serde_json::to_value(&dto).expect("ser");
    assert_eq!(
        v,
        serde_json::json!({
            "status": "checkUnavailable",
            "headline": "La vérification de version n'a pas pu être faite.",
            "notice": "Rustory reste pleinement utilisable. La vérification réessaiera au \
                       prochain lancement.",
            "currentVersion": "0.2.0-rc.1",
        })
    );
    assert!(v.get("latestVersion").is_none());
}

// ===== Frozen wire tags =====

#[test]
fn wire_tags_are_frozen() {
    assert_eq!(
        UpdateAvailability::UpdateAvailable {
            latest: version(1, 0, 0)
        }
        .wire_tag(),
        "updateAvailable"
    );
    assert_eq!(UpdateAvailability::UpToDate.wire_tag(), "upToDate");
    assert_eq!(
        UpdateAvailability::CheckUnavailable.wire_tag(),
        "checkUnavailable"
    );
    assert_eq!(UpdateAvailability::CheckNotRun.wire_tag(), "checkNotRun");
}

// ===== Production endpoint and bounds (the engraved constants) =====

#[test]
fn the_production_endpoint_and_response_cap_are_locked() {
    // The env override (`RUSTORY_UPDATE_CHECK_ENDPOINT`) is a smoke
    // tool; THIS constant is what a distributed copy consults.
    assert_eq!(
        GITHUB_LATEST_RELEASE_ENDPOINT,
        "https://api.github.com/repos/roukmoute/Rustory/releases/latest"
    );
    assert_eq!(MAX_UPDATE_RESPONSE_BYTES, 1024 * 1024);
}

// ===== Version alignment (binary == bundle == npm manifest) =====

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn the_three_version_manifests_stay_aligned_and_conventional() {
    // The check compares CARGO_PKG_VERSION against published releases;
    // tauri.conf.json versions the bundles the user installs and
    // package.json the npm side. A silent drift between them would make
    // the comparison lie about the INSTALLED copy — locked here (the
    // packaging-guard pattern: the verify pipeline never bundles, this
    // test is the only net).
    let cargo_version = env!("CARGO_PKG_VERSION");

    let tauri_conf_raw =
        std::fs::read_to_string(manifest_path("tauri.conf.json")).expect("read tauri.conf.json");
    let tauri_conf: serde_json::Value =
        serde_json::from_str(&tauri_conf_raw).expect("parse tauri.conf.json");
    assert_eq!(
        tauri_conf["version"].as_str().expect("bundle version"),
        cargo_version,
        "tauri.conf.json#version must match CARGO_PKG_VERSION"
    );

    let package_json_raw =
        std::fs::read_to_string(manifest_path("../package.json")).expect("read package.json");
    let package_json: serde_json::Value =
        serde_json::from_str(&package_json_raw).expect("parse package.json");
    assert_eq!(
        package_json["version"].as_str().expect("npm version"),
        cargo_version,
        "package.json#version must match CARGO_PKG_VERSION"
    );

    // And the shared value respects the strict release convention (the
    // domain tripwire, re-asserted at the contract level).
    assert!(parse_release_version(cargo_version).is_some());
}
