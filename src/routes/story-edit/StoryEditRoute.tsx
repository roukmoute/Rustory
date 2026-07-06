import type React from "react";
import { useMemo, useRef } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { StoryEditorShell } from "../../features/story-editor/components/StoryEditorShell";
import { useStoryExport } from "../../features/import-export/hooks/use-story-export";
import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { useNodeEditor } from "../../features/story-editor/hooks/use-node-editor";
import { useStoryEditor } from "../../features/story-editor/hooks/use-story-editor";
import { useStoryRecovery } from "../../features/story-editor/hooks/use-story-recovery";
import { useStructureEditor } from "../../features/story-editor/hooks/use-structure-editor";
import { Button, ProgressIndicator, SurfacePanel } from "../../shared/ui";

import "./StoryEditRoute.css";

export function StoryEditRoute(): React.JSX.Element {
  const { storyId: rawStoryId } = useParams<{ storyId: string }>();
  const navigate = useNavigate();

  // The library encodes ids with encodeURIComponent before pushing them into
  // the URL — decode here before comparing against canonical ids. A malformed
  // encoding (rare) falls back to the raw value; the "introuvable" branch
  // still catches it cleanly.
  const storyId = useMemo(() => {
    if (!rawStoryId) return undefined;
    try {
      return decodeURIComponent(rawStoryId);
    } catch {
      return rawStoryId;
    }
  }, [rawStoryId]);

  const editor = useStoryEditor(storyId);
  const exporter = useStoryExport();
  const recovery = useStoryRecovery(storyId, {
    onApplied: (output) => {
      // The Rust core already persisted the recovered title. Patch the
      // in-memory editor snapshot in place — no follow-up get_story_detail
      // round-trip needed.
      editor.reloadDetailFromOutput(output);
    },
  });
  const { state } = editor;

  // The current node + graph + editability are projected by Rust inside the
  // story detail. The hooks are called unconditionally (Rules of Hooks) with
  // the projections when ready, `null` otherwise — they handle the absence.
  const projectedNode = state.kind === "ready" ? state.detail.node : null;
  const projectedStructure =
    state.kind === "ready" ? state.detail.structure : null;
  const editable = state.kind === "ready" ? state.detail.editable : true;
  // Forward ref: the node editor needs the structure editor's targeted
  // re-read (cross-node recovery), while the structure editor needs the
  // node editor's flush — the ref breaks the declaration cycle.
  const refreshDetailRef = useRef<() => void>(() => undefined);
  const nodeEditor = useNodeEditor(storyId, projectedNode, editable, {
    onCrossNodeRecoveryApplied: () => refreshDetailRef.current(),
    // Every acknowledged node write carries the durable review state
    // (`importState`) — route it back into the story detail so the review
    // chip reconciles from the same truth (correlated by output.id).
    onWriteAcknowledged: editor.applyNodeWriteOutput,
  });

  const flushAll = (): Promise<void> => {
    // Both flushes resolve when their save settles (never reject), so a
    // structural mutation / selection change can truly WAIT for the pending
    // content instead of racing it.
    return Promise.all([
      editor.flushAutoSave(),
      nodeEditor.flushNodeAutoSave(),
    ]).then(() => undefined);
  };

  // Structural mutations + current-node selection. The selection is LOCAL
  // route state (never the Zustand shell store); every mutation flushes the
  // pending content first and reconciles from the Rust-re-projected graph.
  const structureEditor = useStructureEditor({
    storyId,
    structure: projectedStructure,
    editable,
    flushContent: flushAll,
    onStructureCommitted: editor.applyStructureOutput,
    onDetailReloaded: editor.replaceDetail,
  });
  refreshDetailRef.current = structureEditor.refreshDetail;

  const goBack = (): void => {
    // Block the navigation while a recovery apply / discard is in flight:
    // navigating mid-transaction would unmount the hook, strand the IPC, and
    // either commit the recovered title without its UI ack or drop a row that
    // the user just confirmed should be dropped — both states are confusing.
    // The button surface should already be disabled by `recoveryActive`, but a
    // programmatic call (keyboard shortcut, browser back) must also no-op here.
    if (recovery.state.kind === "applying") return;
    // Commit BOTH pending autosaves before the route unmounts: clicking Retour
    // at millisecond 499 of the debounce must not lose the change. Fire and
    // commit in Rust, forget in UI (the route is leaving) — the awaitable
    // form matters for structural mutations, not for the exit path.
    void flushAll();
    // `replace` keeps the browser history a single in/out transition for the
    // library ↔ edit context — back button behavior stays predictable.
    navigate("/library", { replace: true });
  };

  if (state.kind === "loading") {
    return (
      <main
        className="story-edit-route story-edit-route--loading"
        aria-label="Chargement du brouillon"
      >
        <div
          className="story-edit-route__status"
          role="status"
          aria-live="polite"
        >
          <ProgressIndicator
            mode="indeterminate"
            label="Chargement du brouillon local…"
          />
        </div>
      </main>
    );
  }

  if (state.kind === "error") {
    const title =
      state.error.code === "LIBRARY_INCONSISTENT"
        ? "Bibliothèque incohérente, recharge nécessaire"
        : "Reprise indisponible";
    return (
      <main className="story-edit-route" aria-label="Erreur de chargement">
        <LibraryErrorBanner
          error={state.error}
          onRetry={editor.retry}
          title={title}
        />
        <Button variant="secondary" onClick={goBack}>
          Retour à la bibliothèque
        </Button>
      </main>
    );
  }

  if (state.kind === "not-found") {
    return (
      <main
        className="story-edit-route story-edit-route--missing"
        aria-label="Histoire introuvable"
      >
        <SurfacePanel
          elevation={1}
          as="section"
          className="story-edit-route__card"
        >
          <h1 className="story-edit-route__title">Histoire introuvable</h1>
          <p className="story-edit-route__message">
            Cette histoire n'est plus dans ta bibliothèque locale.
          </p>
          <Button variant="secondary" onClick={goBack}>
            Retour à la bibliothèque
          </Button>
        </SurfacePanel>
      </main>
    );
  }

  return (
    <StoryEditorShell
      detail={state.detail}
      draftTitle={state.draftTitle}
      saveStatus={state.saveStatus}
      recovery={recovery}
      exporter={exporter}
      nodeEditor={nodeEditor}
      structureEditor={structureEditor}
      onSetDraftTitle={editor.setDraftTitle}
      onRetrySave={editor.retrySave}
      onFlushAutoSave={flushAll}
      onBack={goBack}
    />
  );
}
