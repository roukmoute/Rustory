import { beforeEach, describe, expect, it } from "vitest";

import { useDropShell } from "./drop-shell-store";

describe("drop shell store", () => {
  beforeEach(() => {
    useDropShell.setState({ hoverActive: false, pendingSignal: false });
  });

  it("starts with no hover and no pending signal", () => {
    expect(useDropShell.getState().hoverActive).toBe(false);
    expect(useDropShell.getState().pendingSignal).toBe(false);
  });

  it("raiseHover()/clearHover() drive the hover flag", () => {
    useDropShell.getState().raiseHover();
    expect(useDropShell.getState().hoverActive).toBe(true);
    useDropShell.getState().clearHover();
    expect(useDropShell.getState().hoverActive).toBe(false);
  });

  it("clearHover() is idempotent (hover-ended arrives on Leave AND Drop)", () => {
    useDropShell.getState().raiseHover();
    useDropShell.getState().clearHover();
    useDropShell.getState().clearHover();
    expect(useDropShell.getState().hoverActive).toBe(false);
  });

  it("raiseSignal() marks a signal pending and clearSignal() consumes it", () => {
    useDropShell.getState().raiseSignal();
    expect(useDropShell.getState().pendingSignal).toBe(true);
    useDropShell.getState().clearSignal();
    expect(useDropShell.getState().pendingSignal).toBe(false);
  });

  it("raiseSignal() is idempotent while unconsumed (a boolean, never a queue)", () => {
    useDropShell.getState().raiseSignal();
    useDropShell.getState().raiseSignal();
    expect(useDropShell.getState().pendingSignal).toBe(true);
    useDropShell.getState().clearSignal();
    expect(useDropShell.getState().pendingSignal).toBe(false);
  });

  it("the hover flag and the signal are independent axes", () => {
    useDropShell.getState().raiseHover();
    useDropShell.getState().raiseSignal();
    useDropShell.getState().clearHover();
    expect(useDropShell.getState().pendingSignal).toBe(true);
    expect(useDropShell.getState().hoverActive).toBe(false);
  });
});
