import { beforeEach, describe, expect, it } from "vitest";

import { useOsOpenShell } from "./os-open-shell-store";

describe("os-open shell store", () => {
  beforeEach(() => {
    useOsOpenShell.setState({ pendingSignal: false });
  });

  it("starts with no pending signal", () => {
    expect(useOsOpenShell.getState().pendingSignal).toBe(false);
  });

  it("raise() marks a signal pending", () => {
    useOsOpenShell.getState().raise();
    expect(useOsOpenShell.getState().pendingSignal).toBe(true);
  });

  it("clear() consumes the signal", () => {
    useOsOpenShell.getState().raise();
    useOsOpenShell.getState().clear();
    expect(useOsOpenShell.getState().pendingSignal).toBe(false);
  });

  it("raise() is idempotent while unconsumed (a boolean, never a queue)", () => {
    useOsOpenShell.getState().raise();
    useOsOpenShell.getState().raise();
    expect(useOsOpenShell.getState().pendingSignal).toBe(true);
    useOsOpenShell.getState().clear();
    expect(useOsOpenShell.getState().pendingSignal).toBe(false);
  });
});
