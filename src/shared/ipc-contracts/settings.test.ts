import { describe, expect, it } from "vitest";

import {
  isDeviceSupportLine,
  isFileAssociation,
  isLocalArtifactLine,
  isStartUpdateApplyOutcome,
  isSupportProfile,
  isUpdateApplyCompletedEvent,
  isUpdateApplyFailedEvent,
  isUpdateApplyPlan,
  isUpdateApplyProgressEvent,
  isUpdateApplyState,
  isUpdateAvailability,
} from "./settings";

/** The EXACT official payload Rust serializes (mirror of the contract
 *  test `the_official_device_matrix_serializes_exactly`). */
function officialProfile() {
  return {
    devices: [
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "origineV1",
        cohortLabel: "Origine v1",
        metadataFormatLabel: "Format métadonnées v3",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: true,
          },
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "midGenV2",
        cohortLabel: "Mid-Gen v2",
        metadataFormatLabel: "Format métadonnées v6",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: true,
          },
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "v3",
        cohortLabel: "V3",
        metadataFormatLabel: "Format métadonnées v7",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: false,
            reason: "Rétro-ingénierie du format en cours",
          },
          {
            operation: "writeStory",
            label: "Transfert vers la Lunii",
            available: false,
            reason: "Rétro-ingénierie du format en cours",
          },
        ],
      },
      {
        family: "flam",
        familyLabel: "FLAM",
        cohort: "flamGen1",
        cohortLabel: "Gen1",
        capabilities: [
          {
            operation: "readLibrary",
            label: "Lecture bibliothèque appareil",
            available: true,
          },
          {
            operation: "inspectStory",
            label: "Inspection d'histoire",
            available: true,
          },
          {
            operation: "importStory",
            label: "Copie dans la bibliothèque locale",
            available: true,
          },
          {
            operation: "writeStory",
            label: "Transfert vers l'appareil",
            available: false,
            reason: "Écriture non prouvée sur matériel réel",
          },
        ],
      },
    ],
    localArtifacts: [
      {
        kind: "rustoryArtifact",
        label: "Artefact d'histoire Rustory (.rustory)",
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Import et export",
      },
      {
        kind: "structuredFolder",
        label: "Dossier structuré",
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Création d'une histoire",
      },
      {
        kind: "structuredArchive",
        label: "Archive structurée",
        available: false,
        reason: "Lecture d'archives non prise en charge",
      },
    ],
    fileAssociation: officialFileAssociation(),
  };
}

/** The EXACT official file-association block Rust serializes (mirror
 *  of the contract test
 *  `the_official_file_association_block_serializes_exactly`) — no
 *  `currentInstall`: the probe spoke on no platform by default. */
function officialFileAssociation() {
  return {
    extensionLabel: ".rustory",
    channels: [
      {
        channel: "linuxSystemPackage",
        label: "Paquet Linux (.deb / .rpm)",
        registered: true,
        statusLabel: "Enregistrée à l'installation",
        detail:
          "L'association est déclarée par le paquet et active dès l'installation.",
      },
      {
        channel: "linuxAppImage",
        label: "AppImage (Linux)",
        registered: false,
        statusLabel: "Non enregistrée d'office",
        detail:
          "Une AppImage ne modifie pas ton système : rien n'est enregistré automatiquement.",
        reason:
          "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
      },
      {
        channel: "windowsInstaller",
        label: "Installeur Windows (.msi / .exe)",
        registered: true,
        statusLabel: "Enregistrée à l'installation",
        detail:
          "L'installeur déclare l'association. Windows peut te demander de confirmer et respecte ton choix existant.",
      },
      {
        channel: "macosAppBundle",
        label: "Application macOS (.dmg)",
        registered: true,
        statusLabel: "Enregistrée par le système",
        detail:
          "macOS enregistre l'association quand l'application est déposée dans Applications.",
      },
    ],
  };
}

describe("isSupportProfile", () => {
  it("accepts the exact official payload Rust serializes", () => {
    expect(isSupportProfile(officialProfile())).toBe(true);
  });

  it("rejects a profile with a device line missing (partial profiles never render)", () => {
    const profile = officialProfile();
    profile.devices.splice(2, 1); // drop the V3 line
    expect(isSupportProfile(profile)).toBe(false);
  });

  it("rejects a profile with a duplicated cohort", () => {
    const profile = officialProfile();
    profile.devices[1] = profile.devices[0];
    expect(isSupportProfile(profile)).toBe(false);
  });

  it("rejects a profile with an artifact line missing", () => {
    const profile = officialProfile();
    profile.localArtifacts.pop(); // drop the deferred archive line
    expect(isSupportProfile(profile)).toBe(false);
  });

  it("rejects a non-object and a missing collection", () => {
    expect(isSupportProfile(null)).toBe(false);
    expect(isSupportProfile("profile")).toBe(false);
    expect(isSupportProfile({ devices: [] })).toBe(false);
  });

  it("rejects a profile missing its file-association block", () => {
    const profile = officialProfile() as Record<string, unknown>;
    delete profile.fileAssociation;
    expect(isSupportProfile(profile)).toBe(false);
  });

  it("accepts a profile whose probe spoke (a valid currentInstall)", () => {
    const profile = officialProfile();
    (profile.fileAssociation as Record<string, unknown>).currentInstall = {
      kind: "appImage",
      notice:
        "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
    };
    expect(isSupportProfile(profile)).toBe(true);
  });
});

describe("isFileAssociation", () => {
  it("accepts the exact official block Rust serializes (no probe verdict)", () => {
    expect(isFileAssociation(officialFileAssociation())).toBe(true);
  });

  it("accepts each frozen current-install couple", () => {
    for (const [kind, notice] of [
      [
        "appImage",
        "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
      ],
      [
        "systemPackage",
        "Ton installation actuelle provient d'un paquet système : l'association est enregistrée.",
      ],
      [
        "localBuild",
        "Cette version de Rustory n'a pas été installée par un paquet officiel : elle n'enregistre pas d'association d'office.",
      ],
    ]) {
      const block = officialFileAssociation() as Record<string, unknown>;
      block.currentInstall = { kind, notice };
      expect(isFileAssociation(block)).toBe(true);
    }
  });

  it("rejects a drifted extension label", () => {
    expect(
      isFileAssociation({
        ...officialFileAssociation(),
        extensionLabel: ".zip",
      }),
    ).toBe(false);
  });

  it("rejects a channel line missing (non-registered lines stay visible)", () => {
    const block = officialFileAssociation();
    block.channels.splice(1, 1); // drop the AppImage line
    expect(isFileAssociation(block)).toBe(false);
  });

  it("rejects shuffled channels (canonical wire order required)", () => {
    const block = officialFileAssociation();
    block.channels.reverse();
    expect(isFileAssociation(block)).toBe(false);
  });

  it("rejects an unknown channel and a drifted channel label", () => {
    const block = officialFileAssociation();
    (block.channels[0] as Record<string, unknown>).channel = "flatpak";
    expect(isFileAssociation(block)).toBe(false);

    const drifted = officialFileAssociation();
    (drifted.channels[0] as Record<string, unknown>).label = "Paquet Linux";
    expect(isFileAssociation(drifted)).toBe(false);
  });

  it("rejects an officially REGISTERED channel arriving non-registered (the decision is locked)", () => {
    const block = officialFileAssociation();
    block.channels[0] = {
      ...block.channels[0],
      registered: false,
      statusLabel: "Non enregistrée d'office",
      reason:
        "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
    };
    expect(isFileAssociation(block)).toBe(false);
  });

  it("rejects the AppImage channel arriving registered", () => {
    const block = officialFileAssociation();
    const appimage = block.channels[1] as Record<string, unknown>;
    appimage.registered = true;
    appimage.statusLabel = "Enregistrée à l'installation";
    delete appimage.reason;
    expect(isFileAssociation(block)).toBe(false);
  });

  it("rejects a drifted status label and a drifted detail", () => {
    const status = officialFileAssociation();
    (status.channels[0] as Record<string, unknown>).statusLabel = "Activée";
    expect(isFileAssociation(status)).toBe(false);

    const detail = officialFileAssociation();
    (detail.channels[3] as Record<string, unknown>).detail =
      "macOS fait le nécessaire.";
    expect(isFileAssociation(detail)).toBe(false);
  });

  it("rejects a registered channel that carries a reason", () => {
    const block = officialFileAssociation();
    (block.channels[0] as Record<string, unknown>).reason = "superflu";
    expect(isFileAssociation(block)).toBe(false);
  });

  it("rejects a non-registered channel with a drifted or missing reason (bare ✗ never renders)", () => {
    const drifted = officialFileAssociation();
    (drifted.channels[1] as Record<string, unknown>).reason =
      "Utilise un autre canal.";
    expect(isFileAssociation(drifted)).toBe(false);

    const missing = officialFileAssociation();
    delete (missing.channels[1] as Record<string, unknown>).reason;
    expect(isFileAssociation(missing)).toBe(false);
  });

  it("rejects an unknown install kind and a drifted notice", () => {
    const unknown = officialFileAssociation() as Record<string, unknown>;
    unknown.currentInstall = { kind: "flatpak", notice: "peu importe" };
    expect(isFileAssociation(unknown)).toBe(false);

    const drifted = officialFileAssociation() as Record<string, unknown>;
    drifted.currentInstall = {
      kind: "appImage",
      notice: "Tu utilises une AppImage.",
    };
    expect(isFileAssociation(drifted)).toBe(false);
  });
});

describe("isDeviceSupportLine", () => {
  const v3Line = () => officialProfile().devices[2];
  const flamLine = () => officialProfile().devices[3];

  it("rejects an unknown cohort", () => {
    expect(isDeviceSupportLine({ ...v3Line(), cohort: "v4" })).toBe(false);
  });

  it("rejects a drifted cohort label (frozen couples only)", () => {
    expect(isDeviceSupportLine({ ...v3Line(), cohortLabel: "V3 beta" })).toBe(
      false,
    );
  });

  it("rejects a cohort claiming the wrong family", () => {
    expect(
      isDeviceSupportLine({
        ...v3Line(),
        family: "flam",
        familyLabel: "FLAM",
      }),
    ).toBe(false);
  });

  it("rejects a FLAM line carrying an invented metadata format", () => {
    expect(
      isDeviceSupportLine({
        ...flamLine(),
        metadataFormatLabel: "Format métadonnées v1",
      }),
    ).toBe(false);
  });

  it("rejects a Lunii line missing its metadata format label", () => {
    const line = v3Line() as Record<string, unknown>;
    delete line.metadataFormatLabel;
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects shuffled capabilities (canonical wire order required)", () => {
    const line = v3Line();
    line.capabilities.reverse();
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects an available capability that carries a reason", () => {
    const line = v3Line();
    (line.capabilities[0] as Record<string, unknown>).reason = "superflu";
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects a non-available capability with a drifted reason", () => {
    const line = v3Line();
    (line.capabilities[2] as Record<string, unknown>).reason =
      "Bientôt disponible";
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects a non-available capability with NO reason (bare ✗ never renders)", () => {
    const line = v3Line();
    delete (line.capabilities[2] as Record<string, unknown>).reason;
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects a capability closed outside the official limits (no frozen reason exists)", () => {
    const line = v3Line();
    line.capabilities[0] = {
      operation: "readLibrary",
      label: "Lecture bibliothèque appareil",
      available: false,
      reason: "Rétro-ingénierie du format en cours",
    };
    expect(isDeviceSupportLine(line)).toBe(false);
  });

  it("rejects an officially CLOSED capability arriving available (the support decision is locked)", () => {
    // The three officially closed cells, each presented as available:
    // a regression of the Rust DTO must fail closed, never render a
    // green chip the official matrix forbids.
    const v3 = v3Line();
    v3.capabilities[2] = {
      operation: "importStory",
      label: "Copie dans la bibliothèque locale",
      available: true,
    };
    expect(isDeviceSupportLine(v3)).toBe(false);

    const v3write = v3Line();
    v3write.capabilities[3] = {
      operation: "writeStory",
      label: "Transfert vers la Lunii",
      available: true,
    };
    expect(isDeviceSupportLine(v3write)).toBe(false);

    const flam = flamLine();
    flam.capabilities[3] = {
      operation: "writeStory",
      label: "Transfert vers l'appareil",
      available: true,
    };
    expect(isDeviceSupportLine(flam)).toBe(false);
  });

  it("rejects an officially OPEN capability arriving closed", () => {
    // Even carrying a plausible frozen reason: the availability itself
    // is a frozen decision of the official matrix.
    const flam = flamLine();
    flam.capabilities[2] = {
      operation: "importStory",
      label: "Copie dans la bibliothèque locale",
      available: false,
      reason: "Écriture non prouvée sur matériel réel",
    };
    expect(isDeviceSupportLine(flam)).toBe(false);
  });

  it("rejects a Lunii write label on a FLAM line (family-correct couples)", () => {
    const line = flamLine();
    (line.capabilities[3] as Record<string, unknown>).label =
      "Transfert vers la Lunii";
    expect(isDeviceSupportLine(line)).toBe(false);
  });
});

describe("isLocalArtifactLine", () => {
  const rustoryLine = () => officialProfile().localArtifacts[0];
  const archiveLine = () => officialProfile().localArtifacts[2];

  it("rejects an unknown kind and a drifted label", () => {
    expect(isLocalArtifactLine({ ...rustoryLine(), kind: "zipBundle" })).toBe(
      false,
    );
    expect(isLocalArtifactLine({ ...rustoryLine(), label: ".rustory" })).toBe(
      false,
    );
  });

  it("rejects an available line with a drifted capability wording", () => {
    expect(
      isLocalArtifactLine({
        ...rustoryLine(),
        capabilitiesLabel: "Lecture et écriture",
      }),
    ).toBe(false);
  });

  it("rejects an available line that carries a reason", () => {
    expect(isLocalArtifactLine({ ...rustoryLine(), reason: "superflu" })).toBe(
      false,
    );
  });

  it("rejects a deferred archive arriving available (its capability copy does not exist)", () => {
    expect(
      isLocalArtifactLine({
        ...archiveLine(),
        available: true,
        capabilitiesLabel: "Import",
        reason: undefined,
      }),
    ).toBe(false);
  });

  it("rejects an officially available kind arriving deferred (no frozen reason exists)", () => {
    expect(
      isLocalArtifactLine({
        ...rustoryLine(),
        available: false,
        capabilitiesLabel: undefined,
        reason: "Lecture d'archives non prise en charge",
      }),
    ).toBe(false);
  });

  it("rejects an archive line carrying an invented format label", () => {
    expect(
      isLocalArtifactLine({ ...archiveLine(), formatLabel: "Format v1" }),
    ).toBe(false);
  });

  it("rejects a deferred line whose reason drifted", () => {
    expect(
      isLocalArtifactLine({ ...archiveLine(), reason: "Pas encore prête" }),
    ).toBe(false);
  });
});

describe("isUpdateAvailability", () => {
  /** The EXACT `updateAvailable` payload Rust serializes (mirror of the
   *  update-availability contract tests, example versions). */
  function updateAvailablePayload() {
    return {
      status: "updateAvailable",
      headline: "Nouvelle version disponible : 9.9.9.",
      notice:
        "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
      currentVersion: "0.1.0",
      latestVersion: "9.9.9",
    };
  }

  function upToDatePayload() {
    return {
      status: "upToDate",
      headline: "Aucune version plus récente n'est publiée.",
      notice: "Aucune action n'est nécessaire.",
      currentVersion: "0.1.0",
    };
  }

  it("accepts the four exact states Rust serializes", () => {
    expect(isUpdateAvailability(updateAvailablePayload())).toBe(true);
    expect(isUpdateAvailability(upToDatePayload())).toBe(true);
    expect(
      isUpdateAvailability({
        status: "checkUnavailable",
        headline: "La vérification de version n'a pas pu être faite.",
        notice:
          "Rustory reste pleinement utilisable. La vérification réessaiera au prochain lancement.",
        currentVersion: "0.1.0",
      }),
    ).toBe(true);
    expect(
      isUpdateAvailability({
        status: "checkNotRun",
        headline:
          "La vérification de version n'est pas exécutée pour cette copie.",
        notice:
          "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : aucune vérification réseau n'est effectuée.",
        currentVersion: "0.1.0",
      }),
    ).toBe(true);
  });

  it("rejects an unknown status and non-object payloads", () => {
    expect(
      isUpdateAvailability({ ...upToDatePayload(), status: "unknown" }),
    ).toBe(false);
    expect(isUpdateAvailability(null)).toBe(false);
    expect(isUpdateAvailability("upToDate")).toBe(false);
    expect(isUpdateAvailability(undefined)).toBe(false);
  });

  it("rejects a latestVersion outside the updateAvailable state", () => {
    // Present IFF `updateAvailable` — a stray key is a drift.
    expect(
      isUpdateAvailability({ ...upToDatePayload(), latestVersion: "9.9.9" }),
    ).toBe(false);
  });

  it("rejects an updateAvailable payload missing its latestVersion", () => {
    const payload: Record<string, unknown> = updateAvailablePayload();
    delete payload.latestVersion;
    expect(isUpdateAvailability(payload)).toBe(false);
  });

  it("rejects unconventional versions", () => {
    expect(
      isUpdateAvailability({
        ...updateAvailablePayload(),
        latestVersion: "v9.9.9",
      }),
    ).toBe(false);
    expect(
      isUpdateAvailability({
        ...upToDatePayload(),
        currentVersion: "0.1",
      }),
    ).toBe(false);
    expect(
      isUpdateAvailability({
        ...upToDatePayload(),
        currentVersion: "01.1.0",
      }),
    ).toBe(false);
  });

  it("rejects a drifted constant copy", () => {
    expect(
      isUpdateAvailability({
        ...upToDatePayload(),
        headline: "Tu es à jour !",
      }),
    ).toBe(false);
    expect(
      isUpdateAvailability({
        ...upToDatePayload(),
        notice: "Tout va bien.",
      }),
    ).toBe(false);
  });

  it("rejects a composed copy that does not match the payload's own versions", () => {
    // The headline names another version than the wire's latestVersion —
    // a recomposition drift, never rendered as authoritative.
    expect(
      isUpdateAvailability({
        ...updateAvailablePayload(),
        headline: "Nouvelle version disponible : 8.8.8.",
      }),
    ).toBe(false);
    expect(
      isUpdateAvailability({
        ...updateAvailablePayload(),
        notice:
          "Ta version actuelle est 9.9.9. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
      }),
    ).toBe(false);
  });

  /** An updateAvailable payload whose versions AND recomposed copies are
   *  coherent with each other — isolates the version-relation checks
   *  from the copy-recomposition checks. */
  function coherentUpdateAvailable(current: string, latest: string) {
    return {
      status: "updateAvailable",
      headline: `Nouvelle version disponible : ${latest}.`,
      notice: `Ta version actuelle est ${current}. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.`,
      currentVersion: current,
      latestVersion: latest,
    };
  }

  it("rejects an updateAvailable equal to the current version — the domain never signals equality", () => {
    expect(isUpdateAvailability(coherentUpdateAvailable("0.1.0", "0.1.0"))).toBe(
      false,
    );
  });

  it("rejects an updateAvailable older than the current version — a downgrade never renders", () => {
    expect(isUpdateAvailability(coherentUpdateAvailable("2.0.0", "1.9.9"))).toBe(
      false,
    );
    expect(isUpdateAvailability(coherentUpdateAvailable("0.1.1", "0.1.0"))).toBe(
      false,
    );
    expect(isUpdateAvailability(coherentUpdateAvailable("0.2.0", "0.1.9"))).toBe(
      false,
    );
  });

  it("accepts a strictly newer version on each component", () => {
    expect(isUpdateAvailability(coherentUpdateAvailable("0.1.0", "1.0.0"))).toBe(
      true,
    );
    expect(isUpdateAvailability(coherentUpdateAvailable("0.1.0", "0.2.0"))).toBe(
      true,
    );
    expect(isUpdateAvailability(coherentUpdateAvailable("0.1.0", "0.1.1"))).toBe(
      true,
    );
  });

  it("rejects a version component beyond the Rust u64 domain", () => {
    // u64::MAX itself parses (the exact bound of the binary's domain)…
    expect(
      isUpdateAvailability(
        coherentUpdateAvailable("0.1.0", "18446744073709551615.0.0"),
      ),
    ).toBe(true);
    // …one past it can never be emitted by the binary: a drift.
    expect(
      isUpdateAvailability(
        coherentUpdateAvailable("0.1.0", "18446744073709551616.0.0"),
      ),
    ).toBe(false);
    expect(
      isUpdateAvailability({
        ...upToDatePayload(),
        currentVersion: "99999999999999999999.0.0",
      }),
    ).toBe(false);
  });
});

describe("isUpdateApplyPlan", () => {
  const integratedPlan = () => ({
    mode: "integrated",
    headline: "Cette copie peut installer les mises à jour de Rustory.",
    guidance:
      "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
  });

  const manualPlan = () => ({
    mode: "manual",
    reason: "channel_unproven",
    headline:
      "La mise à jour intégrée n'est pas encore disponible pour cette installation.",
    guidance:
      "Rustory ne peut pas confirmer le canal de cette copie. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  });

  it("accepts the exact integrated and manual payloads Rust serializes", () => {
    expect(isUpdateApplyPlan(integratedPlan())).toBe(true);
    expect(isUpdateApplyPlan(manualPlan())).toBe(true);
    expect(
      isUpdateApplyPlan({
        mode: "manual",
        reason: "trust_chain_not_configured",
        headline:
          "La mise à jour intégrée n'est pas encore activée pour cette copie.",
        guidance:
          "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
      }),
    ).toBe(true);
    expect(
      isUpdateApplyPlan({
        mode: "manual",
        reason: "package_manager_owned",
        headline:
          "La mise à jour de Rustory passe par ton gestionnaire de paquets.",
        guidance:
          "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil de ton système, puis relance Rustory.",
      }),
    ).toBe(true);
    expect(
      isUpdateApplyPlan({
        mode: "manual",
        reason: "unofficial_install",
        headline:
          "La mise à jour intégrée n'est pas disponible pour cette copie.",
        guidance:
          "Cette copie n'est pas passée par un canal de distribution officiel. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
      }),
    ).toBe(true);
    expect(
      isUpdateApplyPlan({
        mode: "manual",
        reason: "development_build",
        headline:
          "La mise à jour intégrée n'est pas disponible pour un build de développement.",
        guidance:
          "Reconstruis Rustory depuis les sources pour obtenir la dernière version.",
      }),
    ).toBe(true);
  });

  it("rejects an unknown mode, an unknown reason and non-object payloads", () => {
    expect(isUpdateApplyPlan({ ...integratedPlan(), mode: "auto" })).toBe(false);
    expect(isUpdateApplyPlan({ ...manualPlan(), reason: "because" })).toBe(
      false,
    );
    expect(isUpdateApplyPlan(null)).toBe(false);
    expect(isUpdateApplyPlan("manual")).toBe(false);
  });

  it("rejects a reason on the integrated plan and a missing reason on a manual one", () => {
    // `reason` is present IFF manual — both drifts fail closed.
    expect(
      isUpdateApplyPlan({ ...integratedPlan(), reason: "channel_unproven" }),
    ).toBe(false);
    const missing: Record<string, unknown> = manualPlan();
    delete missing.reason;
    expect(isUpdateApplyPlan(missing)).toBe(false);
  });

  it("rejects the integrated literal smuggled as a manual reason", () => {
    expect(isUpdateApplyPlan({ ...manualPlan(), reason: "integrated" })).toBe(
      false,
    );
  });

  it("rejects a drifted copy on either branch", () => {
    expect(
      isUpdateApplyPlan({ ...integratedPlan(), headline: "Mets à jour !" }),
    ).toBe(false);
    expect(
      isUpdateApplyPlan({
        ...manualPlan(),
        guidance: "Va sur le site officiel.",
      }),
    ).toBe(false);
    // A couple swapped between reasons is a drift too.
    expect(
      isUpdateApplyPlan({
        ...manualPlan(),
        reason: "unofficial_install",
      }),
    ).toBe(false);
  });
});

describe("isUpdateApplyState", () => {
  const runningState = () => ({
    status: "running",
    jobId: "j1",
    phase: "downloading",
    percent: 42,
    headline: "Téléchargement de la mise à jour en cours…",
    notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
  });

  const failedState = () => ({
    status: "failed",
    stage: "verification",
    headline: "L'authenticité de la mise à jour n'a pas pu être confirmée.",
    notice:
      "Rien n'a été installé : Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  });

  it("accepts the four exact states Rust serializes", () => {
    expect(isUpdateApplyState({ status: "idle" })).toBe(true);
    expect(isUpdateApplyState(runningState())).toBe(true);
    expect(
      isUpdateApplyState({
        status: "running",
        jobId: "j2",
        phase: "checking",
        headline: "Vérification de la mise à jour en cours…",
        notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
      }),
    ).toBe(true);
    expect(
      isUpdateApplyState({
        status: "readyToRestart",
        headline: "La mise à jour de Rustory est prête.",
        notice:
          "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
      }),
    ).toBe(true);
    expect(isUpdateApplyState(failedState())).toBe(true);
  });

  it("rejects an unknown status and non-object payloads", () => {
    expect(isUpdateApplyState({ status: "paused" })).toBe(false);
    expect(isUpdateApplyState(null)).toBe(false);
    expect(isUpdateApplyState(undefined)).toBe(false);
  });

  it("rejects stray keys on the idle state", () => {
    expect(isUpdateApplyState({ status: "idle", percent: 3 })).toBe(false);
    expect(
      isUpdateApplyState({ status: "idle", headline: "…" }),
    ).toBe(false);
  });

  it("rejects a non-integer or out-of-range percent", () => {
    expect(isUpdateApplyState({ ...runningState(), percent: 41.5 })).toBe(
      false,
    );
    expect(isUpdateApplyState({ ...runningState(), percent: 101 })).toBe(
      false,
    );
    expect(isUpdateApplyState({ ...runningState(), percent: -1 })).toBe(false);
  });

  it("accepts a running state without percent (unknown fraction)", () => {
    const payload: Record<string, unknown> = runningState();
    delete payload.percent;
    expect(isUpdateApplyState(payload)).toBe(true);
  });

  it("rejects a running headline drifted from its phase couple", () => {
    expect(
      isUpdateApplyState({
        ...runningState(),
        headline: "Vérification de la mise à jour en cours…",
      }),
    ).toBe(false);
  });

  it("rejects a stage outside the failed state and a phase on a failed one", () => {
    expect(
      isUpdateApplyState({ ...runningState(), stage: "download" }),
    ).toBe(false);
    expect(
      isUpdateApplyState({ ...failedState(), phase: "downloading" }),
    ).toBe(false);
  });

  it("rejects a failed couple drifted from its stage", () => {
    expect(
      isUpdateApplyState({
        ...failedState(),
        stage: "download",
      }),
    ).toBe(false);
    expect(
      isUpdateApplyState({
        ...failedState(),
        notice: "Réessaie.",
      }),
    ).toBe(false);
  });

  it("requires the correlation id on a running state — a live flight is always re-attachable", () => {
    const payload: Record<string, unknown> = runningState();
    delete payload.jobId;
    expect(isUpdateApplyState(payload)).toBe(false);
    expect(isUpdateApplyState({ ...runningState(), jobId: "" })).toBe(false);
  });

  it("rejects a correlation id outside the running state", () => {
    expect(isUpdateApplyState({ status: "idle", jobId: "j1" })).toBe(false);
    expect(
      isUpdateApplyState({
        status: "readyToRestart",
        jobId: "j1",
        headline: "La mise à jour de Rustory est prête.",
        notice:
          "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
      }),
    ).toBe(false);
    expect(isUpdateApplyState({ ...failedState(), jobId: "j1" })).toBe(false);
  });
});

describe("isStartUpdateApplyOutcome", () => {
  it("accepts the three exact outcomes Rust serializes", () => {
    expect(
      isStartUpdateApplyOutcome({ outcome: "started", jobId: "j1" }),
    ).toBe(true);
    expect(isStartUpdateApplyOutcome({ outcome: "alreadyRunning" })).toBe(
      true,
    );
    expect(isStartUpdateApplyOutcome({ outcome: "notEligible" })).toBe(true);
  });

  it("rejects a started outcome without a job id and refusals carrying one", () => {
    expect(isStartUpdateApplyOutcome({ outcome: "started" })).toBe(false);
    expect(isStartUpdateApplyOutcome({ outcome: "started", jobId: "" })).toBe(
      false,
    );
    expect(
      isStartUpdateApplyOutcome({ outcome: "alreadyRunning", jobId: "j1" }),
    ).toBe(false);
    expect(
      isStartUpdateApplyOutcome({ outcome: "notEligible", jobId: "j1" }),
    ).toBe(false);
  });

  it("rejects an unknown outcome and non-object payloads", () => {
    expect(isStartUpdateApplyOutcome({ outcome: "queued" })).toBe(false);
    expect(isStartUpdateApplyOutcome(null)).toBe(false);
  });
});

describe("update apply event guards", () => {
  it("accepts the exact progress payloads with and without percent", () => {
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "downloading",
        percent: 7,
        sequence: 3,
      }),
    ).toBe(true);
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "checking",
        sequence: 0,
      }),
    ).toBe(true);
  });

  it("rejects a rotten progress payload", () => {
    expect(
      isUpdateApplyProgressEvent({
        jobId: "",
        phase: "downloading",
        sequence: 1,
      }),
    ).toBe(false);
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "uploading",
        sequence: 1,
      }),
    ).toBe(false);
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "downloading",
        percent: 12.5,
        sequence: 1,
      }),
    ).toBe(false);
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "downloading",
        sequence: -1,
      }),
    ).toBe(false);
    expect(
      isUpdateApplyProgressEvent({
        jobId: "j1",
        phase: "downloading",
        sequence: 1.5,
      }),
    ).toBe(false);
  });

  it("accepts and rejects completed payloads", () => {
    expect(isUpdateApplyCompletedEvent({ jobId: "j1", sequence: 5 })).toBe(
      true,
    );
    expect(isUpdateApplyCompletedEvent({ jobId: "", sequence: 5 })).toBe(
      false,
    );
    expect(isUpdateApplyCompletedEvent({ jobId: "j1" })).toBe(false);
  });

  it("accepts a failed payload locked on its stage couple and rejects drifts", () => {
    const failed = {
      jobId: "j1",
      sequence: 4,
      stage: "download",
      headline: "Le téléchargement de la mise à jour n'a pas abouti.",
      notice:
        "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
    };
    expect(isUpdateApplyFailedEvent(failed)).toBe(true);
    expect(isUpdateApplyFailedEvent({ ...failed, stage: "feed" })).toBe(false);
    expect(
      isUpdateApplyFailedEvent({ ...failed, headline: "Erreur réseau." }),
    ).toBe(false);
    expect(isUpdateApplyFailedEvent({ ...failed, stage: "panic" })).toBe(
      false,
    );
  });
});

describe("update apply guards against prototype-inherited keys", () => {
  it("rejects inherited keys smuggled as a manual reason — never a crash", () => {
    for (const hostile of ["constructor", "toString", "__proto__", "hasOwnProperty"]) {
      expect(
        isUpdateApplyPlan({
          mode: "manual",
          reason: hostile,
          headline: "x",
          guidance: "y",
        }),
      ).toBe(false);
    }
  });

  it("rejects inherited keys smuggled as a phase or a stage — never a crash", () => {
    for (const hostile of ["constructor", "toString", "__proto__"]) {
      expect(
        isUpdateApplyState({
          status: "running",
          jobId: "j1",
          phase: hostile,
          headline: "x",
          notice: "y",
        }),
      ).toBe(false);
      expect(
        isUpdateApplyState({
          status: "failed",
          stage: hostile,
          headline: "x",
          notice: "y",
        }),
      ).toBe(false);
      expect(
        isUpdateApplyProgressEvent({
          jobId: "j1",
          phase: hostile,
          sequence: 1,
        }),
      ).toBe(false);
      expect(
        isUpdateApplyFailedEvent({
          jobId: "j1",
          sequence: 1,
          stage: hostile,
          headline: "x",
          notice: "y",
        }),
      ).toBe(false);
    }
  });
});
