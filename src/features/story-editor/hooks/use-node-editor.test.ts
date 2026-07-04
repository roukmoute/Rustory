import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/story", () => ({
  updateNodeContent: vi.fn(),
  attachNodeMedia: vi.fn(),
  removeNodeMedia: vi.fn(),
  recordNodeDraft: vi.fn().mockResolvedValue(undefined),
  readRecoverableNodeDraft: vi.fn().mockResolvedValue({ kind: "none" }),
  discardNodeDraft: vi.fn().mockResolvedValue(undefined),
}));

const invalidateMock = vi.hoisted(() => vi.fn());
vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: invalidateMock,
}));

import {
  attachNodeMedia,
  readRecoverableNodeDraft,
  recordNodeDraft,
  removeNodeMedia,
  updateNodeContent,
} from "../../../ipc/commands/story";
import type {
  NodeContentDto,
  NodeWriteOutput,
} from "../../../shared/ipc-contracts/story";
import { useNodeEditor } from "./use-node-editor";

const NODE: NodeContentDto = {
  id: "n1",
  text: "",
  label: "",
  image: null,
  audio: null,
};

function outputWith(node: Partial<NodeContentDto>): NodeWriteOutput {
  return {
    id: "s1",
    updatedAt: "2026-06-27T10:00:00.000Z",
    contentChecksum: "a".repeat(64),
    node: { ...NODE, ...node },
  };
}

describe("useNodeEditor", () => {
  beforeEach(() => {
    vi.mocked(updateNodeContent).mockReset();
    vi.mocked(attachNodeMedia).mockReset();
    vi.mocked(removeNodeMedia).mockReset();
    vi.mocked(recordNodeDraft).mockReset().mockResolvedValue(undefined);
    vi.mocked(readRecoverableNodeDraft)
      .mockReset()
      .mockResolvedValue({ kind: "none" });
    invalidateMock.mockReset();
  });

  it("seeds its state from the projected node", () => {
    const { result } = renderHook(() =>
      useNodeEditor("s1", { ...NODE, text: "Bonjour", label: "Début" }, true),
    );
    expect(result.current.nodeId).toBe("n1");
    expect(result.current.text).toBe("Bonjour");
    expect(result.current.label).toBe("Début");
    expect(result.current.editable).toBe(true);
  });

  it("debounces a text edit into a single update_node_content", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(updateNodeContent).mockResolvedValue(outputWith({ text: "abc" }));
      const { result } = renderHook(() => useNodeEditor("s1", NODE, true));

      act(() => result.current.setText("a"));
      act(() => result.current.setText("ab"));
      act(() => result.current.setText("abc"));
      expect(result.current.saveStatus.kind).toBe("pending");
      expect(updateNodeContent).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(updateNodeContent).toHaveBeenCalledTimes(1);
      expect(updateNodeContent).toHaveBeenCalledWith({
        storyId: "s1",
        nodeId: "n1",
        text: "abc",
        label: "",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("buffers the node text via record_node_draft (NFR8)", async () => {
    vi.useFakeTimers();
    try {
      const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
      act(() => result.current.setText("x"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(150);
      });
      expect(recordNodeDraft).toHaveBeenCalledWith({
        storyId: "s1",
        nodeId: "n1",
        draftText: "x",
        draftLabel: "",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not save when the value returns to the persisted one", async () => {
    vi.useFakeTimers();
    try {
      const { result } = renderHook(() =>
        useNodeEditor("s1", { ...NODE, text: "saved" }, true),
      );
      act(() => result.current.setText("saved-edited"));
      act(() => result.current.setText("saved"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(updateNodeContent).not.toHaveBeenCalled();
      expect(result.current.saveStatus.kind).toBe("idle");
    } finally {
      vi.useRealTimers();
    }
  });

  it("flushNodeAutoSave commits immediately without the debounce", async () => {
    vi.mocked(updateNodeContent).mockResolvedValue(outputWith({ text: "z" }));
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    act(() => result.current.setText("z"));
    await act(async () => {
      result.current.flushNodeAutoSave();
      await Promise.resolve();
    });
    expect(updateNodeContent).toHaveBeenCalledTimes(1);
  });

  it("acknowledges an attached media by reconciling the slot from the output", async () => {
    vi.mocked(attachNodeMedia).mockResolvedValue({
      kind: "attached",
      output: outputWith({
        image: { assetId: "a1", mediaType: "image", state: "ready", format: "png", byteSize: 9 },
      }),
    });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await act(async () => {
      result.current.attachMedia("image");
    });
    await waitFor(() => expect(result.current.image?.state).toBe("ready"));
    expect(result.current.imageBusy).toBe(false);
    expect(invalidateMock).toHaveBeenCalled();
  });

  it("surfaces a blocking attach error at the slot, leaving the node editable", async () => {
    vi.mocked(attachNodeMedia).mockRejectedValue({
      code: "MEDIA_INVALID",
      message: "Format non pris en charge.",
      userAction: "Choisis un PNG.",
      details: null,
    });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await act(async () => {
      result.current.attachMedia("image");
    });
    await waitFor(() =>
      expect(result.current.imageError?.code).toBe("MEDIA_INVALID"),
    );
    expect(result.current.image).toBeNull();
  });

  it("clears a slot on remove", async () => {
    vi.mocked(removeNodeMedia).mockResolvedValue(outputWith({ image: null }));
    const { result } = renderHook(() =>
      useNodeEditor(
        "s1",
        { ...NODE, image: { assetId: "a1", mediaType: "image", state: "ready", format: "png", byteSize: 9 } },
        true,
      ),
    );
    await act(async () => {
      result.current.removeMedia("image");
    });
    await waitFor(() => expect(result.current.image).toBeNull());
  });

  it("ignores edits when the node is read-only (imported story)", () => {
    const { result } = renderHook(() => useNodeEditor("s1", NODE, false));
    act(() => result.current.setText("nope"));
    expect(result.current.text).toBe("");
  });

  it("offers a recoverable node draft and applies the buffered value", async () => {
    vi.mocked(readRecoverableNodeDraft).mockResolvedValue({
      kind: "recoverable",
      storyId: "s1",
      nodeId: "n1",
      draftText: "buffered",
      draftLabel: "",
      draftAt: "2026-06-27T12:00:00.000Z",
      persistedText: "",
      persistedLabel: "",
    });
    vi.mocked(updateNodeContent).mockResolvedValue(outputWith({ text: "buffered" }));
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await waitFor(() => expect(result.current.recovery.kind).toBe("recoverable"));

    await act(async () => {
      result.current.applyRecovery();
      await Promise.resolve();
    });
    expect(result.current.recovery.kind).toBe("none");
    expect(updateNodeContent).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n1",
      text: "buffered",
      label: "",
    });
  });

  it("applies a recovered draft to ITS OWN node when it differs from the displayed one", async () => {
    // The editor opened on the start node (n1) while the crash interrupted
    // n2: the recovered content must be persisted EXPLICITLY to n2 — never
    // through the displayed node's save path — and the displayed fields
    // stay untouched.
    vi.mocked(readRecoverableNodeDraft).mockResolvedValue({
      kind: "recoverable",
      storyId: "s1",
      nodeId: "n2",
      draftText: "texte de n2",
      draftLabel: "lab n2",
      draftAt: "2026-07-04T12:00:00.000Z",
      persistedText: "ancien n2",
      persistedLabel: "",
    });
    vi.mocked(updateNodeContent).mockResolvedValue(
      outputWith({ id: "n2", text: "texte de n2", label: "lab n2" }),
    );
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await waitFor(() => expect(result.current.recovery.kind).toBe("recoverable"));

    await act(async () => {
      result.current.applyRecovery();
      await Promise.resolve();
    });
    expect(result.current.recovery.kind).toBe("none");
    expect(updateNodeContent).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n2",
      text: "texte de n2",
      label: "lab n2",
    });
    // The displayed node's live fields are NOT clobbered by the other
    // node's recovered content.
    expect(result.current.text).toBe("");
    expect(result.current.label).toBe("");
  });

  it("surfaces a failed cross-node recovery apply and re-proposes the offer", async () => {
    vi.mocked(readRecoverableNodeDraft).mockResolvedValue({
      kind: "recoverable",
      storyId: "s1",
      nodeId: "n2",
      draftText: "texte de n2",
      draftLabel: "",
      draftAt: "2026-07-04T12:00:00.000Z",
      persistedText: "",
      persistedLabel: "",
    });
    vi.mocked(updateNodeContent).mockRejectedValue({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Rustory n'a pas pu enregistrer ta modification.",
      userAction: "Réessaie dans un instant.",
      details: null,
    });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await waitFor(() => expect(result.current.recovery.kind).toBe("recoverable"));

    await act(async () => {
      result.current.applyRecovery();
      await Promise.resolve();
      await Promise.resolve();
    });
    // An EXPLICIT gesture failed: the error is visible AND the offer is
    // re-proposed (the probe is per-story deduplicated — a silent drop
    // would strand the draft for the whole session).
    expect(result.current.recoveryApplyError?.message).toContain(
      "n'a pas pu enregistrer",
    );
    expect(result.current.recovery.kind).toBe("recoverable");
  });

  it("triggers the owner's targeted re-read after a cross-node recovery commit", async () => {
    vi.mocked(readRecoverableNodeDraft).mockResolvedValue({
      kind: "recoverable",
      storyId: "s1",
      nodeId: "n2",
      draftText: "texte de n2",
      draftLabel: "",
      draftAt: "2026-07-04T12:00:00.000Z",
      persistedText: "",
      persistedLabel: "",
    });
    vi.mocked(updateNodeContent).mockResolvedValue(
      outputWith({ id: "n2", text: "texte de n2" }),
    );
    const onCrossNodeRecoveryApplied = vi.fn();
    const { result } = renderHook(() =>
      useNodeEditor("s1", NODE, true, { onCrossNodeRecoveryApplied }),
    );
    await waitFor(() => expect(result.current.recovery.kind).toBe("recoverable"));

    await act(async () => {
      result.current.applyRecovery();
      await Promise.resolve();
      await Promise.resolve();
    });
    // The commit changed the canonical row outside the structural write
    // path: the owner re-reads authoritatively (structureJson/checksum pair,
    // navigator labels).
    expect(onCrossNodeRecoveryApplied).toHaveBeenCalledTimes(1);
    expect(result.current.recoveryApplyError).toBeNull();
  });

  it("re-buffers a cross-node recovery best-effort when its save fails", async () => {
    vi.mocked(readRecoverableNodeDraft).mockResolvedValue({
      kind: "recoverable",
      storyId: "s1",
      nodeId: "n2",
      draftText: "texte de n2",
      draftLabel: "",
      draftAt: "2026-07-04T12:00:00.000Z",
      persistedText: "",
      persistedLabel: "",
    });
    vi.mocked(updateNodeContent).mockRejectedValue({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "x",
      userAction: "y",
      details: null,
    });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    await waitFor(() => expect(result.current.recovery.kind).toBe("recoverable"));

    await act(async () => {
      result.current.applyRecovery();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(recordNodeDraft).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n2",
      draftText: "texte de n2",
      draftLabel: "",
    });
  });

  it("flush covers the LATEST keystroke even when an older save is in flight (NFR8)", async () => {
    // Save of "v1" in flight; the user types "v2"; an awaited flush must not
    // settle until "v2" itself is persisted — returning the stale in-flight
    // promise alone would let a structural mutation proceed with "v2"
    // neither committed nor buffered.
    let resolveFirst: (v: NodeWriteOutput) => void = () => undefined;
    const calls: Array<{ text: string }> = [];
    vi.mocked(updateNodeContent).mockImplementation((input) => {
      calls.push({ text: input.text });
      if (calls.length === 1) {
        return new Promise((resolve) => {
          resolveFirst = resolve;
        });
      }
      return Promise.resolve(outputWith({ text: input.text }));
    });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));

    // First save (v1) fires and stays in flight.
    act(() => result.current.setText("v1"));
    let firstFlush: Promise<void> = Promise.resolve();
    act(() => {
      firstFlush = result.current.flushNodeAutoSave();
    });
    void firstFlush;
    expect(calls).toEqual([{ text: "v1" }]);

    // The draft moves on to v2 while v1 is still in flight.
    act(() => result.current.setText("v2"));
    let settled = false;
    let flushed: Promise<void> = Promise.resolve();
    act(() => {
      flushed = result.current.flushNodeAutoSave().then(() => {
        settled = true;
      });
    });
    await act(async () => {
      await Promise.resolve();
    });
    // Still pending: v1 has not landed, v2 not fired yet.
    expect(settled).toBe(false);

    act(() => {
      resolveFirst(outputWith({ text: "v1" }));
    });
    await act(async () => {
      await flushed;
    });
    // The awaited flush settled only AFTER the follow-up save carried v2.
    expect(calls).toEqual([{ text: "v1" }, { text: "v2" }]);
    expect(settled).toBe(true);
  });

  it("flushes pending text BEFORE a media action so it is not stranded (F2)", async () => {
    vi.mocked(updateNodeContent).mockResolvedValue(outputWith({ text: "dirty" }));
    vi.mocked(attachNodeMedia).mockResolvedValue({ kind: "cancelled" });
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));
    act(() => result.current.setText("dirty"));
    await act(async () => {
      result.current.attachMedia("image");
      await Promise.resolve();
    });
    expect(updateNodeContent).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n1",
      text: "dirty",
      label: "",
    });
    expect(attachNodeMedia).toHaveBeenCalled();
  });

  it("commits a dirty node on unmount (non-button navigation) (F3)", async () => {
    vi.mocked(updateNodeContent).mockResolvedValue(outputWith({ text: "leaving" }));
    const { result, unmount } = renderHook(() => useNodeEditor("s1", NODE, true));
    act(() => result.current.setText("leaving"));
    await act(async () => {
      unmount();
      await Promise.resolve();
    });
    expect(updateNodeContent).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n1",
      text: "leaving",
      label: "",
    });
  });

  it("never starts a second content write while one is in flight (single-flight, P5)", async () => {
    let resolveFirst: (value: NodeWriteOutput) => void = () => {};
    vi.mocked(updateNodeContent).mockReturnValueOnce(
      new Promise<NodeWriteOutput>((resolve) => {
        resolveFirst = resolve;
      }),
    );
    const { result } = renderHook(() => useNodeEditor("s1", NODE, true));

    act(() => result.current.setText("first"));
    await act(async () => {
      result.current.flushNodeAutoSave();
      await Promise.resolve();
    });
    expect(updateNodeContent).toHaveBeenCalledTimes(1);

    // A second flush while the first write is still in flight must NOT fire a
    // concurrent write: two writes can land on the SQLite mutex out of order
    // and let an older value overwrite a newer one. The hook re-plans instead.
    act(() => result.current.setText("second"));
    await act(async () => {
      result.current.flushNodeAutoSave();
      await Promise.resolve();
    });
    expect(updateNodeContent).toHaveBeenCalledTimes(1);

    // Drain the deferred write so the hook settles cleanly.
    await act(async () => {
      resolveFirst(outputWith({ text: "first" }));
      await Promise.resolve();
    });
  });

  it("buffers a recovery draft on unmount when a save is in flight and dirty (P8)", async () => {
    let resolveSave: (value: NodeWriteOutput) => void = () => {};
    vi.mocked(updateNodeContent).mockReturnValueOnce(
      new Promise<NodeWriteOutput>((resolve) => {
        resolveSave = resolve;
      }),
    );
    const { result, unmount } = renderHook(() => useNodeEditor("s1", NODE, true));
    act(() => result.current.setText("inflight"));
    await act(async () => {
      result.current.flushNodeAutoSave();
      await Promise.resolve();
    });

    // A save is now in flight. Unmounting before it resolves means its own
    // `.catch` can no longer buffer on failure (the component is gone), so the
    // cleanup must record a recovery draft itself — otherwise a kill before the
    // next open could lose the keystroke (NFR8).
    await act(async () => {
      unmount();
      await Promise.resolve();
    });
    expect(recordNodeDraft).toHaveBeenCalledWith({
      storyId: "s1",
      nodeId: "n1",
      draftText: "inflight",
      draftLabel: "",
    });

    await act(async () => {
      resolveSave(outputWith({ text: "inflight" }));
      await Promise.resolve();
    });
  });

  it("re-seeds the slot when a projection goes ready→attention at a constant asset (F12)", () => {
    const ready: NodeContentDto["image"] = {
      assetId: "a1",
      mediaType: "image",
      state: "ready",
      format: "png",
      byteSize: 9,
    };
    const { result, rerender } = renderHook(
      ({ node }: { node: NodeContentDto }) => useNodeEditor("s1", node, true),
      { initialProps: { node: { ...NODE, image: ready } } },
    );
    expect(result.current.image?.state).toBe("ready");
    // Same assetId, but the projection now reports the source as missing.
    rerender({
      node: {
        ...NODE,
        image: { assetId: "a1", mediaType: "image", state: "attention" },
      },
    });
    expect(result.current.image?.state).toBe("attention");
  });
});
