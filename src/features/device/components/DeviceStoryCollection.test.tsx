import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DeviceStoryCollection } from "./DeviceStoryCollection";
import type { DeviceLibraryState } from "../hooks/use-device-library";

function renderState(
  state: DeviceLibraryState,
  extra: { isRefreshing?: boolean; deviceLabel?: string; onRetry?: () => void } = {},
) {
  return render(
    <DeviceStoryCollection
      state={state}
      isRefreshing={extra.isRefreshing ?? false}
      deviceLabel={extra.deviceLabel}
      onRetry={extra.onRetry ?? (() => {})}
    />,
  );
}

describe("<DeviceStoryCollection />", () => {
  it("renders nothing in the idle state (no readable device)", () => {
    const { container } = renderState({ kind: "idle" });
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a calm in-context progress while loading", () => {
    renderState({ kind: "loading" });
    const region = screen.getByRole("region", {
      name: /bibliothèque de l'appareil/i,
    });
    expect(
      within(region).getByRole("progressbar", {
        name: /lecture de la bibliothèque de l'appareil/i,
      }),
    ).toBeInTheDocument();
  });

  it("lists device stories distinctly, with provenance and opaque identifiers", () => {
    renderState(
      {
        kind: "ready",
        deviceIdentifier: "0123456789abcdef0123456789abcdef",
        stories: [
          { uuid: "u1", shortId: "0000ABCD", hidden: false, contentPresent: true },
          { uuid: "u2", shortId: "0000BEEF", hidden: true, contentPresent: true },
          { uuid: "u3", shortId: "0000F00D", hidden: false, contentPresent: false },
        ],
      },
      { deviceLabel: "Lunii V3" },
    );

    const region = screen.getByRole("region", {
      name: /bibliothèque de l'appareil/i,
    });
    // Provenance is explicit in the section heading + chip.
    expect(
      within(region).getByRole("heading", {
        name: /histoires sur l'appareil — lunii v3/i,
      }),
    ).toBeInTheDocument();
    expect(within(region).getAllByText(/sur l'appareil/i).length).toBeGreaterThan(0);

    // No asserted title — each entry is an opaque, unrecognized identity.
    expect(
      within(region).getAllByText(/histoire non reconnue/i),
    ).toHaveLength(3);
    expect(within(region).getByText("0000ABCD")).toBeInTheDocument();
    expect(within(region).getByText("0000BEEF")).toBeInTheDocument();

    // Per-entry flags.
    expect(within(region).getByText(/masquée/i)).toBeInTheDocument();
    expect(within(region).getByText(/contenu incomplet/i)).toBeInTheDocument();
  });

  it("distinguishes the empty device from the not-yet-loaded state", () => {
    renderState({
      kind: "ready",
      deviceIdentifier: "0123456789abcdef0123456789abcdef",
      stories: [],
    });
    expect(
      screen.getByRole("heading", { name: /aucune histoire sur l'appareil/i }),
    ).toBeInTheDocument();
    // Empty is NOT a loading state — no progress bar.
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("surfaces a recoverable error in context (never a toast) and retries", async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    renderState(
      {
        kind: "error",
        error: {
          code: "DEVICE_SCAN_FAILED",
          message: "Lecture de la bibliothèque appareil indisponible.",
          userAction: "Vérifie la connexion de la Lunii puis réessaie.",
          details: null,
        },
      },
      { onRetry },
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/bibliothèque de l'appareil indisponible/i);
    expect(alert).toHaveTextContent(/vérifie la connexion de la lunii/i);

    await user.click(screen.getByRole("button", { name: /réessayer/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("shows an actualisation hint instead of the count during a refresh", () => {
    renderState(
      {
        kind: "ready",
        deviceIdentifier: "0123456789abcdef0123456789abcdef",
        stories: [{ uuid: "u1", shortId: "0000ABCD", hidden: false, contentPresent: true }],
      },
      { isRefreshing: true },
    );
    expect(screen.getByText(/actualisation/i)).toBeInTheDocument();
  });
});
