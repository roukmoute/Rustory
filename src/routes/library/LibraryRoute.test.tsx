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

import { invalidateConnectedLuniiCache } from "../../features/device/hooks/use-connected-lunii";
import { invalidateDeviceLibraryCache } from "../../features/device/hooks/use-device-library";
import { invalidateLibraryOverviewCache } from "../../features/library/hooks/use-library-overview";
import { useLibraryShell } from "../../shell/state/library-shell-store";
import {
  LibraryRoute,
  mapDeviceForPanel,
  mapStoryValidationToView,
  mapTransferPreviewToComparison,
} from "./LibraryRoute";

const STORY_EDIT_MARKER_TITLE = "Edit stub for";

function renderLibrary(options: { strict?: boolean } = {}) {
  const routes: RouteObject[] = [
    { path: "/library", element: <LibraryRoute /> },
    {
      path: "/story/:storyId/edit",
      element: <div data-testid="story-edit-stub">{STORY_EDIT_MARKER_TITLE}</div>,
    },
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

    await user.click(
      await screen.findByRole("button", { name: /réessayer/i }),
    );

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
      screen.getByRole("navigation", { name: /filtres bibliothèque/i }),
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
      within(main).getByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).toBeInTheDocument();

    const nav = screen.getByRole("navigation", {
      name: /filtres bibliothèque/i,
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
      screen.getByRole("navigation", { name: /filtres bibliothèque/i }),
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

  it("Ctrl+click on a second card toggles multi-selection and disables Éditer with the canonical reason", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: "s1", title: "Le soleil" },
        { id: "s2", title: "La lune" },
      ],
    });
    renderLibrary();

    await user.click(
      await screen.findByRole("button", { name: /le soleil/i }),
    );
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

    await user.click(
      await screen.findByRole("button", { name: /le soleil/i }),
    );
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

    await user.click(
      await screen.findByRole("button", { name: /le soleil/i }),
    );

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
    expect(
      alert.textContent ?? "",
    ).toMatch(/bibliothèque locale contient des histoires en double/i);
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
      expect(useLibraryShell.getState().selectedStoryIds.has("s1")).toBe(
        false,
      ),
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

  it("after a successful create_story, invalidates the cache and navigates to /story/:id/edit", async () => {
    const user = userEvent.setup();
    // First fetch returns an empty library; the second fetch — triggered
    // after invalidation — returns the freshly created story.
    mockGet
      .mockResolvedValueOnce({ stories: [] })
      .mockResolvedValueOnce({
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
      await user.type(within(dialog).getByLabelText(/^titre$/i), "Mon histoire");
      await user.click(within(dialog).getByRole("button", { name: /^créer$/i }));

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
    expect(within(panel).getByText(/détection indisponible/i)).toBeInTheDocument();
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
    mockDevice
      .mockResolvedValueOnce({ kind: "none" })
      .mockResolvedValueOnce({
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
      expect(within(panel).getAllByText(/profil ambigu/i).length).toBeGreaterThan(0),
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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

  it("shows a recoverable device-library error in the center without breaking the local library (AC #3)", async () => {
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    expect(within(inspector).getByText(/profil non supporté/i)).toBeInTheDocument();

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
      within(inspector).getByText("Copie indisponible: déjà dans ta bibliothèque"),
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
    await user.click(within(inspector).getByRole("button", { name: /fermer/i }));
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    expect(comparison).toHaveTextContent(/2 autres histoires.*resteront inchangées/i);
  });

  it("shows the replacement verdict when the selected story's pack is on the device (AC1)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    expect(comparison).toHaveTextContent(/1 autre histoire.*restera inchangée/i);
  });

  it("keeps the send CTA disabled even when the comparison is ready (AC2)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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

    expect(comparison).toHaveTextContent(/sélectionne une seule histoire locale/i);
    expect(comparison).not.toHaveTextContent(/nouvelle sur l'appareil/i);
    expect(comparison).not.toHaveTextContent(/déjà présente sur l'appareil/i);
  });

  it("distinguishes the no-device hint when one story is selected but no readable device (AC3)", async () => {
    const user = userEvent.setup();
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
    mockGet.mockResolvedValueOnce({ stories: [{ id: "s1", title: "Le soleil" }] });
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
      mapDeviceForPanel(
        { kind: "ready", device: { kind: "none" } },
        true,
      ),
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
});
