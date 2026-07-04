import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  addNodeOption,
  addStoryNode,
  deleteStoryNode,
  getStoryDetail,
  moveStoryNode,
  removeNodeOption,
  setNodeOptionLink,
} from "../../../ipc/commands/story";
import type {
  StoryDetailDto,
  StoryStructure,
  StructureWriteOutput,
} from "../../../shared/ipc-contracts/story";

import { useStructureEditor } from "./use-structure-editor";

vi.mock("../../../ipc/commands/story", () => ({
  addStoryNode: vi.fn(),
  deleteStoryNode: vi.fn(),
  moveStoryNode: vi.fn(),
  addNodeOption: vi.fn(),
  setNodeOptionLink: vi.fn(),
  removeNodeOption: vi.fn(),
  getStoryDetail: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

const STRUCTURE: StoryStructure = {
  startNodeId: "n1",
  nodes: [
    { id: "n1", label: "Départ", isStart: true, hasIssue: false, options: [] },
    { id: "n2", label: "", isStart: false, hasIssue: false, options: [] },
  ],
};

function ackWith(structure: StoryStructure): StructureWriteOutput {
  return {
    id: "story-1",
    updatedAt: "2026-07-04T10:00:00.000Z",
    contentChecksum: "a".repeat(64),
    structureJson: '{"schemaVersion":3,"startNodeId":"n1","nodes":[]}',
    structure,
  };
}

function detailWith(nodeId: string): StoryDetailDto {
  return {
    id: "story-1",
    title: "Histoire",
    schemaVersion: 3,
    structureJson: "{}",
    contentChecksum: "a".repeat(64),
    createdAt: "2026-07-04T09:00:00.000Z",
    updatedAt: "2026-07-04T10:00:00.000Z",
    editable: true,
    structure: STRUCTURE,
    node: { id: nodeId, text: "", label: "", image: null, audio: null },
  };
}

interface HarnessArgs {
  storyId?: string;
  editable?: boolean;
}

const setupRefs: { current: { flushContent: ReturnType<typeof vi.fn> } | null } = {
  current: null,
};

function setup(args: HarnessArgs = {}) {
  const flushContent = vi.fn();
  const onStructureCommitted = vi.fn();
  const onDetailReloaded = vi.fn();
  setupRefs.current = { flushContent };
  const view = renderHook(
    (props: { storyId: string | undefined; editable: boolean }) =>
      useStructureEditor({
        storyId: props.storyId,
        structure: STRUCTURE,
        editable: props.editable,
        flushContent,
        onStructureCommitted,
        onDetailReloaded,
      }),
    {
      initialProps: {
        storyId: args.storyId ?? "story-1",
        editable: args.editable ?? true,
      },
    },
  );
  return { ...view, flushContent, onStructureCommitted, onDetailReloaded };
}

beforeEach(() => {
  vi.mocked(addStoryNode).mockReset();
  vi.mocked(deleteStoryNode).mockReset();
  vi.mocked(moveStoryNode).mockReset();
  vi.mocked(addNodeOption).mockReset();
  vi.mocked(setNodeOptionLink).mockReset();
  vi.mocked(removeNodeOption).mockReset();
  vi.mocked(getStoryDetail).mockReset();
});

describe("useStructureEditor", () => {
  it("flushes pending content BEFORE the mutation and reconciles from the ACK", async () => {
    const order: string[] = [];
    const { result, flushContent, onStructureCommitted } = setup();
    flushContent.mockImplementation(() => order.push("flush"));
    vi.mocked(addStoryNode).mockImplementation(() => {
      order.push("mutate");
      return Promise.resolve(ackWith(STRUCTURE));
    });

    act(() => {
      result.current.addNode();
    });
    await waitFor(() => expect(onStructureCommitted).toHaveBeenCalled());

    expect(order).toEqual(["flush", "mutate"]);
    expect(onStructureCommitted).toHaveBeenCalledWith(ackWith(STRUCTURE));
    expect(result.current.busy).toBe(false);
    expect(result.current.lastError).toBeNull();
  });

  it("is single-flight: a second action during an in-flight mutation no-ops", async () => {
    const { result, onStructureCommitted } = setup();
    let resolveFirst: (v: StructureWriteOutput) => void = () => undefined;
    vi.mocked(addStoryNode).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
    );

    act(() => {
      result.current.addNode();
    });
    expect(result.current.busy).toBe(true);
    // The mutation starts after the awaited flush settles (a microtask).
    await waitFor(() => expect(addStoryNode).toHaveBeenCalledTimes(1));
    act(() => {
      result.current.addNode();
      result.current.moveNode("n2", "up");
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(addStoryNode).toHaveBeenCalledTimes(1);
    expect(moveStoryNode).not.toHaveBeenCalled();

    act(() => {
      resolveFirst(ackWith(STRUCTURE));
    });
    await waitFor(() => expect(onStructureCommitted).toHaveBeenCalledTimes(1));
    expect(result.current.busy).toBe(false);
  });

  it("WAITS for the content flush before firing the mutation (never races it)", async () => {
    const { result, flushContent, onStructureCommitted } = setup();
    let resolveFlush: () => void = () => undefined;
    flushContent.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveFlush = resolve;
        }),
    );
    vi.mocked(addStoryNode).mockResolvedValue(ackWith(STRUCTURE));

    act(() => {
      result.current.addNode();
    });
    // The flush promise is still pending: the mutation must NOT have fired.
    await act(async () => {
      await Promise.resolve();
    });
    expect(flushContent).toHaveBeenCalledTimes(1);
    expect(addStoryNode).not.toHaveBeenCalled();

    act(() => {
      resolveFlush();
    });
    await waitFor(() => expect(addStoryNode).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onStructureCommitted).toHaveBeenCalled());
  });

  it("WAITS for the content flush before the targeted selection re-read", async () => {
    const { result, flushContent } = setup();
    let resolveFlush: () => void = () => undefined;
    flushContent.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveFlush = resolve;
        }),
    );
    vi.mocked(getStoryDetail).mockResolvedValue(detailWith("n2"));

    act(() => {
      result.current.selectNode("n2");
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(getStoryDetail).not.toHaveBeenCalled();
    expect(result.current.busy).toBe(true);

    act(() => {
      resolveFlush();
    });
    await waitFor(() => expect(result.current.selectedNodeId).toBe("n2"));
  });

  it("drops a superseded response after the story changed (never reconciled)", async () => {
    const { result, rerender, onStructureCommitted } = setup();
    let resolveLate: (v: StructureWriteOutput) => void = () => undefined;
    vi.mocked(addStoryNode).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveLate = resolve;
        }),
    );

    act(() => {
      result.current.addNode();
    });
    // The story changes while the mutation is in flight.
    rerender({ storyId: "story-2", editable: true });
    act(() => {
      resolveLate(ackWith(STRUCTURE));
    });
    await Promise.resolve();
    expect(onStructureCommitted).not.toHaveBeenCalled();
    expect(result.current.busy).toBe(false);
  });

  it("surfaces a refused mutation as a localized error, nothing reconciled", async () => {
    const { result, onStructureCommitted } = setup();
    vi.mocked(setNodeOptionLink).mockRejectedValue({
      code: "LIBRARY_INCONSISTENT",
      message: "La destination choisie n'existe plus dans l'histoire.",
      userAction: "Recharge l'éditeur puis choisis un nœud existant.",
      details: null,
    });

    act(() => {
      result.current.setOptionLink("n1", 0, "ghost");
    });
    await waitFor(() => expect(result.current.lastError).not.toBeNull());

    expect(result.current.lastError?.context).toEqual({
      kind: "option",
      nodeId: "n1",
      optionIndex: 0,
    });
    expect(result.current.lastError?.error.message).toContain(
      "n'existe plus",
    );
    expect(onStructureCommitted).not.toHaveBeenCalled();
    expect(result.current.busy).toBe(false);

    act(() => {
      result.current.clearError();
    });
    expect(result.current.lastError).toBeNull();
  });

  it("selectNode flushes, re-reads the detail TARGETED at the node, and updates the selection", async () => {
    const order: string[] = [];
    const { result, flushContent, onDetailReloaded } = setup();
    flushContent.mockImplementation(() => order.push("flush"));
    vi.mocked(getStoryDetail).mockImplementation((input) => {
      order.push(`read:${input.nodeId ?? "start"}`);
      return Promise.resolve(detailWith(input.nodeId ?? "n1"));
    });

    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(onDetailReloaded).toHaveBeenCalled());

    expect(order).toEqual(["flush", "read:n2"]);
    expect(getStoryDetail).toHaveBeenCalledWith({
      storyId: "story-1",
      nodeId: "n2",
    });
    expect(result.current.selectedNodeId).toBe("n2");
  });

  it("selectNode on the already-selected node is a no-op", async () => {
    const { result } = setup();
    vi.mocked(getStoryDetail).mockResolvedValue(detailWith("n2"));
    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.selectedNodeId).toBe("n2"));
    vi.mocked(getStoryDetail).mockClear();

    act(() => {
      result.current.selectNode("n2");
    });
    expect(getStoryDetail).not.toHaveBeenCalled();
  });

  it("supersedes the selection continuation when the story changes during the flush", async () => {
    const { result, rerender, onDetailReloaded } = setup();
    let resolveFlush: () => void = () => undefined;
    const view = result;
    // First render's flushContent is the mock captured by setup(); make it
    // pending so the story switch happens mid-flush.
    const flush = new Promise<void>((resolve) => {
      resolveFlush = resolve;
    });
    // Re-wire flushContent through a fresh mock implementation.
    // (setup()'s flushContent is shared by reference.)
    void view;
    const { flushContent } = setupRefs.current!;
    flushContent.mockImplementation(() => flush);

    act(() => {
      result.current.selectNode("n2");
    });
    // The story changes while the flush is pending.
    rerender({ storyId: "story-2", editable: true });
    act(() => {
      resolveFlush();
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    // The superseded continuation neither re-read nor landed a selection
    // from the OLD story.
    expect(getStoryDetail).not.toHaveBeenCalled();
    expect(onDetailReloaded).not.toHaveBeenCalled();
    expect(result.current.selectedNodeId).toBeNull();
  });

  it("releases busy and surfaces a global error when the flush rejects", async () => {
    const { result, flushContent } = setup();
    flushContent.mockRejectedValue(new Error("flush broke its contract"));

    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.lastError).not.toBeNull());
    // The editor is NOT bricked: busy released, the error surfaced.
    expect(result.current.busy).toBe(false);
    expect(result.current.lastError?.context).toEqual({ kind: "global" });
  });

  it("surfaces a vanished story on selection instead of dying silently", async () => {
    const { result } = setup();
    vi.mocked(getStoryDetail).mockResolvedValue(null);

    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.lastError).not.toBeNull());
    expect(result.current.busy).toBe(false);
    expect(result.current.lastError?.error.message).toContain("introuvable");
    expect(result.current.lastError?.context).toEqual({ kind: "global" });
  });

  it("surfaces an out-of-contract detail payload instead of dying silently", async () => {
    const { result } = setup();
    vi.mocked(getStoryDetail).mockResolvedValue({
      id: "story-1",
    } as never);

    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.lastError).not.toBeNull());
    expect(result.current.lastError?.error.message).toContain(
      "forme inattendue",
    );
  });

  it("deleting the SELECTED node falls back to the start node via a targeted re-read", async () => {
    const { result, onDetailReloaded } = setup();
    vi.mocked(getStoryDetail).mockResolvedValue(detailWith("n2"));
    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.selectedNodeId).toBe("n2"));

    vi.mocked(deleteStoryNode).mockResolvedValue(ackWith(STRUCTURE));
    vi.mocked(getStoryDetail).mockResolvedValue(detailWith("n1"));
    onDetailReloaded.mockClear();

    act(() => {
      result.current.deleteNode("n2");
    });
    await waitFor(() => expect(onDetailReloaded).toHaveBeenCalled());

    // The post-delete re-read targets the START node (nodeId omitted).
    const readCalls = vi.mocked(getStoryDetail).mock.calls;
    expect(readCalls[readCalls.length - 1]?.[0]).toEqual({
      storyId: "story-1",
      nodeId: undefined,
    });
    expect(result.current.selectedNodeId).toBe("n1");
  });

  it("deleting a NON-selected node reconciles from the ACK without a re-read", async () => {
    const { result, onStructureCommitted, onDetailReloaded } = setup();
    vi.mocked(deleteStoryNode).mockResolvedValue(ackWith(STRUCTURE));

    act(() => {
      result.current.deleteNode("n2");
    });
    await waitFor(() => expect(onStructureCommitted).toHaveBeenCalled());
    expect(getStoryDetail).not.toHaveBeenCalled();
    expect(onDetailReloaded).not.toHaveBeenCalled();
  });

  it("never fires a mutation for a non-editable (imported) story", () => {
    const { result } = setup({ editable: false });
    act(() => {
      result.current.addNode();
      result.current.deleteNode("n2");
      result.current.moveNode("n2", "up");
      result.current.addOption("n1", "X");
      result.current.setOptionLink("n1", 0, "n2");
      result.current.removeOption("n1", 0);
    });
    expect(addStoryNode).not.toHaveBeenCalled();
    expect(deleteStoryNode).not.toHaveBeenCalled();
    expect(moveStoryNode).not.toHaveBeenCalled();
    expect(addNodeOption).not.toHaveBeenCalled();
    expect(setNodeOptionLink).not.toHaveBeenCalled();
    expect(removeNodeOption).not.toHaveBeenCalled();
  });

  it("resets the selection when the STORY changes (meaningful re-seed)", async () => {
    const { result, rerender } = setup();
    vi.mocked(getStoryDetail).mockResolvedValue(detailWith("n2"));
    act(() => {
      result.current.selectNode("n2");
    });
    await waitFor(() => expect(result.current.selectedNodeId).toBe("n2"));

    rerender({ storyId: "story-2", editable: true });
    expect(result.current.selectedNodeId).toBeNull();
    expect(result.current.lastError).toBeNull();
    expect(result.current.busy).toBe(false);
  });
});
