import { StrictMode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
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

vi.mock("../../ipc/commands/settings", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/settings")
  >("../../ipc/commands/settings");
  return {
    ...actual,
    readSupportProfile: () => mockReadSupportProfile(),
    readUpdateApplyPlan: () => mockReadUpdateApplyPlan(),
    readUpdateApplyState: () => mockReadUpdateApplyState(),
  };
});

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

import { SettingsRoute } from "./SettingsRoute";
import { useUpdateApplyShell } from "../../shell/state/update-apply-shell-store";
import { useUpdateShell } from "../../shell/state/update-shell-store";

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
    useUpdateShell.setState({ availability: null });
    useUpdateApplyShell.setState({
      plan: null,
      state: null,
      jobId: null,
      restartInviteFolded: false,
    });
  });

  it("renders the standalone screen: main landmark, h1, version header and the five sections", async () => {
    renderSettings();
    expect(
      screen.getByRole("main", { name: "Profil de support" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 1, name: "Profil de support" }),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(screen.getByText("Gen1")).toBeInTheDocument();
    expect(screen.getByText("Dossier structuré")).toBeInTheDocument();
    expect(screen.getByText("Paquet Linux (.deb / .rpm)")).toBeInTheDocument();
    expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    expect(
      screen.getByText(/La distribution officielle autorise/),
    ).toBeInTheDocument();
  });

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
      screen.getByRole("main", { name: "Profil de support" }),
    ).toHaveAttribute("aria-busy", "true");
    resolveProfile(officialProfile());
    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Profil de support" }),
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
    await router.navigate(-1);
    await waitFor(() => {
      expect(screen.getByTestId(LIBRARY_MARKER)).toBeInTheDocument();
    });
    await router.navigate(-1);
    await waitFor(() => {
      expect(screen.getByTestId("elsewhere-stub")).toBeInTheDocument();
    });
    expect(screen.queryByText("Profil de support")).not.toBeInTheDocument();
  });

  it("fails closed per section when the profile read rejects: sources stay served", async () => {
    mockReadSupportProfile.mockRejectedValue(new Error("drift"));
    renderSettings();
    await waitFor(() => {
      expect(screen.getAllByRole("status")).toHaveLength(3);
    });
    for (const status of screen.getAllByRole("status")) {
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
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent(
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
    expect(screen.queryByText(/^Version /)).not.toBeInTheDocument();
  });

  it("renders correctly under StrictMode (mount token pins the reads to the live mount)", async () => {
    renderSettings({ strict: true });
    await waitFor(() => {
      expect(screen.getByText("Origine v1")).toBeInTheDocument();
    });
    expect(screen.getByText("Version 0.1.0")).toBeInTheDocument();
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent),
    ).toEqual([
      "Appareils",
      "Artefacts locaux",
      "Association de fichiers",
      "Sources de contenu",
      "Politique de distribution",
    ]);
  });

  it("renders the update status line UNDER the version line when a verdict exists", async () => {
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
    // The status line lives in the SAME heading block, under the
    // untouched `Version {version}` line — never an ambiguity about the
    // installed version.
    const heading = screen.getByText("Version 0.1.0").parentElement;
    expect(heading).toContainElement(statusLine as HTMLElement);
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
    expect(line?.querySelector(".ds-chip")).toBeNull();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
