import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { listen } from "@tauri-apps/api/event";

import { subscribeJobEvents, type JobSubscription } from "./job-events";

const STORY = "0197a5d0-0000-7000-8000-000000000000";

const handlers = new Map<string, (event: { payload: unknown }) => void>();
const unlistenSpies: Array<() => void> = [];

function fire(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

const flush = (): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, 0));

function makeSubscription(): JobSubscription {
  return {
    jobId: "j1",
    jobType: "prepare_story",
    targetStoryId: STORY,
    onProgress: vi.fn(),
    onCompleted: vi.fn(),
    onFailed: vi.fn(),
  };
}

const progress = (sequence: number, phase: "preflight" | "prepare" = "preflight") => ({
  jobId: "j1",
  jobType: "prepare_story",
  targetStoryId: STORY,
  phase,
  progress: null,
  sequence,
  message: null,
});

describe("subscribeJobEvents", () => {
  beforeEach(() => {
    handlers.clear();
    unlistenSpies.length = 0;
    vi.mocked(listen).mockReset();
    vi.mocked(listen).mockImplementation((name: string, cb: unknown) => {
      handlers.set(name, cb as (event: { payload: unknown }) => void);
      const un = vi.fn();
      unlistenSpies.push(un);
      return Promise.resolve(un as never);
    });
  });

  it("routes a matching progress event to onProgress", () => {
    const sub = makeSubscription();
    subscribeJobEvents(sub);
    fire("job:progress", progress(1));
    expect(sub.onProgress).toHaveBeenCalledTimes(1);
  });

  it("ignores an event for a different job / story", () => {
    const sub = makeSubscription();
    subscribeJobEvents(sub);
    fire("job:progress", { ...progress(1), jobId: "other" });
    fire("job:progress", { ...progress(1), targetStoryId: "other-story" });
    expect(sub.onProgress).not.toHaveBeenCalled();
  });

  it("drops out-of-order / duplicate events via the monotonic sequence", () => {
    const sub = makeSubscription();
    subscribeJobEvents(sub);
    fire("job:progress", progress(2, "prepare"));
    fire("job:progress", progress(1, "preflight")); // late — dropped
    fire("job:progress", progress(2, "prepare")); // duplicate — dropped
    expect(sub.onProgress).toHaveBeenCalledTimes(1);
    expect(sub.onProgress).toHaveBeenCalledWith(progress(2, "prepare"));
  });

  it("lets a higher-sequence terminal through after progress", () => {
    const sub = makeSubscription();
    subscribeJobEvents(sub);
    fire("job:progress", progress(1));
    fire("job:completed", {
      jobId: "j1",
      jobType: "prepare_story",
      targetStoryId: STORY,
      sequence: 3,
    });
    expect(sub.onCompleted).toHaveBeenCalledTimes(1);
  });

  it("ignores a malformed payload", () => {
    const sub = makeSubscription();
    subscribeJobEvents(sub);
    fire("job:progress", { ...progress(1), phase: "bogus" }); // unknown phase — invalid
    fire("job:failed", { jobId: "j1" }); // missing fields
    expect(sub.onProgress).not.toHaveBeenCalled();
    expect(sub.onFailed).not.toHaveBeenCalled();
  });

  it("detaches every listener on unsubscribe", async () => {
    const sub = makeSubscription();
    const unsubscribe = subscribeJobEvents(sub);
    await flush(); // let the async listen() registrations resolve
    unsubscribe();
    expect(unlistenSpies).toHaveLength(3);
    for (const spy of unlistenSpies) {
      expect(spy).toHaveBeenCalled();
    }
  });
});
