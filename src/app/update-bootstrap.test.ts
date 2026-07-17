import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../ipc/commands/settings", () => ({
  readUpdateAvailability: vi.fn(),
}));

import { readUpdateAvailability } from "../ipc/commands/settings";
import type { UpdateAvailability } from "../shared/ipc-contracts/settings";
import { useUpdateShell } from "../shell/state/update-shell-store";
import {
  bootstrapUpdateAvailability,
  resetUpdateBootstrapForTests,
} from "./update-bootstrap";

const VERDICT: UpdateAvailability = {
  status: "upToDate",
  headline: "Aucune version plus récente n'est publiée.",
  notice: "Aucune action n'est nécessaire.",
  currentVersion: "0.1.0",
};

/** Let the bootstrap's fire-and-forget promise chain settle. */
async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("bootstrapUpdateAvailability", () => {
  beforeEach(() => {
    vi.mocked(readUpdateAvailability).mockReset();
    resetUpdateBootstrapForTests();
    useUpdateShell.setState({ availability: null });
  });

  it("reads once and pours the verdict into the shell store", async () => {
    vi.mocked(readUpdateAvailability).mockResolvedValueOnce(VERDICT);
    bootstrapUpdateAvailability();
    await flushMicrotasks();
    expect(readUpdateAvailability).toHaveBeenCalledTimes(1);
    expect(useUpdateShell.getState().availability).toEqual(VERDICT);
  });

  it("is one-shot: a second call never re-reads (StrictMode-safe)", async () => {
    vi.mocked(readUpdateAvailability).mockResolvedValue(VERDICT);
    bootstrapUpdateAvailability();
    bootstrapUpdateAvailability();
    await flushMicrotasks();
    expect(readUpdateAvailability).toHaveBeenCalledTimes(1);
  });

  it("stays totally silent on a facade rejection — the app lives without a verdict", async () => {
    vi.mocked(readUpdateAvailability).mockRejectedValueOnce(
      new Error("contract drift"),
    );
    bootstrapUpdateAvailability();
    await flushMicrotasks();
    // No verdict, no surface — and no unhandled rejection (the chain
    // swallows it by contract).
    expect(useUpdateShell.getState().availability).toBeNull();
  });

  it("re-arms for tests through the dedicated guard reset", async () => {
    vi.mocked(readUpdateAvailability).mockResolvedValue(VERDICT);
    bootstrapUpdateAvailability();
    await flushMicrotasks();
    resetUpdateBootstrapForTests();
    bootstrapUpdateAvailability();
    await flushMicrotasks();
    expect(readUpdateAvailability).toHaveBeenCalledTimes(2);
  });
});
