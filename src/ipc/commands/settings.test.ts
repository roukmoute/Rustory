import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  SupportProfileContractDriftError,
  readSupportProfile,
} from "./settings";

/** Compact builder of the EXACT official payload Rust serializes (the
 *  byte-for-byte fixture lives in the contract-guard tests; this one
 *  only exercises the facade). */
function officialProfile() {
  const cap = (
    operation: string,
    label: string,
    available: boolean,
    reason?: string,
  ) => ({ operation, label, available, ...(reason ? { reason } : {}) });
  const readCaps = [
    cap("readLibrary", "Lecture bibliothèque appareil", true),
    cap("inspectStory", "Inspection d'histoire", true),
    cap("importStory", "Copie dans la bibliothèque locale", true),
  ];
  return {
    devices: [
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "origineV1",
        cohortLabel: "Origine v1",
        metadataFormatLabel: "Format métadonnées v3",
        capabilities: [
          ...readCaps,
          cap("writeStory", "Transfert vers la Lunii", true),
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "midGenV2",
        cohortLabel: "Mid-Gen v2",
        metadataFormatLabel: "Format métadonnées v6",
        capabilities: [
          ...readCaps,
          cap("writeStory", "Transfert vers la Lunii", true),
        ],
      },
      {
        family: "lunii",
        familyLabel: "Lunii",
        cohort: "v3",
        cohortLabel: "V3",
        metadataFormatLabel: "Format métadonnées v7",
        capabilities: [
          cap("readLibrary", "Lecture bibliothèque appareil", true),
          cap("inspectStory", "Inspection d'histoire", true),
          cap(
            "importStory",
            "Copie dans la bibliothèque locale",
            false,
            "Rétro-ingénierie du format en cours",
          ),
          cap(
            "writeStory",
            "Transfert vers la Lunii",
            false,
            "Rétro-ingénierie du format en cours",
          ),
        ],
      },
      {
        family: "flam",
        familyLabel: "FLAM",
        cohort: "flamGen1",
        cohortLabel: "Gen1",
        capabilities: [
          ...readCaps,
          cap(
            "writeStory",
            "Transfert vers l'appareil",
            false,
            "Écriture non prouvée sur matériel réel",
          ),
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

describe("readSupportProfile", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated profile from the pure read", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(officialProfile());
    const profile = await readSupportProfile();
    expect(invoke).toHaveBeenCalledWith("read_support_profile");
    expect(profile.devices).toHaveLength(4);
    expect(profile.devices[3].cohortLabel).toBe("Gen1");
    expect(profile.devices[3].capabilities[3].reason).toBe(
      "Écriture non prouvée sur matériel réel",
    );
    expect(profile.localArtifacts).toHaveLength(3);
    expect(profile.localArtifacts[2].reason).toBe(
      "Lecture d'archives non prise en charge",
    );
  });

  it("rejects with SupportProfileContractDriftError on a drifted payload", async () => {
    const raw = { devices: [{ cohort: "v4" }], localArtifacts: [] };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await readSupportProfile().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as SupportProfileContractDriftError;
    expect(err).toBeInstanceOf(SupportProfileContractDriftError);
    expect(err.raw).toBe(raw);
  });

  it("normalizes an IPC rejection into an AppError (fail-closed upstream)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc down"));
    const err = (await readSupportProfile().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("UNKNOWN");
  });
});
