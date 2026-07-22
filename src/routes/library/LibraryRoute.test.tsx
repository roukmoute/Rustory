import { StrictMode } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  RouterProvider,
  createMemoryRouter,
  type RouteObject,
} from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockGet = vi.fn();
const mockDevice = vi.fn();
const mockDeviceLibrary = vi.fn();
const mockImport = vi.fn();
const mockTransferPreview = vi.fn();
const mockStoryValidation = vi.fn();
const mockCatalogStatus = vi.fn();
const mockCatalogRefresh = vi.fn();
const mockCatalogImport = vi.fn();
const mockStartPrepare = vi.fn();
const mockReadPreparation = vi.fn();
const mockStartTransfer = vi.fn();
const mockReadTransfer = vi.fn();
const mockReadTransferOutcome = vi.fn();
const mockDiscardTransferOutcome = vi.fn();
const mockAnalyzeFolder = vi.fn();
const mockAcceptFolder = vi.fn();
const mockAnalyzeArtifact = vi.fn();
const mockAcceptArtifact = vi.fn();
const mockAnalyzeOsOpen = vi.fn();
const mockDiscardOsOpen = vi.fn();
const mockAnalyzeDrop = vi.fn();
const mockDiscardDrop = vi.fn();
const mockCreateStory = vi.fn();
const mockDeleteStories = vi.fn();
const mockFetchRssPreview = vi.fn();
const mockAcceptRssCreation = vi.fn();
const mockReadContentSourcePolicy = vi.fn();

/** The current official content-source policy, exactly as
 *  `read_content_source_policy` serializes it. */
const OFFICIAL_CONTENT_SOURCE_POLICY = {
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

vi.mock("../../ipc/commands/library", () => ({
  getLibraryOverview: () => ({
    promise: mockGet(),
    cancel: () => {},
  }),
}));

vi.mock("../../ipc/commands/device", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/device")
  >("../../ipc/commands/device");
  return {
    ...actual,
    readConnectedLunii: () => ({
      promise: mockDevice(),
      cancel: () => {},
    }),
  };
});

vi.mock("../../ipc/commands/device-library", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/device-library")
  >("../../ipc/commands/device-library");
  return {
    ...actual,
    readDeviceLibrary: () => ({
      promise: mockDeviceLibrary(),
      cancel: () => {},
    }),
  };
});

vi.mock("../../ipc/commands/device-import", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/device-import")
  >("../../ipc/commands/device-import");
  return {
    ...actual,
    importDeviceStory: (input: unknown) => mockImport(input),
  };
});

vi.mock("../../ipc/commands/transfer-preview", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/transfer-preview")
  >("../../ipc/commands/transfer-preview");
  return {
    ...actual,
    readTransferPreview: () => ({
      promise: mockTransferPreview(),
      cancel: () => {},
    }),
  };
});

vi.mock("../../ipc/commands/story-validation", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story-validation")
  >("../../ipc/commands/story-validation");
  return {
    ...actual,
    readStoryValidation: () => ({
      promise: mockStoryValidation(),
      cancel: () => {},
    }),
  };
});

vi.mock("../../ipc/commands/device-catalog", () => ({
  getOfficialCatalogStatus: () => mockCatalogStatus(),
  refreshOfficialCatalog: () => mockCatalogRefresh(),
  importOfficialCatalog: () => mockCatalogImport(),
  readPackCover: () => Promise.resolve(null),
}));

vi.mock("../../ipc/commands/story-preparation", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story-preparation")
  >("../../ipc/commands/story-preparation");
  return {
    ...actual,
    startPrepareStory: (input: unknown) => mockStartPrepare(input),
    readPreparationState: (input: unknown) => mockReadPreparation(input),
  };
});

vi.mock("../../ipc/commands/story-transfer", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story-transfer")
  >("../../ipc/commands/story-transfer");
  return {
    ...actual,
    startTransferStory: (input: unknown) => mockStartTransfer(input),
    readTransferState: (input: unknown) => mockReadTransfer(input),
    readTransferOutcome: (input: unknown) => mockReadTransferOutcome(input),
    discardTransferOutcome: (input: unknown) =>
      mockDiscardTransferOutcome(input),
  };
});

vi.mock("../../ipc/events/job-events", () => ({
  // The render tests drive the panel through the optimistic preflight + the
  // authoritative re-read; no live event is fired, so a no-op unsubscribe is
  // enough.
  subscribeJobEvents: () => () => {},
}));

vi.mock("../../ipc/commands/story", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story")
  >("../../ipc/commands/story");
  return {
    ...actual,
    createStory: (input: unknown) => mockCreateStory(input),
    deleteStories: (input: unknown) => mockDeleteStories(input),
  };
});

vi.mock("../../ipc/commands/import-export", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/import-export")
  >("../../ipc/commands/import-export");
  return {
    ...actual,
    analyzeStructuredFolderForCreation: () => mockAnalyzeFolder(),
    acceptStructuredCreation: (input: unknown) => mockAcceptFolder(input),
    analyzeArtifactForImport: () => mockAnalyzeArtifact(),
    acceptArtifactImport: (input: unknown) => mockAcceptArtifact(input),
    analyzeOsOpenRequest: () => mockAnalyzeOsOpen(),
    discardOsOpenRequest: () => mockDiscardOsOpen(),
    analyzeDropRequest: () => mockAnalyzeDrop(),
    discardDropRequest: () => mockDiscardDrop(),
    fetchRssSourcePreview: (url: string) => mockFetchRssPreview(url),
    acceptRssStoryCreation: (url: string, ref: unknown) =>
      mockAcceptRssCreation(url, ref),
    readContentSourcePolicy: () => mockReadContentSourcePolicy(),
  };
});

import { invalidateConnectedLuniiCache } from "../../features/device/hooks/use-connected-lunii";
import { invalidateDeviceLibraryCache } from "../../features/device/hooks/use-device-library";
import { invalidateLibraryOverviewCache } from "../../features/library/hooks/use-library-overview";
import type { ConnectedDeviceDto } from "../../shared/ipc-contracts/device";
import { useDropShell } from "../../shell/state/drop-shell-store";
import { useLibraryShell } from "../../shell/state/library-shell-store";
import { useOsOpenShell } from "../../shell/state/os-open-shell-store";
import { useUpdateShell } from "../../shell/state/update-shell-store";
import {
  LibraryRoute,
  mapDeviceForPanel,
  mapPreparationView,
  mapStoryValidationToView,
  mapTransferPreviewToComparison,
  mapTransferView,
} from "./LibraryRoute";

const STORY_EDIT_MARKER_TITLE = "Edit stub for";

function renderLibrary(options: { strict?: boolean } = {}) {
  const routes: RouteObject[] = [
    { path: "/library", element: <LibraryRoute /> },
    {
      path: "/story/:storyId/edit",
      element: (
        <div data-testid="story-edit-stub">{STORY_EDIT_MARKER_TITLE}</div>
      ),
    },
    // The support-profile screen the navigation entry and the
    // `Consulter le profil de support` gesture both target IN-APP.
    { path: "/settings", element: <div data-testid="settings-stub" /> },
  ];
  const router = createMemoryRouter(routes, {
    initialEntries: ["/library"],
  });
  const tree = <RouterProvider router={router} />;
  render(options.strict ? <StrictMode>{tree}</StrictMode> : tree);
  return router;
}

describe("<LibraryRoute />", () => {
  beforeEach(() => {
    mockGet.mockReset();
    mockDevice.mockReset();
    // Default: the device probe never resolves so the panel stays in
    // the scanning state. Tests that care about a specific device
    // outcome override this with `mockDevice.mockResolvedValueOnce(...)`
    // before rendering.
    mockDevice.mockImplementation(() => new Promise(() => {}));
    // Default: the device-library read resolves to "none" so the device
    // section stays absent unless a test opts into a readable payload.
    mockDeviceLibrary.mockReset();
    mockDeviceLibrary.mockResolvedValue({ kind: "none" });
    mockImport.mockReset();
    // Default: the transfer-preview read folds away (noDevice) so the
    // comparison stays sober unless a test opts into a readable comparison.
    mockTransferPreview.mockReset();
    mockTransferPreview.mockResolvedValue({ kind: "noDevice" });
    // Default: the story-validation read folds away (noDevice) so the
    // validation stays sober unless a test opts into a verdict.
    mockStoryValidation.mockReset();
    mockStoryValidation.mockResolvedValue({ kind: "noDevice" });
    // Default: the official-catalog status reads as an empty cache so the
    // panel renders without hitting the real IPC bridge.
    mockCatalogStatus.mockReset();
    mockCatalogStatus.mockResolvedValue({ count: 0 });
    mockCatalogRefresh.mockReset();
    mockCatalogRefresh.mockResolvedValue({ count: 0 });
    mockCatalogImport.mockReset();
    mockCatalogImport.mockResolvedValue({ kind: "cancelled" });
    // Default: preparation is user-triggered, so on mount nothing is called.
    // Tests that exercise the Préparer CTA override these.
    mockStartPrepare.mockReset();
    mockStartPrepare.mockResolvedValue({
      jobId: "0197a5d0-0000-7000-8000-0000000000aa",
      storyId: "s1",
    });
    mockReadPreparation.mockReset();
    mockReadPreparation.mockResolvedValue({ kind: "idle" });
    // Default: transfer is user-triggered, so on mount nothing is called.
    // Tests that exercise the Envoyer CTA override these.
    mockStartTransfer.mockReset();
    mockStartTransfer.mockResolvedValue({
      jobId: "0197a5d0-0000-7000-8000-0000000000bb",
      storyId: "s1",
    });
    mockReadTransfer.mockReset();
    mockReadTransfer.mockResolvedValue({ kind: "idle" });
    // Default: no durable transfer memory, and a purge that succeeds. Tests that
    // exercise re-hydration / Abandonner override these.
    mockReadTransferOutcome.mockReset();
    mockReadTransferOutcome.mockResolvedValue(null);
    mockDiscardTransferOutcome.mockReset();
    mockDiscardTransferOutcome.mockResolvedValue(undefined);
    // Default: the folder analysis is user-triggered; a cancelled dialog is
    // the safe default. Tests exercising the folder flow override this.
    mockAnalyzeFolder.mockReset();
    mockAnalyzeFolder.mockResolvedValue({ kind: "cancelled" });
    // Default: the folder accept settles into a fresh card. Tests that
    // exercise a commit failure override it.
    mockAcceptFolder.mockReset();
    mockAcceptFolder.mockResolvedValue({
      id: "0197a5d0-0000-7000-8000-0000000000dd",
      title: "Le voyage de Nour",
      importState: "recognized",
    });
    // Default: the artifact import analysis is user-triggered too.
    mockAnalyzeArtifact.mockReset();
    mockAnalyzeArtifact.mockResolvedValue({ kind: "cancelled" });
    // Default: the accept phase settles into a fresh card. Tests that
    // exercise a commit failure override it.
    mockAcceptArtifact.mockReset();
    mockAcceptArtifact.mockResolvedValue({
      id: "0197a5d0-0000-7000-8000-0000000000cc",
      title: "Le Soleil",
      importState: "needsReview",
    });
    // Default: no pending OS-open intent — the mount pull answers `none`
    // (the total silent no-op). Tests exercising the channel override it.
    mockAnalyzeOsOpen.mockReset();
    mockAnalyzeOsOpen.mockResolvedValue({ kind: "none" });
    mockDiscardOsOpen.mockReset();
    mockDiscardOsOpen.mockResolvedValue(undefined);
    // Default: no pending drop intent either — same silent no-op.
    mockAnalyzeDrop.mockReset();
    mockAnalyzeDrop.mockResolvedValue({ kind: "none" });
    mockDiscardDrop.mockReset();
    mockDiscardDrop.mockResolvedValue(undefined);
    // Default: the primary title creation never settles unless a test
    // drives it explicitly.
    mockCreateStory.mockReset();
    // Default: deletion resolves with an empty ack; tests exercising the
    // flow override it with the confirmed ids.
    mockDeleteStories.mockReset();
    mockDeleteStories.mockResolvedValue({ deletedIds: [] });
    mockCreateStory.mockImplementation(() => new Promise(() => {}));
    // Default: the RSS preview is user-triggered; a never-settling fetch is
    // overridden by the tests that exercise the flow.
    mockFetchRssPreview.mockReset();
    mockFetchRssPreview.mockImplementation(() => new Promise(() => {}));
    mockAcceptRssCreation.mockReset();
    mockAcceptRssCreation.mockImplementation(() => new Promise(() => {}));
    // Default: the policy read resolves with the official distribution
    // (rss enabled) so the dialog's RSS entry is actionable. Tests that
    // exercise the fail-closed path override with a rejection.
    mockReadContentSourcePolicy.mockReset();
    mockReadContentSourcePolicy.mockResolvedValue(
      OFFICIAL_CONTENT_SOURCE_POLICY,
    );
    // The hooks keep module-local stale-while-revalidate caches; reset
    // them between tests so no stray snapshot bleeds across cases.
    invalidateLibraryOverviewCache();
    invalidateConnectedLuniiCache();
    invalidateDeviceLibraryCache();
    useLibraryShell.setState({
      selectedStoryIds: new Set(),
      query: "",
      sort: "titre-asc",
    });
    useOsOpenShell.setState({ pendingSignal: false });
    useDropShell.setState({ hoverActive: false, pendingSignal: false });
    useUpdateShell.setState({ availability: null });
  });

  it("shows the loading state before the IPC call resolves", () => {
    mockGet.mockImplementation(() => new Promise(() => {}));
    renderLibrary();

    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an actionable empty state with active Créer CTAs that open the dialog", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();

    expect(
      await screen.findByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).toBeInTheDocument();

    // Two entry points into the create flow are exposed: the header CTA
    // and the one inside the empty-state region. Both are keyboard
    // reachable and neither is disabled when the library is ready.
    const primaryCtas = screen.getAllByRole("button", {
      name: /créer une histoire/i,
    });
    expect(primaryCtas).toHaveLength(2);
    for (const cta of primaryCtas) {
      expect(cta).not.toBeDisabled();
      expect(cta).not.toHaveAttribute("aria-disabled", "true");
    }
    // The legacy "indisponible" reason must not be displayed anymore when
    // a handler is wired — the UI cannot be both actionable and reason-gated.
    expect(
      screen.queryByText(/création d'histoire indisponible/i),
    ).not.toBeInTheDocument();
  });

  it("shows a localized error and a Réessayer button when storage init fails", async () => {
    mockGet.mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Le stockage local est inaccessible.");
    expect(alert).toHaveTextContent("Vérifie les permissions puis relance.");

    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();

    expect(screen.getByRole("button", { name: /réessayer/i })).toBeEnabled();
  });

  it("wraps non-AppError rejections as UNKNOWN instead of fabricating a storage failure", async () => {
    mockGet.mockRejectedValueOnce(new Error("kaboom"));
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/une erreur inattendue/i);
    expect(alert).not.toHaveTextContent(/stockage local/i);
  });

  it("rejects a malformed overview payload instead of rendering it", async () => {
    mockGet.mockResolvedValueOnce({ unexpected: true } as never);
    renderLibrary();

    const alert = await screen.findByRole("alert");
    // A drifted wire shape now maps to the canonical LIBRARY_INCONSISTENT
    // surface — same treatment as a Rust-side duplicate-id error.
    expect(alert).toHaveTextContent(
      /bibliothèque incohérente, recharge nécessaire/i,
    );
    expect(alert).toHaveTextContent(/bibliothèque incohérente/i);
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
  });

  it("retries the IPC call when Réessayer is pressed and recovers on success", async () => {
    const user = userEvent.setup();
    mockGet
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "Le stockage local est inaccessible.",
        userAction: "Vérifie les permissions puis relance.",
        details: null,
      })
      .mockResolvedValueOnce({ stories: [] });

    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /réessayer/i }));

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
      ).toBeInTheDocument(),
    );

    expect(mockGet).toHaveBeenCalledTimes(2);
  });

  it("ignores a late response from a superseded IPC call (StrictMode race)", async () => {
    let resolveFirst!: (v: { stories: unknown[] }) => void;
    mockGet
      .mockImplementationOnce(
        () => new Promise((res) => (resolveFirst = res as never)),
      )
      .mockResolvedValueOnce({ stories: [] });

    renderLibrary({ strict: true });

    expect(
      await screen.findByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).toBeInTheDocument();

    // Let the late ghost response settle into state (or fail to). waitFor
    // on an absence returns immediately — drain the microtask queue
    // explicitly so the ghost, if accepted by a racing setState, would
    // have rendered by now.
    resolveFirst({ stories: [{ id: "GHOST", title: "Fantôme" }] });
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    expect(screen.queryByText(/fantôme/i)).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();
  });

  it("surfaces a timeout-shaped error from the IPC facade as UNKNOWN", async () => {
    mockGet.mockRejectedValueOnce({
      code: "UNKNOWN",
      message: "Rustory a mis trop de temps à charger la bibliothèque.",
      userAction: "Relance l'application.",
      details: null,
    });
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/mis trop de temps/i);
  });

  it("renders three columns with semantic regions (nav/main/aside)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    expect(
      screen.getByRole("navigation", { name: /navigation bibliothèque/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("main", { name: /collection d'histoires/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).toBeInTheDocument();
  });

  it("anchors the empty state in the center column, never in the nav or the panel", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    expect(
      await within(main).findByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).toBeInTheDocument();

    const nav = screen.getByRole("navigation", {
      name: /navigation bibliothèque/i,
    });
    expect(
      within(nav).queryByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("shows the Lunii Decision Panel with 'Aucun appareil connecté' once detection completes with kind=none", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({ kind: "none" });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getAllByText(/aucun appareil connecté/i).length,
      ).toBeGreaterThan(0),
    );
  });

  it("preserves the 3-column layout when an error is surfaced — not a bare error screen", async () => {
    mockGet.mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });
    renderLibrary();
    await screen.findByRole("alert");

    expect(
      screen.getByRole("navigation", { name: /navigation bibliothèque/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).toBeInTheDocument();
    const main = screen.getByRole("main", {
      name: /collection d'histoires/i,
    });
    expect(within(main).getByRole("alert")).toBeInTheDocument();
  });

  // --- Selection + navigation ---

  it("clicking a card marks it selected and shows '1 histoire sélectionnée' in the panel", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    renderLibrary();

    const card = await screen.findByRole("button", { name: /le soleil/i });
    await user.click(card);

    expect(card).toHaveAttribute("aria-pressed", "true");
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByText(/^1 histoire sélectionnée$/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/2 histoires — 1 sélectionnée/),
    ).toBeInTheDocument();
  });

  it("deletes the confirmed selection, clears it, and re-reads the overview", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "À supprimer" },
        { id: "s2", title: "Conservée" },
      ],
    });
    // The re-read AFTER the deletion answers without the removed story.
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s2", title: "Conservée" }],
    });
    mockDeleteStories.mockResolvedValueOnce({ deletedIds: ["s1"] });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /à supprimer/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await user.click(
      within(panel).getByRole("button", { name: /^supprimer$/i }),
    );
    await user.click(
      within(panel).getByRole("button", { name: /confirmer la suppression/i }),
    );

    expect(mockDeleteStories).toHaveBeenCalledWith({ ids: ["s1"] });
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /à supprimer/i }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: /conservée/i }),
    ).toBeInTheDocument();
    expect(
      within(panel).getByText(/^aucune histoire sélectionnée$/i),
    ).toBeInTheDocument();
  });

  it("keeps the library and the selection intact when the deletion fails, and surfaces the alert", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Increvable" }],
    });
    mockDeleteStories.mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Rustory n'a pas pu supprimer la sélection.",
      userAction: "Réessaie dans un instant.",
      details: null,
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /increvable/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await user.click(
      within(panel).getByRole("button", { name: /^supprimer$/i }),
    );
    await user.click(
      within(panel).getByRole("button", { name: /confirmer la suppression/i }),
    );

    expect(
      await within(panel).findByRole("alert"),
    ).toHaveTextContent(/n'a pas pu supprimer la sélection/i);
    // All-or-nothing: the card is still there, still selected — the user
    // can retry the same confirmed intent.
    expect(
      screen.getByRole("button", { name: /increvable/i }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("Ctrl+click on a second card toggles multi-selection and disables Éditer with the canonical reason", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("button", { name: /la lune/i }));
    await user.keyboard("{/Control}");

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByText(/^2 histoires sélectionnées$/i),
    ).toBeInTheDocument();

    const edit = within(panel).getByRole("button", { name: /^éditer$/i });
    expect(edit).toHaveAttribute("aria-disabled", "true");
    const reason = document.getElementById(
      edit.getAttribute("aria-describedby") as string,
    );
    expect(reason).toHaveTextContent(
      /reprise indisponible: sélection multiple/i,
    );
  });

  it("clicking outside cards (inside the main collection header) does not touch the selection", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    expect(useLibraryShell.getState().selectedStoryIds.has("s1")).toBe(true);

    // The previous revision of this test clicked on the `<nav>` "Filtres"
    // heading; that region is inside the nav landmark, not the collection
    // header. Target the collection's own h1 so we actually exercise a
    // click inside the `<main>` that carries the cards.
    const main = screen.getByRole("main", {
      name: /collection d'histoires/i,
    });
    const title = within(main).getByRole("heading", {
      name: /bibliothèque/i,
    });
    await user.click(title);
    expect(useLibraryShell.getState().selectedStoryIds.has("s1")).toBe(true);
  });

  it("double-clicking a card navigates to /story/:id/edit", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    const router = renderLibrary();

    await user.dblClick(
      await screen.findByRole("button", { name: /le soleil/i }),
    );

    await waitFor(() =>
      expect(router.state.location.pathname).toBe("/story/s1/edit"),
    );
    expect(screen.getByTestId("story-edit-stub")).toBeInTheDocument();
  });

  it("encodes the id with encodeURIComponent before building the edit URL", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "abc/space id", title: "Titre spécial" }],
    });
    const router = renderLibrary();

    await user.dblClick(
      await screen.findByRole("button", { name: /titre spécial/i }),
    );

    await waitFor(() =>
      expect(router.state.location.pathname).toBe(
        "/story/abc%2Fspace%20id/edit",
      ),
    );
  });

  it("selecting a single card then clicking Éditer in the panel navigates to /story/:id/edit", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    const router = renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await user.click(within(panel).getByRole("button", { name: /^éditer$/i }));

    await waitFor(() =>
      expect(router.state.location.pathname).toBe("/story/s1/edit"),
    );
  });

  it("derives a 'present' selection so a stale id cannot activate Éditer before the prune effect flushes", async () => {
    // Seed a lingering selection for an id the fresh overview does NOT
    // contain. Before the prune effect runs, the render still reads the
    // store; the route MUST NOT let that stale id light up the Éditer CTA.
    useLibraryShell.setState({ selectedStoryIds: new Set(["ghost"]) });
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    // The chip reflects "0" selection for this render — ghost is filtered.
    const region = within(panel).getByRole("region", {
      name: /sélection courante/i,
    });
    expect(region).toHaveTextContent(/aucune histoire sélectionnée/i);
    const edit = within(region).getByRole("button", { name: /^éditer$/i });
    expect(edit).toHaveAttribute("aria-disabled", "true");
  });

  it("renders the LIBRARY_INCONSISTENT canonical title when the Rust side reports duplicate ids", async () => {
    mockGet.mockRejectedValueOnce({
      code: "LIBRARY_INCONSISTENT",
      message: "La bibliothèque locale contient des histoires en double.",
      userAction: "Recharge Rustory pour reconstruire la vue cohérente.",
      details: null,
    });
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      /bibliothèque incohérente, recharge nécessaire/i,
    );
    expect(alert.textContent ?? "").toMatch(
      /bibliothèque locale contient des histoires en double/i,
    );
  });

  it("renders the cached overview immediately on remount (stale-while-revalidate)", async () => {
    // Priming render: populate the hook-local cache.
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    const firstRouter = renderLibrary();
    await screen.findByRole("button", { name: /le soleil/i });

    // Navigate away so the route unmounts; the hook cache must survive.
    firstRouter.navigate("/story/s1/edit");
    await waitFor(() =>
      expect(firstRouter.state.location.pathname).toBe("/story/s1/edit"),
    );

    // Re-render the library while the next IPC call is still pending.
    mockGet.mockImplementationOnce(() => new Promise(() => {}));
    firstRouter.navigate("/library");
    await waitFor(() =>
      expect(firstRouter.state.location.pathname).toBe("/library"),
    );

    // Card is visible immediately from the cache — no `loading` flash.
    expect(
      await screen.findByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  it("prunes a stale selection when the overview no longer contains the selected id", async () => {
    // Seed a lingering selection — could come from a previous in-memory
    // session or from a future refresh path.
    useLibraryShell.setState({ selectedStoryIds: new Set(["s1"]) });
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s2", title: "La lune" }],
    });

    renderLibrary();

    await waitFor(() =>
      expect(useLibraryShell.getState().selectedStoryIds.has("s1")).toBe(false),
    );
    expect(useLibraryShell.getState().selectedStoryIds.size).toBe(0);
  });

  it("opens the Créer dialog when the header CTA is pressed", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    renderLibrary();

    // Wait for the library to render so the header CTA is mounted.
    await screen.findByRole("button", { name: /le soleil/i });
    const headerCta = screen.getByRole("button", {
      name: /créer une histoire/i,
    });
    expect(
      screen.queryByRole("dialog", { name: /créer une histoire/i }),
    ).not.toBeInTheDocument();
    await user.click(headerCta);
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    expect(within(dialog).getByLabelText(/^titre$/i)).toBeInTheDocument();
  });

  it("keeps the folder entry inert while a .rustory analysis is in flight (cross-flow exclusivity)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    // A `.rustory` analysis that never settles: the import flow stays busy.
    mockAnalyzeArtifact.mockImplementationOnce(() => new Promise(() => {}));
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    await user.click(
      screen.getByRole("button", { name: /importer une histoire/i }),
    );

    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    const folderButton = within(dialog).getByRole("button", {
      name: "Choisir un dossier…",
    });
    expect(folderButton).toHaveAttribute("aria-disabled", "true");
    await user.click(folderButton);
    // No second native dialog may open: the folder analysis is never
    // started and the creation dialog stays where the user left it.
    expect(mockAnalyzeFolder).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: /créer une histoire/i }),
    ).toBeInTheDocument();
  });

  it("keeps the collection's import CTA inert while an RSS fetch is in flight (cross-flow exclusivity)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    // An RSS fetch that never settles: the flow stays busy.
    mockFetchRssPreview.mockImplementationOnce(() => new Promise(() => {}));
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    // Open the creation dialog and hand over to the RSS flow.
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    );
    // Type an address and fetch — the flow is now busy.
    await user.type(
      screen.getByLabelText("Adresse du flux RSS"),
      "https://exemple.fr/flux.xml",
    );
    await user.click(screen.getByRole("button", { name: "Récupérer le flux" }));
    expect(mockFetchRssPreview).toHaveBeenCalledTimes(1);

    const importButton = screen.getByRole("button", {
      name: /importer une histoire/i,
    });
    expect(importButton).toBeDisabled();
    await user.click(importButton);
    expect(mockAnalyzeArtifact).not.toHaveBeenCalled();
  });

  it("keeps the folder entry inert while an RSS review surface is open (cross-flow exclusivity)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    // Open the RSS surface through the dialog's third entry: the flow is
    // now ACTIVE (surface open) even though nothing is in flight yet.
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    );
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();

    // Re-open the creation dialog: both secondary entries are inert while
    // the RSS surface is open — two review surfaces must never stack.
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    const folderButton = within(dialog).getByRole("button", {
      name: "Choisir un dossier…",
    });
    expect(folderButton).toHaveAttribute("aria-disabled", "true");
    await user.click(folderButton);
    expect(mockAnalyzeFolder).not.toHaveBeenCalled();
    const rssButton = within(dialog).getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
  });

  it("a completed RSS creation surfaces the à revoir chip on the created library card (review steps visible)", async () => {
    const user = userEvent.setup();
    // Initial overview: one native story. After the creation, the reload
    // returns the fresh card WITH its durable import state.
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockGet.mockResolvedValue({
      stories: [
        { id: "s1", title: "Le soleil" },
        {
          id: "s-rss",
          title: "Episode 1",
          importState: "needsReview",
          importReport: [
            {
              aspect: "source",
              category: "ambiguous",
              message:
                "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
            },
          ],
        },
      ],
    });
    mockFetchRssPreview.mockResolvedValueOnce({
      sourceHost: "exemple.fr",
      items: [
        {
          title: "Episode 1",
          summary: "Premier texte.",
          hasEnclosure: false,
          itemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
        },
      ],
      findings: [
        {
          aspect: "source",
          category: "ambiguous",
          message:
            "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
        },
      ],
      state: "needsReview",
      blocked: false,
    });
    mockAcceptRssCreation.mockResolvedValueOnce({
      kind: "created",
      story: { id: "s-rss", title: "Episode 1", importState: "needsReview" },
      report: [],
    });
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    // Full journey: dialog → third entry → type → fetch → select → accept.
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    await user.click(
      await screen.findByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    );
    await user.type(
      screen.getByLabelText("Adresse du flux RSS"),
      "https://exemple.fr/flux.xml",
    );
    await user.click(screen.getByRole("button", { name: "Récupérer le flux" }));
    await user.click(await screen.findByRole("button", { name: /Episode 1/ }));
    await user.click(
      screen.getByRole("button", { name: "Créer le brouillon" }),
    );
    // The success terminal renders (live region + chip), then the reloaded
    // library card carries the review chip — THE visible materialization
    // of the pending review (rendered NEXT to the card button, with the
    // provenance marker).
    await screen.findAllByText("Histoire créée dans ta bibliothèque");
    await screen.findByRole("button", { name: /episode 1/i });
    expect(await screen.findByText("à revoir")).toBeInTheDocument();
    expect(screen.getByText("Importée")).toBeInTheDocument();
  });

  it("reads the content-source policy at every dialog opening and renders the policy-driven sources section", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    // One point-in-time read per opening — no cache.
    expect(mockReadContentSourcePolicy).toHaveBeenCalledTimes(1);
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    // The enabled RSS entry with its label + frozen activation marker.
    const rssButton = within(dialog).getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    await waitFor(() => expect(rssButton).not.toHaveAttribute("aria-disabled"));
    expect(within(dialog).getByText("Flux RSS")).toBeInTheDocument();
    expect(
      within(dialog).getByText("Activée par la distribution officielle"),
    ).toBeInTheDocument();
    // The known non-activated kinds render VISIBLE but DISABLED with the
    // Rust-carried reason.
    for (const label of ["Flux Atom", "Flux JSON Feed"]) {
      expect(
        within(dialog).getByRole("button", { name: label }),
      ).toHaveAttribute("aria-disabled", "true");
    }
    expect(
      within(dialog).getAllByText(
        "Source indisponible: non activée dans la distribution officielle",
      ),
    ).toHaveLength(2);

    // Close, reopen: the policy is read ANEW (point-in-time, no cache).
    await user.click(within(dialog).getByRole("button", { name: "Annuler" }));
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    expect(mockReadContentSourcePolicy).toHaveBeenCalledTimes(2);
  });

  it("ignores an out-of-order policy resolution from a previous opening (opening token)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    // FIRST opening: a slow read we resolve LATER, out of order.
    let resolveFirst: (value: unknown) => void = () => {};
    mockReadContentSourcePolicy.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
    );
    // SECOND opening: the read fails → the CURRENT opening must stay
    // fail-closed even after the first read finally lands.
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    let dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    // Close the first opening while its read is still in flight.
    await user.click(within(dialog).getByRole("button", { name: "Annuler" }));

    mockReadContentSourcePolicy.mockRejectedValueOnce(
      new Error("lecture en échec"),
    );
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });

    // The STALE first read now resolves with the enabled policy — it
    // belongs to a closed opening and must NOT reactivate the entry.
    resolveFirst(OFFICIAL_CONTENT_SOURCE_POLICY);
    await waitFor(() => {
      expect(
        within(dialog).getByRole("button", {
          name: "Démarrer depuis une source externe (RSS)",
        }),
      ).toHaveAttribute("aria-disabled", "true");
    });
    expect(
      within(dialog).getByText(
        "Sources externes indisponibles pour l'instant.",
      ),
    ).toBeInTheDocument();
  });

  it("renders the external-source entries fail-closed when the policy read fails, without blocking the title path", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockReadContentSourcePolicy.mockRejectedValue(
      new Error("ipc indisponible"),
    );
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    // FAIL-CLOSED: the RSS entry renders disabled with the frozen reason —
    // never active-by-default.
    const rssButton = within(dialog).getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
    await user.click(rssButton);
    expect(
      screen.queryByLabelText("Adresse du flux RSS"),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).getByText(
        "Sources externes indisponibles pour l'instant.",
      ),
    ).toBeInTheDocument();
    // The primary title path is NEVER blocked by a policy failure.
    expect(within(dialog).getByLabelText(/^titre$/i)).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: /^créer$/i }),
    ).toBeInTheDocument();
  });

  it("Choisir un dossier… in the Créer dialog closes it and surfaces the folder report in-context", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    // The REAL wire shape of a creatable verdict (five folder aspects) —
    // the mocked facade must speak the wire the guard would let through.
    mockAnalyzeFolder.mockResolvedValueOnce({
      kind: "analyzed",
      quality: "clean",
      state: "recognized",
      findings: [
        {
          aspect: "envelope",
          category: "recognized",
          message: "Le manifest histoire.json est présent et lisible.",
        },
        {
          aspect: "formatVersion",
          category: "recognized",
          message: "La version de format du manifest est prise en charge.",
        },
        {
          aspect: "title",
          category: "recognized",
          message: "Le titre de l'histoire est valide.",
        },
        {
          aspect: "structure",
          category: "recognized",
          message: "La structure de l'histoire est reconnue.",
        },
        {
          aspect: "media",
          category: "recognized",
          message:
            "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
        },
      ],
      creatableSummary: {
        title: "Le voyage de Nour",
        nodeCount: 2,
        retainedMedia: [],
        discardedMedia: [],
      },
      folderName: "mon-dossier",
      folderPath: "/home/user/mon-dossier",
    });
    renderLibrary();

    await screen.findByRole("button", { name: /le soleil/i });
    await user.click(
      screen.getByRole("button", { name: /créer une histoire/i }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: /créer une histoire/i,
    });
    await user.click(
      within(dialog).getByRole("button", { name: "Choisir un dossier…" }),
    );

    // The dialog closes and the in-context report surfaces (no navigation,
    // no toast) with the unique accept CTA.
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: /créer une histoire/i }),
      ).not.toBeInTheDocument(),
    );
    const surface = await screen.findByRole("region", {
      name: "Création depuis un dossier",
    });
    expect(within(surface).getByText("mon-dossier")).toBeInTheDocument();
    expect(
      within(surface).getByRole("button", { name: "Créer l'histoire" }),
    ).toBeInTheDocument();
  });

  it("after a successful create_story, invalidates the cache and navigates to /story/:id/edit", async () => {
    const user = userEvent.setup();
    // First fetch returns an empty library; the second fetch — triggered
    // after invalidation — returns the freshly created story.
    mockGet.mockResolvedValueOnce({ stories: [] }).mockResolvedValueOnce({
      stories: [{ id: "new-id", title: "Mon histoire" }],
    });

    const storyModule = await import("../../ipc/commands/story");
    const createStorySpy = vi.spyOn(storyModule, "createStory");
    createStorySpy.mockResolvedValueOnce({
      id: "new-id",
      title: "Mon histoire",
    });

    try {
      renderLibrary();
      await screen.findByRole("heading", {
        name: /ta bibliothèque est vide/i,
      });

      const creates = screen.getAllByRole("button", {
        name: /créer une histoire/i,
      });
      await user.click(creates[0]);

      const dialog = await screen.findByRole("dialog", {
        name: /créer une histoire/i,
      });
      await user.type(
        within(dialog).getByLabelText(/^titre$/i),
        "Mon histoire",
      );
      await user.click(
        within(dialog).getByRole("button", { name: /^créer$/i }),
      );

      await waitFor(() =>
        expect(createStorySpy).toHaveBeenCalledWith({
          title: "Mon histoire",
        }),
      );
      await screen.findByTestId("story-edit-stub");
    } finally {
      createStorySpy.mockRestore();
    }
  });

  it("never leaks internal planning jargon in the library DOM", async () => {
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    const { container } = render(
      <RouterProvider
        router={createMemoryRouter(
          [{ path: "/library", element: <LibraryRoute /> }],
          { initialEntries: ["/library"] },
        )}
      />,
    );
    await screen.findByRole("button", { name: /le soleil/i });
    expect(container.textContent ?? "").not.toMatch(
      /\bbmad\b|\bstory\s*\d\.\d|\bepic\s*\d/i,
    );
  });

  // --- Device integration ---

  it("passes device state 'absent' to the panel when the scan returns kind=none", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({ kind: "none" });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getAllByText(/aucun appareil connecté/i).length,
      ).toBeGreaterThan(0),
    );
  });

  it("passes device state 'idle' with a deviceLabel when a supported Lunii is detected", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({
      kind: "supported",
      family: "lunii",
      firmwareCohort: "origineV1",
      metadataFormatVersion: 3,
      deviceIdentifier: "abc",
      supportedOperations: {
        readLibrary: true,
        inspectStory: true,
        importStory: true,
        writeStory: false,
      },
    });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByText(/appareil prêt — lunii origine/i),
      ).toBeInTheDocument(),
    );
  });

  it("passes device state 'unsupported' with the canonical reason when an unsupported Lunii is detected", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({
      kind: "unsupported",
      reason: "metadataUnsupported",
      firmwareHint: "metadata_v99",
    });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByText(/format métadonnées v99 non géré/i),
      ).toBeInTheDocument(),
    );
  });

  it("navigates to /settings through the permanent left-column support-profile entry", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    const nav = screen.getByRole("navigation", {
      name: /navigation bibliothèque/i,
    });
    const user = userEvent.setup();
    await user.click(
      within(nav).getByRole("button", { name: "Profil de support" }),
    );
    await waitFor(() => {
      expect(screen.getByTestId("settings-stub")).toBeInTheDocument();
    });
  });

  it("renders no update signal by default — silence while no update is available", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await waitFor(() => {
      expect(
        screen.getByRole("navigation", { name: /navigation bibliothèque/i }),
      ).toBeInTheDocument();
    });
    expect(
      screen.queryByRole("button", {
        name: "Consulter les détails de la mise à jour",
      }),
    ).not.toBeInTheDocument();
    expect(document.querySelector(".update-availability-signal")).toBeNull();
  });

  it("renders the discreet update signal in the left navigation column on an updateAvailable verdict", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
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
    renderLibrary();
    const nav = screen.getByRole("navigation", {
      name: /navigation bibliothèque/i,
    });
    // The signal lives in the NAVIGATION column — never the central
    // surface, never the decision panel.
    expect(
      within(nav).getByText("Nouvelle version disponible : 9.9.9."),
    ).toBeInTheDocument();
    expect(
      within(nav).getByRole("button", {
        name: "Consulter les détails de la mise à jour",
      }),
    ).toBeInTheDocument();
    // Foot anchoring contract: the signal is the LAST direct child of
    // the flex column, carrying its anchor class (`margin-top: auto`
    // pushes it to the column's real bottom).
    const signal = within(nav).getByRole("status");
    expect(signal).toHaveClass("update-availability-signal");
    expect(signal.parentElement).toBe(nav);
    expect(nav.lastElementChild).toBe(signal);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("navigates to /settings through the update signal's Voir les détails gesture", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
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
    renderLibrary();
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "Consulter les détails de la mise à jour",
      }),
    );
    await waitFor(() => {
      expect(screen.getByTestId("settings-stub")).toBeInTheDocument();
    });
  });

  it("navigates to /settings in-app through the Consulter le profil de support gesture — no external browser", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({
      kind: "unsupported",
      reason: "metadataUnsupported",
      firmwareHint: "metadata_v99",
    });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const user = userEvent.setup();
    await waitFor(() =>
      expect(
        within(panel).getByRole("button", {
          name: "Consulter le profil de support officiel",
        }),
      ).toBeInTheDocument(),
    );
    await user.click(
      within(panel).getByRole("button", {
        name: "Consulter le profil de support officiel",
      }),
    );
    await waitFor(() => {
      expect(screen.getByTestId("settings-stub")).toBeInTheDocument();
    });
  });

  it("passes device state 'error' when the scan transport fails (DEVICE_SCAN_FAILED)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockRejectedValueOnce({
      code: "DEVICE_SCAN_FAILED",
      message: "Détection indisponible.",
      userAction: "Réessaie la détection.",
      details: { source: "fs_read", kind: "permission_denied" },
    });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByText(/détection indisponible/i),
      ).toBeInTheDocument(),
    );
  });

  it("renders the library normally when the device scan fails (orthogonality — AC #3)", async () => {
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockRejectedValueOnce({
      code: "DEVICE_SCAN_FAILED",
      message: "Détection indisponible.",
      userAction: "Réessaie.",
      details: null,
    });
    renderLibrary();
    // The library card must render even when the device probe failed.
    await screen.findByRole("button", { name: /le soleil/i });
    // And the panel surfaces the device error state in parallel.
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByText(/détection indisponible/i),
    ).toBeInTheDocument();
  });

  it("renders the device state when the library overview is in error (orthogonality — inverse of AC #3)", async () => {
    mockGet.mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });
    mockDevice.mockResolvedValueOnce({ kind: "none" });
    renderLibrary();
    await screen.findByRole("alert");
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getAllByText(/aucun appareil connecté/i).length,
      ).toBeGreaterThan(0),
    );
  });

  it("the refresh button in the panel re-runs the device scan only", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({ kind: "none" }).mockResolvedValueOnce({
      kind: "supported",
      family: "lunii",
      firmwareCohort: "midGenV2",
      metadataFormatVersion: 6,
      deviceIdentifier: "id2",
      supportedOperations: {
        readLibrary: true,
        inspectStory: true,
        importStory: true,
        writeStory: false,
      },
    });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getAllByText(/aucun appareil connecté/i).length,
      ).toBeGreaterThan(0),
    );

    await user.click(
      within(panel).getByRole("button", { name: /réessayer la détection/i }),
    );

    await waitFor(() =>
      expect(within(panel).getByText(/appareil prêt/i)).toBeInTheDocument(),
    );

    // The library overview was fetched exactly once — refreshing the
    // device must not retrigger it.
    expect(mockGet).toHaveBeenCalledTimes(1);
  });

  it("passes a generic ambiguous reason when 2+ Lunii are detected", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValueOnce({ kind: "ambiguous", candidateCount: 2 });
    renderLibrary();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getAllByText(/profil ambigu/i).length,
      ).toBeGreaterThan(0),
    );
    expect(
      within(panel).getByText(/2 candidats détectés/i),
    ).toBeInTheDocument();
  });

  // --- Device library (read + display) ---

  const supportedV3 = {
    kind: "supported" as const,
    family: "lunii" as const,
    firmwareCohort: "v3" as const,
    metadataFormatVersion: 7,
    deviceIdentifier: "0123456789abcdef0123456789abcdef",
    supportedOperations: {
      readLibrary: true,
      inspectStory: true,
      importStory: false,
      writeStory: false,
    },
  };

  it("lists the device library distinctly in the center column when a supported Lunii exposes packs", async () => {
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue({
      kind: "readable",
      deviceIdentifier: supportedV3.deviceIdentifier,
      stories: [
        {
          uuid: "u1",
          shortId: "0000ABCD",
          hidden: false,
          contentPresent: true,
          alreadyImported: false,
          title: null,
          titleSource: null,
          thumbnail: null,
        },
      ],
    });
    renderLibrary();

    // The LOCAL library renders as usual.
    await screen.findByRole("button", { name: /le soleil/i });

    // The device library appears as a DISTINCT region inside the center
    // column — never merged into the local collection.
    const main = screen.getByRole("main", { name: /collection d'histoires/i });
    const deviceRegion = await within(main).findByRole("region", {
      name: /bibliothèque de l'appareil/i,
    });
    expect(within(deviceRegion).getByText("0000ABCD")).toBeInTheDocument();
    expect(
      within(deviceRegion).getAllByText(/histoire non reconnue/i).length,
    ).toBeGreaterThan(0);
    // The device read did not retrigger the local overview fetch.
    expect(mockGet).toHaveBeenCalledTimes(1);
  });

  it("lights the device-library section for a supported FLAM through the capability matrix alone", async () => {
    // The FLAM Gen1 wire (readLibrary=true) drives readableDeviceId
    // through the SAME derivation as a Lunii — no family-conditional
    // code: the section, the cards and the panel chip all follow.
    const supportedFlam = JSON.parse(
      '{"kind":"supported","family":"flam","firmwareCohort":"flamGen1",' +
        '"deviceIdentifier":"fedcba9876543210fedcba9876543210",' +
        '"supportedOperations":{"readLibrary":true,"inspectStory":true,' +
        '"importStory":true,"writeStory":false}}',
    ) as ConnectedDeviceDto;
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedFlam);
    mockDeviceLibrary.mockResolvedValue({
      kind: "readable",
      deviceIdentifier: "fedcba9876543210fedcba9876543210",
      stories: [
        {
          uuid: "12345678-9abc-def0-1122-334455667788",
          shortId: "55667788",
          hidden: false,
          contentPresent: true,
          alreadyImported: false,
          title: null,
          titleSource: null,
          thumbnail: null,
        },
      ],
    });
    renderLibrary();

    // The panel renders the ready chip through hasAnyCapability.
    await screen.findByText(/appareil prêt — flam/i);

    // The device-library section lights up exactly like a Lunii one.
    const main = screen.getByRole("main", { name: /collection d'histoires/i });
    const deviceRegion = await within(main).findByRole("region", {
      name: /bibliothèque de l'appareil/i,
    });
    expect(within(deviceRegion).getByText("55667788")).toBeInTheDocument();
  });

  it("shows a recoverable device-library error in the center without breaking the local library (AC #3)", async () => {
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockRejectedValue({
      code: "DEVICE_SCAN_FAILED",
      message: "Lecture de la bibliothèque appareil indisponible.",
      userAction: "Vérifie la connexion de la Lunii puis réessaie.",
      details: { source: "fs_read", kind: "not_found" },
    });
    renderLibrary();

    // The local library stays intact and usable.
    await screen.findByRole("button", { name: /le soleil/i });

    // The device-library failure is surfaced IN CONTEXT (center column),
    // recoverable, never a toast.
    const main = screen.getByRole("main", { name: /collection d'histoires/i });
    const alert = await within(main).findByRole("alert");
    expect(alert).toHaveTextContent(/bibliothèque de l'appareil indisponible/i);
    expect(
      within(alert).getByRole("button", { name: /réessayer/i }),
    ).toBeInTheDocument();
  });

  // --- Device story inspection (select before import) ---

  const readableTwo = {
    kind: "readable" as const,
    deviceIdentifier: supportedV3.deviceIdentifier,
    stories: [
      {
        uuid: "u1",
        shortId: "0000ABCD",
        hidden: false,
        contentPresent: true,
        alreadyImported: false,
        title: null,
        titleSource: null,
        thumbnail: null,
      },
      {
        uuid: "u2",
        shortId: "0000BEEF",
        hidden: false,
        contentPresent: true,
        alreadyImported: false,
        title: null,
        titleSource: null,
        thumbnail: null,
      },
    ],
  };

  it("selecting a device story opens the right-column inspector with its identity + provenance (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    const card = await within(main).findByRole("button", {
      name: /identifiant 0000abcd/i,
    });
    // No inspector until something is selected.
    expect(
      screen.queryByRole("region", { name: /histoire sélectionnée/i }),
    ).not.toBeInTheDocument();

    await user.click(card);

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const inspector = within(panel).getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(within(inspector).getByText("0000ABCD")).toBeInTheDocument();
    expect(
      within(inspector).getByText(/pas encore dans ta bibliothèque locale/i),
    ).toBeInTheDocument();
    expect(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("changing the device selection updates the targeted story in the panel (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(
        within(panel).getByRole("region", { name: /histoire sélectionnée/i }),
      ).getByText("0000ABCD"),
    ).toBeInTheDocument();

    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    const inspector = within(panel).getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    expect(within(inspector).getByText("0000BEEF")).toBeInTheDocument();
    expect(within(inspector).queryByText("0000ABCD")).not.toBeInTheDocument();
  });

  it("device-story selection is independent from the local selection — they never merge", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    // Select a LOCAL story first.
    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByText(/^1 histoire sélectionnée$/i),
    ).toBeInTheDocument();

    // Now select a DEVICE story — the two selections coexist.
    const main = screen.getByRole("main", { name: /collection d'histoires/i });
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );

    expect(useLibraryShell.getState().selectedStoryIds.has("s1")).toBe(true);
    expect(
      within(panel).getByText(/^1 histoire sélectionnée$/i),
    ).toBeInTheDocument();
    expect(
      within(panel).getByRole("region", { name: /histoire sélectionnée/i }),
    ).toBeInTheDocument();
  });

  it("clicking the selected device card again clears the inspector (explicit toggle, AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    expect(
      screen.getByRole("region", { name: /histoire sélectionnée/i }),
    ).toBeInTheDocument();

    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    expect(
      screen.queryByRole("region", { name: /histoire sélectionnée/i }),
    ).not.toBeInTheDocument();
  });

  it("does not make device cards selectable when inspectStory is not authorized", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValue({
      ...supportedV3,
      supportedOperations: {
        readLibrary: true,
        inspectStory: false,
        importStory: false,
        writeStory: false,
      },
    });
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    // readLibrary=true → the inventory still lists the entries…
    await within(main).findByText("0000ABCD");
    // …but inspectStory=false → cards are NOT selectable and no inspector.
    expect(
      within(main).queryByRole("button", { name: /identifiant 0000abcd/i }),
    ).toBeNull();
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).queryByRole("region", { name: /histoire sélectionnée/i }),
    ).toBeNull();
  });

  it("clears the device-story inspection when the device goes away (purge, AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice
      .mockResolvedValueOnce(supportedV3)
      .mockResolvedValueOnce({ kind: "none" });
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByRole("region", { name: /histoire sélectionnée/i }),
    ).toBeInTheDocument();

    // The next detection finds no device → the inspection must clear, never
    // dangle on a device that is gone.
    await user.click(
      within(panel).getByRole("button", { name: /réessayer la détection/i }),
    );
    await waitFor(() =>
      expect(
        within(panel).queryByRole("region", {
          name: /histoire sélectionnée/i,
        }),
      ).not.toBeInTheDocument(),
    );
  });

  // --- Device story import (Copier dans ma bibliothèque) ---

  const supportedOrigine = {
    kind: "supported" as const,
    family: "lunii" as const,
    firmwareCohort: "origineV1" as const,
    metadataFormatVersion: 3,
    deviceIdentifier: "0123456789abcdef0123456789abcdef",
    supportedOperations: {
      readLibrary: true,
      inspectStory: true,
      importStory: true,
      writeStory: false,
    },
  };

  const importOutcome = {
    story: { id: "local-1", title: "Histoire de ma Lunii (0000ABCD)" },
    packShortId: "0000ABCD",
    importedAt: "2026-06-10T12:00:00.000Z",
  };

  it("keeps the copy CTA soft-disabled with the profile reason when importStory is gated off (V3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockDevice.mockResolvedValue(supportedV3); // importStory: false
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    const cta = within(inspector).getByRole("button", {
      name: /copier dans ma bibliothèque/i,
    });
    expect(cta).toHaveAttribute("aria-disabled", "true");
    expect(
      within(inspector).getByText(/profil non supporté/i),
    ).toBeInTheDocument();

    // A soft-disabled CTA swallows the activation — no IPC fires.
    await user.click(cta);
    expect(mockImport).not.toHaveBeenCalled();
  });

  it("copies a device story: authoritative re-reads on both sides, preserved selection, flipped CTA (AC1+AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    const refreshedInventory = {
      ...readableTwo,
      stories: [
        { ...readableTwo.stories[0], alreadyImported: true },
        readableTwo.stories[1],
      ],
    };
    mockDeviceLibrary
      .mockResolvedValueOnce(readableTwo)
      .mockResolvedValue(refreshedInventory);
    mockImport.mockResolvedValue(importOutcome);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    const overviewCallsBefore = mockGet.mock.calls.length;
    const inventoryCallsBefore = mockDeviceLibrary.mock.calls.length;

    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );

    // The command received exactly the two identifiers the route holds.
    expect(mockImport).toHaveBeenCalledWith({
      deviceIdentifier: supportedOrigine.deviceIdentifier,
      packUuid: "u1",
    });

    // Sober in-context success with the created title.
    // Twice by design: the always-mounted polite region + the visible chip.
    expect(
      await screen.findAllByText("Histoire copiée dans ta bibliothèque"),
    ).toHaveLength(2);
    expect(
      screen.getByText("Histoire de ma Lunii (0000ABCD)"),
    ).toBeInTheDocument();

    // Both authoritative re-reads fired (local overview + device inventory).
    await waitFor(() =>
      expect(mockGet.mock.calls.length).toBeGreaterThan(overviewCallsBefore),
    );
    await waitFor(() =>
      expect(mockDeviceLibrary.mock.calls.length).toBeGreaterThan(
        inventoryCallsBefore,
      ),
    );

    // The device card now carries the local-copy marker, stays SELECTED
    // (a copy is not a move), and the CTA flips to the new reason.
    await waitFor(() =>
      expect(
        within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
      ).toHaveAttribute("aria-pressed", "true"),
    );
    expect(
      within(main).getAllByText("Dans ta bibliothèque").length,
    ).toBeGreaterThan(0);
    await waitFor(() =>
      expect(
        within(inspector).getByRole("button", {
          name: /copier dans ma bibliothèque/i,
        }),
      ).toHaveAttribute("aria-disabled", "true"),
    );
    expect(
      within(inspector).getByText(
        "Copie indisponible: déjà dans ta bibliothèque",
      ),
    ).toBeInTheDocument();
    // The provenance note follows the local truth too (F4): never
    // "pas encore" on a story whose copy exists.
    expect(
      within(inspector).getByText(
        /vit sur l'appareil et une copie existe déjà/i,
      ),
    ).toBeInTheDocument();
  });

  it("keeps the import status attached to the copied pack — never on another card", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    mockImport.mockResolvedValue(importOutcome);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    // Copy pack A.
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );
    expect(
      await screen.findAllByText("Histoire copiée dans ta bibliothèque"),
    ).toHaveLength(2);

    // Select pack B: ITS status is idle — A's success must not follow.
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    expect(
      screen.queryByText("Histoire copiée dans ta bibliothèque"),
    ).not.toBeInTheDocument();

    // Re-select pack A: its status surfaces again (still held by the hook).
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    expect(
      screen.getAllByText("Histoire copiée dans ta bibliothèque"),
    ).toHaveLength(2);
  });

  it("never attaches A's success to B when B is clicked while A's copy is still in flight", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    // Hold A's copy in flight so the second click lands during the copy.
    let resolveA!: (value: typeof importOutcome) => void;
    mockImport.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveA = resolve;
      }),
    );
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    // Start copying pack A.
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    await user.click(
      within(
        screen.getByRole("region", { name: /histoire sélectionnée/i }),
      ).getByRole("button", { name: /copier dans ma bibliothèque/i }),
    );

    // While A is still copying, select B and click its (active) CTA: the
    // hook swallows the re-entrant trigger and the target stays on A.
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    await user.click(
      within(
        screen.getByRole("region", { name: /histoire sélectionnée/i }),
      ).getByRole("button", { name: /copier dans ma bibliothèque/i }),
    );
    expect(mockImport).toHaveBeenCalledTimes(1);

    // A resolves. B is still the selected card — its inspector must NOT
    // inherit A's success (that was the mis-attachment bug).
    await act(async () => {
      resolveA(importOutcome);
    });
    expect(
      screen.queryByText("Histoire copiée dans ta bibliothèque"),
    ).not.toBeInTheDocument();

    // Re-select A: its success is the one that surfaces.
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    expect(
      await screen.findAllByText("Histoire copiée dans ta bibliothèque"),
    ).toHaveLength(2);
  });

  it("surfaces a copy failure in-context with Réessayer, local library intact (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [{ id: "s1", title: "Le soleil" }] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    mockImport.mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Copie impossible: lecture de l'appareil interrompue.",
      userAction: "Vérifie la connexion de la Lunii puis réessaie la copie.",
      details: { source: "fs_read", kind: "not_found" },
    });
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );

    const alert = await within(inspector).findByRole("alert");
    expect(alert).toHaveTextContent("Copie impossible");
    expect(alert).toHaveTextContent(/lecture de l'appareil interrompue/i);
    // Réessayer re-enters the boundary with the same identifiers.
    mockImport.mockResolvedValueOnce(importOutcome);
    await user.click(within(alert).getByRole("button", { name: /réessayer/i }));
    // Twice by design: the always-mounted polite region + the visible chip.
    expect(
      await screen.findAllByText("Histoire copiée dans ta bibliothèque"),
    ).toHaveLength(2);
    expect(mockImport).toHaveBeenCalledTimes(2);

    // The LOCAL library stayed intact and usable throughout.
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  const importFailure = {
    code: "IMPORT_FAILED" as const,
    message: "Copie impossible: lecture de l'appareil interrompue.",
    userAction: "Vérifie la connexion de la Lunii puis réessaie la copie.",
    details: { source: "fs_read", kind: "not_found" },
  };

  it("keeps a copy failure attached to its pack across selection changes (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    mockImport.mockRejectedValueOnce(importFailure);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    // Copy pack A → it fails in-context.
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    let inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );
    expect(await within(inspector).findByRole("alert")).toHaveTextContent(
      "Copie impossible",
    );

    // Select pack B: its status is idle — A's failure must NOT follow it.
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    // Re-select pack A: its failure surfaces again (held, attached to the pack).
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    inspector = screen.getByRole("region", { name: /histoire sélectionnée/i });
    expect(within(inspector).getByRole("alert")).toHaveTextContent(
      "Copie impossible",
    );
  });

  it("dismisses a copy failure only on an explicit Fermer, never on a selection change (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    mockImport.mockRejectedValueOnce(importFailure);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    let inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );
    await within(inspector).findByRole("alert");

    // A→B→A: the alert survives the round-trip (a selection change never wipes it).
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    inspector = screen.getByRole("region", { name: /histoire sélectionnée/i });
    expect(within(inspector).getByRole("alert")).toBeInTheDocument();

    // The explicit Fermer DOES dismiss it…
    await user.click(
      within(inspector).getByRole("button", { name: /fermer/i }),
    );
    expect(within(inspector).queryByRole("alert")).not.toBeInTheDocument();

    // …and it stays gone after another A→B→A (now genuinely idle).
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000beef/i }),
    );
    await user.click(
      within(main).getByRole("button", { name: /identifiant 0000abcd/i }),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("never surfaces a copy failure in a toast — only an in-context alert (AC3 / UX-DR15)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedOrigine);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    mockImport.mockRejectedValueOnce(importFailure);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    await user.click(
      within(inspector).getByRole("button", {
        name: /copier dans ma bibliothèque/i,
      }),
    );
    const alert = await within(inspector).findByRole("alert");
    expect(alert).toHaveTextContent("Copie impossible");
    // The critical error lives in an alert, never a polite toast (role=status).
    screen
      .queryAllByRole("status")
      .forEach((s) => expect(s).not.toHaveTextContent(/copie impossible/i));
  });

  it("offers the support-profile next gesture in the inspector when a V3 copy is gated off (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValue({ stories: [] });
    mockDevice.mockResolvedValue(supportedV3);
    mockDeviceLibrary.mockResolvedValue(readableTwo);
    renderLibrary();

    const main = await screen.findByRole("main", {
      name: /collection d'histoires/i,
    });
    await user.click(
      await within(main).findByRole("button", {
        name: /identifiant 0000abcd/i,
      }),
    );
    const inspector = screen.getByRole("region", {
      name: /histoire sélectionnée/i,
    });
    // The copy is gated off for V3 with the canonical reason…
    expect(
      within(inspector).getByText(/copie indisponible: profil non supporté/i),
    ).toBeInTheDocument();
    // …and the inspector exposes the next gesture (parity with the panel).
    expect(
      within(inspector).getByRole("button", {
        name: /consulter le profil de support officiel/i,
      }),
    ).toBeInTheDocument();
  });

  // --- Pre-transfer comparison (read-only, AC1 + AC2 + AC3) ---

  const readyNew = {
    kind: "ready" as const,
    deviceIdentifier: supportedV3.deviceIdentifier,
    story: { id: "s1", title: "Le soleil" },
    onDevice: false,
    unchangedCount: 2,
    transferable: false,
  };

  it("renders the pre-send comparison when one local story is selected against a readable device (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockTransferPreview.mockResolvedValue(readyNew);
    renderLibrary();

    // No comparison before a single local story is selected.
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const comparison = within(panel).getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    expect(comparison).toHaveTextContent(/sélectionne une histoire locale/i);

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));

    await waitFor(() =>
      expect(comparison).toHaveTextContent(/nouvelle sur l'appareil/i),
    );
    expect(comparison).toHaveTextContent(/serait ajoutée à l'appareil/i);
    expect(comparison).toHaveTextContent(
      /2 autres histoires.*resteront inchangées/i,
    );
  });

  it("shows the replacement verdict when the selected story's pack is on the device (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockTransferPreview.mockResolvedValue({
      ...readyNew,
      onDevice: true,
      unchangedCount: 1,
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));

    const comparison = within(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).getByRole("region", { name: /comparaison avant envoi/i });
    await waitFor(() =>
      expect(comparison).toHaveTextContent(/déjà présente sur l'appareil/i),
    );
    expect(comparison).toHaveTextContent(/un envoi la remplacerait/i);
    expect(comparison).toHaveTextContent(
      /1 autre histoire.*restera inchangée/i,
    );
  });

  it("keeps the send CTA disabled even when the comparison is ready (AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockTransferPreview.mockResolvedValue(readyNew);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /comparaison avant envoi/i }),
      ).toHaveTextContent(/nouvelle sur l'appareil/i),
    );

    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(send).toHaveAttribute("aria-disabled", "true");
  });

  it("shows no comparison without exactly one local selection (AC3 sober state)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockTransferPreview.mockResolvedValue(readyNew);
    renderLibrary();

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const comparison = within(panel).getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    // 0 selections → sober hint, never a verdict.
    expect(comparison).toHaveTextContent(/sélectionne une histoire locale/i);

    // Select two → still no verdict (multi-transfer is out of scope), with a
    // distinct "narrow to one" hint.
    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("button", { name: /la lune/i }));
    await user.keyboard("{/Control}");

    expect(comparison).toHaveTextContent(
      /sélectionne une seule histoire locale/i,
    );
    expect(comparison).not.toHaveTextContent(/nouvelle sur l'appareil/i);
    expect(comparison).not.toHaveTextContent(/déjà présente sur l'appareil/i);
  });

  it("distinguishes the no-device hint when one story is selected but no readable device (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    // Device present but NOT read-authorized → no readable device id.
    mockDevice.mockResolvedValue({
      ...supportedV3,
      supportedOperations: {
        readLibrary: false,
        inspectStory: false,
        importStory: false,
        writeStory: false,
      },
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const comparison = within(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).getByRole("region", { name: /comparaison avant envoi/i });
    expect(comparison).toHaveTextContent(/branche une lunii lisible/i);
    // The preview must not have fired without a readable device.
    expect(mockTransferPreview).not.toHaveBeenCalled();
  });

  it("surfaces a comparison failure in-context without breaking the local library (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockTransferPreview.mockRejectedValue({
      code: "DEVICE_SCAN_FAILED",
      message: "Comparaison indisponible: l'appareil connecté a changé.",
      userAction: "Rebranche la Lunii souhaitée puis réessaie.",
      details: { source: "device_changed" },
    });
    renderLibrary();

    // The LOCAL library stays intact and usable.
    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();

    // The comparison failure is in-context (role="alert" inside the panel),
    // never a toast.
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const comparison = within(panel).getByRole("region", {
      name: /comparaison avant envoi/i,
    });
    const alert = await within(comparison).findByRole("alert");
    expect(alert).toHaveTextContent(/l'appareil connecté a changé/i);

    // The "Réessaie la comparaison" copy is actionable: a retry CTA re-reads.
    mockTransferPreview.mockResolvedValue(readyNew);
    await user.click(
      within(comparison).getByRole("button", {
        name: /réessayer la comparaison/i,
      }),
    );
    await waitFor(() =>
      expect(comparison).toHaveTextContent(/nouvelle sur l'appareil/i),
    );
  });

  it("shows a recoverable device-changed comparison (not the no-device hint) when a readable device folds (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3); // a readable device IS detected…
    mockTransferPreview.mockResolvedValue({ kind: "noDevice" }); // …but the re-read folds
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const comparison = within(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).getByRole("region", { name: /comparaison avant envoi/i });

    const alert = await within(comparison).findByRole("alert");
    expect(alert).toHaveTextContent(
      /l'appareil a changé pendant la comparaison/i,
    );
    // It must NOT fall back to the misleading "plug a Lunii" hint.
    expect(comparison).not.toHaveTextContent(/branche une lunii lisible/i);
    // …and the retry CTA is offered (reuses the wired refresh).
    expect(
      within(comparison).getByRole("button", {
        name: /réessayer la comparaison/i,
      }),
    ).toBeInTheDocument();
  });

  // --- Pre-transfer validation verdict (read-only, AC1 + AC2 + AC3) ---

  const blockedValidation = {
    kind: "ready" as const,
    deviceIdentifier: supportedV3.deviceIdentifier,
    story: { id: "s1", title: "Le soleil" },
    verdict: "blocked" as const,
    blockers: [
      {
        axis: "structure" as const,
        cause: "checksumMismatch" as const,
        message: "Les données locales de l'histoire ont changé.",
        userAction: "Restaure une sauvegarde saine de l'histoire.",
      },
    ],
  };

  it("renders the validation verdict when one local story is selected against a readable device (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue(blockedValidation);
    renderLibrary();

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const validation = within(panel).getByRole("region", {
      name: /validation avant envoi/i,
    });
    // Sober before a single local story is selected.
    expect(validation).toHaveTextContent(
      /vérifier la compatibilité avant l'envoi/i,
    );

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));

    await waitFor(() => expect(validation).toHaveTextContent(/bloquée/i));
    // The blocker's message + next gesture are rendered verbatim (AC2).
    expect(validation).toHaveTextContent(
      /les données locales de l'histoire ont changé/i,
    );
    expect(validation).toHaveTextContent(/restaure une sauvegarde saine/i);
  });

  it("keeps the send CTA disabled even when the verdict is présumée transférable (AC3/FR34)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue({
      ...blockedValidation,
      verdict: "presumedTransferable",
      blockers: [],
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /validation avant envoi/i }),
      ).toHaveTextContent(/présumée transférable/i),
    );
    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(send).toHaveAttribute("aria-disabled", "true");
  });

  it("shows no validation without exactly one local selection (AC3 sober state)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue(blockedValidation);
    renderLibrary();

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const validation = within(panel).getByRole("region", {
      name: /validation avant envoi/i,
    });
    expect(validation).toHaveTextContent(/vérifier la compatibilité/i);

    // Two selected → still sober (multi-transfer is out of scope), no verdict.
    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("button", { name: /la lune/i }));
    await user.keyboard("{/Control}");

    expect(validation).toHaveTextContent(/vérifier la compatibilité/i);
    expect(validation).not.toHaveTextContent(/bloquée/i);
  });

  it("does not fire validation without a readable device (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue({
      ...supportedV3,
      supportedOperations: {
        readLibrary: false,
        inspectStory: false,
        importStory: false,
        writeStory: false,
      },
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const validation = within(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).getByRole("region", { name: /validation avant envoi/i });
    expect(validation).toHaveTextContent(/vérifier la compatibilité/i);
    expect(mockStoryValidation).not.toHaveBeenCalled();
  });

  it("surfaces a validation failure in-context without breaking the local library (orthogonality, AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockRejectedValue({
      code: "DEVICE_SCAN_FAILED",
      message: "L'appareil a changé pendant la validation.",
      userAction: "Vérifie que la Lunii est branchée puis réessaie.",
      details: { source: "device_changed" },
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const validation = within(
      screen.getByRole("complementary", { name: /panneau de décision/i }),
    ).getByRole("region", { name: /validation avant envoi/i });

    const alert = await within(validation).findByRole("alert");
    expect(alert).toHaveTextContent(
      /l'appareil a changé pendant la validation/i,
    );
    // Orthogonality: the LOCAL library stays intact and usable.
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  // --- Pre-transfer preparation surface (route-level, T10) ---

  const presumedTransferableValidation = {
    ...blockedValidation,
    verdict: "presumedTransferable" as const,
    blockers: [],
  };

  it("offers an active Préparer CTA for a présumée-transférable selection and triggers the preparation (T10)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const prep = within(panel).getByRole("region", { name: /^préparation$/i });
    const cta = within(prep).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(cta).not.toHaveAttribute("aria-disabled", "true"),
    );

    await user.click(cta);
    expect(mockStartPrepare).toHaveBeenCalledWith({
      storyId: "s1",
      deviceIdentifier: supportedV3.deviceIdentifier,
    });
  });

  it("keeps the library usable while a preparation is in flight (T10/AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    // The catch-up re-read stays idle so the panel holds the in-flight phase.
    mockReadPreparation.mockResolvedValue({ kind: "idle" });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const cta = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(cta).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(cta);

    // The preparation surface shows the in-flight phase IN the panel...
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /^préparation$/i }),
      ).toHaveTextContent(/en vérification/i),
    );
    // ...and the center-column library stays rendered + usable (both cards).
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /la lune/i }),
    ).toBeInTheDocument();
  });

  it("surfaces a preparation failure in-context and leaves the local library intact (T10/AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    // The authoritative re-read folds to a recoverable failure.
    mockReadPreparation.mockResolvedValue({
      kind: "retryable",
      story: { id: "s1", title: "Le soleil" },
      cause: "artifactMissing",
      message:
        "Préparation impossible : un fichier nécessaire est introuvable.",
      userAction: "Vérifie l'histoire locale puis relance la préparation.",
      blockers: [],
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const cta = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(cta).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(cta);

    const prep = within(panel).getByRole("region", { name: /^préparation$/i });
    await waitFor(() => expect(prep).toHaveTextContent(/échec récupérable/i));
    // In-context recovery (never a toast), and the local library stays intact.
    expect(
      within(prep).getByRole("button", { name: /relancer la préparation/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  // --- Pure mapper unit tests (mapStoryValidationToView) ---

  it("mapStoryValidationToView maps a folded idle to the sober none state", () => {
    expect(mapStoryValidationToView({ kind: "idle" })).toEqual({
      kind: "none",
    });
  });

  it("mapStoryValidationToView forwards the verdict + blockers on ready", () => {
    const blockers = [
      {
        axis: "deviceProfile" as const,
        cause: "metadataUnsupported" as const,
        message: "Profil non pris en charge.",
        userAction: "Consulte le profil de support.",
      },
    ];
    expect(
      mapStoryValidationToView({
        kind: "ready",
        verdict: "blocked",
        blockers,
        storyTitle: "X",
      }),
    ).toEqual({ kind: "ready", verdict: "blocked", blockers });
  });

  it("mapStoryValidationToView forwards the error on error", () => {
    const error = {
      code: "DEVICE_SCAN_FAILED" as const,
      message: "L'appareil a changé.",
      userAction: "Réessaie.",
      details: null,
    };
    expect(mapStoryValidationToView({ kind: "error", error })).toEqual({
      kind: "error",
      error,
    });
  });

  // --- Pure mapper unit tests (mapTransferPreviewToComparison) ---

  it("mapTransferPreviewToComparison maps a folded idle to the no-device hint", () => {
    expect(mapTransferPreviewToComparison({ kind: "idle" })).toEqual({
      kind: "none",
      reason: "no-device",
    });
  });

  it("mapTransferPreviewToComparison forwards onDevice + unchangedCount on ready", () => {
    expect(
      mapTransferPreviewToComparison({
        kind: "ready",
        onDevice: true,
        unchangedCount: 3,
        storyTitle: "X",
        transferable: false,
      }),
    ).toEqual({ kind: "ready", onDevice: true, unchangedCount: 3 });
  });

  it("mapTransferPreviewToComparison forwards the error on error", () => {
    const error = {
      code: "LIBRARY_INCONSISTENT" as const,
      message: "Histoire introuvable.",
      userAction: "Recharge.",
      details: null,
    };
    expect(mapTransferPreviewToComparison({ kind: "error", error })).toEqual({
      kind: "error",
      error,
    });
  });

  // --- Pure mapper unit tests (mapDeviceForPanel) ---

  it("mapDeviceForPanel returns 'scanning' while the hook is loading", () => {
    expect(mapDeviceForPanel({ kind: "loading" }, true)).toMatchObject({
      deviceState: "scanning",
    });
  });

  it("mapDeviceForPanel returns 'scanning' when isRefreshing flips on top of a ready snapshot", () => {
    expect(
      mapDeviceForPanel({ kind: "ready", device: { kind: "none" } }, true),
    ).toMatchObject({ deviceState: "scanning" });
  });

  it("mapDeviceForPanel forwards the underlying error.userAction as deviceReason", () => {
    const mapped = mapDeviceForPanel(
      {
        kind: "error",
        error: {
          code: "DEVICE_SCAN_FAILED",
          message: "Détection indisponible.",
          userAction: "Réessaie la détection.",
          details: null,
        },
      },
      false,
    );
    expect(mapped.deviceState).toBe("error");
    expect(mapped.deviceReason).toBe("Réessaie la détection.");
  });

  it("mapDeviceForPanel renders the standard copy for a FLAM metadataUnsupported (never the family tag as a version)", () => {
    // An incomplete FLAM structure (str/ or etc/ missing) carries the
    // family tag "flam" as firmwareHint — it must NOT read as a fake
    // version ("format métadonnées flam non géré").
    const mapped = mapDeviceForPanel(
      {
        kind: "ready",
        device: {
          kind: "unsupported",
          reason: "metadataUnsupported",
          firmwareHint: "flam",
        },
      },
      false,
    );
    expect(mapped.deviceState).toBe("unsupported");
    expect(mapped.deviceReason).toBe(
      "Profil non supporté: format métadonnées non géré",
    );
  });

  it("mapDeviceForPanel still renders a genuine metadata version hint as a version", () => {
    const mapped = mapDeviceForPanel(
      {
        kind: "ready",
        device: {
          kind: "unsupported",
          reason: "metadataUnsupported",
          firmwareHint: "metadata_v99",
        },
      },
      false,
    );
    expect(mapped.deviceReason).toBe(
      "Profil non supporté: format métadonnées v99 non géré",
    );
  });

  it("mapDeviceForPanel maps a supported FLAM DTO to idle with the FLAM label, family and its matrix", () => {
    // Real wire: the metadataFormatVersion key is absent for FLAM, and
    // the matrix line carries the activated read capabilities.
    const flamDto = JSON.parse(
      '{"kind":"supported","family":"flam","firmwareCohort":"flamGen1",' +
        '"deviceIdentifier":"fedcba9876543210fedcba9876543210",' +
        '"supportedOperations":{"readLibrary":true,"inspectStory":true,' +
        '"importStory":true,"writeStory":false}}',
    ) as ConnectedDeviceDto;
    const mapped = mapDeviceForPanel({ kind: "ready", device: flamDto }, false);
    expect(mapped.deviceState).toBe("idle");
    expect(mapped.deviceLabel).toBe("FLAM");
    expect(mapped.deviceFamily).toBe("flam");
    expect(mapped.supportedOperations).toEqual({
      readLibrary: true,
      inspectStory: true,
      importStory: true,
      writeStory: false,
    });
  });

  // --- Pure mapper unit tests (mapPreparationView) ---

  const presumedTransferable: ReturnType<typeof mapStoryValidationToView> = {
    kind: "ready",
    verdict: "presumedTransferable",
    blockers: [],
  };

  const STORY_A = "0197a5d0-0000-7000-8000-00000000000a";
  const STORY_B = "0197a5d0-0000-7000-8000-00000000000b";

  it("mapPreparationView enables Préparer for a single selection + readable device + présumée transférable", () => {
    expect(
      mapPreparationView(
        { kind: "idle" },
        STORY_A,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({ kind: "ready" });
  });

  it("mapPreparationView disables Préparer with the selection reasons", () => {
    expect(
      mapPreparationView(
        { kind: "idle" },
        null,
        0,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({
      kind: "unavailable",
      reason: "Préparation indisponible: aucune histoire sélectionnée",
    });
    expect(
      mapPreparationView(
        { kind: "idle" },
        null,
        2,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({
      kind: "unavailable",
      reason: "Préparation indisponible: sélection multiple",
    });
  });

  it("mapPreparationView disables Préparer with the device reasons", () => {
    expect(
      mapPreparationView(
        { kind: "idle" },
        STORY_A,
        1,
        "absent",
        presumedTransferable,
      ),
    ).toEqual({
      kind: "unavailable",
      reason: "Préparation indisponible: aucun appareil connecté",
    });
    expect(
      mapPreparationView(
        { kind: "idle" },
        STORY_A,
        1,
        "unsupported",
        presumedTransferable,
      ),
    ).toEqual({
      kind: "unavailable",
      reason: "Préparation indisponible: profil non supporté",
    });
  });

  it("mapPreparationView disables Préparer with 'corrige les blocages d'abord' when the verdict is not présumée transférable", () => {
    const validations: ReturnType<typeof mapStoryValidationToView>[] = [
      { kind: "ready", verdict: "blocked", blockers: [] },
      { kind: "ready", verdict: "toFix", blockers: [] },
      { kind: "loading" },
      { kind: "none" },
    ];
    for (const validation of validations) {
      expect(
        mapPreparationView(
          { kind: "idle" },
          STORY_A,
          1,
          "readable",
          validation,
        ),
      ).toEqual({
        kind: "unavailable",
        reason: "Préparation indisponible: corrige les blocages d'abord",
      });
    }
  });

  it("mapPreparationView shows the active / terminal state ONLY for the selected target story", () => {
    expect(
      mapPreparationView(
        { kind: "preflight", storyId: STORY_A },
        STORY_A,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({ kind: "preflight" });
    expect(
      mapPreparationView(
        { kind: "preparing", storyId: STORY_A, progress: null },
        STORY_A,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({ kind: "preparing", progress: null });
    expect(
      mapPreparationView(
        {
          kind: "prepared",
          storyId: STORY_A,
          transferable: true,
          deviceIdentifier: "0123456789abcdef0123456789abcdef",
        },
        STORY_A,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({ kind: "prepared" });
    expect(
      mapPreparationView(
        {
          kind: "retryable",
          storyId: STORY_A,
          message: "Échec.",
          userAction: "Relance.",
          blockers: [],
        },
        STORY_A,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({
      kind: "retryable",
      message: "Échec.",
      userAction: "Relance.",
      blockers: [],
    });
  });

  it("mapPreparationView shows the CURRENT selection's gate when an active job targets another story (F4)", () => {
    // Story A is preparing; the user selected story B (présumée transférable):
    // the panel shows B's `ready` gate — A's job stays consultable via its badge.
    expect(
      mapPreparationView(
        { kind: "preparing", storyId: STORY_A, progress: null },
        STORY_B,
        1,
        "readable",
        presumedTransferable,
      ),
    ).toEqual({ kind: "ready" });
  });

  // --- Transfer (real device write) — route flow + pure mapper (T10) ---

  const writableOrigine = {
    ...supportedOrigine,
    supportedOperations: {
      ...supportedOrigine.supportedOperations,
      writeStory: true,
    },
  };

  const preparedReread = {
    kind: "prepared" as const,
    deviceIdentifier: writableOrigine.deviceIdentifier,
    story: { id: "s1", title: "Le soleil" },
    targetCohort: "origine_v1",
    transferable: true,
  };

  it("activates the Envoyer CTA on a writable cohort once the story is Préparée, then triggers the transfer (T10/AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    mockReadPreparation.mockResolvedValue(preparedReread);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });

    // Before preparing, the single send CTA is gated on "prépare l'histoire d'abord".
    const sendBefore = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    await waitFor(() =>
      expect(sendBefore).toHaveAttribute("aria-disabled", "true"),
    );
    expect(
      document.getElementById(
        sendBefore.getAttribute("aria-describedby") as string,
      ),
    ).toHaveTextContent(/prépare l'histoire d'abord/i);

    // Prepare the story → it becomes Préparée.
    const prepare = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(prepare).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(prepare);
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /^préparation$/i }),
      ).toHaveTextContent(/préparée/i),
    );

    // Now the send CTA is active; clicking it starts the transfer (no modal).
    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    await waitFor(() =>
      expect(send).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(send);
    expect(mockStartTransfer).toHaveBeenCalledWith({
      storyId: "s1",
      deviceIdentifier: writableOrigine.deviceIdentifier,
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // --- Durable transfer memory: re-hydration / relaunch / abandon ---

  const rememberedFailure = {
    storyId: "s1",
    terminalKind: "retryable" as const,
    cause: "deviceChanged" as const,
    message: "Envoi interrompu : l'appareil connecté a changé.",
    userAction: "Rebranche la Lunii souhaitée puis relance l'envoi.",
    recordedAt: "2026-06-23T00:00:00.000Z",
  };

  it("re-hydrates a remembered recoverable failure for the selected story with Relancer + Abandonner and a card badge (AC2/AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockReadTransferOutcome.mockResolvedValue(rememberedFailure);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });

    // The remembered terminal is re-offered in-context, exactly as if the
    // `job:failed` had just fired — surviving a restart / re-visit.
    await waitFor(() =>
      expect(
        within(panel).getByText(/l'appareil connecté a changé/i),
      ).toBeInTheDocument(),
    );
    expect(mockReadTransferOutcome).toHaveBeenCalledWith({ storyId: "s1" });
    expect(
      within(panel).getByRole("button", { name: "Relancer le transfert" }),
    ).toBeInTheDocument();
    expect(
      within(panel).getByRole("button", { name: "Abandonner le transfert" }),
    ).toBeInTheDocument();
    // The StoryCard badge reflects the remembered issue (the persistent anchor).
    const card = screen.getByRole("button", { name: /le soleil/i });
    expect(within(card).getByText(/échec récupérable/i)).toBeInTheDocument();
  });

  it("Relancer on a re-hydrated terminal restarts a full cycle with the FRESH writable device id (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockReadTransferOutcome.mockResolvedValue(rememberedFailure);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const relancer = await within(panel).findByRole("button", {
      name: "Relancer le transfert",
    });
    await user.click(relancer);

    // A relaunch is a full new cycle through the send path, with the CURRENT
    // writable device id — never the stored (now-stale) identifier.
    expect(mockStartTransfer).toHaveBeenCalledWith({
      storyId: "s1",
      deviceIdentifier: writableOrigine.deviceIdentifier,
    });
  });

  it("Abandonner on a re-hydrated terminal purges the durable memory and clears the panel terminal (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockReadTransferOutcome.mockResolvedValue({
      ...rememberedFailure,
      terminalKind: "incomplete" as const,
      cause: "writeRejected" as const,
      message:
        "Envoi incomplet : l'appareil peut contenir une copie partielle.",
      userAction: "Relance l'envoi pour rétablir un état sûr.",
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const abandon = await within(panel).findByRole("button", {
      name: "Abandonner le transfert",
    });
    await user.click(abandon);

    expect(mockDiscardTransferOutcome).toHaveBeenCalledWith({ storyId: "s1" });
    await waitFor(() =>
      expect(within(panel).queryByText(/copie partielle/i)).toBeNull(),
    );
  });

  it("never re-hydrates a remembered verified as a live success when the device read is idle (no false success)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockReadTransfer.mockResolvedValue({ kind: "idle" });
    mockReadTransferOutcome.mockResolvedValue({
      storyId: "s1",
      terminalKind: "verified" as const,
      message: "« Le soleil » est maintenant sur la Lunii.",
      userAction: "2 autres histoires de l'appareil restent inchangées.",
      summary: {
        changed: "« Le soleil » est maintenant sur la Lunii.",
        unchanged: "2 autres histoires de l'appareil restent inchangées.",
      },
      recordedAt: "2026-06-23T00:00:00.000Z",
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(mockReadTransferOutcome).toHaveBeenCalledWith({ storyId: "s1" }),
    );
    // A remembered success is NEVER promoted to a live `transférée et vérifiée`.
    expect(within(panel).queryByText(/transférée et vérifiée/i)).toBeNull();
  });

  it("re-hydration yields to a live verified — the device proves the pack over a remembered failure (F1/§2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    // Memory remembers a failure, but the connected device proves the pack present
    // + byte-faithful: the LIVE `verified` always wins (no stale failure over a real
    // success, no false success either way).
    mockReadTransferOutcome.mockResolvedValue(rememberedFailure);
    mockReadTransfer.mockResolvedValue({
      kind: "verified",
      deviceIdentifier: writableOrigine.deviceIdentifier,
      story: { id: "s1", title: "Le soleil" },
      summary: {
        changed: "« Le soleil » est maintenant sur la Lunii.",
        unchanged: "Aucune autre histoire de l'appareil n'a été modifiée.",
      },
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByText(/transférée et vérifiée/i),
      ).toBeInTheDocument(),
    );
    // The remembered failure is NOT shown — the live success superseded it.
    expect(
      within(panel).queryByText(/l'appareil connecté a changé/i),
    ).toBeNull();
  });

  it("offers the reconnect hint instead of an inert Relancer when no writable device is connected (C1 gate)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3); // readable but NOT writable
    mockReadTransferOutcome.mockResolvedValue(rememberedFailure);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() =>
      expect(
        within(panel).getByText(/l'appareil connecté a changé/i),
      ).toBeInTheDocument(),
    );
    // No writable device → the reconnect hint replaces an inert Relancer (C1);
    // Abandonner stays available.
    expect(
      within(panel).getByText(/rebranche la lunii pour relancer/i),
    ).toBeInTheDocument();
    expect(
      within(panel).queryByRole("button", { name: "Relancer le transfert" }),
    ).toBeNull();
    expect(
      within(panel).getByRole("button", { name: "Abandonner le transfert" }),
    ).toBeInTheDocument();
  });

  it("blocks the Envoyer CTA when the story was Préparée for a DIFFERENT device (re-prepare required, F6)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine); // connected writable device
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    // Prepared for ANOTHER device (≠ the currently-connected writable device): a
    // device swap must force a re-preparation before any send (no stale-descriptor
    // cross-device send — the same gate covers a V1/V2/V3 swap).
    mockReadPreparation.mockResolvedValue({
      ...preparedReread,
      deviceIdentifier: "ffffffffffffffffffffffffffffffff",
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const prepare = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(prepare).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(prepare);
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /^préparation$/i }),
      ).toHaveTextContent(/préparée/i),
    );

    // Préparée, but for another device → the send CTA stays disabled, asking to
    // (re-)prepare for the connected device. No write is ever started.
    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    expect(send).toHaveAttribute("aria-disabled", "true");
    expect(
      document.getElementById(send.getAttribute("aria-describedby") as string),
    ).toHaveTextContent(/prépare l'histoire d'abord/i);
    expect(mockStartTransfer).not.toHaveBeenCalled();
  });

  it("blocks the Envoyer CTA on a non-writable V3 cohort with 'profil non supporté' (T10/AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(supportedV3); // writeStory: false
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    await waitFor(() => {
      const send = within(panel).getByRole("button", {
        name: /envoyer vers la lunii/i,
      });
      expect(send).toHaveAttribute("aria-disabled", "true");
      expect(
        document.getElementById(
          send.getAttribute("aria-describedby") as string,
        ),
      ).toHaveTextContent(/profil non supporté/i);
    });
    expect(mockStartTransfer).not.toHaveBeenCalled();
  });

  it("keeps the library usable while a transfer is in flight (T10/AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    mockReadPreparation.mockResolvedValue(preparedReread);
    // The transfer catch-up re-read stays idle so the panel holds "en transfert".
    mockReadTransfer.mockResolvedValue({ kind: "idle" });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const prepare = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(prepare).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(prepare);
    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    await waitFor(() =>
      expect(send).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(send);

    // The transfer surface shows the in-flight phase IN the panel...
    await waitFor(() =>
      expect(
        within(panel).getByRole("region", { name: /^transfert$/i }),
      ).toHaveTextContent(/en transfert/i),
    );
    // ...and the center-column library stays rendered + usable (both cards).
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /la lune/i }),
    ).toBeInTheDocument();
  });

  it("surfaces a transfer failure in-context and leaves the local library intact (T10/AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [{ id: "s1", title: "Le soleil" }],
    });
    mockDevice.mockResolvedValue(writableOrigine);
    mockStoryValidation.mockResolvedValue(presumedTransferableValidation);
    mockReadPreparation.mockResolvedValue(preparedReread);
    // The authoritative transfer re-read folds to a recoverable failure.
    mockReadTransfer.mockResolvedValue({
      kind: "retryable",
      story: { id: "s1", title: "Le soleil" },
      cause: "interrupted",
      message: "Transfert interrompu : l'appareil a été retiré.",
      userAction: "Rebranche la Lunii puis relance l'envoi.",
    });
    renderLibrary();

    await user.click(await screen.findByRole("button", { name: /le soleil/i }));
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    const prepare = within(panel).getByRole("button", { name: /^préparer$/i });
    await waitFor(() =>
      expect(prepare).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(prepare);
    const send = within(panel).getByRole("button", {
      name: /envoyer vers la lunii/i,
    });
    await waitFor(() =>
      expect(send).not.toHaveAttribute("aria-disabled", "true"),
    );
    await user.click(send);

    const transfer = within(panel).getByRole("region", {
      name: /^transfert$/i,
    });
    await waitFor(() =>
      expect(transfer).toHaveTextContent(/échec récupérable/i),
    );
    // In-context recovery (never a toast), and the local library stays intact.
    expect(
      within(transfer).getByRole("button", { name: /relancer le transfert/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  // --- Pure mapper unit tests (mapTransferView) ---

  it("mapTransferView enables Envoyer for a single selection + writable device + Préparée transférable", () => {
    expect(
      mapTransferView({ kind: "idle" }, STORY_A, 1, "idle", true, true, true),
    ).toEqual({ kind: "ready" });
  });

  it("mapTransferView disables Envoyer with the selection reasons", () => {
    expect(
      mapTransferView({ kind: "idle" }, null, 0, "idle", true, true, true),
    ).toEqual({
      kind: "unavailable",
      reason: "Envoi indisponible: aucune histoire sélectionnée",
    });
    expect(
      mapTransferView({ kind: "idle" }, null, 2, "idle", true, true, true),
    ).toEqual({
      kind: "unavailable",
      reason: "Envoi indisponible: sélection multiple",
    });
  });

  it("mapTransferView maps each non-writable device state to a standardized reason", () => {
    const cases = [
      ["absent", "Envoi indisponible: aucun appareil connecté"],
      ["idle", "Envoi indisponible: profil non supporté"], // V3: supported but not writable
      ["unsupported", "Envoi indisponible: profil non supporté"],
      ["ambiguous", "Envoi indisponible: profil ambigu"],
      ["scanning", "Envoi indisponible: détection en cours"],
      ["error", "Envoi indisponible: détection en échec"],
    ] as const;
    for (const [deviceState, reason] of cases) {
      expect(
        mapTransferView(
          { kind: "idle" },
          STORY_A,
          1,
          deviceState,
          false,
          true,
          true,
        ),
      ).toEqual({ kind: "unavailable", reason });
    }
  });

  it("mapTransferView asks to prepare first when writable but not Préparée", () => {
    expect(
      mapTransferView({ kind: "idle" }, STORY_A, 1, "idle", true, false, false),
    ).toEqual({
      kind: "unavailable",
      reason: "Envoi indisponible: prépare l'histoire d'abord",
    });
  });

  it("mapTransferView blocks a native (Préparée but not transferable) story before any write", () => {
    expect(
      mapTransferView({ kind: "idle" }, STORY_A, 1, "idle", true, true, false),
    ).toEqual({
      kind: "unavailable",
      reason:
        "Envoi indisponible: histoire native non transférable (pas de pack appareil)",
    });
  });

  it("mapTransferView shows the active / terminal state ONLY for the selected target story", () => {
    expect(
      mapTransferView(
        { kind: "transferring", storyId: STORY_A, progress: null, phase: null },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({ kind: "transferring", progress: null, phase: null });
    // The FINAL verify phase maps to the transient verifying view.
    expect(
      mapTransferView(
        {
          kind: "transferring",
          storyId: STORY_A,
          progress: null,
          phase: "verify",
        },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({ kind: "verifying" });
    // The proven success terminal carries the AC2 summary lines (composed in Rust).
    expect(
      mapTransferView(
        {
          kind: "verified",
          storyId: STORY_A,
          summary: {
            changed: "« Mon histoire » est maintenant sur la Lunii.",
            unchanged: "3 autres histoires de l'appareil restent inchangées.",
          },
        },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({
      kind: "verified",
      changed: "« Mon histoire » est maintenant sur la Lunii.",
      unchanged: "3 autres histoires de l'appareil restent inchangées.",
    });
    // The honest état partiel terminal.
    expect(
      mapTransferView(
        {
          kind: "partial",
          storyId: STORY_A,
          message: "État partiel.",
          userAction: "Relance.",
        },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({
      kind: "partial",
      message: "État partiel.",
      userAction: "Relance.",
    });
    expect(
      mapTransferView(
        {
          kind: "retryable",
          storyId: STORY_A,
          message: "Échec.",
          userAction: "Relance.",
        },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({ kind: "retryable", message: "Échec.", userAction: "Relance." });
    expect(
      mapTransferView(
        {
          kind: "incomplete",
          storyId: STORY_A,
          message: "Copie partielle.",
          userAction: "Relance pour rétablir un état sûr.",
        },
        STORY_A,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({
      kind: "incomplete",
      message: "Copie partielle.",
      userAction: "Relance pour rétablir un état sûr.",
    });
  });

  it("anchors a failure issue to its story: hidden when another is selected, restored on re-select (C5/T7)", () => {
    const failedA = {
      kind: "retryable" as const,
      storyId: STORY_A,
      message: "Le transfert a échoué.",
      userAction: "Relance.",
    };
    // Selecting another story B: A's full panel context is NOT shown — the
    // StoryCard badge is the persistent anchor across selection changes.
    expect(
      mapTransferView(failedA, STORY_B, 1, "idle", true, true, true).kind,
    ).not.toBe("retryable");
    // Re-selecting A restores the full context (alert + Relancer/Abandonner).
    expect(
      mapTransferView(failedA, STORY_A, 1, "idle", true, true, true),
    ).toEqual({
      kind: "retryable",
      message: "Le transfert a échoué.",
      userAction: "Relance.",
    });
  });

  it("mapTransferView blocks a NEW send while a transfer targets another story (single-flight, F4)", () => {
    // Story A is transferring; the user selected story B (writable + Préparée).
    // Single-flight: B's send is REFUSED — the hook tracks one job and the device
    // volume must never see two concurrent writes. A's write stays consultable via
    // its badge (the selected-and-transferring case is handled above this branch).
    expect(
      mapTransferView(
        { kind: "transferring", storyId: STORY_A, progress: null, phase: null },
        STORY_B,
        1,
        "idle",
        true,
        true,
        true,
      ),
    ).toEqual({
      kind: "unavailable",
      reason: "Envoi indisponible: un transfert est déjà en cours",
    });
  });

  // ===== OS-open intent reaction (`OS Open Contract`) =====

  const OS_OPEN_ANALYZED = {
    kind: "analyzed" as const,
    quality: "partial" as const,
    state: "needsReview" as const,
    findings: [
      {
        aspect: "title" as const,
        category: "ambiguous" as const,
        message: "Le titre a été normalisé à l'import.",
      },
    ],
    importableContent: {
      title: "Le Soleil",
      structureJson: '{"schemaVersion":1,"nodes":[]}',
      contentChecksum: "a".repeat(64),
      createdAt: "2026-06-20T10:00:00.000Z",
      updatedAt: "2026-06-24T14:15:00.000Z",
    },
    sourceName: "histoire.rustory",
    artifactChecksum: "b".repeat(64),
  };

  const OS_OPEN_MULTIPLE_FILES_COPY =
    "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément.";
  const OS_OPEN_BUSY_COPY =
    "Une opération est déjà en cours dans la bibliothèque. Termine-la, puis rouvre le fichier.";

  it("pulls the OS-open intent once at mount and renders NOTHING on none (total no-op)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();

    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    });
    // Nothing renders, nothing announces: no review region, no OS-open
    // notice, no alert (the empty-state statuses are the library's own).
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(OS_OPEN_MULTIPLE_FILES_COPY),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(OS_OPEN_BUSY_COPY)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("opens the EXISTING import review from a cold-start intent (mount pull → analyzed)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    renderLibrary();

    // The verdict (sourceName + findings) feeds the SAME review surface as
    // the dialog import — quality chip + report, no new component.
    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Le titre a été normalisé à l'import."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    ).toBeInTheDocument();
    // The dialog picker was never opened — the intent came from the OS.
    expect(mockAnalyzeArtifact).not.toHaveBeenCalled();
  });

  it("reacts to a warm os-open signal: consumes it and opens the review", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    });

    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    act(() => {
      useOsOpenShell.getState().raise();
    });

    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    // The signal was consumed (a boolean relay, not a queue).
    expect(useOsOpenShell.getState().pendingSignal).toBe(false);
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(2);
  });

  it("queues a signal landing DURING an OS-open settlement — the mono-slot serialization, never the busy refusal", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    // A's pull hangs (the OS read is settling); B's verdict is distinct so
    // the final display is attributable.
    const OS_OPEN_ANALYZED_B = {
      ...OS_OPEN_ANALYZED,
      sourceName: "b.rustory",
      findings: [
        {
          aspect: "title" as const,
          category: "ambiguous" as const,
          message: "Le titre du second fichier a été normalisé.",
        },
      ],
    };
    let settleFirst!: (value: unknown) => void;
    mockAnalyzeOsOpen
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            settleFirst = resolve;
          }),
      )
      .mockResolvedValueOnce(OS_OPEN_ANALYZED_B);
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    // The user opens B while A's read settles — the exact gesture the
    // multi-file copy advises ("Rouvre chaque fichier séparément").
    act(() => {
      useOsOpenShell.getState().raise();
    });

    // NEVER the busy refusal: no discard (that would throw B away — the
    // Rust slot already holds B), no lying busy copy.
    expect(mockDiscardOsOpen).not.toHaveBeenCalled();
    expect(screen.queryByText(OS_OPEN_BUSY_COPY)).not.toBeInTheDocument();
    // The queued pull WAITS for A's settlement (mono-slot).
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);

    // A settles (its stale verdict — Rust-side B already replaced it, so
    // the real backend would answer B; either way the queue pulls again).
    await act(async () => {
      settleFirst(OS_OPEN_ANALYZED);
    });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(2);
    });
    // B's verdict is what ends displayed — the newest gesture wins.
    expect(
      await screen.findByText("Le titre du second fichier a été normalisé."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Le titre a été normalisé à l'import."),
    ).not.toBeInTheDocument();
    expect(mockDiscardOsOpen).not.toHaveBeenCalled();
  });

  it("renders the multipleFiles calm limit VERBATIM as a status — never an alert", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeOsOpen.mockResolvedValueOnce({
      kind: "multipleFiles",
      message: OS_OPEN_MULTIPLE_FILES_COPY,
    });
    renderLibrary();

    // Present twice by design: the persistent status region (the reliable
    // announcement) + the visual calm-limit block.
    const occurrences = await screen.findAllByText(
      OS_OPEN_MULTIPLE_FILES_COPY,
    );
    expect(occurrences).toHaveLength(2);
    expect(
      occurrences.some((el) => el.getAttribute("role") === "status"),
    ).toBe(true);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // No review opened — nothing was partially processed.
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    // The calm limit is dismissable explicitly — the visual block leaves
    // AND the persistent region empties.
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(
      screen.queryByText(OS_OPEN_MULTIPLE_FILES_COPY),
    ).not.toBeInTheDocument();
  });

  it("keeps a PERSISTENT live region mounted before any notice and routes the copy through it", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    });

    // The region EXISTS (empty) before any copy lands: only CHANGES of an
    // existing live region are reliably announced — a region born filled
    // is not (the surfaces' `__live` pattern).
    const liveRegion = () =>
      screen
        .getAllByRole("status")
        .find((el) => el.className.includes("library-route__os-open-live"));
    expect(liveRegion()).toBeDefined();
    expect(liveRegion()).toHaveTextContent("");

    mockAnalyzeOsOpen.mockResolvedValueOnce({
      kind: "multipleFiles",
      message: OS_OPEN_MULTIPLE_FILES_COPY,
    });
    act(() => {
      useOsOpenShell.getState().raise();
    });
    // The SAME persistent region receives the copy on settlement.
    await waitFor(() => {
      expect(liveRegion()).toHaveTextContent(OS_OPEN_MULTIPLE_FILES_COPY);
    });
  });

  it("declines an intent while an import is busy: discard + busy copy, the living flow INTACT", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    // A dialog-import analysis that never settles keeps the flow busy.
    mockAnalyzeArtifact.mockImplementation(() => new Promise(() => {}));
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    });

    await userEvent.click(
      screen.getByRole("button", { name: "Importer une histoire" }),
    );
    expect(
      screen.getByText("Analyse de l'artefact…"),
    ).toBeInTheDocument();

    act(() => {
      useOsOpenShell.getState().raise();
    });

    // Twice by design: the persistent status region + the visual block.
    const occurrences = await screen.findAllByText(OS_OPEN_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    expect(
      occurrences.some((el) => el.getAttribute("role") === "status"),
    ).toBe(true);
    await waitFor(() => {
      expect(mockDiscardOsOpen).toHaveBeenCalledTimes(1);
    });
    // The intent was NOT analyzed (the busy gate consumed it calmly)…
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    // …and the LIVING flow was never interrupted.
    expect(
      screen.getByText("Analyse de l'artefact…"),
    ).toBeInTheDocument();
  });

  it("surfaces an unreadable OS-open file as the failed state; Réessayer replays the SAME intent", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeOsOpen.mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction:
        "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
      details: { source: "file_read", stage: "metadata" },
    });
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Import impossible: fichier illisible.");

    // `Réessayer` replays the still-pending intent — never the file picker.
    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(2);
    expect(mockAnalyzeArtifact).not.toHaveBeenCalled();
  });

  it("Fermer on an OS-open read failure abandons the pending intent (discard) and returns to idle", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeOsOpen.mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction: "Réessaie.",
      details: { source: "file_read", stage: "open" },
    });
    renderLibrary();

    await screen.findByRole("alert");
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(mockDiscardOsOpen).toHaveBeenCalledTimes(1);
    });
  });

  it("keeps the StrictMode double-effect harmless: one review, the second pull answers none", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    // The Rust-side one-shot take: the FIRST pull carries the verdict,
    // every later pull answers `none`.
    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    renderLibrary({ strict: true });

    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    // Exactly ONE review region — never a doubled surface.
    expect(
      screen.getAllByRole("region", { name: "Import d'une histoire" }),
    ).toHaveLength(1);
  });

  it("accepts an OS-open reviewed story through the UNCHANGED accept phase and reloads the overview", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    renderLibrary();

    await screen.findByText("Partiellement exploitable");
    await userEvent.click(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    );

    // Both the polite live region and the success chip carry the copy.
    expect(
      (await screen.findAllByText("Histoire importée dans ta bibliothèque"))
        .length,
    ).toBeGreaterThanOrEqual(1);
    expect(mockAcceptArtifact).toHaveBeenCalledTimes(1);
    // The accept phase is the EXISTING accept_artifact_import (mocked at
    // the facade): the overview re-read happened (initial + post-import).
    expect(mockGet.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("Réessayer after a FAILED accept re-runs the commit with the preserved verdict — never a dead re-pull", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeOsOpen.mockResolvedValueOnce(OS_OPEN_ANALYZED);
    mockAcceptArtifact
      .mockRejectedValueOnce({
        code: "IMPORT_FAILED",
        message: "Import impossible: enregistrement local refusé.",
        userAction: "Réessaie.",
        details: { source: "db_commit", stage: "commit" },
      })
      .mockResolvedValueOnce({
        id: "0197a5d0-0000-7000-8000-0000000000cc",
        title: "Le Soleil",
        importState: "needsReview",
      });
    renderLibrary();

    await screen.findByText("Partiellement exploitable");
    await userEvent.click(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Import impossible: enregistrement local refusé.",
    );

    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(
      (await screen.findAllByText("Histoire importée dans ta bibliothèque"))
        .length,
    ).toBeGreaterThanOrEqual(1);
    // The retry re-ran the COMMIT with the preserved verdict…
    expect(mockAcceptArtifact).toHaveBeenCalledTimes(2);
    // …and never re-pulled the long-consumed intent (only the mount pull
    // and the intent pull ran).
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
  });

  it("declines an intent arriving during the PRIMARY creation submission — the creation stays intact", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    });

    // Open the creation dialog, type a title, submit — the creation hangs.
    await userEvent.click(
      screen.getAllByRole("button", { name: /créer une histoire/i })[0],
    );
    await userEvent.type(screen.getByLabelText("Titre"), "Mon histoire");
    await userEvent.click(screen.getByRole("button", { name: "Créer" }));
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();

    act(() => {
      useOsOpenShell.getState().raise();
    });

    // Twice by design: the persistent status region + the visual block.
    const occurrences = await screen.findAllByText(OS_OPEN_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    await waitFor(() => {
      expect(mockDiscardOsOpen).toHaveBeenCalledTimes(1);
    });
    // The intent was NOT analyzed (only the mount pull ran)…
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(1);
    // …and the LIVE submission was never interrupted.
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();
  });

  it("gates the sibling flow CTAs while a silent OS-open pull is in flight", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    // The mount pull hangs: the OS read is in flight, silently.
    let settlePull!: (value: unknown) => void;
    mockAnalyzeOsOpen.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settlePull = resolve;
        }),
    );
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    // Silent for the user (no surface, no alert)… but the import CTA is
    // gated: no sibling flow may start under a live OS read.
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    const importCta = screen.getByRole("button", {
      name: "Importer une histoire",
    });
    expect(importCta).toBeDisabled();

    await act(async () => {
      settlePull({ kind: "none" });
    });
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Importer une histoire" }),
      ).not.toBeDisabled();
    });
  });

  // ===== Drop intent reaction (`Drop Intent Contract`) =====

  const DROP_ARTIFACT_VERDICT = {
    kind: "artifact" as const,
    quality: "partial" as const,
    state: "needsReview" as const,
    findings: [
      {
        aspect: "title" as const,
        category: "ambiguous" as const,
        message: "Le titre a été normalisé à l'import.",
      },
    ],
    importableContent: {
      title: "Le Soleil",
      structureJson: '{"schemaVersion":1,"nodes":[]}',
      contentChecksum: "a".repeat(64),
      createdAt: "2026-06-20T10:00:00.000Z",
      updatedAt: "2026-06-24T14:15:00.000Z",
    },
    sourceName: "histoire.rustory",
    artifactChecksum: "b".repeat(64),
  };

  // The REAL wire shape of a creatable drop verdict: exactly the five
  // folder aspects (the facade guard refuses anything less — a route test
  // must never accept a form the IPC facade would reject).
  const DROP_FOLDER_VERDICT = {
    kind: "folder" as const,
    quality: "clean" as const,
    state: "recognized" as const,
    findings: [
      {
        aspect: "envelope" as const,
        category: "recognized" as const,
        message: "Le manifest histoire.json est présent et lisible.",
      },
      {
        aspect: "formatVersion" as const,
        category: "recognized" as const,
        message: "La version de format du manifest est prise en charge.",
      },
      {
        aspect: "title" as const,
        category: "recognized" as const,
        message: "Le titre de l'histoire est valide.",
      },
      {
        aspect: "structure" as const,
        category: "recognized" as const,
        message: "La structure de l'histoire est reconnue.",
      },
      {
        aspect: "media" as const,
        category: "recognized" as const,
        message:
          "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
      },
    ],
    creatableSummary: {
      title: "Le voyage de Nour",
      nodeCount: 2,
      retainedMedia: ["couverture.png"],
      discardedMedia: [],
    },
    folderName: "mon-dossier",
    folderPath: "/home/user/mon-dossier",
  };

  const DROP_MULTIPLE_ITEMS_COPY =
    "Rustory traite un seul élément déposé à la fois. Dépose chaque élément séparément.";
  const DROP_BUSY_COPY =
    "Une opération est déjà en cours dans la bibliothèque. Termine-la, puis dépose à nouveau ton fichier ou ton dossier.";

  it("pulls the drop intent once at mount and renders NOTHING on none (total no-op)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();

    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    });
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(DROP_MULTIPLE_ITEMS_COPY),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(DROP_BUSY_COPY)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("opens the EXISTING import review from a dormant dropped FILE (mount pull → artifact)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    renderLibrary();

    // The verdict (sourceName + findings) feeds the SAME review surface as
    // the dialog import — quality chip + report, no new component.
    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Le titre a été normalisé à l'import."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Importer ce qui est reconnu" }),
    ).toBeInTheDocument();
    // Neither picker was ever opened — the intent came from the drop.
    expect(mockAnalyzeArtifact).not.toHaveBeenCalled();
    expect(mockAnalyzeFolder).not.toHaveBeenCalled();
  });

  it("opens the EXISTING folder-creation review from a dropped FOLDER (the drop replaces the picker)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    renderLibrary();

    // The verdict feeds the SAME `Création depuis un dossier` surface —
    // the report names what will be created, exactly like the picker path.
    expect(
      await screen.findByRole("region", { name: "Création depuis un dossier" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Le voyage de Nour/)).toBeInTheDocument();
    // The folder picker was never opened.
    expect(mockAnalyzeFolder).not.toHaveBeenCalled();
  });

  it("reacts to a warm drop signal: consumes it and opens the review", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    });

    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    act(() => {
      useDropShell.getState().raiseSignal();
    });

    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    // The signal was consumed (a boolean relay, not a queue).
    expect(useDropShell.getState().pendingSignal).toBe(false);
    expect(mockAnalyzeDrop).toHaveBeenCalledTimes(2);
  });

  it("accepts a dropped folder through the UNCHANGED accept phase (folderPath round-trip)", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    renderLibrary();

    await screen.findByRole("region", { name: "Création depuis un dossier" });
    await userEvent.click(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    );
    expect(
      (await screen.findAllByText("Histoire créée dans ta bibliothèque"))
        .length,
    ).toBeGreaterThanOrEqual(1);
    // The accept is the EXISTING phase 2, fed the DTO's folderPath.
    expect(mockAcceptFolder).toHaveBeenCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
  });

  it("Réessayer after a FAILED folder accept re-commits the preserved verdict — never a re-pull", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    mockAcceptFolder
      .mockRejectedValueOnce({
        code: "IMPORT_FAILED",
        message: "Création impossible: enregistrement local refusé.",
        userAction: "Réessaie.",
        details: { source: "db_commit" },
      })
      .mockResolvedValueOnce({
        id: "0197a5d0-0000-7000-8000-0000000000dd",
        title: "Le voyage de Nour",
        importState: "recognized",
      });
    renderLibrary();
    await screen.findByRole("region", { name: "Création depuis un dossier" });

    await userEvent.click(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Création impossible: enregistrement local refusé.",
    );

    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(
      (await screen.findAllByText("Histoire créée dans ta bibliothèque"))
        .length,
    ).toBeGreaterThanOrEqual(1);
    // The retry re-ran the COMMIT with the preserved verdict (folderPath)…
    expect(mockAcceptFolder).toHaveBeenCalledTimes(2);
    expect(mockAcceptFolder).toHaveBeenLastCalledWith({
      folderPath: "/home/user/mon-dossier",
    });
    // …and never re-pulled the long-consumed intent, never the picker.
    expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    expect(mockAnalyzeFolder).not.toHaveBeenCalled();
  });

  it("renders the multipleItems calm limit VERBATIM through the PERSISTENT drop live region", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    });

    // The DEDICATED region EXISTS (empty) before any copy lands: only
    // CHANGES of an existing live region are reliably announced.
    const liveRegion = () =>
      screen
        .getAllByRole("status")
        .find((el) => el.className.includes("library-route__drop-live"));
    expect(liveRegion()).toBeDefined();
    expect(liveRegion()).toHaveTextContent("");

    mockAnalyzeDrop.mockResolvedValueOnce({
      kind: "multipleItems",
      message: DROP_MULTIPLE_ITEMS_COPY,
    });
    act(() => {
      useDropShell.getState().raiseSignal();
    });

    // Present twice by design: the persistent status region + the visual
    // calm-limit block. NOTHING was processed — no review opened.
    const occurrences = await screen.findAllByText(DROP_MULTIPLE_ITEMS_COPY);
    expect(occurrences).toHaveLength(2);
    expect(
      occurrences.some((el) => el.getAttribute("role") === "status"),
    ).toBe(true);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(liveRegion()).toHaveTextContent(DROP_MULTIPLE_ITEMS_COPY);
    });
    // Dismissable explicitly — the block leaves AND the region empties.
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(
      screen.queryByText(DROP_MULTIPLE_ITEMS_COPY),
    ).not.toBeInTheDocument();
  });

  it("declines a drop while an import is busy: discard + busy copy VERBATIM, the living flow INTACT", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    // A dialog-import analysis that never settles keeps the flow busy.
    mockAnalyzeArtifact.mockImplementation(() => new Promise(() => {}));
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    });

    await userEvent.click(
      screen.getByRole("button", { name: "Importer une histoire" }),
    );
    expect(screen.getByText("Analyse de l'artefact…")).toBeInTheDocument();

    act(() => {
      useDropShell.getState().raiseSignal();
    });

    // Twice by design: the persistent status region + the visual block.
    const occurrences = await screen.findAllByText(DROP_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    expect(
      occurrences.some((el) => el.getAttribute("role") === "status"),
    ).toBe(true);
    await waitFor(() => {
      expect(mockDiscardDrop).toHaveBeenCalledTimes(1);
    });
    // The intent was NOT analyzed (the busy gate consumed it calmly)…
    expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    // …and the LIVING flow was never interrupted.
    expect(screen.getByText("Analyse de l'artefact…")).toBeInTheDocument();
  });

  it("declines a drop while an OS-open read settles (cross-channel gate, drop side)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    // The OS-open mount pull hangs: a live OS read is in flight. The drop
    // MOUNT pull serializes behind it (the two channels' runs never write
    // the machine concurrently), so it has not reached the wire yet.
    let settleOsOpen!: (value: unknown) => void;
    mockAnalyzeOsOpen.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settleOsOpen = resolve;
        }),
    );
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });
    expect(mockAnalyzeDrop).not.toHaveBeenCalled();

    act(() => {
      useDropShell.getState().raiseSignal();
    });

    // The WARM intent is declined calmly: busy copy + discard — never an
    // interleave with the live OS read.
    const occurrences = await screen.findAllByText(DROP_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    await waitFor(() => {
      expect(mockDiscardDrop).toHaveBeenCalledTimes(1);
    });
    expect(mockAnalyzeDrop).not.toHaveBeenCalled();

    // Once the OS read settles, the queued MOUNT pull serves the channel
    // (a `none` answer — the warm intent was discarded).
    await act(async () => {
      settleOsOpen({ kind: "none" });
    });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);
    });
  });

  it("declines an OS-open intent while a DROP review is displayed (cross-channel gate, os-open side)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    renderLibrary();

    // The drop-fed review is on screen (a consumed one-shot verdict).
    await screen.findByText("Partiellement exploitable");
    const pullsSoFar = mockAnalyzeOsOpen.mock.calls.length;

    act(() => {
      useOsOpenShell.getState().raise();
    });

    // The OS-open intent is declined calmly (its OWN busy copy) and the
    // drop review stays INTACT — never silently replaced.
    const occurrences = await screen.findAllByText(OS_OPEN_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    await waitFor(() => {
      expect(mockDiscardOsOpen).toHaveBeenCalledTimes(1);
    });
    expect(mockAnalyzeOsOpen).toHaveBeenCalledTimes(pullsSoFar);
    expect(screen.getByText("Partiellement exploitable")).toBeInTheDocument();
  });

  it("surfaces an unreadable dropped element as the failed state; Réessayer replays the SAME intent", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction:
        "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
      details: { source: "file_read", stage: "metadata" },
    });
    renderLibrary();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Import impossible: fichier illisible.");

    // `Réessayer` replays the still-pending intent — never a picker.
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    await userEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    expect(mockAnalyzeDrop).toHaveBeenCalledTimes(2);
    expect(mockAnalyzeArtifact).not.toHaveBeenCalled();
  });

  it("Fermer on a drop read failure abandons the pending intent (discard) and returns to idle", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockRejectedValueOnce({
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction: "Réessaie.",
      details: { source: "file_read", stage: "open" },
    });
    renderLibrary();

    await screen.findByRole("alert");
    await userEvent.click(screen.getByRole("button", { name: "Fermer" }));

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(mockDiscardDrop).toHaveBeenCalledTimes(1);
    });
  });

  it("Abandonner on a drop-fed review also discards the pending Rust intent", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    renderLibrary();

    await screen.findByText("Partiellement exploitable");
    await userEvent.click(screen.getByRole("button", { name: "Abandonner" }));

    expect(
      screen.queryByText("Partiellement exploitable"),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(mockDiscardDrop).toHaveBeenCalledTimes(1);
    });
    // Pure frontend reset otherwise: nothing was created.
    expect(mockAcceptArtifact).not.toHaveBeenCalled();
  });

  it("a folder verdict settling DURING a folder commit is declined: busy copy, the commit intact", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    // The commit hangs so the settlement window is deterministic.
    let resolveCommit!: (value: unknown) => void;
    mockAcceptFolder.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveCommit = resolve;
        }),
    );
    renderLibrary();
    await screen.findByRole("region", { name: "Création depuis un dossier" });

    // The user drops a SECOND folder (its pull starts — the busy gate does
    // not fire: only a review is displayed, no live flow yet)…
    let settleSecond!: (value: unknown) => void;
    mockAnalyzeDrop.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settleSecond = resolve;
        }),
    );
    act(() => {
      useDropShell.getState().raiseSignal();
    });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(2);
    });
    // …then clicks `Créer l'histoire` on the displayed review: the commit
    // starts while the second pull is still reading.
    await userEvent.click(
      screen.getByRole("button", { name: "Créer l'histoire" }),
    );
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();

    // The second verdict settles MID-COMMIT: the injection is declined —
    // the frozen busy copy renders, the commit screen is never rewritten.
    await act(async () => {
      settleSecond({
        ...DROP_FOLDER_VERDICT,
        folderName: "second-dossier",
        folderPath: "/home/user/second-dossier",
      });
    });
    const occurrences = await screen.findAllByText(DROP_BUSY_COPY);
    expect(occurrences).toHaveLength(2);
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();

    // The living commit finishes normally.
    await act(async () => {
      resolveCommit({
        id: "0197a5d0-0000-7000-8000-0000000000dd",
        title: "Le voyage de Nour",
        importState: "recognized",
      });
    });
    expect(
      (await screen.findAllByText("Histoire créée dans ta bibliothèque"))
        .length,
    ).toBeGreaterThanOrEqual(1);
  });

  it("Abandonner on a drop-fed FOLDER review drops a late settlement — nothing reopens", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    renderLibrary();
    await screen.findByRole("region", { name: "Création depuis un dossier" });

    // A newer drop lands; its pull hangs while the user closes the review.
    let settleLate!: (value: unknown) => void;
    mockAnalyzeDrop.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settleLate = resolve;
        }),
    );
    act(() => {
      useDropShell.getState().raiseSignal();
    });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(2);
    });
    await userEvent.click(screen.getByRole("button", { name: "Abandonner" }));
    expect(
      screen.queryByRole("region", { name: "Création depuis un dossier" }),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(mockDiscardDrop).toHaveBeenCalledTimes(1);
    });

    // The LATE folder settlement lands AFTER the terminal gesture: it is
    // dropped — the review the user just closed never reopens.
    await act(async () => {
      settleLate(DROP_FOLDER_VERDICT);
    });
    expect(
      screen.queryByRole("region", { name: "Création depuis un dossier" }),
    ).not.toBeInTheDocument();
  });

  it("queues a signal landing DURING a drop settlement — the DEDICATED mono-slot, the last verdict alone displayed", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    const DROP_ARTIFACT_B = {
      ...DROP_ARTIFACT_VERDICT,
      sourceName: "b.rustory",
      findings: [
        {
          aspect: "title" as const,
          category: "ambiguous" as const,
          message: "Le titre du second élément a été normalisé.",
        },
      ],
    };
    let settleFirst!: (value: unknown) => void;
    mockAnalyzeDrop
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            settleFirst = resolve;
          }),
      )
      .mockResolvedValueOnce(DROP_ARTIFACT_B);
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    // The user drops B while A's read settles.
    act(() => {
      useDropShell.getState().raiseSignal();
    });

    // NEVER the busy refusal for the channel's own settling: no discard,
    // no lying busy copy — the pull QUEUES.
    expect(mockDiscardDrop).not.toHaveBeenCalled();
    expect(screen.queryByText(DROP_BUSY_COPY)).not.toBeInTheDocument();
    expect(mockAnalyzeDrop).toHaveBeenCalledTimes(1);

    await act(async () => {
      settleFirst(DROP_ARTIFACT_VERDICT);
    });
    await waitFor(() => {
      expect(mockAnalyzeDrop).toHaveBeenCalledTimes(2);
    });
    // The LAST verdict alone is displayed — the newest gesture wins.
    expect(
      await screen.findByText("Le titre du second élément a été normalisé."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Le titre a été normalisé à l'import."),
    ).not.toBeInTheDocument();
    expect(mockDiscardDrop).not.toHaveBeenCalled();
  });

  it("a dropped FOLDER after a dropped FILE leaves the folder review ALONE displayed", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    renderLibrary();
    await screen.findByText("Partiellement exploitable");

    mockAnalyzeDrop.mockResolvedValueOnce(DROP_FOLDER_VERDICT);
    act(() => {
      useDropShell.getState().raiseSignal();
    });

    // The channel's newest settlement is the ONLY one displayed: the
    // folder review opens, the earlier drop-fed import review closes.
    expect(
      await screen.findByRole("region", { name: "Création depuis un dossier" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Partiellement exploitable"),
    ).not.toBeInTheDocument();
  });

  it("keeps the StrictMode double-effect harmless: one review, the second pull answers none", async () => {
    mockGet.mockResolvedValue({ stories: [] });
    // The Rust-side one-shot take: the FIRST pull carries the verdict,
    // every later pull answers `none` (the default).
    mockAnalyzeDrop.mockResolvedValueOnce(DROP_ARTIFACT_VERDICT);
    renderLibrary({ strict: true });

    expect(
      await screen.findByText("Partiellement exploitable"),
    ).toBeInTheDocument();
    // Exactly ONE review region — never a doubled surface.
    expect(
      screen.getAllByRole("region", { name: "Import d'une histoire" }),
    ).toHaveLength(1);
  });

  it("gates the sibling flow CTAs while a silent drop pull is in flight", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    // The mount pull hangs: the drop read is in flight, silently.
    let settlePull!: (value: unknown) => void;
    mockAnalyzeDrop.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settlePull = resolve;
        }),
    );
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    // Silent for the user… but the import CTA is gated: no sibling flow
    // may start under a live drop read.
    expect(
      screen.queryByRole("region", { name: "Import d'une histoire" }),
    ).not.toBeInTheDocument();
    const importCta = screen.getByRole("button", {
      name: "Importer une histoire",
    });
    expect(importCta).toBeDisabled();

    await act(async () => {
      settlePull({ kind: "none" });
    });
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Importer une histoire" }),
      ).not.toBeDisabled();
    });
  });
});
