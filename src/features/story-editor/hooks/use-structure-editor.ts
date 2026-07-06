import { useCallback, useEffect, useRef, useState } from "react";

import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import {
  addNodeOption,
  addStoryNode,
  deleteStoryNode,
  getStoryDetail,
  moveStoryNode,
  removeNodeOption,
  setNodeOptionLink,
} from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type {
  StoryDetailDto,
  StoryStructure,
  StructureWriteOutput,
} from "../../../shared/ipc-contracts/story";
import { isStoryDetailDto } from "../../../shared/ipc-contracts/story";

/** Where a refused structural mutation surfaces its inline alert. */
export type StructureErrorContext =
  | { kind: "node"; nodeId: string }
  | { kind: "option"; nodeId: string; optionIndex: number }
  | { kind: "global" };

export interface StructureError {
  error: AppError;
  context: StructureErrorContext;
}

export interface UseStructureEditor {
  /** The node the editor zone shows. `null` = the start node (Rust's
   *  default projection). */
  selectedNodeId: string | null;
  /** A structural mutation or a targeted re-read is in flight
   *  (single-flight: new actions no-op until it settles). */
  busy: boolean;
  /** The last refused mutation, localized to the acted-on entry. */
  lastError: StructureError | null;
  clearError: () => void;
  /** Select a node as the current node (flushes pending content first,
   *  then re-reads the detail targeted at that node). */
  selectNode: (nodeId: string) => void;
  /** Authoritative targeted re-read of the CURRENT selection — for external
   *  commits (a cross-node recovery apply) that changed the canonical row
   *  outside the structural write path. */
  refreshDetail: () => void;
  addNode: () => void;
  /** Create an empty node AND link the given option to it, atomically. */
  addNodeAndLink: (nodeId: string, optionIndex: number) => void;
  deleteNode: (nodeId: string) => void;
  moveNode: (nodeId: string, direction: "up" | "down") => void;
  addOption: (nodeId: string, label: string) => void;
  setOptionLink: (
    nodeId: string,
    optionIndex: number,
    target: string | null,
  ) => void;
  removeOption: (nodeId: string, optionIndex: number) => void;
}

export interface UseStructureEditorArgs {
  storyId: string | undefined;
  /** The Rust-projected graph (`detail.structure`), `null` when degraded. */
  structure: StoryStructure | null;
  /** Defensive non-editable flag: every action no-ops when `false`. A
   *  device pack never mounts the structural controls at all and a
   *  `.rustory` import is fully editable — this is defense in depth
   *  (Rust's edit-scope guard is the authority). */
  editable: boolean;
  /** Flush the pending content autosaves (title + node) BEFORE any
   *  structural mutation or selection change — a mid-debounce keystroke
   *  must never be lost nor land after the structure moved. The returned
   *  promise is AWAITED: the mutation / targeted re-read only starts once
   *  the flushed save has settled. Must never reject. */
  flushContent: () => void | Promise<void>;
  /** Reconcile the authoritative detail from a structural write ACK. */
  onStructureCommitted: (output: StructureWriteOutput) => void;
  /** Replace the whole detail after a targeted authoritative re-read. */
  onDetailReloaded: (detail: StoryDetailDto) => void;
}

/**
 * Structural-mutation hook for the story graph. Every mutation is an
 * EXPLICIT, ACKNOWLEDGED action (never debounced, never optimistic):
 * single-flight (`busy` gates re-entry), call-correlated (a superseded
 * response is dropped, never reconciled), and the UI state is rebuilt ONLY
 * from the `StructureWriteOutput.structure` the Rust core re-projected —
 * never from a locally recomposed graph.
 *
 * The selection of the current node is LOCAL state here (never the Zustand
 * shell store): selecting re-reads `get_story_detail(storyId, nodeId)`
 * authoritatively so the node editor re-seeds from Rust. Deleting the
 * selected node falls back to the start node through the same targeted
 * re-read.
 */
export function useStructureEditor(
  args: UseStructureEditorArgs,
): UseStructureEditor {
  const {
    storyId,
    structure,
    editable,
    flushContent,
    onStructureCommitted,
    onDetailReloaded,
  } = args;

  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastError, setLastError] = useState<StructureError | null>(null);

  const mountedRef = useRef(true);
  const activeCallRef = useRef(0);
  const busyRef = useRef(false);
  const selectedNodeIdRef = useRef<string | null>(null);
  selectedNodeIdRef.current = selectedNodeId;

  // Meaningful re-seed key (not an object reference): the selection resets
  // when the STORY changes, never on a mere re-render or a re-projection of
  // the same story.
  const lastStoryRef = useRef<string | undefined>(storyId);
  useEffect(() => {
    if (lastStoryRef.current === storyId) return;
    lastStoryRef.current = storyId;
    activeCallRef.current += 1;
    busyRef.current = false;
    setSelectedNodeId(null);
    setBusy(false);
    setLastError(null);
  }, [storyId]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const clearError = useCallback(() => {
    setLastError(null);
  }, []);

  /** Shared spine of every acknowledged mutation. */
  const runMutation = useCallback(
    (
      context: StructureErrorContext,
      mutate: () => Promise<StructureWriteOutput>,
      afterCommit?: (output: StructureWriteOutput) => void,
    ) => {
      if (!editable || !storyId) return;
      // Single-flight: explicit actions never queue a second concurrent
      // write — the controls are disabled while busy, and a programmatic
      // re-entry no-ops.
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setLastError(null);
      const callId = ++activeCallRef.current;
      // The pending content commits FIRST — and is AWAITED — so a keystroke
      // typed mid-debounce is durably persisted before the structure moves;
      // firing the mutation while the save is still in flight could land
      // the two writes out of order.
      void Promise.resolve()
        .then(() => flushContent())
        .then(() => mutate())
        .then((output) => {
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) return;
          setBusy(false);
          onStructureCommitted(output);
          invalidateLibraryOverviewCache();
          afterCommit?.(output);
        })
        .catch((err: unknown) => {
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) return;
          setBusy(false);
          setLastError({ error: toAppError(err), context });
        });
    },
    [editable, storyId, flushContent, onStructureCommitted],
  );

  /**
   * Authoritative targeted re-read (selection change / post-delete
   * fallback). `nodeId = null` targets the start node.
   */
  const reloadDetail = useCallback(
    (nodeId: string | null) => {
      if (!storyId) return;
      const callId = ++activeCallRef.current;
      busyRef.current = true;
      setBusy(true);
      void getStoryDetail({ storyId, nodeId: nodeId ?? undefined })
        .then((detail) => {
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) return;
          setBusy(false);
          if (detail === null) {
            // The story vanished concurrently: the selection click must not
            // die silently — mirror the write path's hard surfacing.
            setLastError({
              error: {
                code: "LIBRARY_INCONSISTENT",
                message: "Histoire introuvable, recharge la bibliothèque.",
                userAction: "Retourne à la bibliothèque et recharge la liste.",
                details: null,
              },
              context: { kind: "global" },
            });
            return;
          }
          if (!isStoryDetailDto(detail)) {
            // A drifted payload is refused loudly, symmetric with the
            // structural-write drift error — never a silent no-op.
            setLastError({
              error: {
                code: "LIBRARY_INCONSISTENT",
                message: "get_story_detail a renvoyé une forme inattendue.",
                userAction: "Relance Rustory pour reconstruire la vue cohérente.",
                details: null,
              },
              context: { kind: "global" },
            });
            return;
          }
          onDetailReloaded(detail);
          // The selection follows what Rust actually projected: a stale id
          // over a healthy graph falls back to the start node.
          setSelectedNodeId(detail.node?.id ?? null);
        })
        .catch((err: unknown) => {
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) return;
          setBusy(false);
          setLastError({ error: toAppError(err), context: { kind: "global" } });
        });
    },
    [storyId, onDetailReloaded],
  );

  const selectNode = useCallback(
    (nodeId: string) => {
      if (!storyId) return;
      if (busyRef.current) return;
      if (nodeId === selectedNodeIdRef.current) return;
      // Flush the node being LEFT — awaited — before the selection moves
      // (NFR6/NFR8): the targeted re-read must observe the flushed content,
      // not race it. The busy latch covers the flush window so no other
      // action interleaves, and the callId is taken BEFORE the flush so a
      // story switch during the wait supersedes the whole continuation (the
      // re-read of the OLD story must never run, nor its selection land).
      busyRef.current = true;
      setBusy(true);
      const callId = ++activeCallRef.current;
      void Promise.resolve()
        .then(() => flushContent())
        .then(() => {
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) {
            if (mountedRef.current) setBusy(false);
            return;
          }
          reloadDetail(nodeId);
        })
        .catch((err: unknown) => {
          // `flushContent` is contracted to never reject, but a broken
          // contract must not brick the structure editor with `busy` latched
          // forever — release and surface.
          const current = callId === activeCallRef.current;
          if (current) busyRef.current = false;
          if (!mountedRef.current || !current) return;
          setBusy(false);
          setLastError({ error: toAppError(err), context: { kind: "global" } });
        });
    },
    [storyId, flushContent, reloadDetail],
  );

  const addNode = useCallback(() => {
    if (!storyId) return;
    runMutation({ kind: "global" }, () => addStoryNode({ storyId }));
  }, [storyId, runMutation]);

  const addNodeAndLink = useCallback(
    (nodeId: string, optionIndex: number) => {
      if (!storyId) return;
      runMutation({ kind: "option", nodeId, optionIndex }, () =>
        addStoryNode({ storyId, linkFrom: { nodeId, optionIndex } }),
      );
    },
    [storyId, runMutation],
  );

  const deleteNode = useCallback(
    (nodeId: string) => {
      if (!storyId) return;
      const wasSelected =
        nodeId === selectedNodeIdRef.current ||
        // `null` selection = the start node; Rust refuses deleting it, so
        // the fallback below only ever runs for an explicitly selected node.
        (selectedNodeIdRef.current === null &&
          structure?.startNodeId === nodeId);
      runMutation(
        { kind: "node", nodeId },
        () => deleteStoryNode({ storyId, nodeId }),
        () => {
          // Deleting the SELECTED node: the projected node content is now
          // stale — fall back to the start node through a targeted
          // authoritative re-read (structure ↔ node stay coherent).
          if (wasSelected) reloadDetail(null);
        },
      );
    },
    [storyId, structure, runMutation, reloadDetail],
  );

  const moveNode = useCallback(
    (nodeId: string, direction: "up" | "down") => {
      if (!storyId) return;
      runMutation({ kind: "node", nodeId }, () =>
        moveStoryNode({ storyId, nodeId, direction }),
      );
    },
    [storyId, runMutation],
  );

  const addOption = useCallback(
    (nodeId: string, label: string) => {
      if (!storyId) return;
      runMutation({ kind: "node", nodeId }, () =>
        addNodeOption({ storyId, nodeId, label }),
      );
    },
    [storyId, runMutation],
  );

  const setOptionLink = useCallback(
    (nodeId: string, optionIndex: number, target: string | null) => {
      if (!storyId) return;
      runMutation({ kind: "option", nodeId, optionIndex }, () =>
        setNodeOptionLink({ storyId, nodeId, optionIndex, target }),
      );
    },
    [storyId, runMutation],
  );

  const removeOption = useCallback(
    (nodeId: string, optionIndex: number) => {
      if (!storyId) return;
      runMutation({ kind: "option", nodeId, optionIndex }, () =>
        removeNodeOption({ storyId, nodeId, optionIndex }),
      );
    },
    [storyId, runMutation],
  );

  const refreshDetail = useCallback(() => {
    if (!storyId) return;
    if (busyRef.current) return;
    reloadDetail(selectedNodeIdRef.current);
  }, [storyId, reloadDetail]);

  return {
    selectedNodeId,
    busy,
    lastError,
    clearError,
    selectNode,
    refreshDetail,
    addNode,
    addNodeAndLink,
    deleteNode,
    moveNode,
    addOption,
    setOptionLink,
    removeOption,
  };
}
