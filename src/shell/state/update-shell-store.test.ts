import { beforeEach, describe, expect, it } from "vitest";

import type { UpdateAvailability } from "../../shared/ipc-contracts/settings";
import { useUpdateShell } from "./update-shell-store";

const VERDICT: UpdateAvailability = {
  status: "updateAvailable",
  headline: "Nouvelle version disponible : 9.9.9.",
  notice:
    "Ta version actuelle est 0.1.0. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.",
  currentVersion: "0.1.0",
  latestVersion: "9.9.9",
};

describe("useUpdateShell", () => {
  beforeEach(() => {
    useUpdateShell.setState({ availability: null });
  });

  it("starts with no verdict — the surfaces render nothing", () => {
    expect(useUpdateShell.getState().availability).toBeNull();
  });

  it("pours the read verdict in for the two calm consumers", () => {
    useUpdateShell.getState().setAvailability(VERDICT);
    expect(useUpdateShell.getState().availability).toEqual(VERDICT);
  });
});
