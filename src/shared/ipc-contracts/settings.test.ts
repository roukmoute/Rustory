import { describe, expect, it } from "vitest";

import {
  isDeviceSupportLine,
  isLocalArtifactLine,
  isSupportProfile,
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
