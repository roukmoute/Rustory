import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../ipc/commands/story", () => ({
  getStoryDetail: vi.fn(),
  saveStory: vi.fn(),
  createStory: vi.fn(),
}));

const { invalidateLibraryOverviewCacheMock } = vi.hoisted(() => ({
  invalidateLibraryOverviewCacheMock: vi.fn(),
}));

vi.mock("../../features/library/hooks/use-library-overview", () => ({
  useLibraryOverview: () => ({
    state: { kind: "ready", overview: { stories: [] } },
    cached: { stories: [] },
    isRefreshing: false,
    retry: () => undefined,
    invalidate: () => undefined,
  }),
  invalidateLibraryOverviewCache: invalidateLibraryOverviewCacheMock,
}));

import { getStoryDetail, saveStory } from "../../ipc/commands/story";
import type { StoryDetailDto } from "../../shared/ipc-contracts/story";
import { LibraryRoute } from "../library/LibraryRoute";
import { StoryEditRoute } from "./StoryEditRoute";

const STORY_ID = "abc";

function buildDetail(overrides: Partial<StoryDetailDto> = {}): StoryDetailDto {
  return {
    id: STORY_ID,
    title: "Le soleil couchant",
    schemaVersion: 1,
    structureJson: '{"schemaVersion":1,"nodes":[]}',
    contentChecksum: "a".repeat(64),
    createdAt: "2026-04-23T09:00:00.000Z",
    updatedAt: "2026-04-23T09:00:00.000Z",
    ...overrides,
  };
}

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
    vi.mocked(getStoryDetail).mockReset();
    vi.mocked(saveStory).mockReset();
    vi.mocked(saveStory).mockResolvedValue({
      id: STORY_ID,
      title: "",
      updatedAt: "2026-04-23T00:00:00.000Z",
    });
    invalidateLibraryOverviewCacheMock.mockReset();
    // Silence unhandled rejections that escape the component when the mock
    // rejects synchronously and the test renders a different branch.
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the loading state while the detail is pending", async () => {
    let resolveDetail: (value: StoryDetailDto | null) => void = () => {};
    vi.mocked(getStoryDetail).mockReturnValueOnce(
      new Promise<StoryDetailDto | null>((resolve) => {
        resolveDetail = resolve;
      }),
    );
    renderRoute(`/story/${STORY_ID}/edit`);

    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(
      screen.getByText(/chargement du brouillon local/i),
    ).toBeInTheDocument();

    resolveDetail(buildDetail());
  });

  it("renders the draft-local surface with canonical vocabulary when the detail is present", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    renderRoute(`/story/${STORY_ID}/edit`);

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

  it("decodes a percent-encoded storyId before issuing get_story_detail", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(
      buildDetail({ id: "abc/space id", title: "Titre spécial" }),
    );
    renderRoute("/story/abc%2Fspace%20id/edit");

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /titre spécial/i }),
      ).toBeInTheDocument(),
    );
    expect(getStoryDetail).toHaveBeenCalledWith({ storyId: "abc/space id" });
  });

  it("renders 'Histoire introuvable' when the backend returns null", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(null);
    renderRoute(`/story/missing/edit`);

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
    vi.mocked(getStoryDetail).mockRejectedValue({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Stockage local inaccessible",
      userAction: "Réessaie plus tard.",
      details: null,
    });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
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
    vi.mocked(getStoryDetail).mockRejectedValue({
      code: "LIBRARY_INCONSISTENT",
      message: "Bibliothèque incohérente.",
      userAction: "Recharge pour reconstruire la vue.",
      details: null,
    });
    renderRoute(`/story/${STORY_ID}/edit`);

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
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    const router = renderRoute(`/story/${STORY_ID}/edit`);

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
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    const { container } = render(
      <RouterProvider
        router={createMemoryRouter(
          [{ path: "/story/:storyId/edit", element: <StoryEditRoute /> }],
          { initialEntries: [`/story/${STORY_ID}/edit`] },
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

  // -------- Autosave scenarios --------

  it("renders the editable title field initialized with the persisted value", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    expect(field).toHaveValue("Le soleil couchant");
  });

  it("starts with the 'Brouillon local' chip when nothing is pending", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    // Use scoped query to avoid matching the `<p>` prose ("brouillon local").
    const main = screen.getByRole("main", {
      name: /reprise d'un brouillon local/i,
    });
    expect(
      main.querySelector(".story-edit-route__save-status"),
    ).toHaveTextContent("Brouillon local");
  });

  it("shows Enregistré after a successful autosave and invalidates the library cache", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Nouveau titre",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );

    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Nouveau titre" } });
      });

      // Debounce fires, save resolves.
      await act(async () => {
        vi.advanceTimersByTime(500);
        await Promise.resolve();
        await Promise.resolve();
      });

      const main = screen.getByRole("main", {
        name: /reprise d'un brouillon local/i,
      });
      expect(
        main.querySelector(".story-edit-route__save-status"),
      ).toHaveTextContent(/enregistré/i);
      expect(invalidateLibraryOverviewCacheMock).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows an alert with Réessayer l'enregistrement when the save fails (AC3)", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Rustory n'a pas pu enregistrer ta modification.",
      userAction: "Réessaie dans un instant.",
      details: { source: "sqlite_update", kind: "busy" },
    });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Échec garanti" } });
      });
      await act(async () => {
        vi.advanceTimersByTime(500);
        await Promise.resolve();
        await Promise.resolve();
      });

      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent(
        /rustory n'a pas pu enregistrer ta modification/i,
      );
      expect(alert).toHaveTextContent(/réessaie dans un instant/i);
      expect(
        screen.getByRole("button", { name: /réessayer l'enregistrement/i }),
      ).toBeInTheDocument();
      // Draft is preserved — the user doesn't lose what they typed.
      expect(field).toHaveValue("Échec garanti");
      // AC3: no cache invalidation on failure — the prior persisted
      // state is unchanged, so `/library` does not need to refetch.
      expect(invalidateLibraryOverviewCacheMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("never uses a Toast for the save failure (UX-DR15 compliance)", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "m",
      userAction: "a",
      details: null,
    });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Essai" } });
      });
      await act(async () => {
        vi.advanceTimersByTime(500);
        await Promise.resolve();
        await Promise.resolve();
      });

      // Toast primitive mounts with the `.ds-toast` class; its absence
      // proves the failure is not routed through a toast.
      expect(document.querySelector(".ds-toast")).toBeNull();
      // The canonical alert surface is present (role="alert").
      expect(screen.getByRole("alert")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("retries the save with the attempted title when Réessayer l'enregistrement is clicked", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory)
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "m",
        userAction: "a",
        details: null,
      })
      .mockResolvedValueOnce({
        id: STORY_ID,
        title: "Réessayé",
        updatedAt: "2026-04-23T10:00:00.000Z",
      });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Réessayé" } });
      });
      await act(async () => {
        vi.advanceTimersByTime(500);
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByRole("alert")).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /réessayer l'enregistrement/i }),
        );
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(vi.mocked(saveStory)).toHaveBeenLastCalledWith({
        id: STORY_ID,
        title: "Réessayé",
      });
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("rephrases an INVALID_STORY_TITLE message with the Enregistrement prefix on the edit surface", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockRejectedValueOnce({
      code: "INVALID_STORY_TITLE",
      message:
        "Création impossible: titre trop long (120 caractères maximum, 3 en trop)",
      userAction: "Raccourcis le titre à 120 caractères maximum.",
      details: null,
    });
    renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Trop long" } });
      });
      await act(async () => {
        vi.advanceTimersByTime(500);
        await Promise.resolve();
        await Promise.resolve();
      });

      const alert = screen.getByRole("alert");
      // The edit surface rephrases "Création impossible" → "Enregistrement
      // impossible". The rest of the canonical reason (including the
      // "N en trop" suffix) is preserved verbatim.
      expect(alert).toHaveTextContent(
        /enregistrement impossible: titre trop long \(120 caractères maximum, 3 en trop\)/i,
      );
      expect(alert).not.toHaveTextContent(/création impossible/i);
    } finally {
      vi.useRealTimers();
    }
  });

  it("flushes a pending autosave before navigating back to /library", async () => {
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Sauvé in extremis",
      updatedAt: "2026-04-23T10:00:00.000Z",
    });
    const router = renderRoute(`/story/${STORY_ID}/edit`);

    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: /titre de l'histoire/i }),
      ).toBeInTheDocument(),
    );
    const field = screen.getByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    vi.useFakeTimers();
    try {
      await act(async () => {
        fireEvent.change(field, { target: { value: "Sauvé in extremis" } });
      });
      // Only 100ms elapsed — debounce has NOT fired yet.
      await act(async () => {
        vi.advanceTimersByTime(100);
      });
      expect(vi.mocked(saveStory)).not.toHaveBeenCalled();
      await act(async () => {
        fireEvent.click(
          screen.getByRole("button", { name: /retour à la bibliothèque/i }),
        );
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(vi.mocked(saveStory)).toHaveBeenCalledWith({
        id: STORY_ID,
        title: "Sauvé in extremis",
      });
    } finally {
      vi.useRealTimers();
    }
    // Navigate is synchronous via react-router; waitFor under real timers
    // catches the post-fake-timer tick without blocking.
    await waitFor(() =>
      expect(router.state.location.pathname).toBe("/library"),
    );
  });
});
