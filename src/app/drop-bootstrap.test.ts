import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../ipc/events/drop-events", () => ({
  subscribeDropHover: vi.fn(),
  subscribeDropHoverEnded: vi.fn(),
  subscribeDropRequested: vi.fn(),
}));
vi.mock("./router", () => ({
  router: { navigate: vi.fn() },
}));

import {
  subscribeDropHover,
  subscribeDropHoverEnded,
  subscribeDropRequested,
} from "../ipc/events/drop-events";
import { useDropShell } from "../shell/state/drop-shell-store";
import { bootstrapDropSignals } from "./drop-bootstrap";
import { router } from "./router";

describe("bootstrapDropSignals", () => {
  beforeEach(() => {
    vi.mocked(subscribeDropHover).mockReset().mockReturnValue(() => {});
    vi.mocked(subscribeDropHoverEnded)
      .mockReset()
      .mockReturnValue(() => {});
    vi.mocked(subscribeDropRequested)
      .mockReset()
      .mockReturnValue(() => {});
    vi.mocked(router.navigate).mockReset();
    useDropShell.setState({ hoverActive: false, pendingSignal: false });
  });

  it("subscribes the three signals once and returns a combined unsubscribe", () => {
    const unHover = vi.fn();
    const unEnded = vi.fn();
    const unRequested = vi.fn();
    vi.mocked(subscribeDropHover).mockReturnValueOnce(unHover);
    vi.mocked(subscribeDropHoverEnded).mockReturnValueOnce(unEnded);
    vi.mocked(subscribeDropRequested).mockReturnValueOnce(unRequested);

    const unsubscribe = bootstrapDropSignals();
    expect(subscribeDropHover).toHaveBeenCalledTimes(1);
    expect(subscribeDropHoverEnded).toHaveBeenCalledTimes(1);
    expect(subscribeDropRequested).toHaveBeenCalledTimes(1);

    unsubscribe();
    expect(unHover).toHaveBeenCalledTimes(1);
    expect(unEnded).toHaveBeenCalledTimes(1);
    expect(unRequested).toHaveBeenCalledTimes(1);
  });

  it("raises the hover flag on drop:hover and clears it on drop:hover-ended", () => {
    let hoverHandler: (() => void) | undefined;
    let endedHandler: (() => void) | undefined;
    vi.mocked(subscribeDropHover).mockImplementationOnce((handler) => {
      hoverHandler = handler;
      return () => {};
    });
    vi.mocked(subscribeDropHoverEnded).mockImplementationOnce((handler) => {
      endedHandler = handler;
      return () => {};
    });
    bootstrapDropSignals();

    hoverHandler?.();
    expect(useDropShell.getState().hoverActive).toBe(true);
    endedHandler?.();
    expect(useDropShell.getState().hoverActive).toBe(false);
    // No navigation, no signal: hover is purely decorative.
    expect(router.navigate).not.toHaveBeenCalled();
    expect(useDropShell.getState().pendingSignal).toBe(false);
  });

  it("closes the overlay, raises the relay and navigates replace on drop:requested", () => {
    let requestedHandler: (() => void) | undefined;
    vi.mocked(subscribeDropRequested).mockImplementationOnce((handler) => {
      requestedHandler = handler;
      return () => {};
    });
    bootstrapDropSignals();
    useDropShell.setState({ hoverActive: true });

    requestedHandler?.();
    // Belt and braces: the overlay closes even without a Leave.
    expect(useDropShell.getState().hoverActive).toBe(false);
    expect(useDropShell.getState().pendingSignal).toBe(true);
    // `replace` keeps the back button sane — a system-driven redirection
    // never stacks a history entry.
    expect(router.navigate).toHaveBeenCalledWith("/library", {
      replace: true,
    });
  });

  it("navigates through an injected router when one is provided", () => {
    let requestedHandler: (() => void) | undefined;
    vi.mocked(subscribeDropRequested).mockImplementationOnce((handler) => {
      requestedHandler = handler;
      return () => {};
    });
    const navigate = vi.fn();
    bootstrapDropSignals({ navigate });
    requestedHandler?.();
    expect(navigate).toHaveBeenCalledWith("/library", { replace: true });
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("raises a catch-up pull once the registration settles — the lost wake-up window is closed", () => {
    // The race: an intent lands AFTER the library-mount pull but BEFORE
    // `listen()` is effective — its event has no consumer. The bootstrap
    // must raise one authoritative catch-up on settlement so the pull
    // serves the slipped intent (a `none` answer stays a total no-op).
    let settled: (() => void) | undefined;
    vi.mocked(subscribeDropRequested).mockImplementationOnce(
      (_handler, onSettled) => {
        settled = onSettled;
        return () => {};
      },
    );
    const navigate = vi.fn();
    bootstrapDropSignals({ navigate });
    expect(useDropShell.getState().pendingSignal).toBe(false);

    settled?.();
    expect(useDropShell.getState().pendingSignal).toBe(true);
    // The catch-up is a PULL, never a navigation (the boot already lands
    // on /library; a pending intent elsewhere waits for the next visit).
    expect(navigate).not.toHaveBeenCalled();
  });
});
