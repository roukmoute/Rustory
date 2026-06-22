import { create } from "zustand";

/**
 * Minimal, UI-continuity-only visibility of in-flight long-running jobs. This is
 * NEVER the canonical truth (that is re-read authoritatively via
 * `read_preparation_state`) — it only carries enough to keep a phase indicator
 * coherent across re-renders. Updates are idempotent by the monotonic
 * `sequence`; a terminal event purges the entry.
 *
 * No persistence: a fresh app launch never restores a stale job, per the
 * architecture Zustand contract.
 */
export interface JobShellEntry {
  jobId: string;
  jobType: string;
  targetStoryId: string;
  phase: string;
  progress: number | null;
  sequence: number;
}

export interface JobShellState {
  activeJobs: ReadonlyMap<string, JobShellEntry>;
  /** Upsert a job's phase. Idempotent: an entry whose `sequence` is not strictly
   *  newer than the tracked one is ignored (late / duplicate delivery). */
  trackJobProgress: (entry: JobShellEntry) => void;
  /** Purge a job on its terminal event (or on supersession). */
  clearJob: (jobId: string) => void;
}

const EMPTY_JOBS: ReadonlyMap<string, JobShellEntry> = new Map();

export const useJobShell = create<JobShellState>((set) => ({
  activeJobs: EMPTY_JOBS,

  trackJobProgress: (entry) =>
    set((state) => {
      const existing = state.activeJobs.get(entry.jobId);
      if (existing && existing.sequence >= entry.sequence) {
        return state;
      }
      const next = new Map(state.activeJobs);
      next.set(entry.jobId, entry);
      return { activeJobs: next };
    }),

  clearJob: (jobId) =>
    set((state) => {
      if (!state.activeJobs.has(jobId)) {
        return state;
      }
      const next = new Map(state.activeJobs);
      next.delete(jobId);
      return { activeJobs: next };
    }),
}));
