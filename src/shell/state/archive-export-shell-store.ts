import { create } from "zustand";

import type { AppError } from "../../shared/errors/app-error";

/**
 * Where a story archive export stands, as the library shows it.
 *
 * - `exporting` — from the click until the archive is written. `percent` is
 *   the integer percent (0..99) streamed by the export, or `null` before
 *   the first tick (the save dialog is open, or the first media is still
 *   being converted — no reliable fraction yet).
 * - `exported` — settled: where the archive is and what it carries.
 * - `failed` — settled: the normalized refusal, with the story it concerns
 *   so a retry targets the same one.
 */
export type StoryArchiveExportStatus =
  | { kind: "idle" }
  | {
      kind: "exporting";
      storyId: string;
      storyTitle: string;
      percent: number | null;
    }
  | {
      kind: "exported";
      storyTitle: string;
      destinationPath: string;
      bytesWritten: number;
      imageCount: number;
      audioCount: number;
    }
  | { kind: "failed"; storyId: string; storyTitle: string; error: AppError };

/**
 * UI-continuity-only relay of the story archive export. An export of a long
 * story converts every sound and can run for minutes: the user may open the
 * editor or the settings meanwhile, which unmounts the library screen. The
 * status therefore lives HERE rather than in the screen's own state — the
 * export keeps being tracked, and its outcome is still there when the user
 * comes back to the library.
 *
 * NEVER a business truth: the archive is written by Rust whatever this
 * store holds. One export at a time — the `exporting` status IS the
 * re-entrancy guard. No persistence (a fresh launch starts idle).
 */
export interface ArchiveExportShellState {
  status: StoryArchiveExportStatus;
  setStatus: (status: StoryArchiveExportStatus) => void;
  /** Refresh the in-flight percent of `storyId`'s export. A tick that lands
   *  once the export settled — or for another story — is ignored: it must
   *  never resurrect an `exporting` status. */
  setProgress: (storyId: string, percent: number) => void;
}

export const useArchiveExportShell = create<ArchiveExportShellState>((set) => ({
  status: { kind: "idle" },
  setStatus: (status) => set({ status }),
  setProgress: (storyId, percent) =>
    set((state) =>
      state.status.kind === "exporting" && state.status.storyId === storyId
        ? { status: { ...state.status, percent } }
        : state,
    ),
}));
