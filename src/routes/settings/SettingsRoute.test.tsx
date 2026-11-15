import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  RouterProvider,
  createMemoryRouter,
  type RouteObject,
} from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockReadSupportProfile = vi.fn();
const mockReadContentSourcePolicy = vi.fn();
const mockGetVersion = vi.fn();
const mockReadUpdateApplyPlan = vi.fn();
const mockReadUpdateApplyState = vi.fn();
const mockRefreshUpdateAvailability = vi.fn();
const mockSubscribeUpdateApplyEvents = vi.fn();
const mockCatalogStatus = vi.fn();
const mockCatalogRefresh = vi.fn();
const mockCatalogImport = vi.fn();
const mockInvalidatePackCoverCache = vi.fn();
const mockInvalidateDeviceLibraryCache = vi.fn();

vi.mock("../../ipc/commands/device-catalog", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/device-catalog")
  >("../../ipc/commands/device-catalog");
  return {
    ...actual,
    getOfficialCatalogStatus: () => mockCatalogStatus(),
    refreshOfficialCatalog: () => mockCatalogRefresh(),
    importOfficialCatalog: () => mockCatalogImport(),
  };
});

vi.mock("../../features/device/hooks/use-pack-cover", async () => {
  const actual = await vi.importActual<
    typeof import("../../features/device/hooks/use-pack-cover")
  >("../../features/device/hooks/use-pack-cover");
  return {
    ...actual,
    invalidatePackCoverCache: () => {
      mockInvalidatePackCoverCache();
      actual.invalidatePackCoverCache();
    },
  };
});

vi.mock("../../features/device/hooks/use-device-library", async () => {
  const actual = await vi.importActual<
    typeof import("../../features/device/hooks/use-device-library")
  >("../../features/device/hooks/use-device-library");
  return {
    ...actual,
    invalidateDeviceLibraryCache: () => {
      mockInvalidateDeviceLibraryCache();
      actual.invalidateDeviceLibraryCache();
    },
  };
});

vi.mock("../../ipc/commands/settings", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/settings")
  >("../../ipc/commands/settings");
  return {
    ...actual,
    readSupportProfile: () => mockReadSupportProfile(),
    readUpdateApplyPlan: () => mockReadUpdateApplyPlan(),
    readUpdateApplyState: () => mockReadUpdateApplyState(),
    refreshUpdateAvailability: () => mockRefreshUpdateAvailability(),
  };
});

vi.mock("../../ipc/events/update-apply-events", () => ({
  subscribeUpdateApplyEvents: (...args: unknown[]) => mockSubscribeUpdateApplyEvents(...args),
}));

vi.mock("../../ipc/commands/import-export", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/import-export")
  >("../../ipc/commands/import-export");
  return {
    ...actual,
    readContentSourcePolicy: () => mockReadContentSourcePolicy(),
  };
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => mockGetVersion(),
}));

// The announcement-voice section has its own read and its own tests
// (`AnnouncementVoiceSettings.test.tsx`); here it settles on one voice so
// it never adds a `status` line to the profile sections under test.
vi.mock("../../ipc/commands/presentation", () => ({
  readAnnouncementVoices: () =>
    Promise.resolve({
      voices: [
        {
          id: "system:say:Thomas",
          name: "Thomas",
          language: "fr-FR",
          engine: "system",
        },
      ],
      selectedVoiceId: "system:say:Thomas",
      selectedIsStored: false,
      embedded: {
        state: "installed",
        version: "2023.11.14-2",
        downloadBytes: 89667631,
        voiceId: "embedded:fr_FR-siwis-medium",
        voiceName: "Voix neuronale française (Siwis)",
      },
    }),
  setAnnouncementVoice: vi.fn(),
  previewAnnouncementVoice: vi.fn(),
  installEmbeddedVoice: vi.fn(),
}));

import { SettingsRoute } from "./SettingsRoute";
import { useUpdateApplyShell } from "../../shell/state/update-apply-shell-store";
import { useUpdateShell } from "../../shell/state/update-shell-store";
import type { UpdateApplyState, UpdateAvailability } from "../../shared/ipc-contracts/settings";

/** Compact builder of the official profile payload (byte-for-byte
 *  literals are guarded by the contract tests; the route tests only
 *  need a valid shape). */
function officialProfile() {
  const cap = (
    operation: string,
    label: string,
    available: boolean,
    reason?: string,
  ) => ({ operation, label, available, ...(reason ? { reason } : {}) });
  const readCaps = (importAvailable = true) => [
    cap("readLibrary", "Lecture bibliothèque appareil", true),
    cap("inspectStory", "Inspection d'histoire", true),
    importAvailable
      ? cap("importStory", "Copie dans la bibliothèque locale", true)
      : cap(
          "importStory",
          "Copie dans la bibliothèque locale",
          false,
          "Rétro-ingénierie du format en cours",
        ),
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
          ...readCaps(),
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
          ...readCaps(),
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
          ...readCaps(false),
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
          ...readCaps(),
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
        formatLabel: "Format v1",
        available: true,
        capabilitiesLabel: "Création d'une histoire",
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

function officialPolicy() {
  return {
    sources: [
      {
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      },
      {
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
      {
        kind: "jsonFeed",
        label: "Flux JSON Feed",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
    ],
  };
}

const LIBRARY_MARKER = "library-stub";
const SUPPORT_DETAILS = "Compatibilité et informations";
const UPDATE_DETAILS = "Version et mises à jour";

function settingsDetails(name: string) {
  const summary = screen.getByText(name).closest("summary");
  expect(summary).not.toBeNull();
  const details = summary!.closest("details") as HTMLDetailsElement;
  expect(details).not.toBeNull();
  return { summary: summary!, details };
}

async function openSettingsDetails(name: string) {
  const { summary, details } = settingsDetails(name);
  if (!details.open) await userEvent.click(summary);
  expect(details).toHaveAttribute("open");
  return details;
}

function updateAvailableVerdict(): UpdateAvailability {
  return {
    status: "updateAvailable",
    headline: "Nouvelle version disponible : 9.9.9.",
    notice: "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
    currentVersion: "0.1.0",
    latestVersion: "9.9.9",
  };
}

const RUNNING_UPDATE: UpdateApplyState = {
  status: "running",
  jobId: "update-job-1",
  phase: "downloading",
  percent: 24,
  headline: "Téléchargement de la mise à jour en cours…",
  notice: "Tu peux continuer à utiliser Rustory pendant cette opération.",
};

function renderSettings(options: { strict?: boolean } = {}) {
  const routes: RouteObject[] = [
    { path: "/settings", element: <SettingsRoute /> },
    { path: "/library", element: <div data-testid={LIBRARY_MARKER} /> },
  ];
  const router = createMemoryRouter(routes, { initialEntries: ["/settings"] });
  const tree = <RouterProvider router={router} />;
  render(options.strict ? <StrictMode>{tree}</StrictMode> : tree);
  return router;
}

describe("<SettingsRoute />", () => {
  beforeEach(() => {
    mockCatalogStatus.mockReset();
    mockCatalogStatus.mockResolvedValue({ count: 17 });
    mockCatalogRefresh.mockReset();
    mockCatalogRefresh.mockResolvedValue({ count: 23 });
    mockCatalogImport.mockReset();
    mockCatalogImport.mockResolvedValue({ kind: "cancelled" });
    mockInvalidatePackCoverCache.mockReset();
    mockInvalidateDeviceLibraryCache.mockReset();
    mockReadSupportProfile.mockReset();
    mockReadSupportProfile.mockResolvedValue(officialProfile());
    mockReadContentSourcePolicy.mockReset();
    mockReadContentSourcePolicy.mockResolvedValue(officialPolicy());
    mockGetVersion.mockReset();
    mockGetVersion.mockResolvedValue("0.1.0");
    mockReadUpdateApplyPlan.mockReset();
    mockReadUpdateApplyPlan.mockResolvedValue({
      mode: "integrated",
      headline: "Cette copie peut installer les mises à jour de Rustory.",
      guidance:
        "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
    });
    mockReadUpdateApplyState.mockReset();
    mockReadUpdateApplyState.mockResolvedValue({ status: "idle" });
    mockRefreshUpdateAvailability.mockReset();
    mockRefreshUpdateAvailability.mockResolvedValue({
      status: "upToDate",
      headline: "Aucune version plus récente n'est publiée.",
      notice: "Aucune action n'est nécessaire.",
      currentVersion: "0.1.0",
    });
    mockSubscribeUpdateApplyEvents.mockReset();
    mockSubscribeUpdateApplyEvents.mockImplementation(() => ({
      ready: Promise.resolve(),
      unsubscribe: vi.fn(),
    }));
    useUpdateShell.setState({ availability: null });
    useUpdateApplyShell.setState({
      plan: null,
      state: null,
      jobId: null,
      restartInviteFolded: false,
    });
  });

  it("shows voices, catalogue and help while keeping informational sections folded by default", async () => {
    renderSettings();
    expect(
      screen.getByRole("main", { name: "Aide et réglages" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 1, name: "Aide et réglages" }),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeVisible();
    });
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(settingsDetails(SUPPORT_DETAILS).details).not.toHaveAttribute("open");
    expect(settingsDetails(UPDATE_DETAILS).details).not.toHaveAttribute("open");
    expect(screen.getByText("Origine v1")).not.toBeVisible();
    expect(screen.getByRole("heading", { name: "Voix des annonces" })).toBeVisible();
    const catalog = screen.getByRole("region", { name: "Catalogue officiel" });
    expect(
      await within(catalog).findByText("17 titres dans le catalogue."),
    ).toBeInTheDocument();
    expect(
      catalog.compareDocumentPosition(
        screen.getByRole("button", { name: "Revoir l'accueil" }),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(mockCatalogStatus).toHaveBeenCalledOnce();
    expect(mockCatalogRefresh).not.toHaveBeenCalled();
    expect(mockCatalogImport).not.toHaveBeenCalled();
    expect(mockInvalidatePackCoverCache).not.toHaveBeenCalled();
    expect(mockInvalidateDeviceLibraryCache).not.toHaveBeenCalled();
    expect(mockRefreshUpdateAvailability).not.toHaveBeenCalled();
    await openSettingsDetails(SUPPORT_DETAILS);
    expect(screen.getByRole("region", { name: "Profil de support" })).toBeVisible();
    expect(screen.getByText("Gen1")).toBeVisible();
    expect(screen.getByText("Dossier structuré")).toBeInTheDocument();
    expect(screen.getByText("Paquet Linux (.deb / .rpm)")).toBeInTheDocument();
    expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    expect(
      screen.getByText(/La distribution officielle autorise/),
    ).toBeInTheDocument();
  });

  it.each(["refresh", "import"] as const)(
    "holds Retour during catalogue %s and invalidates both caches only after success",
    async (action) => {
      const user = userEvent.setup();
      let complete!: (
        outcome: { count: number } | { kind: "imported"; count: number },
      ) => void;
      const operation =
        action === "refresh" ? mockCatalogRefresh : mockCatalogImport;
      operation.mockImplementation(
        () =>
          new Promise((resolve) => {
            complete = resolve;
          }),
      );
      const router = renderSettings();
      const catalog = screen.getByRole("region", {
        name: "Catalogue officiel",
      });
      await within(catalog).findByText("17 titres dans le catalogue.");
      const back = screen.getByRole("button", {
        name: "Retour à la bibliothèque",
      });
      expect(back).toBeEnabled();
      await user.click(
        within(catalog).getByRole("button", {
          name:
            action === "refresh"
              ? "Récupérer / mettre à jour"
              : "Importer depuis un fichier",
        }),
      );
      expect(operation).toHaveBeenCalledOnce();
      expect(back).toBeDisabled();
      await user.click(back);
      expect(router.state.location.pathname).toBe("/settings");
      expect(mockInvalidatePackCoverCache).not.toHaveBeenCalled();
      expect(mockInvalidateDeviceLibraryCache).not.toHaveBeenCalled();

      await act(async () =>
        complete(
          action === "refresh"
            ? { count: 23 }
            : { kind: "imported", count: 23 },
        ),
      );
      expect(
        within(catalog).getByText("23 titres dans le catalogue."),
      ).toBeInTheDocument();
      expect(mockInvalidatePackCoverCache).toHaveBeenCalledOnce();
      expect(mockInvalidateDeviceLibraryCache).toHaveBeenCalledOnce();
      expect(back).toBeEnabled();
      await user.click(back);
      expect(screen.getByTestId(LIBRARY_MARKER)).toBeInTheDocument();
      expect(operation).toHaveBeenCalledOnce();
    },
  );

  it("leaves both caches and the cached count intact when catalogue file selection is cancelled", async () => {
    const user = userEvent.setup();
    renderSettings();
    const catalog = screen.getByRole("region", { name: "Catalogue officiel" });
    await within(catalog).findByText("17 titres dans le catalogue.");
    await user.click(
      within(catalog).getByRole("button", {
        name: "Importer depuis un fichier",
      }),
    );
    expect(mockCatalogImport).toHaveBeenCalledOnce();
    expect(mockCatalogRefresh).not.toHaveBeenCalled();
    expect(
      within(catalog).getByText("17 titres dans le catalogue."),
    ).toBeInTheDocument();
    expect(mockInvalidatePackCoverCache).not.toHaveBeenCalled();
    expect(mockInvalidateDeviceLibraryCache).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Retour à la bibliothèque" }),
    ).toBeEnabled();
  });

  it("keeps the cached inventory valid and restores Retour after a catalogue refresh fails", async () => {
    const user = userEvent.setup();
    mockCatalogRefresh.mockRejectedValue({
      code: "OFFICIAL_CATALOG_UNAVAILABLE",
      message: "Le catalogue officiel est indisponible.",
      userAction: "Réessaie plus tard.",
      details: null,
    });
    renderSettings();
    const catalog = screen.getByRole("region", { name: "Catalogue officiel" });
    await within(catalog).findByText("17 titres dans le catalogue.");
    await user.click(
      within(catalog).getByRole("button", {
        name: "Récupérer / mettre à jour",
      }),
    );
    expect(await within(catalog).findByRole("alert")).toHaveTextContent(
      "Le catalogue officiel est indisponible.",
    );
    expect(
      within(catalog).getByText("17 titres dans le catalogue."),
    ).toBeInTheDocument();
    expect(mockInvalidatePackCoverCache).not.toHaveBeenCalled();
    expect(mockInvalidateDeviceLibraryCache).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Retour à la bibliothèque" }),
    ).toBeEnabled();
    await openSettingsDetails(SUPPORT_DETAILS);
    expect(screen.getByText("Origine v1")).toBeVisible();
  });

  it.each(["pending", "failed"] as const)(
    "keeps support, sources and navigation available when the catalogue read is %s",
    async (catalogState) => {
      const user = userEvent.setup();
      if (catalogState === "pending")
        mockCatalogStatus.mockImplementation(() => new Promise(() => {}));
      else
        mockCatalogStatus.mockRejectedValue(
          new Error("Catalogue local illisible"),
        );
      const router = renderSettings();
      await screen.findByText("Origine v1");
      await openSettingsDetails(SUPPORT_DETAILS);
      expect(screen.getByText("Flux RSS")).toBeVisible();
      expect(
        screen.getByRole("heading", { name: "Voix des annonces" }),
      ).toBeInTheDocument();
      const catalog = screen.getByRole("region", {
        name: "Catalogue officiel",
      });
      expect(
        within(catalog).getByText(
          catalogState === "pending"
            ? "Lecture du catalogue…"
            : "Le catalogue n'a pas pu être lu.",
        ),
      ).toBeInTheDocument();
      expect(mockCatalogRefresh).not.toHaveBeenCalled();
      expect(mockCatalogImport).not.toHaveBeenCalled();
      expect(mockInvalidatePackCoverCache).not.toHaveBeenCalled();
      expect(mockInvalidateDeviceLibraryCache).not.toHaveBeenCalled();
      const back = screen.getByRole("button", {
        name: "Retour à la bibliothèque",
      });
      expect(back).toBeEnabled();
      await user.click(back);
      expect(router.state.location.pathname).toBe("/library");
    },
  );

  it("marks the screen busy while the reads are pending, then clears it", async () => {
    let resolveProfile: (value: unknown) => void = () => {};
    mockReadSupportProfile.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveProfile = resolve;
        }),
    );
    renderSettings();
    expect(
      screen.getByRole("main", { name: "Aide et réglages" }),
    ).toHaveAttribute("aria-busy", "true");
    resolveProfile(officialProfile());
    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Aide et réglages" }),
      ).toHaveAttribute("aria-busy", "false");
    });
  });

  it("navigates back to the library through the Retour button", async () => {
    const user = userEvent.setup();
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    await user.click(
      screen.getByRole("button", { name: "Retour à la bibliothèque" }),
    );
    await waitFor(() => {
      expect(screen.getByTestId(LIBRARY_MARKER)).toBeInTheDocument();
    });
  });

  it("replaces the history entry on Retour — the back button never bounces to the profile just left", async () => {
    // Library → settings → Retour: the round-trip must stay a single
    // in/out transition (the StoryEditRoute pattern). Going BACK after
    // the Retour lands before the library visit, never on /settings.
    const user = userEvent.setup();
    const routes: RouteObject[] = [
      { path: "/settings", element: <SettingsRoute /> },
      { path: "/library", element: <div data-testid={LIBRARY_MARKER} /> },
      { path: "/elsewhere", element: <div data-testid="elsewhere-stub" /> },
    ];
    const router = createMemoryRouter(routes, {
      initialEntries: ["/elsewhere", "/library", "/settings"],
      initialIndex: 2,
    });
    render(<RouterProvider router={router} />);
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    await user.click(
      screen.getByRole("button", { name: "Retour à la bibliothèque" }),
    );
    await waitFor(() => {
      expect(screen.getByTestId(LIBRARY_MARKER)).toBeInTheDocument();
    });
    // The Retour REPLACED the /settings entry: back skips the profile.
    await act(async () => router.navigate(-1));
    await waitFor(() => {
      expect(screen.getByTestId(LIBRARY_MARKER)).toBeInTheDocument();
    });
    await act(async () => router.navigate(-1));
    await waitFor(() => {
      expect(screen.getByTestId("elsewhere-stub")).toBeInTheDocument();
    });
    expect(screen.queryByText("Profil de support")).not.toBeInTheDocument();
  });

  it("fails closed per section when the profile read rejects: sources stay served", async () => {
    mockReadSupportProfile.mockRejectedValue(new Error("drift"));
    renderSettings();
    expect(await screen.findByText("Certaines informations ne sont pas disponibles.")).toBeVisible();
    expect(settingsDetails(SUPPORT_DETAILS).details).not.toHaveAttribute("open");
    await openSettingsDetails(SUPPORT_DETAILS);
    const profile = screen.getByRole("region", { name: "Profil de support" });
    await waitFor(() => {
      expect(within(profile).getAllByRole("status")).toHaveLength(3);
    });
    for (const status of within(profile).getAllByRole("status")) {
      expect(status).toHaveTextContent(
        "Le profil de support n'a pas pu être lu.",
      );
    }
    // The sources section still renders from ITS successful read.
    await waitFor(() => {
      expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    });
    expect(
      screen.queryByRole("button", { name: /réessayer/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("fails closed per section when the policy read rejects: the profile sections stay served", async () => {
    mockReadContentSourcePolicy.mockRejectedValue(new Error("drift"));
    renderSettings();
    expect(await screen.findByText("Certaines informations ne sont pas disponibles.")).toBeVisible();
    expect(settingsDetails(SUPPORT_DETAILS).details).not.toHaveAttribute("open");
    await openSettingsDetails(SUPPORT_DETAILS);
    const profile = screen.getByRole("region", { name: "Profil de support" });
    await waitFor(() => {
      expect(within(profile).getByRole("status")).toHaveTextContent(
        "Les sources de contenu n'ont pas pu être lues.",
      );
    });
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(screen.getByText("Archive structurée")).toBeInTheDocument();
  });

  it("omits the version line when the version read fails — never an invented value", async () => {
    mockGetVersion.mockRejectedValue(new Error("no runtime"));
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(screen.queryByText(/^Version \d/)).not.toBeInTheDocument();
    expect(settingsDetails(UPDATE_DETAILS).summary).toBeVisible();
  });

  it("renders correctly under StrictMode (mount token pins the reads to the live mount)", async () => {
    renderSettings({ strict: true });
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Voix des annonces" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Catalogue officiel" })).toBeVisible();
    expect(settingsDetails(SUPPORT_DETAILS).details).not.toHaveAttribute("open");
    await openSettingsDetails(SUPPORT_DETAILS);
    const profile = screen.getByRole("region", { name: "Profil de support" });
    expect(
      within(profile)
        .getAllByRole("heading", { level: 3 })
        .map((h) => h.textContent),
    ).toEqual([
      "Appareils",
      "Artefacts locaux",
      "Association de fichiers",
      "Sources de contenu",
      "Politique de distribution",
    ]);
  });

  it("keeps the support profile mounted while folding without re-reading its data", async () => {
    const user = userEvent.setup();
    renderSettings();
    const cohort = await screen.findByText("Origine v1");
    expect(cohort).not.toBeVisible();
    await openSettingsDetails(SUPPORT_DETAILS);
    const profile = screen.getByRole("region", { name: "Profil de support" });
    const { summary, details } = settingsDetails(SUPPORT_DETAILS);
    expect(profile).toBeVisible();

    await user.click(summary);
    expect(details).not.toHaveAttribute("open");
    expect(profile).toBeInTheDocument();
    expect(profile).not.toBeVisible();
    await user.click(summary);
    expect(screen.getByRole("region", { name: "Profil de support" })).toBe(profile);
    expect(screen.getByText("Origine v1")).toBe(cohort);
    expect(cohort).toBeVisible();
    expect(mockReadSupportProfile).toHaveBeenCalledOnce();
    expect(mockReadContentSourcePolicy).toHaveBeenCalledOnce();
    expect(mockGetVersion).toHaveBeenCalledOnce();
    expect(mockCatalogStatus).toHaveBeenCalledOnce();
    expect(mockCatalogRefresh).not.toHaveBeenCalled();
    expect(mockCatalogImport).not.toHaveBeenCalled();
  });

  it("preserves an in-flight version check while folded and opens on the available result", async () => {
    const user = userEvent.setup();
    let resolveCheck!: (value: UpdateAvailability) => void;
    mockRefreshUpdateAvailability.mockImplementation(() => new Promise((resolve) => {
      resolveCheck = resolve;
    }));
    renderSettings();
    await screen.findByText("Version 0.1.0");
    await openSettingsDetails(UPDATE_DETAILS);
    const { summary, details } = settingsDetails(UPDATE_DETAILS);
    const check = screen.getByRole("button", { name: "Rechercher une mise à jour maintenant" });
    await user.click(check);
    expect(check).toHaveAttribute("aria-busy", "true");
    expect(check).toHaveTextContent("Recherche en cours…");

    await user.click(summary);
    expect(details).not.toHaveAttribute("open");
    expect(check).toBeInTheDocument();
    expect(check).not.toBeVisible();
    await user.click(summary);
    expect(screen.getByRole("button", { name: "Rechercher une mise à jour maintenant" })).toBe(check);
    expect(check).toHaveAttribute("aria-busy", "true");
    expect(mockRefreshUpdateAvailability).toHaveBeenCalledOnce();
    await user.click(summary);

    await act(async () => resolveCheck(updateAvailableVerdict()));
    expect(details).toHaveAttribute("open");
    expect(screen.getByText("Nouvelle version disponible : 9.9.9.")).toBeVisible();
    expect(check).not.toHaveAttribute("aria-busy");
    expect(mockRefreshUpdateAvailability).toHaveBeenCalledOnce();
    expect(mockReadSupportProfile).toHaveBeenCalledOnce();
    expect(mockReadContentSourcePolicy).toHaveBeenCalledOnce();
  });

  it.each<UpdateApplyState>([
    RUNNING_UPDATE,
    {
      status: "readyToRestart",
      headline: "La mise à jour de Rustory est prête.",
      notice: "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
    },
    {
      status: "failed",
      stage: "download",
      headline: "Le téléchargement de la mise à jour n'a pas abouti.",
      notice: "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
    },
  ])("reopens a folded updates section when the apply state becomes $status", async (state) => {
    const user = userEvent.setup();
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    renderSettings();
    await screen.findByRole("button", { name: "Télécharger et installer la mise à jour de Rustory" });
    const { summary, details } = settingsDetails(UPDATE_DETAILS);
    await user.click(summary);
    expect(details).not.toHaveAttribute("open");

    act(() => useUpdateApplyShell.setState({ state }));

    expect(details).toHaveAttribute("open");
    expect(screen.getByText(state.headline!)).toBeVisible();
    if (state.status === "running") {
      expect(screen.getByRole("progressbar", { name: state.headline })).toHaveAttribute("aria-valuenow", "24");
    } else if (state.status === "readyToRestart") {
      expect(screen.getByRole("button", { name: "Redémarrer Rustory pour terminer la mise à jour" })).toBeVisible();
    } else {
      expect(screen.getByRole("button", { name: "Réessayer la mise à jour" })).toBeVisible();
    }
    expect(mockReadUpdateApplyPlan).toHaveBeenCalledOnce();
    expect(mockReadUpdateApplyState).toHaveBeenCalledOnce();
  });

  it("keeps a running update visible and its live subscription intact when folding is attempted", async () => {
    useUpdateShell.setState({ availability: updateAvailableVerdict() });
    mockReadUpdateApplyState.mockResolvedValue(RUNNING_UPDATE);
    renderSettings();
    const progress = await screen.findByRole("progressbar", { name: RUNNING_UPDATE.headline });
    await waitFor(() => expect(mockReadUpdateApplyState).toHaveBeenCalledTimes(2));
    expect(mockSubscribeUpdateApplyEvents).toHaveBeenCalledOnce();
    const subscription = mockSubscribeUpdateApplyEvents.mock.calls[0][0];
    const unsubscribe = mockSubscribeUpdateApplyEvents.mock.results[0].value.unsubscribe;
    const { summary, details } = settingsDetails(UPDATE_DETAILS);

    expect(summary).toHaveAttribute("aria-disabled", "true");
    // Happy DOM toggles details before React's delegated MouseEvent handler.
    // Verify cancellation with a bubbling click without that premature native action.
    const click = new Event("click", { bubbles: true, cancelable: true });
    fireEvent(summary, click);
    expect(click.defaultPrevented).toBe(true);
    expect(details).toHaveAttribute("open");
    expect(progress).toBeInTheDocument();
    expect(progress).toBeVisible();
    act(() => subscription.onProgress({
      jobId: "update-job-1",
      phase: "downloading",
      percent: 68,
      sequence: 1,
    }));
    expect(progress).toHaveAttribute("aria-valuenow", "68");
    expect(screen.getByRole("progressbar", { name: RUNNING_UPDATE.headline })).toBe(progress);
    expect(progress).toBeVisible();
    expect(unsubscribe).not.toHaveBeenCalled();
    expect(mockSubscribeUpdateApplyEvents).toHaveBeenCalledOnce();
    expect(mockReadUpdateApplyPlan).toHaveBeenCalledOnce();
    expect(mockReadUpdateApplyState).toHaveBeenCalledTimes(2);
  });

  it("automatically opens updates and shows the verdict below the actual installed version", async () => {
    useUpdateShell.setState({
      availability: {
        status: "updateAvailable",
        headline: "Nouvelle version disponible : 9.9.9.",
        notice:
          "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
        currentVersion: "0.1.0",
        latestVersion: "9.9.9",
      },
    });
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    });
    const statusLine = screen
      .getByText("Nouvelle version disponible : 9.9.9.")
      .closest("[role='status']");
    expect(statusLine).not.toBeNull();
    const updates = settingsDetails(UPDATE_DETAILS).details;
    expect(updates).toHaveAttribute("open");
    expect(updates).toContainElement(statusLine as HTMLElement);
    expect(statusLine).toBeVisible();
    expect(
      screen
        .getByText("Version 0.1.0")
        .compareDocumentPosition(statusLine as HTMLElement) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("renders no update status line while no verdict exists — never a waiting state", async () => {
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    });
    expect(
      document.querySelector(".update-status-line"),
    ).not.toBeInTheDocument();
    expect(settingsDetails(UPDATE_DETAILS).details).not.toHaveAttribute("open");
  });

  it("renders the update-apply zone UNDER the status line on a positive verdict", async () => {
    useUpdateShell.setState({
      availability: {
        status: "updateAvailable",
        headline: "Nouvelle version disponible : 9.9.9.",
        notice:
          "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
        currentVersion: "0.1.0",
        latestVersion: "9.9.9",
      },
    });
    renderSettings();
    // The zone read its plan and rendered the idle CTA.
    const startButton = await screen.findByRole("button", {
      name: "Télécharger et installer la mise à jour de Rustory",
    });
    // The status line and the version line stay UNCHANGED above it.
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    });
    const statusLine = screen
      .getByText("Nouvelle version disponible : 9.9.9.")
      .closest("[role='status']");
    expect(statusLine).not.toBeNull();
    expect(
      (statusLine as HTMLElement).compareDocumentPosition(startButton) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      screen.getByText("Version 0.1.0").compareDocumentPosition(startButton) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("renders no update-apply zone without a positive verdict", async () => {
    // No verdict at all…
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    });
    expect(document.querySelector(".update-apply-zone")).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "Télécharger et installer la mise à jour de Rustory",
      }),
    ).not.toBeInTheDocument();
    // …and the zone never even read its plan.
    expect(mockReadUpdateApplyPlan).not.toHaveBeenCalled();
    expect(mockReadUpdateApplyState).not.toHaveBeenCalled();
  });

  it("renders no update-apply zone on a non-positive verdict", async () => {
    useUpdateShell.setState({
      availability: {
        status: "upToDate",
        headline: "Aucune version plus récente n'est publiée.",
        notice: "Aucune action n'est nécessaire.",
        currentVersion: "0.1.0",
      },
    });
    renderSettings();
    await waitFor(() => {
      expect(
        screen.getByText("Aucune version plus récente n'est publiée."),
      ).toBeInTheDocument();
    });
    expect(settingsDetails(UPDATE_DETAILS).details).not.toHaveAttribute("open");
    await openSettingsDetails(UPDATE_DETAILS);
    expect(screen.getByText("Aucune version plus récente n'est publiée.")).toBeVisible();
    expect(document.querySelector(".update-apply-zone")).toBeNull();
    expect(mockReadUpdateApplyPlan).not.toHaveBeenCalled();
  });

  it("renders the calm checkNotRun verdict without any chip or alarm", async () => {
    useUpdateShell.setState({
      availability: {
        status: "checkNotRun",
        headline:
          "La vérification de version n'est pas exécutée pour cette copie.",
        notice:
          "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : aucune vérification réseau n'est effectuée.",
        currentVersion: "0.1.0",
      },
    });
    renderSettings();
    await waitFor(() => {
      expect(
        screen.getByText(
          "La vérification de version n'est pas exécutée pour cette copie.",
        ),
      ).toBeInTheDocument();
    });
    const line = document.querySelector(".update-status-line");
    expect(line).not.toBeNull();
    expect(settingsDetails(UPDATE_DETAILS).details).not.toHaveAttribute("open");
    await openSettingsDetails(UPDATE_DETAILS);
    expect(line).toBeVisible();
    expect(line?.querySelector(".ds-chip")).toBeNull();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
