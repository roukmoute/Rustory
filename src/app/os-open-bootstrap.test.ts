import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../ipc/events/os-open-events", () => ({
  subscribeOsOpenRequested: vi.fn(),
}));
vi.mock("./router", () => ({
  router: { navigate: vi.fn() },
}));

import { subscribeOsOpenRequested } from "../ipc/events/os-open-events";
import { useOsOpenShell } from "../shell/state/os-open-shell-store";
import { bootstrapOsOpenSignal } from "./os-open-bootstrap";
import { router } from "./router";

describe("bootstrapOsOpenSignal", () => {
  beforeEach(() => {
    vi.mocked(subscribeOsOpenRequested).mockReset();
    vi.mocked(router.navigate).mockReset();
    useOsOpenShell.setState({ pendingSignal: false });
  });

  it("subscribes once and returns the unsubscribe", () => {
    const unsubscribe = vi.fn();
    vi.mocked(subscribeOsOpenRequested).mockReturnValueOnce(unsubscribe);
    expect(bootstrapOsOpenSignal()).toBe(unsubscribe);
    expect(subscribeOsOpenRequested).toHaveBeenCalledTimes(1);
  });

  it("raises the shell relay and navigates to /library with replace on each signal", () => {
    let captured: (() => void) | undefined;
    vi.mocked(subscribeOsOpenRequested).mockImplementationOnce((handler) => {
      captured = handler;
      return () => {};
    });
    bootstrapOsOpenSignal();
    expect(captured).toBeDefined();

    captured?.();
    expect(useOsOpenShell.getState().pendingSignal).toBe(true);
    // `replace` keeps the back button sane — a system-driven redirection
    // never stacks a history entry.
    expect(router.navigate).toHaveBeenCalledWith("/library", {
      replace: true,
    });
  });

  it("navigates through an injected router when one is provided", () => {
    let captured: (() => void) | undefined;
    vi.mocked(subscribeOsOpenRequested).mockImplementationOnce((handler) => {
      captured = handler;
      return () => {};
    });
    const navigate = vi.fn();
    bootstrapOsOpenSignal({ navigate });
    captured?.();
    expect(navigate).toHaveBeenCalledWith("/library", { replace: true });
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("raises a catch-up pull once the registration settles — the lost wake-up window is closed", () => {
    // The race: an intent lands AFTER the library-mount pull but BEFORE
    // `listen()` is effective — its event has no consumer. The bootstrap
    // must raise one authoritative catch-up on settlement so the pull
    // serves the slipped intent (a `none` answer stays a total no-op).
    let settled: (() => void) | undefined;
    vi.mocked(subscribeOsOpenRequested).mockImplementationOnce(
      (_handler, onSettled) => {
        settled = onSettled;
        return () => {};
      },
    );
    const navigate = vi.fn();
    bootstrapOsOpenSignal({ navigate });
    // Before the registration settles: nothing raised (the event emitted
    // in the window was simply unheard).
    expect(useOsOpenShell.getState().pendingSignal).toBe(false);

    settled?.();
    expect(useOsOpenShell.getState().pendingSignal).toBe(true);
    // The catch-up is a PULL, never a navigation (the boot already lands
    // on /library; a pending intent elsewhere waits for the next visit).
    expect(navigate).not.toHaveBeenCalled();
  });
});
