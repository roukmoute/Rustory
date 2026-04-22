import { StrictMode } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  RouterProvider,
  createMemoryRouter,
  type RouteObject,
} from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockGet = vi.fn();

vi.mock("../../ipc/commands/library", () => ({
  getLibraryOverview: () => ({
    promise: mockGet(),
    cancel: () => {},
  }),
}));

import { invalidateLibraryOverviewCache } from "../../features/library/hooks/use-library-overview";
import { useLibraryShell } from "../../shell/state/library-shell-store";
import { LibraryRoute } from "./LibraryRoute";

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
    // The hook keeps a module-local stale-while-revalidate cache; reset
    // it between tests so no stray snapshot bleeds across cases.
    invalidateLibraryOverviewCache();
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

  it("shows an actionable empty state with a keyboard-reachable disabled CTA", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();

    expect(
      await screen.findByRole("heading", {
        name: /ta bibliothèque est vide/i,
      }),
    ).toBeInTheDocument();

    const primary = screen.getByRole("button", {
      name: /créer une histoire/i,
    });
    expect(primary).not.toBeDisabled();
    expect(primary).toHaveAttribute("aria-disabled", "true");

    const describedBy = primary.getAttribute("aria-describedby");
    expect(describedBy).toBeTruthy();
    const reason = document.getElementById(describedBy as string);
    expect(reason).toHaveTextContent(/création d'histoire indisponible/i);
    expect(reason?.textContent).not.toMatch(/story\s*1/i);
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

  it("shows the Lunii Decision Panel with 'Aucun appareil connecté' on boot", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });
    renderLibrary();
    await screen.findByRole("heading", { name: /ta bibliothèque est vide/i });

    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });
    expect(
      within(panel).getByText(/aucun appareil connecté/i),
    ).toBeInTheDocument();
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
});
