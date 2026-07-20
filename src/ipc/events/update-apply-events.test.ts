import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { listen } from "@tauri-apps/api/event";

import {
  subscribeUpdateApplyEvents,
  type UpdateApplySubscription,
} from "./update-apply-events";

const handlers = new Map<string, (event: { payload: unknown }) => void>();
const unlistenSpies: Array<() => void> = [];

function fire(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

const flush = (): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, 0));

function makeSubscription(): UpdateApplySubscription {
  return {
    jobId: "j1",
    onProgress: vi.fn(),
    onCompleted: vi.fn(),
    onFailed: vi.fn(),
  };
}

const progress = (
  sequence: number,
  phase: "checking" | "downloading" | "installing" = "downloading",
  percent?: number,
) => ({
  jobId: "j1",
  phase,
  ...(percent === undefined ? {} : { percent }),
  sequence,
});

const failed = (sequence: number) => ({
  jobId: "j1",
  sequence,
  stage: "download",
  headline: "Le téléchargement de la mise à jour n'a pas abouti.",
  notice:
    "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
});

describe("subscribeUpdateApplyEvents", () => {
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
    subscribeUpdateApplyEvents(sub);
    fire("update:progress", progress(1, "downloading", 5));
    expect(sub.onProgress).toHaveBeenCalledTimes(1);
    expect(sub.onProgress).toHaveBeenCalledWith(
      progress(1, "downloading", 5),
    );
  });

  it("ignores an event for a different job", () => {
    const sub = makeSubscription();
    subscribeUpdateApplyEvents(sub);
    fire("update:progress", { ...progress(1), jobId: "other" });
    fire("update:completed", { jobId: "other", sequence: 2 });
    fire("update:failed", { ...failed(3), jobId: "other" });
    expect(sub.onProgress).not.toHaveBeenCalled();
    expect(sub.onCompleted).not.toHaveBeenCalled();
    expect(sub.onFailed).not.toHaveBeenCalled();
  });

  it("drops out-of-order / duplicate events via the monotonic sequence", () => {
    const sub = makeSubscription();
    subscribeUpdateApplyEvents(sub);
    fire("update:progress", progress(2, "downloading", 10));
    fire("update:progress", progress(1, "checking")); // late — dropped
    fire("update:progress", progress(2, "downloading", 10)); // duplicate — dropped
    expect(sub.onProgress).toHaveBeenCalledTimes(1);
    expect(sub.onProgress).toHaveBeenCalledWith(
      progress(2, "downloading", 10),
    );
  });

  it("lets a higher-sequence terminal through after progress", () => {
    const sub = makeSubscription();
    subscribeUpdateApplyEvents(sub);
    fire("update:progress", progress(1));
    fire("update:completed", { jobId: "j1", sequence: 3 });
    expect(sub.onCompleted).toHaveBeenCalledTimes(1);
    fire("update:failed", failed(2)); // late after the terminal — dropped
    expect(sub.onFailed).not.toHaveBeenCalled();
  });

  it("routes a failed terminal with its frozen couple", () => {
    const sub = makeSubscription();
    subscribeUpdateApplyEvents(sub);
    fire("update:failed", failed(4));
    expect(sub.onFailed).toHaveBeenCalledTimes(1);
    expect(sub.onFailed).toHaveBeenCalledWith(failed(4));
  });

  it("ignores a malformed payload", () => {
    const sub = makeSubscription();
    subscribeUpdateApplyEvents(sub);
    fire("update:progress", { ...progress(1), phase: "uploading" });
    fire("update:progress", { ...progress(1), percent: 12.5 });
    fire("update:failed", { jobId: "j1", sequence: 1 }); // missing fields
    fire("update:failed", { ...failed(1), headline: "Erreur réseau." }); // drifted copy
    expect(sub.onProgress).not.toHaveBeenCalled();
    expect(sub.onFailed).not.toHaveBeenCalled();
  });

  it("detaches every listener on unsubscribe", async () => {
    const sub = makeSubscription();
    const handle = subscribeUpdateApplyEvents(sub);
    await flush(); // let the async listen() registrations resolve
    handle.unsubscribe();
    expect(unlistenSpies).toHaveLength(3);
    for (const spy of unlistenSpies) {
      expect(spy).toHaveBeenCalled();
    }
  });

  it("signals ready only once the three registrations settled", async () => {
    // The catch-up re-read must run AFTER the listeners are installed:
    // `ready` is that safe point.
    let resolveThird: (fn: () => void) => void = () => {};
    vi.mocked(listen)
      .mockImplementationOnce((name: string, cb: unknown) => {
        handlers.set(name, cb as (event: { payload: unknown }) => void);
        return Promise.resolve(vi.fn() as never);
      })
      .mockImplementationOnce((name: string, cb: unknown) => {
        handlers.set(name, cb as (event: { payload: unknown }) => void);
        return Promise.resolve(vi.fn() as never);
      })
      .mockImplementationOnce(
        (name: string, cb: unknown) =>
          new Promise((resolve) => {
            handlers.set(name, cb as (event: { payload: unknown }) => void);
            resolveThird = resolve as (fn: () => void) => void;
          }),
      );
    const sub = makeSubscription();
    const handle = subscribeUpdateApplyEvents(sub);
    let settled = false;
    void handle.ready.then(() => {
      settled = true;
    });
    await flush();
    expect(settled).toBe(false);
    resolveThird(vi.fn());
    await flush();
    expect(settled).toBe(true);
  });

  it("still signals ready when a registration fails — the re-read is the fallback", async () => {
    vi.mocked(listen)
      .mockImplementationOnce(() => Promise.resolve(vi.fn() as never))
      .mockImplementationOnce(() => Promise.reject(new Error("no runtime")))
      .mockImplementationOnce(() => Promise.resolve(vi.fn() as never));
    const sub = makeSubscription();
    const handle = subscribeUpdateApplyEvents(sub);
    await expect(handle.ready).resolves.toBeUndefined();
  });
});
