//! Official file-association registry: WHETHER each official
//! distribution channel registers Rustory as the OS handler for its
//! supported file type (`.rustory` — the single double-clickable file
//! of the local-artifact registry), decided line by line — the exact
//! pattern of the device support matrix
//! (`domain::device::support_matrix`) and of the local-artifact
//! registry (`local_artifact`). The registry mirrors WORD FOR WORD the
//! per-channel table of `docs/architecture/device-support-profile.md#File
//! Association Contract`: the registration is a PACKAGING-TIME fact
//! (`bundle.fileAssociations`, the desktop-entry template, the
//! shared-mime-info XML) — this module only DOCUMENTS it honestly, it
//! never mutates an OS preference.
//!
//! The module also carries the PURE Linux install probe: the frontier
//! hands it the `APPIMAGE` marker, the current executable path and the
//! presence of the package's shared-mime-info XML — every decision
//! (including the corroboration of a possibly inherited marker) lives
//! here. Windows/macOS have no reliable runtime marker — no probe
//! exists for them by design (no claim is ever invented).
//!
//! Pure domain: channel in, documented registration out, zero I/O.

/// Closed set of the official distribution channels the release
/// runbook ships — one variant per documented channel line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAssociationChannel {
    /// The Linux system package (`.deb` / `.rpm`).
    LinuxSystemPackage,
    /// The Linux AppImage (self-contained, installs nothing).
    LinuxAppImage,
    /// The Windows installer (`.msi` / `.exe`).
    WindowsInstaller,
    /// The macOS app bundle (`.dmg`).
    MacosAppBundle,
}

/// Every official channel, in the stable rendering order of the
/// documented table. Tripwire: a new enum variant fails the exhaustive
/// `match` below, forcing an explicit registry decision for it.
pub const ALL_FILE_ASSOCIATION_CHANNELS: [FileAssociationChannel; 4] = [
    FileAssociationChannel::LinuxSystemPackage,
    FileAssociationChannel::LinuxAppImage,
    FileAssociationChannel::WindowsInstaller,
    FileAssociationChannel::MacosAppBundle,
];

impl FileAssociationChannel {
    /// Stable camelCase wire tag (support-profile DTO). Must stay
    /// byte-identical to the TS mirror's closed set.
    pub const fn wire_tag(self) -> &'static str {
        match self {
            Self::LinuxSystemPackage => "linuxSystemPackage",
            Self::LinuxAppImage => "linuxAppImage",
            Self::WindowsInstaller => "windowsInstaller",
            Self::MacosAppBundle => "macosAppBundle",
        }
    }
}

/// Registration state of ONE channel line: the CLOSED set of DOCUMENTED
/// registration modes, or the not-registered state WITH its frozen
/// user-facing reason. A reason-less limit is unrepresentable by
/// construction — a non-registered channel can never reach the screen
/// as a bare ✗ (the local-artifact `Deferred { reason }` pattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAssociationRegistration {
    /// The package/installer declares the association at install time
    /// (Linux system package, Windows installer).
    InstalledWithPackage,
    /// The operating system registers the association itself when the
    /// app lands where it expects it (macOS Launch Services).
    RegisteredBySystem,
    /// The channel registers nothing by default, with its frozen
    /// honest reason (Linux AppImage).
    NotRegisteredByDefault { reason: &'static str },
}

impl FileAssociationRegistration {
    pub const fn is_registered(self) -> bool {
        !matches!(self, Self::NotRegisteredByDefault { .. })
    }

    /// The frozen reason of a non-registered channel — `None` on a
    /// registered one (the status itself replaces it).
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::InstalledWithPackage | Self::RegisteredBySystem => None,
            Self::NotRegisteredByDefault { reason } => Some(reason),
        }
    }
}

/// One line of the official registry: a known channel, the registration
/// the distribution documents on it and the frozen one-line detail the
/// screen renders verbatim under the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileAssociationLine {
    pub channel: FileAssociationChannel,
    pub registration: FileAssociationRegistration,
    pub detail: &'static str,
}

/// THE official file-association registry of this distribution —
/// decided line by line, never wholesale (the device support-matrix
/// pattern: every line carries its own justification).
const OFFICIAL_FILE_ASSOCIATIONS: &[FileAssociationLine] = &[
    // Linux system package ✅ registered at install — the package
    // installs the desktop entry (custom template with `%F`) plus the
    // shared-mime-info XML; the distro package triggers update the
    // databases at install time.
    FileAssociationLine {
        channel: FileAssociationChannel::LinuxSystemPackage,
        registration: FileAssociationRegistration::InstalledWithPackage,
        detail: "L'association est déclarée par le paquet et active dès l'installation.",
    },
    // Linux AppImage ❌ not registered by default — an AppImage
    // installs NOTHING into the system at launch; the integration
    // belongs to an external tool or a manual desktop entry. The line
    // stays VISIBLE with its honest frozen reason, never silently
    // dropped (the deferred local-artifact pattern).
    FileAssociationLine {
        channel: FileAssociationChannel::LinuxAppImage,
        registration: FileAssociationRegistration::NotRegisteredByDefault {
            reason: "Tu peux ajouter l'association avec un outil d'intégration AppImage \
                     ou une entrée d'application manuelle.",
        },
        detail: "Une AppImage ne modifie pas ton système : rien n'est enregistré \
                 automatiquement.",
    },
    // Windows installer ✅ registered at install — NSIS/WiX declare
    // the ProgId; Rustory becomes a CANDIDATE handler and Windows
    // protects an existing user default (`UserChoice`): the user
    // confirms through the OS dialog — respected, never fought.
    FileAssociationLine {
        channel: FileAssociationChannel::WindowsInstaller,
        registration: FileAssociationRegistration::InstalledWithPackage,
        detail: "L'installeur déclare l'association. Windows peut te demander de \
                 confirmer et respecte ton choix existant.",
    },
    // macOS app ✅ registered by the system — the bundler injects the
    // document types into the Info.plist; Launch Services registers
    // the app when it lands in Applications (the DMG plays no role).
    FileAssociationLine {
        channel: FileAssociationChannel::MacosAppBundle,
        registration: FileAssociationRegistration::RegisteredBySystem,
        detail: "macOS enregistre l'association quand l'application est déposée dans \
                 Applications.",
    },
];

/// The official registry, as a borrowed slice: the support-profile
/// wire serializes it line by line.
pub fn official_file_association_lines() -> &'static [FileAssociationLine] {
    OFFICIAL_FILE_ASSOCIATIONS
}

/// The absolute path where the Linux system package installs the
/// shared-mime-info XML declaring `application/x-rustory` — THE
/// package artifact that proves the association was declared to the
/// system (the desktop entry travels in the same package, so the XML
/// suffices as the witness). The frontier probes its existence for
/// the install probe below; the packaging contract test locks the
/// bundle `files` maps on this exact target.
pub const LINUX_PACKAGE_MIME_XML: &str = "/usr/share/mime/packages/fr.roukmoute.rustory.xml";

/// Closed set of the Linux install kinds the pure probe can decide —
/// the CURRENT install, not a channel promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxInstallKind {
    /// The running process was launched from an AppImage.
    AppImage,
    /// The running executable lives under `/usr/` AND the package's
    /// association artifact is installed — a system package put it
    /// there.
    SystemPackage,
    /// The running executable is known but unpackaged (a local build,
    /// a dev run, a hand-copied binary).
    LocalBuild,
}

/// Whether a path sits inside an AppImage mount point — AppImageKit
/// mounts every image to `$TMPDIR/.mount_<prefix><hash>` (the same
/// structural fact tauri-utils checks when it warns about a stale
/// `APPIMAGE` variable), so a `.mount_*`-prefixed component is the
/// corroborating witness of a genuinely mounted AppImage.
fn is_appimage_mount_path(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::Normal(name)
                if name.to_string_lossy().starts_with(".mount_")
        )
    })
}

/// PURE classification of the current Linux install. The FRONTIER
/// hands in the raw observations (`app.env().appimage`,
/// `std::env::current_exe()`, the presence of the package's
/// shared-mime-info XML at [`LINUX_PACKAGE_MIME_XML`]); the decision
/// lives here, testable without any environment. Every claim needs
/// CORROBORATION — a contradicted or uncorroborable observation set
/// yields `None`, never a guess:
///
/// - a non-empty `appimage` marker is hearsay on its own (it survives
///   into children of ANOTHER AppImage and into polluted
///   environments; tauri-utils only logs the incoherence and keeps
///   the variable): the claim additionally requires the executable to
///   sit inside an AppImage mount point (`.mount_*` component). A
///   known executable OUTSIDE such a mount contradicts the marker, an
///   unknown executable cannot corroborate it — `None` in both cases,
///   never a false channel limit;
/// - no marker, an executable under `/usr/` — EXCLUDING `/usr/local`,
///   which the FHS reserves for local, non-packaged installs — AND
///   the package's mime XML installed →
///   [`LinuxInstallKind::SystemPackage`] (`/usr` alone proves
///   nothing: a hand-copied binary lands there too — the package's
///   own association artifact is the required witness, and a
///   `/usr/local` copy is never package-provided even when the
///   official package is independently installed elsewhere);
/// - any other known executable → [`LinuxInstallKind::LocalBuild`];
/// - an indeterminable executable → `None`: NO claim is ever invented
///   (the screen simply omits the notice).
pub fn classify_linux_install(
    appimage: Option<&std::ffi::OsStr>,
    current_exe: Option<&std::path::Path>,
    package_mime_xml_present: bool,
) -> Option<LinuxInstallKind> {
    if appimage.is_some_and(|marker| !marker.is_empty()) {
        return match current_exe {
            Some(exe) if is_appimage_mount_path(exe) => Some(LinuxInstallKind::AppImage),
            _ => None,
        };
    }
    let exe = current_exe?;
    if exe.starts_with("/usr") && !exe.starts_with("/usr/local") && package_mime_xml_present {
        Some(LinuxInstallKind::SystemPackage)
    } else {
        Some(LinuxInstallKind::LocalBuild)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== The official registry — one line = one test, mirroring the
    // documented per-channel table. =====

    #[test]
    fn official_registry_registers_the_linux_system_package_at_install() {
        let line = official_file_association_lines()
            .iter()
            .find(|line| line.channel == FileAssociationChannel::LinuxSystemPackage)
            .expect("linux system package line");
        assert_eq!(
            line.registration,
            FileAssociationRegistration::InstalledWithPackage
        );
        assert!(line.registration.is_registered());
        assert_eq!(line.registration.reason(), None);
        assert_eq!(
            line.detail,
            "L'association est déclarée par le paquet et active dès l'installation."
        );
    }

    #[test]
    fn official_registry_documents_the_appimage_as_not_registered_with_its_reason() {
        // Documents the CURRENT distribution state: the AppImage line
        // stays VISIBLE with its honest frozen reason — an assisted
        // AppImage integration appearing one day is an announced
        // re-scope of this test.
        let line = official_file_association_lines()
            .iter()
            .find(|line| line.channel == FileAssociationChannel::LinuxAppImage)
            .expect("appimage line");
        assert!(!line.registration.is_registered());
        let reason = line
            .registration
            .reason()
            .expect("a non-registered channel carries a reason");
        assert_eq!(
            reason,
            "Tu peux ajouter l'association avec un outil d'intégration AppImage \
             ou une entrée d'application manuelle."
        );
        assert_eq!(
            line.detail,
            "Une AppImage ne modifie pas ton système : rien n'est enregistré \
             automatiquement."
        );
    }

    #[test]
    fn official_registry_registers_the_windows_installer_at_install() {
        let line = official_file_association_lines()
            .iter()
            .find(|line| line.channel == FileAssociationChannel::WindowsInstaller)
            .expect("windows installer line");
        assert_eq!(
            line.registration,
            FileAssociationRegistration::InstalledWithPackage
        );
        assert!(line.registration.is_registered());
        assert_eq!(line.registration.reason(), None);
        assert_eq!(
            line.detail,
            "L'installeur déclare l'association. Windows peut te demander de \
             confirmer et respecte ton choix existant."
        );
    }

    #[test]
    fn official_registry_documents_macos_as_registered_by_the_system() {
        let line = official_file_association_lines()
            .iter()
            .find(|line| line.channel == FileAssociationChannel::MacosAppBundle)
            .expect("macos line");
        assert_eq!(
            line.registration,
            FileAssociationRegistration::RegisteredBySystem
        );
        assert!(line.registration.is_registered());
        assert_eq!(line.registration.reason(), None);
        assert_eq!(
            line.detail,
            "macOS enregistre l'association quand l'application est déposée dans \
             Applications."
        );
    }

    #[test]
    fn official_registry_carries_every_known_channel_exactly_once() {
        for channel in ALL_FILE_ASSOCIATION_CHANNELS {
            let lines = official_file_association_lines()
                .iter()
                .filter(|line| line.channel == channel)
                .count();
            assert_eq!(lines, 1, "channel {channel:?} must have exactly one line");
        }
        assert_eq!(
            official_file_association_lines().len(),
            ALL_FILE_ASSOCIATION_CHANNELS.len(),
            "no line may carry an unknown channel"
        );
    }

    #[test]
    fn official_registry_preserves_the_documented_rendering_order() {
        let channels: Vec<FileAssociationChannel> = official_file_association_lines()
            .iter()
            .map(|line| line.channel)
            .collect();
        assert_eq!(channels, ALL_FILE_ASSOCIATION_CHANNELS.to_vec());
    }

    #[test]
    fn every_official_detail_is_non_empty() {
        for line in official_file_association_lines() {
            assert!(
                !line.detail.is_empty(),
                "channel {:?}: a line never renders without its detail",
                line.channel
            );
        }
    }

    // ===== Registration — availability/reason coherent by construction =====

    #[test]
    fn registration_reason_is_coherent_with_the_registered_state() {
        assert!(FileAssociationRegistration::InstalledWithPackage.is_registered());
        assert_eq!(
            FileAssociationRegistration::InstalledWithPackage.reason(),
            None
        );
        assert!(FileAssociationRegistration::RegisteredBySystem.is_registered());
        assert_eq!(
            FileAssociationRegistration::RegisteredBySystem.reason(),
            None
        );
        let not_registered = FileAssociationRegistration::NotRegisteredByDefault { reason: "why" };
        assert!(!not_registered.is_registered());
        assert_eq!(not_registered.reason(), Some("why"));
    }

    // ===== Wire tags — stable, distinct, exhaustive =====

    #[test]
    fn channel_wire_tags_are_stable() {
        // Exhaustive by construction: iterating the ALL_ tripwire array.
        let tags: Vec<&str> = ALL_FILE_ASSOCIATION_CHANNELS
            .iter()
            .map(|channel| channel.wire_tag())
            .collect();
        assert_eq!(
            tags,
            vec![
                "linuxSystemPackage",
                "linuxAppImage",
                "windowsInstaller",
                "macosAppBundle"
            ]
        );
    }

    #[test]
    fn wire_tags_are_pairwise_distinct() {
        for (i, a) in ALL_FILE_ASSOCIATION_CHANNELS.iter().enumerate() {
            for b in &ALL_FILE_ASSOCIATION_CHANNELS[i + 1..] {
                assert_ne!(a.wire_tag(), b.wire_tag());
            }
        }
    }

    #[test]
    fn all_file_association_channels_tripwire_is_exhaustive() {
        // Compile-time tripwire: adding a channel variant breaks this
        // exhaustive match, forcing ALL_FILE_ASSOCIATION_CHANNELS (and
        // the official registry, through the exactly-once test above)
        // to absorb the newcomer explicitly.
        for channel in ALL_FILE_ASSOCIATION_CHANNELS {
            match channel {
                FileAssociationChannel::LinuxSystemPackage
                | FileAssociationChannel::LinuxAppImage
                | FileAssociationChannel::WindowsInstaller
                | FileAssociationChannel::MacosAppBundle => {}
            }
        }
    }

    // ===== The pure Linux install probe =====

    #[test]
    fn probe_classifies_a_corroborated_appimage_marker_as_appimage() {
        // Marker + executable inside an AppImage mount point: the two
        // observations agree — the claim stands.
        assert_eq!(
            classify_linux_install(
                Some(std::ffi::OsStr::new("/home/user/Rustory.AppImage")),
                Some(std::path::Path::new(
                    "/tmp/.mount_Rustor1a2b/usr/bin/rustory"
                )),
                false,
            ),
            Some(LinuxInstallKind::AppImage)
        );
    }

    #[test]
    fn probe_claims_nothing_when_the_appimage_marker_is_contradicted_or_uncorroborated() {
        // An inherited/polluted marker with an executable OUTSIDE any
        // AppImage mount contradicts it: no claim, never a false
        // channel limit (tauri-utils itself only logs this case).
        assert_eq!(
            classify_linux_install(
                Some(std::ffi::OsStr::new("/home/user/Autre.AppImage")),
                Some(std::path::Path::new("/usr/bin/rustory")),
                true,
            ),
            None
        );
        assert_eq!(
            classify_linux_install(
                Some(std::ffi::OsStr::new("/home/user/Autre.AppImage")),
                Some(std::path::Path::new("/home/user/rustory")),
                false,
            ),
            None
        );
        // An unknown executable cannot corroborate the marker either.
        assert_eq!(
            classify_linux_install(
                Some(std::ffi::OsStr::new("/x/Rustory.AppImage")),
                None,
                false
            ),
            None
        );
    }

    #[test]
    fn probe_treats_an_empty_appimage_marker_as_absent() {
        // An empty marker carries no path — it never claims AppImage.
        assert_eq!(
            classify_linux_install(
                Some(std::ffi::OsStr::new("")),
                Some(std::path::Path::new("/usr/bin/rustory")),
                true,
            ),
            Some(LinuxInstallKind::SystemPackage)
        );
    }

    #[test]
    fn probe_requires_the_package_artifact_to_claim_a_system_package() {
        // Executable under `/usr` + the package's mime XML installed:
        // the SystemPackage claim is proven by the package's own
        // association artifact.
        assert_eq!(
            classify_linux_install(None, Some(std::path::Path::new("/usr/bin/rustory")), true),
            Some(LinuxInstallKind::SystemPackage)
        );
        // A hand-copied binary under `/usr` WITHOUT the package
        // artifact is no package: the honest claim is a local build
        // (its notice only speaks of what THIS copy provides).
        assert_eq!(
            classify_linux_install(None, Some(std::path::Path::new("/usr/bin/rustory")), false),
            Some(LinuxInstallKind::LocalBuild)
        );
    }

    #[test]
    fn probe_never_claims_a_system_package_for_a_usr_local_executable() {
        // The FHS reserves `/usr/local` for local, NON-packaged
        // installs: a binary compiled or copied there is never
        // package-provided — even when the official package (and its
        // XML witness) is independently installed elsewhere, the
        // RUNNING copy is a local build and its notice must only
        // speak for that copy.
        assert_eq!(
            classify_linux_install(
                None,
                Some(std::path::Path::new("/usr/local/bin/rustory")),
                true,
            ),
            Some(LinuxInstallKind::LocalBuild)
        );
        assert_eq!(
            classify_linux_install(
                None,
                Some(std::path::Path::new("/usr/local/bin/rustory")),
                false,
            ),
            Some(LinuxInstallKind::LocalBuild)
        );
    }

    #[test]
    fn probe_classifies_a_known_executable_elsewhere_as_a_local_build() {
        assert_eq!(
            classify_linux_install(
                None,
                Some(std::path::Path::new(
                    "/home/user/projects/rustory/target/release/rustory"
                )),
                false,
            ),
            Some(LinuxInstallKind::LocalBuild)
        );
        // Even with the package installed elsewhere on the system:
        // THIS running copy is still a local build (the notice speaks
        // of this copy only).
        assert_eq!(
            classify_linux_install(
                None,
                Some(std::path::Path::new(
                    "/home/user/projects/rustory/target/release/rustory"
                )),
                true,
            ),
            Some(LinuxInstallKind::LocalBuild)
        );
        // Component-wise prefix: `/usrx` is NOT under `/usr`.
        assert_eq!(
            classify_linux_install(None, Some(std::path::Path::new("/usrx/bin/rustory")), true),
            Some(LinuxInstallKind::LocalBuild)
        );
    }

    #[test]
    fn probe_claims_nothing_on_an_indeterminable_executable() {
        // No marker, no executable: the probe stays silent — the
        // screen omits the notice rather than inventing a state.
        assert_eq!(classify_linux_install(None, None, false), None);
        assert_eq!(classify_linux_install(None, None, true), None);
    }

    #[test]
    fn the_package_mime_xml_target_matches_the_reverse_dns_shared_mime_location() {
        // The witness path the frontier probes IS the packaging
        // target the bundle files maps declare (the packaging
        // contract test locks the other side of this couple).
        assert_eq!(
            LINUX_PACKAGE_MIME_XML,
            "/usr/share/mime/packages/fr.roukmoute.rustory.xml"
        );
    }
}
