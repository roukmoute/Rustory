import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/settings", () => ({
  refreshUpdateAvailability: vi.fn(),
}));

import { refreshUpdateAvailability } from "../../../ipc/commands/settings";
import { useUpdateShell } from "../../../shell/state/update-shell-store";
import { UpdateCheckButton } from "./UpdateCheckButton";

describe("UpdateCheckButton", () => {
  beforeEach(() => {
    vi.mocked(refreshUpdateAvailability).mockReset();
    useUpdateShell.setState({ availability: null });
  });
  afterEach(() => {
    useUpdateShell.setState({ availability: null });
  });

  it("re-checks on click and pours the fresh verdict into the shared store", async () => {
    const user = userEvent.setup();
    vi.mocked(refreshUpdateAvailability).mockResolvedValueOnce({
      status: "updateAvailable",
      headline: "Nouvelle version disponible : 9.9.9.",
      notice: "…",
      currentVersion: "0.1.0",
      latestVersion: "9.9.9",
    });
    render(<UpdateCheckButton />);
    await user.click(
      screen.getByRole("button", { name: /rechercher une mise à jour/i }),
    );
    expect(refreshUpdateAvailability).toHaveBeenCalledTimes(1);
    // The fresh verdict becomes the shared truth (banner + status line read it).
    await waitFor(() =>
      expect(useUpdateShell.getState().availability?.status).toBe(
        "updateAvailable",
      ),
    );
  });

  it("shows a calm neutral note when the check cannot reach the server", async () => {
    const user = userEvent.setup();
    vi.mocked(refreshUpdateAvailability).mockResolvedValueOnce({
      status: "checkUnavailable",
      headline: "La vérification de version n'a pas pu être faite.",
      notice: "Rustory reste pleinement utilisable.",
      currentVersion: "0.1.0",
    });
    render(<UpdateCheckButton />);
    await user.click(
      screen.getByRole("button", { name: /rechercher une mise à jour/i }),
    );
    expect(
      await screen.findByText(/Vérification impossible pour le moment/i),
    ).toBeInTheDocument();
  });

  it("stays calm (a neutral note, no error surface) when the facade rejects", async () => {
    const user = userEvent.setup();
    vi.mocked(refreshUpdateAvailability).mockRejectedValueOnce(
      new Error("drift"),
    );
    render(<UpdateCheckButton />);
    await user.click(
      screen.getByRole("button", { name: /rechercher une mise à jour/i }),
    );
    expect(
      await screen.findByText(/Vérification impossible pour le moment/i),
    ).toBeInTheDocument();
    // Never an alarm-toned alert for a mere update-check hiccup.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("swallows a re-entrant click while a check is in flight", async () => {
    const user = userEvent.setup();
    let release: () => void = () => {};
    vi.mocked(refreshUpdateAvailability).mockImplementation(
      () =>
        new Promise<Awaited<ReturnType<typeof refreshUpdateAvailability>>>(
          (resolve) => {
            release = () =>
              resolve({
                status: "upToDate",
                headline: "Tu as la dernière version.",
                notice: "…",
                currentVersion: "0.8.0",
              });
          },
        ),
    );
    render(<UpdateCheckButton />);
    const button = screen.getByRole("button", {
      name: /rechercher une mise à jour/i,
    });
    await user.click(button);
    await user.click(button);
    release();
    await waitFor(() =>
      expect(refreshUpdateAvailability).toHaveBeenCalledTimes(1),
    );
  });
});
