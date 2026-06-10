import { fireEvent, render, screen, within } from "@testing-library/react";
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

  // --- Selection for inspection (AC1, AC3) ---

  const readyTwo: DeviceLibraryState = {
    kind: "ready",
    deviceIdentifier: "0123456789abcdef0123456789abcdef",
    stories: [
      { uuid: "u1", shortId: "0000ABCD", hidden: false, contentPresent: true },
      { uuid: "u2", shortId: "0000BEEF", hidden: false, contentPresent: true },
    ],
  };

  it("entries stay static (no button role) when no selection handler is wired", () => {
    renderState(readyTwo);
    // Listing-only mode preserves the pre-inspection behavior.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("0000ABCD")).toBeInTheDocument();
  });

  it("a wired card is a role=button focus stop and clicking it reports the uuid", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    const first = screen.getByRole("button", {
      name: /identifiant 0000abcd/i,
    });
    expect(first).toHaveAttribute("aria-pressed", "false");
    await user.click(first);
    expect(onSelectStory).toHaveBeenCalledWith("u1");
  });

  it("reflects the selected uuid with aria-pressed and a visible non-color marker", () => {
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid="u2"
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    const selected = screen.getByRole("button", {
      name: /identifiant 0000beef/i,
    });
    expect(selected).toHaveAttribute("aria-pressed", "true");
    expect(within(selected).getByText("✓")).toBeInTheDocument();
    const other = screen.getByRole("button", {
      name: /identifiant 0000abcd/i,
    });
    expect(other).toHaveAttribute("aria-pressed", "false");
  });

  it("Space toggles the selection from the keyboard", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    screen.getByRole("button", { name: /identifiant 0000abcd/i }).focus();
    await user.keyboard(" ");
    expect(onSelectStory).toHaveBeenCalledWith("u1");
  });

  it("Enter toggles the selection from the keyboard", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    screen.getByRole("button", { name: /identifiant 0000abcd/i }).focus();
    await user.keyboard("{Enter}");
    expect(onSelectStory).toHaveBeenCalledWith("u1");
  });

  it("ignores OS key auto-repeat so a held Enter/Space does not flicker the selection", () => {
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    const card = screen.getByRole("button", { name: /identifiant 0000abcd/i });
    fireEvent.keyDown(card, { key: "Enter", repeat: true });
    fireEvent.keyDown(card, { key: " ", repeat: true });
    expect(onSelectStory).not.toHaveBeenCalled();
  });

  it("folds the structural flags into the card accessible name", () => {
    render(
      <DeviceStoryCollection
        state={{
          kind: "ready",
          deviceIdentifier: "0123456789abcdef0123456789abcdef",
          stories: [
            { uuid: "u9", shortId: "0000F00D", hidden: true, contentPresent: false },
          ],
        }}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    // The flags must be reachable in the accessible name, not only as chips.
    expect(
      screen.getByRole("button", {
        name: /identifiant 0000f00d, masquée, contenu incomplet/i,
      }),
    ).toBeInTheDocument();
  });

  it("rescues keyboard focus to the section heading when the selected card is removed by a re-read", () => {
    const { rerender } = render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid="u1"
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    const card = screen.getByRole("button", { name: /identifiant 0000abcd/i });
    card.focus();
    expect(card).toHaveFocus();

    // The re-read no longer lists u1 (the parent has not purged the selection
    // yet): the focused card unmounts and focus would otherwise fall to <body>.
    rerender(
      <DeviceStoryCollection
        state={{
          kind: "ready",
          deviceIdentifier: "0123456789abcdef0123456789abcdef",
          stories: [
            { uuid: "u2", shortId: "0000BEEF", hidden: false, contentPresent: true },
          ],
        }}
        isRefreshing={false}
        selectedUuid="u1"
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );

    expect(
      screen.getByRole("heading", { name: /histoires sur l'appareil/i }),
    ).toHaveFocus();
  });

  it("does not steal focus on mount, nor when a re-read keeps the focused card", () => {
    const { rerender } = render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuid={null}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    const heading = screen.getByRole("heading", {
      name: /histoires sur l'appareil/i,
    });
    expect(heading).not.toHaveFocus();

    // The focused card survives the re-read (still listed) → focus stays put.
    const card = screen.getByRole("button", { name: /identifiant 0000abcd/i });
    card.focus();
    rerender(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={true}
        selectedUuid="u1"
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(card).toHaveFocus();
    expect(heading).not.toHaveFocus();
  });
});
