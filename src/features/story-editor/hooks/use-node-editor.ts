import { useCallback, useEffect, useRef, useState } from "react";

import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import {
  attachNodeMedia,
  discardNodeDraft,
  readRecoverableNodeDraft,
  recordNodeDraft,
  removeNodeMedia,
  updateNodeContent,
} from "../../../ipc/commands/story";
import { toAppError, type AppError } from "../../../shared/errors/app-error";
import type {
  NodeContentDto,
  NodeMediaSlot,
  NodeMediaSlotKind,
  NodeWriteOutput,
  RecoverableNodeDraft,
} from "../../../shared/ipc-contracts/story";

/** Autosave debounce after the last node keystroke. Mirrors the title autosave. */
export const NODE_AUTOSAVE_DEBOUNCE_MS = 500;
/** Recovery-draft buffer debounce — shorter so a kill -9 between keystrokes
 *  still preserves the typed node text (NFR8). */
export const NODE_DRAFT_RECORD_DEBOUNCE_MS = 150;
/** How long the "Enregistré" chip stays before settling back to idle. */
export const NODE_AUTOSAVE_SAVED_VISIBLE_MS = 3000;

type TimerRef = { current: ReturnType<typeof setTimeout> | null };

function clearTimer(ref: TimerRef): void {
  if (ref.current !== null) {
    clearTimeout(ref.current);
    ref.current = null;
  }
}

export type NodeSaveStatus =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "saving" }
  | { kind: "saved" }
  | { kind: "failed"; error: AppError };

/** A pending node-content recovery offer (NFR8). `nodeId` is the node the
 *  BUFFER belongs to — not necessarily the node currently displayed; the
 *  apply path targets it explicitly so a draft never lands on the wrong
 *  node. */
export type NodeRecovery =
  | { kind: "none" }
  | {
      kind: "recoverable";
      nodeId: string;
      draftText: string;
      draftLabel: string;
      draftAt: string;
      persistedText: string;
      persistedLabel: string;
    };

export interface UseNodeEditor {
  /** Stable id of the current node, or `null` when none is projected. */
  nodeId: string | null;
  /** Whether the node may be edited — a defensive non-editable projection:
   *  a device pack never mounts these controls at all, and a `.rustory`
   *  import is fully editable (full edit scope). */
  editable: boolean;
  /** Live draft values for the two fields. */
  text: string;
  label: string;
  saveStatus: NodeSaveStatus;
  /** Current media slot projections (reconciled after every write). */
  image: NodeMediaSlot | null;
  audio: NodeMediaSlot | null;
  /** Per-slot blocking error (e.g. an unsupported file at attach). */
  imageError: AppError | null;
  audioError: AppError | null;
  imageBusy: boolean;
  audioBusy: boolean;
  /** Node-content recovery offer. */
  recovery: NodeRecovery;
  /** A failed EXPLICIT recovery apply (surfaced inline in the banner —
   *  the offer is re-proposed so the gesture can be retried). */
  recoveryApplyError: AppError | null;
  setText: (next: string) => void;
  setLabel: (next: string) => void;
  /** Commit a pending node autosave immediately (Retour, a structural
   *  mutation, a node-selection change, unmount). Resolves when the flushed
   *  (or already in-flight) save has SETTLED; never rejects. */
  flushNodeAutoSave: () => Promise<void>;
  attachMedia: (slot: NodeMediaSlotKind) => void;
  removeMedia: (slot: NodeMediaSlotKind) => void;
  applyRecovery: () => void;
  discardRecovery: () => void;
}

interface UseNodeEditorOptions {
  debounceMs?: number;
  savedVisibleMs?: number;
  recordDraftDebounceMs?: number;
  /** Called after a CROSS-NODE recovery apply committed (the recovered
   *  content landed on a node other than the displayed one): the owner
   *  triggers an authoritative targeted re-read so the local detail
   *  (structureJson/checksum pair, navigator labels) does not go stale. */
  onCrossNodeRecoveryApplied?: () => void;
  /** Called with every acknowledged node write (content save, media attach
   *  / remove) so the owner can reconcile story-level metadata the ACK
   *  carries (`importState`) into its own detail — the review chip must
   *  never lie after a node write settles a pending review. */
  onWriteAcknowledged?: (output: NodeWriteOutput) => void;
}

/**
 * Editor hook for a story's current node — text + metadata autosave and the
 * image / audio media actions. Mirrors `useStoryEditor`'s autosave discipline
 * (debounce, call-correlation, flush, overview invalidation) but writes node
 * content through `update_node_content` (never the title path). Media actions
 * are explicit and persisted immediately, acknowledged in under a second.
 *
 * The hook consumes the node PROJECTED by Rust (`detail.node`); it never parses
 * `structureJson`. After every successful write it reconciles its slots from
 * the re-projected node the Rust core returns.
 */
export function useNodeEditor(
  storyId: string | undefined,
  projectedNode: NodeContentDto | null,
  editable: boolean,
  options: UseNodeEditorOptions = {},
): UseNodeEditor {
  const debounceMs = options.debounceMs ?? NODE_AUTOSAVE_DEBOUNCE_MS;
  const savedVisibleMs = options.savedVisibleMs ?? NODE_AUTOSAVE_SAVED_VISIBLE_MS;
  const recordDraftDebounceMs =
    options.recordDraftDebounceMs ?? NODE_DRAFT_RECORD_DEBOUNCE_MS;

  const nodeId = projectedNode?.id ?? null;

  // The persisted node values (source of truth), refreshed when the projection
  // changes (initial load, recovery apply, storyId switch).
  const [persisted, setPersisted] = useState(() => ({
    text: projectedNode?.text ?? "",
    label: projectedNode?.label ?? "",
  }));
  const [text, setTextState] = useState(persisted.text);
  const [label, setLabelState] = useState(persisted.label);
  const [saveStatus, setSaveStatus] = useState<NodeSaveStatus>({ kind: "idle" });
  const [image, setImage] = useState<NodeMediaSlot | null>(
    projectedNode?.image ?? null,
  );
  const [audio, setAudio] = useState<NodeMediaSlot | null>(
    projectedNode?.audio ?? null,
  );
  const [imageError, setImageError] = useState<AppError | null>(null);
  const [audioError, setAudioError] = useState<AppError | null>(null);
  const [imageBusy, setImageBusy] = useState(false);
  const [audioBusy, setAudioBusy] = useState(false);
  const [recovery, setRecovery] = useState<NodeRecovery>({ kind: "none" });
  const [recoveryApplyError, setRecoveryApplyError] = useState<AppError | null>(
    null,
  );
  const onCrossNodeRecoveryAppliedRef = useRef(
    options.onCrossNodeRecoveryApplied,
  );
  onCrossNodeRecoveryAppliedRef.current = options.onCrossNodeRecoveryApplied;
  const onWriteAcknowledgedRef = useRef(options.onWriteAcknowledged);
  onWriteAcknowledgedRef.current = options.onWriteAcknowledged;

  const persistedRef = useRef(persisted);
  persistedRef.current = persisted;
  const textRef = useRef(text);
  textRef.current = text;
  const labelRef = useRef(label);
  labelRef.current = label;
  // Mirrors of the live inputs so the `[]`-deps unmount cleanup reads the
  // freshest values rather than the ones captured at mount (F3).
  const storyIdRef = useRef(storyId);
  storyIdRef.current = storyId;
  const nodeIdRef = useRef(nodeId);
  nodeIdRef.current = nodeId;
  const editableRef = useRef(editable);
  editableRef.current = editable;

  const mountedRef = useRef(true);
  const activeCallRef = useRef(0);
  const saveInFlightRef = useRef(false);
  // The settled-promise of the in-flight content save, so a flush caller
  // (structural mutation / selection change) can AWAIT it instead of racing
  // the write it depends on.
  const saveInFlightPromiseRef = useRef<Promise<void> | null>(null);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savedIdleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const recordTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Re-seed local state whenever the projected node identity / values change
  // (a new storyId, a recovery apply patched the detail). Comparing on a
  // stringified identity keeps StrictMode re-renders from clobbering live edits.
  const projectionKey = projectedNode
    ? JSON.stringify({
        id: projectedNode.id,
        text: projectedNode.text,
        label: projectedNode.label,
        image: projectedNode.image,
        audio: projectedNode.audio,
      })
    : "none";
  const lastProjectionRef = useRef<string | null>(null);
  useEffect(() => {
    if (lastProjectionRef.current === projectionKey) return;
    lastProjectionRef.current = projectionKey;
    activeCallRef.current += 1;
    saveInFlightRef.current = false;
    clearTimer(debounceTimerRef);
    clearTimer(savedIdleTimerRef);
    clearTimer(recordTimerRef);
    setPersisted({
      text: projectedNode?.text ?? "",
      label: projectedNode?.label ?? "",
    });
    setTextState(projectedNode?.text ?? "");
    setLabelState(projectedNode?.label ?? "");
    setImage(projectedNode?.image ?? null);
    setAudio(projectedNode?.audio ?? null);
    setSaveStatus({ kind: "idle" });
  }, [projectionKey, projectedNode, nodeId]);

  // A content save owns ONLY text/label; it must NOT re-apply the media slots
  // from its snapshot (those are owned by the media actions, so a content
  // save's stale view of a slot can never clobber a concurrent media change).
  const reconcileContentFromOutput = useCallback((output: NodeWriteOutput) => {
    setPersisted({ text: output.node.text, label: output.node.label });
    invalidateLibraryOverviewCache();
    onWriteAcknowledgedRef.current?.(output);
  }, []);

  // A media action owns ONLY its targeted slot — never text/label nor the OTHER
  // slot, so two concurrent media actions (image + audio) resolved out of order
  // cannot resurrect each other's stale state (lost update).
  const reconcileSlotFromOutput = useCallback(
    (output: NodeWriteOutput, slot: NodeMediaSlotKind) => {
      if (slot === "image") setImage(output.node.image);
      else setAudio(output.node.audio);
      invalidateLibraryOverviewCache();
      onWriteAcknowledgedRef.current?.(output);
    },
    [],
  );

  // Forward reference so `scheduleSave`'s timer can reach `fireSave` without a
  // declaration cycle (same indirection as `useStoryEditor`).
  const fireSaveRef = useRef<(() => Promise<void>) | null>(null);

  const scheduleSave = useCallback(() => {
    clearTimer(debounceTimerRef);
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      if (!mountedRef.current) return;
      if (
        textRef.current === persistedRef.current.text &&
        labelRef.current === persistedRef.current.label
      ) {
        setSaveStatus({ kind: "idle" });
        return;
      }
      fireSaveRef.current?.();
    }, debounceMs);
  }, [debounceMs]);

  const fireSave = useCallback((): Promise<void> => {
    if (!storyId || !nodeId) return Promise.resolve();
    // Single-flight: never start a SECOND content write while one is in flight.
    // Two in-flight writes can land on the SQLite mutex out of order, letting an
    // older value overwrite a newer one (lost update — `callId` only guards the
    // RESPONSE, not the write). Re-plan instead — and CHAIN the returned
    // promise: it settles only once the in-flight save has landed AND, if the
    // draft moved past it meanwhile, a follow-up save has carried the LATEST
    // keystroke. Returning the stale in-flight promise alone would let an
    // awaiting structural mutation / selection change proceed with the newest
    // input neither committed nor buffered (NFR8).
    if (saveInFlightRef.current) {
      scheduleSave();
      const inflight = saveInFlightPromiseRef.current ?? Promise.resolve();
      return inflight.then(() => {
        const dirty =
          textRef.current !== persistedRef.current.text ||
          labelRef.current !== persistedRef.current.label;
        if (!dirty) return undefined;
        return fireSaveRef.current?.() ?? undefined;
      });
    }
    const callId = ++activeCallRef.current;
    saveInFlightRef.current = true;
    clearTimer(savedIdleTimerRef);
    const attemptedText = textRef.current;
    const attemptedLabel = labelRef.current;
    setSaveStatus({ kind: "saving" });

    const settled = updateNodeContent({
      storyId,
      nodeId,
      text: attemptedText,
      label: attemptedLabel,
    })
      .then((output) => {
        const current = callId === activeCallRef.current;
        if (current) {
          saveInFlightRef.current = false;
          saveInFlightPromiseRef.current = null;
        }
        // Reconcile ONLY when this is still the current call: a superseded
        // response (a newer save / a storyId switch) must not re-apply its
        // stale snapshot over fresher state.
        if (!mountedRef.current || !current) return;
        reconcileContentFromOutput(output);
        // If the user has typed past the value we just saved, keep the
        // status pending and re-plan a save instead of falsely painting saved.
        if (
          textRef.current !== output.node.text ||
          labelRef.current !== output.node.label
        ) {
          setSaveStatus({ kind: "pending" });
          scheduleSave();
          return;
        }
        setSaveStatus({ kind: "saved" });
        savedIdleTimerRef.current = setTimeout(() => {
          savedIdleTimerRef.current = null;
          if (!mountedRef.current) return;
          setSaveStatus((prev) => (prev.kind === "saved" ? { kind: "idle" } : prev));
        }, savedVisibleMs);
      })
      .catch((err: unknown) => {
        const current = callId === activeCallRef.current;
        if (current) {
          saveInFlightRef.current = false;
          saveInFlightPromiseRef.current = null;
        }
        if (!mountedRef.current || !current) return;
        setSaveStatus({ kind: "failed", error: toAppError(err) });
      });
    saveInFlightPromiseRef.current = settled;
    return settled;
  }, [storyId, nodeId, reconcileContentFromOutput, savedVisibleMs, scheduleSave]);

  fireSaveRef.current = fireSave;

  const scheduleRecordDraft = useCallback(() => {
    if (!storyId || !nodeId) return;
    clearTimer(recordTimerRef);
    recordTimerRef.current = setTimeout(() => {
      recordTimerRef.current = null;
      void recordNodeDraft({
        storyId,
        nodeId,
        draftText: textRef.current,
        draftLabel: labelRef.current,
      }).catch(() => undefined);
    }, recordDraftDebounceMs);
  }, [storyId, nodeId, recordDraftDebounceMs]);

  const planEdit = useCallback(() => {
    const dirty =
      textRef.current !== persistedRef.current.text ||
      labelRef.current !== persistedRef.current.label;
    if (!dirty) {
      clearTimer(debounceTimerRef);
      clearTimer(recordTimerRef);
      setSaveStatus({ kind: "idle" });
      return;
    }
    setSaveStatus({ kind: "pending" });
    scheduleSave();
    scheduleRecordDraft();
  }, [scheduleSave, scheduleRecordDraft]);

  const setText = useCallback(
    (next: string) => {
      if (!editable) return;
      setTextState(next);
      textRef.current = next;
      planEdit();
    },
    [editable, planEdit],
  );

  const setLabel = useCallback(
    (next: string) => {
      if (!editable) return;
      setLabelState(next);
      labelRef.current = next;
      planEdit();
    },
    [editable, planEdit],
  );

  const flushNodeAutoSave = useCallback((): Promise<void> => {
    if (!editable || !storyId || !nodeId) return Promise.resolve();
    if (
      textRef.current === persistedRef.current.text &&
      labelRef.current === persistedRef.current.label
    ) {
      // Nothing NEW to flush — but a save may still be in flight carrying
      // this exact value: hand its settled-promise to the caller.
      return saveInFlightPromiseRef.current ?? Promise.resolve();
    }
    clearTimer(debounceTimerRef);
    return fireSave();
  }, [editable, storyId, nodeId, fireSave]);

  const attachMedia = useCallback(
    (slot: NodeMediaSlotKind) => {
      if (!editable || !storyId || !nodeId) return;
      // Commit any dirty text FIRST: a media mutation re-serializes the
      // structure from the canonical body, so an un-flushed keystroke must
      // land before it — and it must not be stranded only in memory.
      flushNodeAutoSave();
      const setBusy = slot === "image" ? setImageBusy : setAudioBusy;
      const setSlotError = slot === "image" ? setImageError : setAudioError;
      setSlotError(null);
      setBusy(true);
      void attachNodeMedia({ storyId, nodeId, slot })
        .then((outcome) => {
          if (!mountedRef.current) return;
          if (outcome.kind === "attached")
            reconcileSlotFromOutput(outcome.output, slot);
          // `cancelled` is a silent no-op.
        })
        .catch((err: unknown) => {
          if (!mountedRef.current) return;
          setSlotError(toAppError(err));
        })
        .finally(() => {
          if (mountedRef.current) setBusy(false);
        });
    },
    [editable, storyId, nodeId, reconcileSlotFromOutput, flushNodeAutoSave],
  );

  const removeMedia = useCallback(
    (slot: NodeMediaSlotKind) => {
      if (!editable || !storyId || !nodeId) return;
      flushNodeAutoSave();
      const setBusy = slot === "image" ? setImageBusy : setAudioBusy;
      const setSlotError = slot === "image" ? setImageError : setAudioError;
      setSlotError(null);
      setBusy(true);
      void removeNodeMedia({ storyId, nodeId, slot })
        .then((output) => {
          if (mountedRef.current) reconcileSlotFromOutput(output, slot);
        })
        .catch((err: unknown) => {
          if (mountedRef.current) setSlotError(toAppError(err));
        })
        .finally(() => {
          if (mountedRef.current) setBusy(false);
        });
    },
    [editable, storyId, nodeId, reconcileSlotFromOutput, flushNodeAutoSave],
  );

  const applyRecovery = useCallback(() => {
    if (recovery.kind !== "recoverable" || !storyId || !nodeId) return;
    const { nodeId: draftNodeId, draftText, draftLabel } = recovery;
    const proposed = recovery;
    setRecovery({ kind: "none" });
    setRecoveryApplyError(null);
    if (draftNodeId !== nodeId) {
      // The buffer belongs to ANOTHER node than the one displayed (the
      // editor opened on the start node while the crash interrupted a
      // different one). Persist the recovered content EXPLICITLY to its own
      // node — never through the displayed node's save path, which would
      // apply the text to the wrong node. The local fields stay untouched.
      void updateNodeContent({
        storyId,
        nodeId: draftNodeId,
        text: draftText,
        label: draftLabel,
      })
        .then(() => {
          invalidateLibraryOverviewCache();
          // The commit changed structure_json / content_checksum /
          // updated_at in the base: the owner re-reads authoritatively so
          // the local detail pair and the navigator labels stay coherent.
          if (mountedRef.current) onCrossNodeRecoveryAppliedRef.current?.();
        })
        .catch((err: unknown) => {
          // An EXPLICIT gesture failed: surface it inline AND re-propose
          // the offer (the probe is deduplicated per story, a silent drop
          // would strand the draft for the whole session). Re-buffer
          // best-effort too, for the next session.
          void recordNodeDraft({
            storyId,
            nodeId: draftNodeId,
            draftText,
            draftLabel,
          }).catch(() => undefined);
          if (!mountedRef.current) return;
          setRecovery(proposed);
          setRecoveryApplyError(toAppError(err));
        });
      return;
    }
    setTextState(draftText);
    textRef.current = draftText;
    setLabelState(draftLabel);
    labelRef.current = draftLabel;
    void fireSave();
  }, [recovery, storyId, nodeId, fireSave]);

  const discardRecovery = useCallback(() => {
    if (recovery.kind !== "recoverable" || !storyId) return;
    const draftAt = recovery.draftAt;
    setRecovery({ kind: "none" });
    setRecoveryApplyError(null);
    void discardNodeDraft({ storyId, expectedDraftAt: draftAt }).catch(
      () => undefined,
    );
  }, [recovery, storyId]);

  // Probe for a recoverable node draft once per story.
  const recoveryProbedRef = useRef<string | null>(null);
  useEffect(() => {
    if (!storyId || !editable) return;
    if (recoveryProbedRef.current === storyId) return;
    recoveryProbedRef.current = storyId;
    let cancelled = false;
    let resolved = false;
    void readRecoverableNodeDraft({ storyId })
      .then((result: RecoverableNodeDraft) => {
        resolved = true;
        if (cancelled || !mountedRef.current) return;
        if (result.kind === "recoverable") {
          setRecovery({
            kind: "recoverable",
            nodeId: result.nodeId,
            draftText: result.draftText,
            draftLabel: result.draftLabel,
            draftAt: result.draftAt,
            persistedText: result.persistedText,
            persistedLabel: result.persistedLabel,
          });
        }
      })
      .catch(() => {
        resolved = true;
      });
    return () => {
      cancelled = true;
      // If the probe was cancelled BEFORE it resolved (a StrictMode
      // unmount/remount in dev), clear the dedup marker so the remount
      // re-probes — otherwise the discarded probe leaves the marker set and the
      // "Brouillon récupéré" banner never appears.
      if (!resolved) recoveryProbedRef.current = null;
    };
  }, [storyId, editable]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearTimer(debounceTimerRef);
      clearTimer(savedIdleTimerRef);
      clearTimer(recordTimerRef);
      // F3: a non-button navigation (browser back, route swap, fast unmount)
      // never calls `flushNodeAutoSave`, and cancelling `recordTimerRef` above
      // could drop a buffer that had not fired yet. If the node is dirty,
      // commit it best-effort; on failure fall back to the recovery buffer so a
      // kill before the next debounce cannot lose the keystroke (NFR8). The two
      // never run in parallel (same discipline as the title autosave cleanup).
      const sid = storyIdRef.current;
      const nid = nodeIdRef.current;
      const text = textRef.current;
      const label = labelRef.current;
      const dirty =
        text !== persistedRef.current.text ||
        label !== persistedRef.current.label;
      if (editableRef.current && sid && nid && dirty) {
        const bufferDraft = () =>
          Promise.resolve()
            .then(() =>
              recordNodeDraft({
                storyId: sid,
                nodeId: nid,
                draftText: text,
                draftLabel: label,
              }),
            )
            .catch(() => undefined);
        if (saveInFlightRef.current) {
          // A save is ALREADY in flight (e.g. `flushNodeAutoSave` fired on
          // Retour). If it fails after unmount its `.catch` buffers nothing, so
          // record a draft anyway — if the save SUCCEEDS, the matching draft is
          // auto-consumed on the next open (it equals the persisted value).
          void bufferDraft();
        } else {
          // No save in flight: fire a best-effort save, fall back to the
          // recovery buffer only if it fails. The two never run in parallel.
          void Promise.resolve()
            .then(() => updateNodeContent({ storyId: sid, nodeId: nid, text, label }))
            .then(() => invalidateLibraryOverviewCache())
            .catch(bufferDraft);
        }
      }
    };
  }, []);

  return {
    nodeId,
    editable,
    text,
    label,
    saveStatus,
    image,
    audio,
    imageError,
    audioError,
    imageBusy,
    audioBusy,
    recovery,
    recoveryApplyError,
    setText,
    setLabel,
    flushNodeAutoSave,
    attachMedia,
    removeMedia,
    applyRecovery,
    discardRecovery,
  };
}
