import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { listen } from "@tauri-apps/api/event";

import { subscribeOsOpenRequested } from "./os-open-events";

const handlers = new Map<string, (event: { payload: unknown }) => void>();
const unlistenSpies: Array<() => void> = [];

function fire(name: string, payload: unknown): void {
  handlers.get(name)?.({ payload });
}

const flush = (): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, 0));

describe("subscribeOsOpenRequested", () => {
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

  it("invokes the handler on the empty versionable signal payload", () => {
    const handler = vi.fn();
    subscribeOsOpenRequested(handler);
    fire("os-open:requested", {});
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("tolerates future payload fields (versionable object)", () => {
    const handler = vi.fn();
    subscribeOsOpenRequested(handler);
    fire("os-open:requested", { futureField: 1 });
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("ignores a malformed (non-object) payload", () => {
    const handler = vi.fn();
    subscribeOsOpenRequested(handler);
    fire("os-open:requested", null);
    fire("os-open:requested", "signal");
    expect(handler).not.toHaveBeenCalled();
  });

  it("stops delivering after unsubscribe and detaches the listener", async () => {
    const handler = vi.fn();
    const unsubscribe = subscribeOsOpenRequested(handler);
    await flush(); // let the async listen() registration resolve
    unsubscribe();
    fire("os-open:requested", {});
    expect(handler).not.toHaveBeenCalled();
    expect(unlistenSpies).toHaveLength(1);
    expect(unlistenSpies[0]).toHaveBeenCalled();
  });

  it("cancels a listener that resolves after an early unsubscribe", async () => {
    const handler = vi.fn();
    const unsubscribe = subscribeOsOpenRequested(handler);
    // Unsubscribe BEFORE the async registration resolves.
    unsubscribe();
    await flush();
    expect(unlistenSpies).toHaveLength(1);
    expect(unlistenSpies[0]).toHaveBeenCalled();
  });

  it("fires onSettled once the registration RESOLVES (the catch-up handshake)", async () => {
    const handler = vi.fn();
    const onSettled = vi.fn();
    subscribeOsOpenRequested(handler, onSettled);
    // The registration is still in flight: no settlement yet.
    expect(onSettled).not.toHaveBeenCalled();
    await flush();
    expect(onSettled).toHaveBeenCalledTimes(1);
    // The settlement is a registration fact — never a signal delivery.
    expect(handler).not.toHaveBeenCalled();
  });

  it("fires onSettled even when the registration REJECTS (controlled recovery)", async () => {
    vi.mocked(listen).mockReset();
    vi.mocked(listen).mockImplementationOnce(() =>
      Promise.reject(new Error("bridge down")),
    );
    const onSettled = vi.fn();
    subscribeOsOpenRequested(vi.fn(), onSettled);
    await flush();
    expect(onSettled).toHaveBeenCalledTimes(1);
  });
});
