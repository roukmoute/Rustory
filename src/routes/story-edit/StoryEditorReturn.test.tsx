import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// IPC façade mocks. The end-to-end round trip mounts the FULL app route tree
// (AppShell → LibraryRoute → StoryEditRoute), so every command the library and
// the editor reach on mount must be stubbed at the façade boundary — exactly as
// the LibraryRoute suite does — while the feature hooks run for real.
// ---------------------------------------------------------------------------
const mockGet = vi.fn();
const mockDevice = vi.fn();
const mockDeviceLibrary = vi.fn();
const mockTransferPreview = vi.fn();
const mockStoryValidation = vi.fn();
const mockCatalogStatus = vi.fn();
const mockReadPreparation = vi.fn();
const mockReadTransfer = vi.fn();
const mockReadTransferOutcome = vi.fn();

vi.mock("../../ipc/commands/library", () => ({
  getLibraryOverview: () => ({ promise: mockGet(), cancel: () => {} }),
}));

vi.mock("../../ipc/commands/device", async () => {
  const actual =
    await vi.importActual<typeof import("../../ipc/commands/device")>(
      "../../ipc/commands/device",
    );
  return {
    ...actual,
    readConnectedLunii: () => ({ promise: mockDevice(), cancel: () => {} }),
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
  refreshOfficialCatalog: () => Promise.resolve({ count: 0 }),
  importOfficialCatalog: () => Promise.resolve({ kind: "cancelled" }),
  readPackCover: () => Promise.resolve(null),
}));

vi.mock("../../ipc/commands/story-preparation", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story-preparation")
  >("../../ipc/commands/story-preparation");
  return {
    ...actual,
    readPreparationState: (input: unknown) => mockReadPreparation(input),
  };
});

vi.mock("../../ipc/commands/story-transfer", async () => {
  const actual = await vi.importActual<
    typeof import("../../ipc/commands/story-transfer")
  >("../../ipc/commands/story-transfer");
  return {
    ...actual,
    readTransferState: (input: unknown) => mockReadTransfer(input),
    readTransferOutcome: (input: unknown) => mockReadTransferOutcome(input),
  };
});

vi.mock("../../ipc/events/job-events", () => ({
  subscribeJobEvents: () => () => {},
}));

vi.mock("../../ipc/commands/story", () => {
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
    readRecoverableDraft: vi.fn().mockResolvedValue({ kind: "none" }),
    applyRecovery: vi.fn(),
    discardDraft: vi.fn().mockResolvedValue(undefined),
    updateNodeContent: vi.fn(),
    attachNodeMedia: vi.fn(),
    removeNodeMedia: vi.fn(),
    readNodeMedia: vi.fn(),
    recordNodeDraft: vi.fn().mockResolvedValue(undefined),
    readRecoverableNodeDraft: vi.fn().mockResolvedValue({ kind: "none" }),
    discardNodeDraft: vi.fn().mockResolvedValue(undefined),
    ApplyRecoveryContractDriftError,
    ReadRecoverableDraftContractDriftError,
  };
});

import { createAppRouter } from "../../app/router";
import { invalidateConnectedLuniiCache } from "../../features/device/hooks/use-connected-lunii";
import { invalidateDeviceLibraryCache } from "../../features/device/hooks/use-device-library";
import { invalidateLibraryOverviewCache } from "../../features/library/hooks/use-library-overview";
import { getStoryDetail, saveStory } from "../../ipc/commands/story";
import type { StoryDetailDto } from "../../shared/ipc-contracts/story";
import { useLibraryShell } from "../../shell/state/library-shell-store";

// Two stories so the preserved selection (OTHER_ID) is provably DISTINCT from
// the story that gets opened + edited (STORY_ID): the selection assertion can
// no longer be satisfied by the opening gesture itself.
const STORY_ID = "story-1";
const OLD_TITLE = "Le soleil couchant";
const NEW_TITLE = "Le soleil levant";
const OTHER_ID = "story-2";
const OTHER_TITLE = "Le soleil rouge";

function buildDetail(): StoryDetailDto {
  return {
    id: STORY_ID,
    title: OLD_TITLE,
    schemaVersion: 1,
    structureJson: '{"schemaVersion":1,"nodes":[]}',
    contentChecksum: "a".repeat(64),
    createdAt: "2026-04-23T09:00:00.000Z",
    updatedAt: "2026-04-23T09:00:00.000Z",
    editable: true,
    structure: {
      startNodeId: "n1",
      nodes: [
        { id: "n1", label: "", isStart: true, hasIssue: false, options: [] },
      ],
    },
    node: { id: "n1", text: "", label: "", image: null, audio: null },
  };
}

describe("Library ↔ editor round trip (AC2)", () => {
  beforeEach(() => {
    // First library read returns the OLD title; every later read (the
    // authoritative re-read on return) reflects the persisted NEW title.
    mockGet.mockReset();
    mockGet.mockResolvedValueOnce({
      stories: [
        { id: STORY_ID, title: OLD_TITLE },
        { id: OTHER_ID, title: OTHER_TITLE },
      ],
    });
    mockGet.mockResolvedValue({
      stories: [
        { id: STORY_ID, title: NEW_TITLE },
        { id: OTHER_ID, title: OTHER_TITLE },
      ],
    });
    // Device probe never resolves → the panel stays in the scanning state and
    // no device-dependent read fires; the local library card is unaffected.
    mockDevice.mockReset();
    mockDevice.mockImplementation(() => new Promise(() => {}));
    mockDeviceLibrary.mockReset();
    mockDeviceLibrary.mockResolvedValue({ kind: "none" });
    mockTransferPreview.mockReset();
    mockTransferPreview.mockResolvedValue({ kind: "noDevice" });
    mockStoryValidation.mockReset();
    mockStoryValidation.mockResolvedValue({ kind: "noDevice" });
    mockCatalogStatus.mockReset();
    mockCatalogStatus.mockResolvedValue({ count: 0 });
    mockReadPreparation.mockReset();
    mockReadPreparation.mockResolvedValue({ kind: "idle" });
    mockReadTransfer.mockReset();
    mockReadTransfer.mockResolvedValue({ kind: "idle" });
    mockReadTransferOutcome.mockReset();
    mockReadTransferOutcome.mockResolvedValue(null);

    vi.mocked(getStoryDetail).mockReset();
    vi.mocked(getStoryDetail).mockResolvedValue(buildDetail());
    vi.mocked(saveStory).mockReset();
    vi.mocked(saveStory).mockResolvedValue({
      id: STORY_ID,
      title: NEW_TITLE,
      updatedAt: "2026-04-23T10:00:00.000Z",
    });

    invalidateLibraryOverviewCache();
    invalidateConnectedLuniiCache();
    invalidateDeviceLibraryCache();

    // The continuity that MUST survive the round trip: a selected story
    // (OTHER_ID — deliberately NOT the one opened below), a non-default search
    // query and a non-default sort.
    useLibraryShell.setState({
      selectedStoryIds: new Set([OTHER_ID]),
      query: "soleil",
      sort: "titre-desc",
    });
  });

  it("preserves selection + query + sort and reflects the edited title on return", async () => {
    const user = userEvent.setup();
    render(<RouterProvider router={createAppRouter(["/library"])} />);

    // 1. Library is up; the selected story is OTHER_ID, while we open a
    //    DIFFERENT story (STORY_ID).
    const card = await screen.findByRole("button", { name: OLD_TITLE });

    // 2. Open the editor with the keyboard "open" gesture (Enter on the focused
    //    card, per the Story Card Interaction Contract). Enter opens WITHOUT
    //    mutating the selection, so the preserved selection asserted below is
    //    isolated from the opening gesture.
    fireEvent.keyDown(card, { key: "Enter" });

    // 3. The dedicated editor context is on screen (for the opened story).
    await screen.findByRole("main", { name: "Éditeur d'histoire" });
    const field = await screen.findByRole("textbox", {
      name: /titre de l'histoire/i,
    });
    await waitFor(() => expect(field).not.toBeDisabled());

    // 4. Rename the opened story, then leave via Retour (which flushes the
    //    autosave before navigating — a mid-debounce keystroke is never lost).
    await user.clear(field);
    await user.type(field, NEW_TITLE);
    await user.click(
      screen.getByRole("button", { name: /retour à la bibliothèque/i }),
    );

    // The flush committed the typed value (no silent loss).
    expect(saveStory).toHaveBeenCalledWith({ id: STORY_ID, title: NEW_TITLE });

    // 5. Back in the library: the opened story's card reflects the edited title
    //    via the authoritative re-read on mount.
    await screen.findByRole("button", { name: NEW_TITLE });
    expect(
      screen.queryByRole("button", { name: OLD_TITLE }),
    ).not.toBeInTheDocument();

    // 6. The selection survived the round trip — and because the opened story
    //    (STORY_ID) is NOT the selected one (OTHER_ID), and Enter never
    //    re-selects, this proves preservation, not a side effect of opening.
    //    The query and sort survived too — no silent erasure of context (AC2).
    const shell = useLibraryShell.getState();
    expect([...shell.selectedStoryIds]).toEqual([OTHER_ID]);
    expect(shell.query).toBe("soleil");
    expect(shell.sort).toBe("titre-desc");
  });
});
