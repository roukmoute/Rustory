import { beforeEach, describe, expect, it } from "vitest";

import { useJobShell, type JobShellEntry } from "./job-shell-store";

function entry(overrides: Partial<JobShellEntry> = {}): JobShellEntry {
  return {
    jobId: "job-1",
    jobType: "prepare_story",
    targetStoryId: "s1",
    phase: "preflight",
    progress: null,
    sequence: 1,
    ...overrides,
  };
}

describe("job-shell-store", () => {
  beforeEach(() => {
    // Reset to a clean map between tests (the store is a module singleton).
    useJobShell.setState({ activeJobs: new Map() });
  });

  it("tracks a job's phase", () => {
    useJobShell.getState().trackJobProgress(entry());
    const job = useJobShell.getState().activeJobs.get("job-1");
    expect(job?.phase).toBe("preflight");
  });

  it("advances to a newer sequence", () => {
    useJobShell.getState().trackJobProgress(entry({ phase: "preflight", sequence: 1 }));
    useJobShell
      .getState()
      .trackJobProgress(entry({ phase: "prepare", sequence: 2, progress: 0.5 }));
    const job = useJobShell.getState().activeJobs.get("job-1");
    expect(job?.phase).toBe("prepare");
    expect(job?.progress).toBe(0.5);
  });

  it("ignores a stale or duplicate sequence (idempotent)", () => {
    useJobShell.getState().trackJobProgress(entry({ phase: "prepare", sequence: 2 }));
    // A late progress event with a lower sequence must not regress the phase.
    useJobShell.getState().trackJobProgress(entry({ phase: "preflight", sequence: 1 }));
    // A duplicate at the same sequence is also a no-op.
    useJobShell.getState().trackJobProgress(entry({ phase: "preflight", sequence: 2 }));
    expect(useJobShell.getState().activeJobs.get("job-1")?.phase).toBe("prepare");
  });

  it("purges a job on clear", () => {
    useJobShell.getState().trackJobProgress(entry());
    useJobShell.getState().clearJob("job-1");
    expect(useJobShell.getState().activeJobs.has("job-1")).toBe(false);
  });

  it("clearing an unknown job is a no-op that preserves the map reference", () => {
    const before = useJobShell.getState().activeJobs;
    useJobShell.getState().clearJob("nope");
    expect(useJobShell.getState().activeJobs).toBe(before);
  });
});
