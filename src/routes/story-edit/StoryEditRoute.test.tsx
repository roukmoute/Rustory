import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LibraryRoute } from "../library/LibraryRoute";
import { StoryEditRoute } from "./StoryEditRoute";

const getLibraryOverviewMock = vi.fn();

vi.mock("../../ipc/commands/library", () => ({
  getLibraryOverview: () => ({
    promise: getLibraryOverviewMock(),
    cancel: () => {},
  }),
  LIBRARY_OVERVIEW_TIMEOUT_MS: 2000,
  LIBRARY_OVERVIEW_TIMEOUT_ERROR: {
    code: "UNKNOWN",
    message: "timeout",
    userAction: "retry",
    details: null,
  },
}));

function renderRoute(initialPath: string) {
  const router = createMemoryRouter(
    [
      { path: "/library", element: <LibraryRoute /> },
      { path: "/story/:storyId/edit", element: <StoryEditRoute /> },
    ],
    { initialEntries: [initialPath] },
  );
  render(<RouterProvider router={router} />);
  return router;
}

describe("<StoryEditRoute />", () => {
  beforeEach(() => {
    getLibraryOverviewMock.mockReset();
    // Silence unhandled rejections that escape the component when the mock
    // rejects synchronously and the test renders a different branch.
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the loading state while the overview is pending", async () => {
    let resolveLoad: (value: unknown) => void = () => {};
    getLibraryOverviewMock.mockReturnValue(
      new Promise((resolve) => {
        resolveLoad = resolve;
      }),
    );
    renderRoute("/story/abc/edit");

    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(
      screen.getByText(/chargement du brouillon local/i),
    ).toBeInTheDocument();

    resolveLoad({ stories: [] });
  });

  it("renders the draft-local surface with canonical vocabulary when the story is present", async () => {
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc", title: "Le soleil couchant" }],
    });
    renderRoute("/story/abc/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /le soleil couchant/i }),
      ).toBeInTheDocument(),
    );

    const main = screen.getByRole("main", {
      name: /reprise d'un brouillon local/i,
    });
    expect(main).toHaveTextContent(/brouillon local/i);
    expect(
      screen.getByText(
        /tu reprends le dernier brouillon local de cette histoire\. l'appareil n'est pas consulté\./i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    ).toBeInTheDocument();
  });

  it("decodes a percent-encoded storyId before looking it up in the overview", async () => {
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc/space id", title: "Titre spécial" }],
    });
    renderRoute("/story/abc%2Fspace%20id/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /titre spécial/i }),
      ).toBeInTheDocument(),
    );
  });

  it("renders 'Histoire introuvable' when the id is missing from the overview", async () => {
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "other", title: "Autre" }],
    });
    renderRoute("/story/missing/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /histoire introuvable/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByText(/cette histoire n'est plus dans ta bibliothèque locale/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    ).toBeInTheDocument();
  });

  it("renders the error banner with a context-appropriate title on the edit route", async () => {
    getLibraryOverviewMock.mockRejectedValue({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Stockage local inaccessible",
      userAction: "Réessaie plus tard.",
      details: null,
    });
    renderRoute("/story/abc/edit");

    await waitFor(() =>
      // The banner title must NOT claim the library is unavailable while
      // the user is on the edit context — the page headline is "Reprise
      // indisponible".
      expect(
        screen.getByRole("heading", { name: /reprise indisponible/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("heading", { name: /bibliothèque indisponible/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/stockage local inaccessible/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    ).toBeInTheDocument();
  });

  it("surfaces a LIBRARY_INCONSISTENT error with the canonical 'recharge nécessaire' title", async () => {
    getLibraryOverviewMock.mockRejectedValue({
      code: "LIBRARY_INCONSISTENT",
      message: "Bibliothèque incohérente.",
      userAction: "Recharge pour reconstruire la vue.",
      details: null,
    });
    renderRoute("/story/abc/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", {
          name: /bibliothèque incohérente, recharge nécessaire/i,
        }),
      ).toBeInTheDocument(),
    );
  });

  it("navigates back to /library when the Retour button is clicked", async () => {
    const user = userEvent.setup();
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc", title: "Le soleil couchant" }],
    });
    const router = renderRoute("/story/abc/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /le soleil couchant/i }),
      ).toBeInTheDocument(),
    );

    await user.click(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    );

    await waitFor(() =>
      expect(router.state.location.pathname).toBe("/library"),
    );
  });

  it("never leaks internal planning jargon in user-facing copy", async () => {
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc", title: "Le soleil couchant" }],
    });
    const { container } = render(
      <RouterProvider
        router={createMemoryRouter(
          [{ path: "/story/:storyId/edit", element: <StoryEditRoute /> }],
          { initialEntries: ["/story/abc/edit"] },
        )}
      />,
    );

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /le soleil couchant/i }),
      ).toBeInTheDocument(),
    );
    expect(container.textContent ?? "").not.toMatch(
      /\bbmad\b|\bstory\s*\d\.\d|\bepic\s*\d/i,
    );
  });
});
