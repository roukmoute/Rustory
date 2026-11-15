import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const getLibraryOverviewMock = vi.fn();
const getStoryDetailMock = vi.fn();
const getOnboardingPreferencesMock = vi.fn();

vi.mock("../ipc/commands/onboarding", () => ({
  getOnboardingPreferences: () => getOnboardingPreferencesMock(),
  updateOnboardingPreferences: async (input: unknown) => input,
}));

vi.mock("../ipc/commands/library", () => ({
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

vi.mock("../ipc/commands/story", () => ({
  createStory: vi.fn(),
  saveStory: vi.fn(),
  getStoryDetail: (...args: unknown[]) => getStoryDetailMock(...args),
  // T8 recovery flow: the route mounts `useStoryRecovery` so every
  // story-edit traversal calls `read_recoverable_draft`. Default to
  // `kind: "none"` so the banner stays hidden in router tests that
  // do not exercise recovery; tests that care about the banner can
  // override the mock locally.
  readRecoverableDraft: () => Promise.resolve({ kind: "none" }),
  recordDraft: () => Promise.resolve(),
  applyRecovery: vi.fn(),
  discardDraft: () => Promise.resolve(),
  updateNodeContent: vi.fn(),
  attachNodeMedia: vi.fn(),
  removeNodeMedia: vi.fn(),
  readNodeMedia: vi.fn(),
  recordNodeDraft: () => Promise.resolve(),
  readRecoverableNodeDraft: () => Promise.resolve({ kind: "none" }),
  discardNodeDraft: () => Promise.resolve(),
}));

// Story 2.1: LibraryRoute mounts `useConnectedLunii` which calls
// `read_connected_lunii`. Default the mock to `kind: "none"` so the
// device panel never blocks router-level navigation tests.
vi.mock("../ipc/commands/device", async () => {
  const actual = await vi.importActual<typeof import("../ipc/commands/device")>(
    "../ipc/commands/device",
  );
  return {
    ...actual,
    readConnectedLunii: () => ({
      promise: Promise.resolve({ kind: "none" }),
      cancel: () => {},
    }),
  };
});

// SettingsRoute mounts two pure reads plus the version read. The
// router tests only assert the landmark, so failed reads (rendering
// the calm per-section unavailable state) are a fine default.
vi.mock("../ipc/commands/settings", async () => {
  const actual = await vi.importActual<
    typeof import("../ipc/commands/settings")
  >("../ipc/commands/settings");
  return {
    ...actual,
    readSupportProfile: () => Promise.reject(new Error("not stubbed")),
  };
});

vi.mock("../ipc/commands/import-export", async () => {
  const actual = await vi.importActual<
    typeof import("../ipc/commands/import-export")
  >("../ipc/commands/import-export");
  return {
    ...actual,
    readContentSourcePolicy: () => Promise.reject(new Error("not stubbed")),
    analyzeOsOpenRequest: () => Promise.resolve({ kind: "none" }),
    analyzeDropRequest: () => Promise.resolve({ kind: "none" }),
  };
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.reject(new Error("not stubbed")),
}));

import { createAppRouter } from "./router";
import { invalidateLibraryOverviewCache } from "../features/library/hooks/use-library-overview";
import { useDraftsShell, useLibraryShell } from "../shell/state/library-shell-store";

describe("router", () => {
  beforeEach(() => {
    invalidateLibraryOverviewCache();
    for (const store of [useLibraryShell, useDraftsShell]) {
      store.setState({ selectedStoryIds: new Set(), query: "", sort: "titre-asc" });
    }
    getOnboardingPreferencesMock.mockReset();
    getOnboardingPreferencesMock.mockResolvedValue({
      welcomeCompleted: true,
      deviceRecoveryPromptDismissed: true,
    });
    getLibraryOverviewMock.mockReset();
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc-123", title: "Le soleil" }],
    });
    getStoryDetailMock.mockReset();
    getStoryDetailMock.mockImplementation(({ storyId }: { storyId: string }) =>
      Promise.resolve({
        id: storyId,
        title: "Le soleil",
        schemaVersion: 1,
        structureJson: '{"schemaVersion":1,"nodes":[]}',
        contentChecksum: "a".repeat(64),
        createdAt: "2026-04-23T09:00:00.000Z",
        updatedAt: "2026-04-23T09:00:00.000Z",
        editable: true,
        editScope: "full",
        importState: null,
        structure: {
          startNodeId: "n1",
          nodes: [
            {
              id: "n1",
              label: "",
              isStart: true,
              hasIssue: false,
              options: [],
            },
          ],
        },
        node: { id: "n1", text: "", label: "", image: null, audio: null },
      }),
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("matches /library to the local collection screen", async () => {
    const router = createAppRouter(["/library"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
  });

  it("opens /device directly as a separate screen without local cards", async () => {
    const router = createAppRouter(["/device"]);
    render(<RouterProvider router={router} />);
    expect(
      await screen.findByRole("heading", { level: 1, name: "Sur l'appareil" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("main", { name: "Histoires sur l'appareil" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Ma bibliothèque" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Le soleil" }),
    ).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/device");
    expect(router.state.location.hash).toBe("");
  });

  it("navigates between exclusive screens through the actual route tree", async () => {
    const user = userEvent.setup();
    const router = createAppRouter(["/library"]);
    render(<RouterProvider router={router} />);
    expect(
      await screen.findByRole("heading", { level: 1, name: "Ma bibliothèque" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "Sur l'appareil" }));
    expect(router.state.location.pathname).toBe("/device");
    expect(
      screen.getByRole("heading", { level: 1, name: "Sur l'appareil" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { level: 1, name: "Ma bibliothèque" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "Ma bibliothèque" }));
    expect(router.state.location.pathname).toBe("/library");
    expect(
      screen.queryByRole("heading", { level: 1, name: "Sur l'appareil" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 1, name: "Ma bibliothèque" }),
    ).toBeInTheDocument();
  });

  it("matches /story/:storyId/edit to StoryEditRoute and mounts its main landmark", async () => {
    const router = createAppRouter(["/story/abc-123/edit"]);
    render(<RouterProvider router={router} />);

    // Assert the route's own landmark directly — proves the :storyId
    // binding rendered StoryEditRoute, not the library fallback. The
    // story heading is a side-effect we also verify.
    await waitFor(() =>
      expect(
        screen.getByRole("main", { name: /éditeur d'histoire/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  it("decodes a percent-encoded storyId before looking it up", async () => {
    const router = createAppRouter(["/story/abc%2D123/edit"]);
    render(<RouterProvider router={router} />);

    await waitFor(() =>
      expect(
        screen.getByRole("main", { name: /éditeur d'histoire/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  it("matches /settings to SettingsRoute and mounts its main landmark", async () => {
    const router = createAppRouter(["/settings"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Profil de support" }),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByRole("heading", { level: 1, name: "Profil de support" }),
    ).toBeInTheDocument();
  });

  it("redirects / to /library", async () => {
    const router = createAppRouter(["/"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
    expect(router.state.location.pathname).toBe("/library");
  });

  it("opens the welcome screen at first launch from persisted incomplete preferences", async () => {
    getOnboardingPreferencesMock.mockResolvedValue({
      welcomeCompleted: false,
      deviceRecoveryPromptDismissed: false,
    });
    const router = createAppRouter(["/"]);
    render(<RouterProvider router={router} />);

    expect(await screen.findByRole("heading", {
      level: 1, name: "Bienvenue sur Rustory",
    })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/welcome");
    expect(screen.queryByRole("main", {
      name: "Collection d'histoires",
    })).not.toBeInTheDocument();
  });

  it("keeps direct draft links direct even before welcome has been completed", async () => {
    getOnboardingPreferencesMock.mockResolvedValue({
      welcomeCompleted: false,
      deviceRecoveryPromptDismissed: false,
    });
    getLibraryOverviewMock.mockResolvedValue({ stories: [
      { id: "native", title: "Mon brouillon" },
      { id: "native-2", title: "Mon deuxième brouillon", transferable: false },
      { id: "imported", title: "Histoire importée", importState: "resolved" },
      { id: "device", title: "Histoire copiée", transferable: true },
    ] });
    const router = createAppRouter(["/drafts"]);
    render(<RouterProvider router={router} />);

    await screen.findByRole("button", { name: "Mon brouillon" });
    expect(router.state.location.pathname).toBe("/drafts");
    expect(screen.getByRole("heading", { level: 1, name: "Mes brouillons" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Mon deuxième brouillon" })).toBeInTheDocument();
    expect(screen.getByText("2 brouillons · enregistrés sur cet ordinateur")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Histoire importée" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Histoire copiée" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Bienvenue sur Rustory" })).not.toBeInTheDocument();
  });

  it("returns from the editor to the originating drafts collection with its search and selection", async () => {
    const user = userEvent.setup();
    const router = createAppRouter(["/drafts"]);
    render(<RouterProvider router={router} />);
    await user.click(await screen.findByRole("button", { name: "Le soleil" }));
    await user.type(screen.getByRole("searchbox"), "soleil");
    await user.click(screen.getByRole("button", { name: "Continuer à écrire" }));

    await screen.findByRole("main", { name: /éditeur d'histoire/i });
    expect(router.state.location.state).toEqual({ collection: "/drafts" });
    await user.click(screen.getByRole("button", { name: /retour à la bibliothèque/i }));
    await screen.findByRole("heading", { level: 1, name: "Mes brouillons" });
    expect(router.state.location.pathname).toBe("/drafts");
    expect(screen.getByRole("searchbox")).toHaveValue("soleil");
    expect(screen.getByRole("button", { name: "Le soleil" })).toHaveAttribute("aria-pressed", "true");
    expect(useLibraryShell.getState().selectedStoryIds.size).toBe(0);
    expect(useLibraryShell.getState().query).toBe("");
  });

  it("redirects unknown paths to /library", async () => {
    const router = createAppRouter(["/does-not-exist"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
  });
});
