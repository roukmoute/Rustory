import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  SupportProfileContractDriftError,
  UpdateApplyContractDriftError,
  UpdateAvailabilityContractDriftError,
  readSupportProfile,
  readUpdateApplyPlan,
  readUpdateApplyState,
  readUpdateAvailability,
  restartForUpdate,
  startUpdateApply,
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
    fileAssociation: {
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
    },
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
    expect(profile.fileAssociation.channels).toHaveLength(4);
    expect(profile.fileAssociation.channels[1].reason).toBe(
      "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
    );
    expect(profile.fileAssociation.currentInstall).toBeUndefined();
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

describe("readUpdateAvailability", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated verdict from the infallible read", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      status: "updateAvailable",
      headline: "Nouvelle version disponible : 9.9.9.",
      notice:
        "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
      currentVersion: "0.1.0",
      latestVersion: "9.9.9",
    });
    const verdict = await readUpdateAvailability();
    expect(invoke).toHaveBeenCalledWith("read_update_availability");
    expect(verdict.status).toBe("updateAvailable");
    expect(verdict.latestVersion).toBe("9.9.9");
  });

  it("resolves the calm transport state as a STATE, never a rejection", async () => {
    // The command is infallible: an offline launch answers
    // `checkUnavailable`, the facade resolves it like any verdict.
    vi.mocked(invoke).mockResolvedValueOnce({
      status: "checkUnavailable",
      headline: "La vérification de version n'a pas pu être faite.",
      notice:
        "Rustory reste pleinement utilisable. La vérification réessaiera au prochain lancement.",
      currentVersion: "0.1.0",
    });
    const verdict = await readUpdateAvailability();
    expect(verdict.status).toBe("checkUnavailable");
    expect(verdict.latestVersion).toBeUndefined();
  });

  it("rejects with UpdateAvailabilityContractDriftError on a drifted payload", async () => {
    const raw = { status: "updateAvailable", headline: "Mets à jour !" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await readUpdateAvailability().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as UpdateAvailabilityContractDriftError;
    expect(err).toBeInstanceOf(UpdateAvailabilityContractDriftError);
    expect(err.raw).toBe(raw);
  });

  it("normalizes an IPC rejection into an AppError (fail-closed upstream)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc down"));
    const err = (await readUpdateAvailability().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("UNKNOWN");
  });
});

describe("readUpdateApplyPlan", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated integrated plan", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      mode: "integrated",
      headline: "Cette copie peut installer les mises à jour de Rustory.",
      guidance:
        "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
    });
    const plan = await readUpdateApplyPlan();
    expect(invoke).toHaveBeenCalledWith("read_update_apply_plan");
    expect(plan.mode).toBe("integrated");
    expect(plan.reason).toBeUndefined();
  });

  it("resolves a manual plan as a STATE, never a rejection", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      mode: "manual",
      reason: "package_manager_owned",
      headline:
        "La mise à jour de Rustory passe par ton gestionnaire de paquets.",
      guidance:
        "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil de ton système, puis relance Rustory.",
    });
    const plan = await readUpdateApplyPlan();
    expect(plan.mode).toBe("manual");
    expect(plan.reason).toBe("package_manager_owned");
  });

  it("rejects with UpdateApplyContractDriftError on a drifted payload", async () => {
    const raw = { mode: "manual", reason: "because" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await readUpdateApplyPlan().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as UpdateApplyContractDriftError;
    expect(err).toBeInstanceOf(UpdateApplyContractDriftError);
    expect(err.raw).toBe(raw);
  });

  it("normalizes an IPC rejection into an AppError (fail-closed upstream)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc down"));
    const err = (await readUpdateApplyPlan().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("UNKNOWN");
  });
});

describe("readUpdateApplyState", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the validated session state", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ status: "idle" });
    const state = await readUpdateApplyState();
    expect(invoke).toHaveBeenCalledWith("read_update_apply_state");
    expect(state.status).toBe("idle");
  });

  it("resolves a running state with its phase couple", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      status: "running",
      jobId: "j1",
      phase: "downloading",
      percent: 12,
      headline: "Téléchargement de la mise à jour en cours…",
      notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
    });
    const state = await readUpdateApplyState();
    expect(state.status).toBe("running");
    expect(state.percent).toBe(12);
  });

  it("rejects with UpdateApplyContractDriftError on a drifted payload", async () => {
    const raw = { status: "running", phase: "uploading" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await readUpdateApplyState().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as UpdateApplyContractDriftError;
    expect(err).toBeInstanceOf(UpdateApplyContractDriftError);
    expect(err.raw).toBe(raw);
  });
});

describe("startUpdateApply", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves an accepted start with its job id", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      outcome: "started",
      jobId: "j1",
    });
    const outcome = await startUpdateApply();
    expect(invoke).toHaveBeenCalledWith("start_update_apply");
    expect(outcome).toEqual({ outcome: "started", jobId: "j1" });
  });

  it("resolves the two refusals as STATES, never rejections", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "alreadyRunning" });
    expect((await startUpdateApply()).outcome).toBe("alreadyRunning");
    vi.mocked(invoke).mockResolvedValueOnce({ outcome: "notEligible" });
    expect((await startUpdateApply()).outcome).toBe("notEligible");
  });

  it("rejects with UpdateApplyContractDriftError on a drifted payload", async () => {
    const raw = { outcome: "started" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await startUpdateApply().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as UpdateApplyContractDriftError;
    expect(err).toBeInstanceOf(UpdateApplyContractDriftError);
    expect(err.raw).toBe(raw);
  });
});

describe("restartForUpdate", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes the guarded restart command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await restartForUpdate();
    expect(invoke).toHaveBeenCalledWith("restart_for_update");
  });

  it("normalizes an IPC rejection into an AppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc down"));
    const err = (await restartForUpdate().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("UNKNOWN");
  });
});
