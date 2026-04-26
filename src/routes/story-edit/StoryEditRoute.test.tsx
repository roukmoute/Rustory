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

vi.mock("../../ipc/commands/story", () => {
  // The recovery hook imports the drift-error classes for its
  // `instanceof` mapping. Re-declared inline to keep them within
  // the hoisted factory closure.
  class ApplyRecoveryContractDriftError extends Error {
    raw: unknown;
    constructor(message: string, options: { raw: unknown }) {
      super(message);
      this.name = "ApplyRecoveryContractDriftError";
      this.raw = options.raw;
    }
  }
  class ReadRecoverableDraftContractDriftError extends Error {
    raw: unknown;
    constructor(message: string, options: { raw: unknown }) {
      super(message);
      this.name = "ReadRecoverableDraftContractDriftError";
      this.raw = options.raw;
    }
  }
  return {
    getStoryDetail: vi.fn(),
    saveStory: vi.fn(),
    createStory: vi.fn(),
    recordDraft: vi.fn().mockResolvedValue(undefined),
    // Default: no recoverable draft for the story. Tests that exercise
    // the recovery banner override `readRecoverableDraft` locally with
    // `vi.mocked(readRecoverableDraft).mockResolvedValueOnce(...)`.
    readRecoverableDraft: vi.fn().mockResolvedValue({ kind: "none" }),
    applyRecovery: vi.fn(),
    discardDraft: vi.fn().mockResolvedValue(undefined),
    ApplyRecoveryContractDriftError,
    ReadRecoverableDraftContractDriftError,
  };
});

vi.mock("../../ipc/commands/import-export", () => ({
  exportStoryWithSaveDialog: vi.fn(),
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

import { exportStoryWithSaveDialog } from "../../ipc/commands/import-export";
import {
  applyRecovery,
  discardDraft,
  getStoryDetail,
  readRecoverableDraft,
  recordDraft,
  saveStory,
} from "../../ipc/commands/story";
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
    // Recovery flow defaults: no draft to recover. Tests that exercise
    // the banner override `readRecoverableDraft` per-case.
    vi.mocked(readRecoverableDraft).mockReset();
    vi.mocked(readRecoverableDraft).mockResolvedValue({ kind: "none" });
    vi.mocked(recordDraft).mockReset();
    vi.mocked(recordDraft).mockResolvedValue();
    vi.mocked(applyRecovery).mockReset();
    vi.mocked(discardDraft).mockReset();
    vi.mocked(discardDraft).mockResolvedValue();
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

  describe("export CTA", () => {
    beforeEach(() => {
      vi.mocked(exportStoryWithSaveDialog).mockReset();
    });

    const exportedOutcome = {
      kind: "exported" as const,
      destinationPath: "/tmp/histoire.rustory",
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    };
    const cancelledOutcome = { kind: "cancelled" as const };

    async function renderReadyEditWithDetail(
      overrides: Partial<StoryDetailDto> = {},
    ) {
      vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail(overrides));
      const router = renderRoute(`/story/${STORY_ID}/edit`);
      await screen.findByRole("heading", {
        name: overrides.title ?? "Le soleil couchant",
      });
      return router;
    }

    it("renders a button labelled Exporter l'histoire in the ready state", async () => {
      await renderReadyEditWithDetail();
      expect(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      ).toBeInTheDocument();
    });

    it("is not rendered while the route is still loading", async () => {
      vi.mocked(getStoryDetail).mockReturnValue(new Promise(() => undefined));
      renderRoute(`/story/${STORY_ID}/edit`);
      // Loading surface uses role=status with the progress indicator.
      await screen.findByRole("status");
      expect(
        screen.queryByRole("button", { name: /Exporter l'histoire/i }),
      ).not.toBeInTheDocument();
    });

    it("invokes the Rust dialog-owning command with the sanitized suggested filename", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail({ title: "Un / Deux : Trois" });
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        cancelledOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      expect(exportStoryWithSaveDialog).toHaveBeenCalledWith({
        storyId: STORY_ID,
        suggestedFilename: "Un_Deux_Trois.rustory",
      });
    });

    it("a cancelled outcome renders no chip and no alert", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        cancelledOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      // Success chip has label "Exporté" as a StateChip label, not a
      // raw text node — the polite region renders "Exporté" only on
      // exported; on cancelled it stays empty.
      expect(
        screen.queryByText(/Exportation en cours/i),
      ).not.toBeInTheDocument();
    });

    it("renders Exporté inside a persistent polite region on success", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        exportedOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      await screen.findByText(/Exporté vers \/tmp\/histoire\.rustory/);
      const liveRegions = document.querySelectorAll(
        "[aria-live='polite']",
      );
      const hasExportedRegion = Array.from(liveRegions).some((el) =>
        el.textContent?.includes("Exporté"),
      );
      expect(hasExportedRegion).toBe(true);
    });

    it("renders a role=alert with message + userAction on failure", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce({
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "Écriture refusée par le système pour ce dossier.",
        userAction: "Choisis un dossier où tu as les droits en écriture.",
        details: null,
      });

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent(/Exportation échouée/);
      expect(alert).toHaveTextContent(
        /Écriture refusée par le système pour ce dossier/,
      );
      expect(alert).toHaveTextContent(
        /Choisis un dossier où tu as les droits en écriture/,
      );
    });

    it("Choisir un autre emplacement re-invokes the Rust command after a failure", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockRejectedValueOnce({
        code: "EXPORT_DESTINATION_UNAVAILABLE",
        message: "err",
        userAction: "act",
        details: null,
      });

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );
      await screen.findByRole("alert");

      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        exportedOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Choisir un autre emplacement/i }),
      );

      await screen.findByText(/Exporté vers/);
      expect(exportStoryWithSaveDialog).toHaveBeenCalledTimes(2);
    });

    it("never calls saveStory: export is strictly read-only on the canonical row", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        exportedOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );
      await screen.findByText(/Exporté vers/);

      expect(saveStory).not.toHaveBeenCalled();
    });

    it("does not invalidate the library overview cache on success", async () => {
      invalidateLibraryOverviewCacheMock.mockReset();
      const user = userEvent.setup();
      await renderReadyEditWithDetail();
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        exportedOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );
      await screen.findByText(/Exporté vers/);

      expect(invalidateLibraryOverviewCacheMock).not.toHaveBeenCalled();
    });

    it("uses the LIVE draft title for the suggested filename (not the persisted one)", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();

      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      await user.clear(field);
      await user.type(field, "Titre en cours");

      // The autosave hook's `flushAutoSave` will fire saveStory before
      // the export command runs — mock a resolved return so the hook's
      // `.then()` chain doesn't throw on an undefined return value.
      vi.mocked(saveStory).mockResolvedValue({
        id: STORY_ID,
        title: "Titre en cours",
        updatedAt: "2026-04-24T10:30:00.000Z",
      });
      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        cancelledOutcome,
      );

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      expect(exportStoryWithSaveDialog).toHaveBeenCalledWith({
        storyId: STORY_ID,
        suggestedFilename: "Titre_en_cours.rustory",
      });
    });

    it("flushes the pending autosave before invoking the export command", async () => {
      const user = userEvent.setup();
      await renderReadyEditWithDetail();

      // Type a new title but don't wait for the 500ms debounce to fire.
      const field = screen.getByRole("textbox", { name: /Titre de l'histoire/i });
      await user.clear(field);
      await user.type(field, "Titre en cours");

      // saveStory must NOT have been called yet — debounce still pending.
      expect(saveStory).not.toHaveBeenCalled();

      vi.mocked(exportStoryWithSaveDialog).mockResolvedValueOnce(
        cancelledOutcome,
      );
      vi.mocked(saveStory).mockResolvedValueOnce({
        id: STORY_ID,
        title: "Titre en cours",
        updatedAt: "2026-04-24T10:30:00.000Z",
      });

      await user.click(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      );

      // flushAutoSave must have fired saveStory with the typed value
      // BEFORE the export command is called — otherwise the artifact
      // would capture a stale pre-debounce title.
      expect(saveStory).toHaveBeenCalledWith({
        id: STORY_ID,
        title: "Titre en cours",
      });
    });
  });

  describe("Recovery banner", () => {
    const RECOVERABLE = {
      kind: "recoverable" as const,
      storyId: STORY_ID,
      draftTitle: "Buffered live",
      draftAt: "2026-04-25T12:00:00.000Z",
      persistedTitle: "Le soleil couchant",
    };

    async function renderRouteWithRecoverableDraft() {
      vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
      vi.mocked(readRecoverableDraft).mockReset();
      vi.mocked(readRecoverableDraft).mockResolvedValueOnce(RECOVERABLE);
      renderRoute(`/story/${STORY_ID}/edit`);
      // Wait for both the editor and the banner to mount.
      await screen.findByRole("region", { name: "Brouillon récupéré" });
    }

    it("disables the Field while readRecoverableDraft is still resolving (race AC1)", async () => {
      // A keystroke between Field mount and resolution must not
      // schedule a recordDraft that overwrites the recoverable row.
      let resolveLater: (
        value:
          | { kind: "none" }
          | (typeof RECOVERABLE),
      ) => void = () => {};
      vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
      vi.mocked(readRecoverableDraft).mockReset();
      vi.mocked(readRecoverableDraft).mockReturnValueOnce(
        new Promise((resolve) => {
          resolveLater = resolve;
        }),
      );
      renderRoute(`/story/${STORY_ID}/edit`);
      await screen.findByRole("heading", { name: "Le soleil couchant" });

      // The Field must already be disabled — recovery is still loading.
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).toBeDisabled();

      // Resolve as `none` and confirm the Field re-enables.
      resolveLater({ kind: "none" });
      await waitFor(() => {
        expect(field).not.toBeDisabled();
      });
    });

    it("renders the RecoveryBanner when the backend returns a recoverable draft", async () => {
      await renderRouteWithRecoverableDraft();
      expect(
        screen.getByRole("region", { name: "Brouillon récupéré" }),
      ).toBeInTheDocument();
      expect(screen.getByText('"Buffered live"')).toBeInTheDocument();
      expect(screen.getByText('"Le soleil couchant"')).toBeInTheDocument();
    });

    it("does not render the RecoveryBanner when the backend returns none", async () => {
      vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
      renderRoute(`/story/${STORY_ID}/edit`);
      await screen.findByRole("heading", { name: "Le soleil couchant" });
      expect(
        screen.queryByRole("region", { name: "Brouillon récupéré" }),
      ).not.toBeInTheDocument();
    });

    it("disables the Field while the recovery banner is visible", async () => {
      await renderRouteWithRecoverableDraft();
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).toBeDisabled();
    });

    it("disables the export button while the recovery banner is visible", async () => {
      await renderRouteWithRecoverableDraft();
      expect(
        screen.getByRole("button", { name: /Exporter l'histoire/i }),
      ).toHaveAttribute("aria-disabled", "true");
    });

    it("applying the recovery updates the H1 to the recovered title", async () => {
      await renderRouteWithRecoverableDraft();
      vi.mocked(applyRecovery).mockResolvedValueOnce({
        id: STORY_ID,
        title: "Buffered live",
        updatedAt: "2026-04-25T12:00:01.000Z",
      });

      await userEvent.click(
        screen.getByRole("button", { name: "Restaurer le brouillon" }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("heading", { name: "Buffered live" }),
        ).toBeInTheDocument();
      });
      // Banner gone, Field re-enabled.
      expect(
        screen.queryByRole("region", { name: "Brouillon récupéré" }),
      ).not.toBeInTheDocument();
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).not.toBeDisabled();
    });

    it("discarding the recovery clears the banner and re-enables the Field", async () => {
      await renderRouteWithRecoverableDraft();
      await userEvent.click(
        screen.getByRole("button", { name: "Conserver l'état enregistré" }),
      );
      await waitFor(() => {
        expect(
          screen.queryByRole("region", { name: "Brouillon récupéré" }),
        ).not.toBeInTheDocument();
      });
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).not.toBeDisabled();
    });

    it("the recovery banner appears BEFORE the h1 in the DOM order", async () => {
      await renderRouteWithRecoverableDraft();
      const region = screen.getByRole("region", { name: "Brouillon récupéré" });
      const h1 = screen.getByRole("heading", { name: "Le soleil couchant" });
      // `compareDocumentPosition` returns a bitmask; bit 4 means
      // `region` precedes `h1` in document order.
      const followingMask = Node.DOCUMENT_POSITION_FOLLOWING;
      // eslint-disable-next-line no-bitwise
      expect(region.compareDocumentPosition(h1) & followingMask).toBe(
        followingMask,
      );
    });

    it('error during apply renders the role="alert" with userAction', async () => {
      await renderRouteWithRecoverableDraft();
      const rustError = {
        code: "RECOVERY_DRAFT_UNAVAILABLE",
        message: "Récupération indisponible.",
        userAction: "Vérifie le disque local.",
        details: null,
      };
      vi.mocked(applyRecovery).mockRejectedValueOnce(rustError);
      await userEvent.click(
        screen.getByRole("button", { name: "Restaurer le brouillon" }),
      );
      await screen.findByRole("alert");
      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent("Récupération indisponible.");
      expect(alert).toHaveTextContent("Vérifie le disque local.");
    });

    it("clicking Réessayer la récupération re-fires readRecoverableDraft", async () => {
      await renderRouteWithRecoverableDraft();
      vi.mocked(applyRecovery).mockRejectedValueOnce({
        code: "RECOVERY_DRAFT_UNAVAILABLE",
        message: "boom",
        userAction: "retry",
        details: null,
      });
      await userEvent.click(
        screen.getByRole("button", { name: "Restaurer le brouillon" }),
      );
      await screen.findByRole("alert");

      vi.mocked(readRecoverableDraft).mockResolvedValueOnce(RECOVERABLE);
      const callsBefore = vi.mocked(readRecoverableDraft).mock.calls.length;
      await userEvent.click(
        screen.getByRole("button", { name: "Réessayer la récupération" }),
      );
      await waitFor(() => {
        expect(vi.mocked(readRecoverableDraft).mock.calls.length).toBe(
          callsBefore + 1,
        );
      });
    });
  });

  describe("Recovery initial-read error banner (AC3)", () => {
    async function renderRouteWithReadError() {
      vi.mocked(getStoryDetail).mockResolvedValueOnce(buildDetail());
      vi.mocked(readRecoverableDraft).mockReset();
      vi.mocked(readRecoverableDraft).mockRejectedValueOnce({
        code: "RECOVERY_DRAFT_UNAVAILABLE",
        message: "Récupération indisponible: vérifie le disque local et réessaie.",
        userAction: "Vérifie l'espace disque et les permissions.",
        details: { source: "sqlite_select" },
      });
      renderRoute(`/story/${STORY_ID}/edit`);
      await screen.findByRole("region", { name: "Récupération indisponible" });
    }

    it("renders the dedicated read-error banner when the initial read fails", async () => {
      await renderRouteWithReadError();
      const region = screen.getByRole("region", {
        name: "Récupération indisponible",
      });
      expect(region).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Récupération indisponible: vérifie le disque local et réessaie.",
      );
    });

    it("does NOT render the diff banner when the initial read fails (no draft to show)", async () => {
      await renderRouteWithReadError();
      expect(
        screen.queryByRole("region", { name: "Brouillon récupéré" }),
      ).not.toBeInTheDocument();
    });

    it("disables the Field while the read-error banner is visible", async () => {
      await renderRouteWithReadError();
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).toBeDisabled();
    });

    it("Réessayer la récupération re-fires readRecoverableDraft and recovers", async () => {
      await renderRouteWithReadError();
      const callsBefore = vi.mocked(readRecoverableDraft).mock.calls.length;
      vi.mocked(readRecoverableDraft).mockResolvedValueOnce({ kind: "none" });
      await userEvent.click(
        screen.getByRole("button", { name: "Réessayer la récupération" }),
      );
      await waitFor(() => {
        expect(vi.mocked(readRecoverableDraft).mock.calls.length).toBe(
          callsBefore + 1,
        );
      });
      // After the retry succeeds with `kind:"none"`, the banner is gone
      // and the Field is editable again.
      await waitFor(() => {
        expect(
          screen.queryByRole("region", { name: "Récupération indisponible" }),
        ).not.toBeInTheDocument();
      });
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).not.toBeDisabled();
    });

    it("Conserver l'état enregistré dismisses the banner without retrying", async () => {
      await renderRouteWithReadError();
      const callsBefore = vi.mocked(readRecoverableDraft).mock.calls.length;
      await userEvent.click(
        screen.getByRole("button", { name: "Conserver l'état enregistré" }),
      );
      await waitFor(() => {
        expect(
          screen.queryByRole("region", { name: "Récupération indisponible" }),
        ).not.toBeInTheDocument();
      });
      // Dismiss must NOT trigger a fresh recovery read.
      expect(vi.mocked(readRecoverableDraft).mock.calls.length).toBe(
        callsBefore,
      );
      const field = screen.getByRole("textbox", {
        name: /Titre de l'histoire/i,
      });
      expect(field).not.toBeDisabled();
    });
  });
});
