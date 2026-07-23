import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

// Covers are resolved from the LOCAL cache via a Tauri command; mock the hook
// so the component test stays offline and deterministic. Honors `hasCover`
// so a pack without a cached cover renders no image.
vi.mock("../hooks/use-pack-cover", () => ({
  usePackCover: (_uuid: string, hasCover: boolean) =>
    hasCover ? "data:image/png;base64,COVER" : null,
}));

import { DeviceStoryCollection } from "./DeviceStoryCollection";
import type { DeviceLibraryState } from "../hooks/use-device-library";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";

/** Build a device story with unrecognized-by-default recognition fields, so
 *  each test only states the facts it cares about. */
function makeStory(overrides: Partial<DeviceStoryDto> = {}): DeviceStoryDto {
  return {
    uuid: "u1",
    shortId: "0000ABCD",
    hidden: false,
    contentPresent: true,
    alreadyImported: false,
    title: null,
    titleSource: null,
    thumbnail: null,
    ...overrides,
  };
}

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
          makeStory({ uuid: "u1", shortId: "0000ABCD" }),
          makeStory({ uuid: "u2", shortId: "0000BEEF", hidden: true }),
          makeStory({ uuid: "u3", shortId: "0000F00D", contentPresent: false }),
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

    // Unrecognized packs keep the opaque "non reconnue" label + identifier.
    expect(
      within(region).getAllByText(/histoire non reconnue/i),
    ).toHaveLength(3);
    expect(within(region).getByText("0000ABCD")).toBeInTheDocument();
    expect(within(region).getByText("0000BEEF")).toBeInTheDocument();

    // Per-entry flags.
    expect(within(region).getByText(/masquée/i)).toBeInTheDocument();
    expect(within(region).getByText(/contenu incomplet/i)).toBeInTheDocument();
  });

  it("shows the real title + provenance chip for recognized packs (AC1)", () => {
    renderState({
      kind: "ready",
      deviceIdentifier: "0123456789abcdef0123456789abcdef",
      stories: [
        makeStory({
          uuid: "u-off",
          shortId: "0000OFFI",
          title: "Suzanne et Gaston",
          titleSource: "official",
        }),
        makeStory({
          uuid: "u-usr",
          shortId: "0000USER",
          title: "Mon histoire",
          titleSource: "user",
        }),
        makeStory({
          uuid: "u-unk",
          shortId: "0000UNKN",
        }),
      ],
    });
    const region = screen.getByRole("region", {
      name: /bibliothèque de l'appareil/i,
    });
    // Real titles surface; the unknown one stays "non reconnue".
    expect(within(region).getByText("Suzanne et Gaston")).toBeInTheDocument();
    expect(within(region).getByText("Mon histoire")).toBeInTheDocument();
    expect(within(region).getAllByText(/histoire non reconnue/i)).toHaveLength(1);
    // Provenance is shown honestly and distinctly — official vs saisi.
    expect(within(region).getByText("Titre officiel")).toBeInTheDocument();
    expect(within(region).getByText("Titre saisi")).toBeInTheDocument();
  });

  it("renders the cached cover for a recognized pack that has one", () => {
    const { container } = renderState({
      kind: "ready",
      deviceIdentifier: "0123456789abcdef0123456789abcdef",
      stories: [
        makeStory({
          uuid: "u-cover",
          shortId: "0000COVR",
          title: "Avec couverture",
          titleSource: "official",
          thumbnail: "u-cover.png",
        }),
        makeStory({ uuid: "u-none", shortId: "0000NONE" }),
      ],
    });
    const covers = container.querySelectorAll<HTMLImageElement>(
      ".device-story-card__cover",
    );
    // Exactly the pack with a thumbnail gets a cover, served as a data: URL.
    expect(covers).toHaveLength(1);
    expect(covers[0].src).toContain("data:image/png;base64,COVER");
  });

  it("renders FLAM inventory entries with the same badges as Lunii ones (family-neutral DTO)", () => {
    // Real FLAM wire shapes: full lowercase story UUIDs from the text
    // index, uppercase 8-hex tails. The chips derive from the SAME
    // neutral flags — no family-conditional rendering exists.
    renderState(
      {
        kind: "ready",
        deviceIdentifier: "fedcba9876543210fedcba9876543210",
        stories: [
          makeStory({
            uuid: "12345678-9abc-def0-1122-334455667788",
            shortId: "55667788",
            hidden: true,
          }),
          makeStory({
            uuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeffff0000",
            shortId: "FFFF0000",
            contentPresent: false,
          }),
          makeStory({
            uuid: "bbbbbbbb-cccc-dddd-eeee-ffff00001111",
            shortId: "00001111",
            alreadyImported: true,
          }),
        ],
      },
      { deviceLabel: "FLAM" },
    );
    expect(
      screen.getByRole("heading", { name: /histoires sur l'appareil — flam/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("Masquée")).toBeInTheDocument();
    expect(screen.getByText("Contenu incomplet")).toBeInTheDocument();
    expect(screen.getByText("Dans ta bibliothèque")).toBeInTheDocument();
  });

  it("hints the empty device with the family-neutral copy (never a Lunii-only wording)", () => {
    renderState(
      {
        kind: "ready",
        deviceIdentifier: "fedcba9876543210fedcba9876543210",
        stories: [],
      },
      { deviceLabel: "FLAM" },
    );
    expect(
      screen.getByText(/l'appareil connecté ne contient aucune histoire lisible\./i),
    ).toBeInTheDocument();
    expect(screen.queryByText(/la lunii connectée/i)).toBeNull();
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
        stories: [makeStory({ uuid: "u1", shortId: "0000ABCD" })],
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
      makeStory({ uuid: "u1", shortId: "0000ABCD" }),
      makeStory({ uuid: "u2", shortId: "0000BEEF" }),
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
        selectedUuids={new Set()}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    const first = screen.getByRole("button", {
      name: /identifiant 0000abcd/i,
    });
    expect(first).toHaveAttribute("aria-pressed", "false");
    await user.click(first);
    // Plain click on an unselected card selects exactly this one.
    expect(onSelectStory).toHaveBeenCalledWith("u1", "replace");
  });

  it("reflects the selected uuid with aria-pressed and a visible non-color marker", () => {
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set(["u2"])}
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

  it("Ctrl+click toggles a card into a multi-selection", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set(["u1"])}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    await user.keyboard("{Control>}");
    await user.click(
      screen.getByRole("button", { name: /identifiant 0000beef/i }),
    );
    await user.keyboard("{/Control}");
    expect(onSelectStory).toHaveBeenCalledWith("u2", "toggle");
  });

  it("marks every selected card of a multi-selection as pressed", () => {
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set(["u1", "u2"])}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(
      screen.getByRole("button", { name: /identifiant 0000abcd/i }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: /identifiant 0000beef/i }),
    ).toHaveAttribute("aria-pressed", "true");
    // The counter announces how many are selected.
    expect(screen.getByText(/2 sélectionnées/i)).toBeInTheDocument();
  });

  it("shows the multi-selection hint only when a selection handler is wired", () => {
    const { rerender } = render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        onRetry={() => {}}
      />,
    );
    expect(
      screen.queryByText(/pour en sélectionner plusieurs/i),
    ).not.toBeInTheDocument();
    rerender(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set()}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(
      screen.getByText(/pour en sélectionner plusieurs/i),
    ).toBeInTheDocument();
  });

  it("Space toggles the card into the multi-selection from the keyboard", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set()}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    screen.getByRole("button", { name: /identifiant 0000abcd/i }).focus();
    await user.keyboard(" ");
    expect(onSelectStory).toHaveBeenCalledWith("u1", "toggle");
  });

  it("Enter selects exactly this one from the keyboard", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set()}
        onSelectStory={onSelectStory}
        onRetry={() => {}}
      />,
    );
    screen.getByRole("button", { name: /identifiant 0000abcd/i }).focus();
    await user.keyboard("{Enter}");
    expect(onSelectStory).toHaveBeenCalledWith("u1", "replace");
  });

  it("ignores OS key auto-repeat so a held Enter/Space does not flicker the selection", () => {
    const onSelectStory = vi.fn();
    render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set()}
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
            makeStory({
              uuid: "u9",
              shortId: "0000F00D",
              hidden: true,
              contentPresent: false,
            }),
          ],
        }}
        isRefreshing={false}
        selectedUuids={new Set()}
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

  it("folds the recognized title + provenance into the card accessible name", () => {
    render(
      <DeviceStoryCollection
        state={{
          kind: "ready",
          deviceIdentifier: "0123456789abcdef0123456789abcdef",
          stories: [
            makeStory({
              uuid: "u-off",
              shortId: "0000OFFI",
              title: "Le Loup",
              titleSource: "official",
            }),
          ],
        }}
        isRefreshing={false}
        selectedUuids={new Set()}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: /le loup, titre officiel, identifiant 0000offi/i,
      }),
    ).toBeInTheDocument();
  });

  it("marks an already-imported entry with the 'Dans ta bibliothèque' chip", () => {
    renderState({
      kind: "ready",
      deviceIdentifier: "0123456789abcdef0123456789abcdef",
      stories: [
        makeStory({ uuid: "u1", shortId: "0000ABCD", alreadyImported: true }),
        makeStory({ uuid: "u2", shortId: "0000BEEF" }),
      ],
    });
    // Exactly one card carries the local-copy marker.
    expect(screen.getAllByText("Dans ta bibliothèque")).toHaveLength(1);
  });

  it("folds the already-imported marker into the card accessible name", () => {
    render(
      <DeviceStoryCollection
        state={{
          kind: "ready",
          deviceIdentifier: "0123456789abcdef0123456789abcdef",
          stories: [
            makeStory({
              uuid: "u8",
              shortId: "0000CAFE",
              alreadyImported: true,
            }),
          ],
        }}
        isRefreshing={false}
        selectedUuids={new Set()}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: /identifiant 0000cafe, dans ta bibliothèque/i,
      }),
    ).toBeInTheDocument();
  });

  it("rescues keyboard focus to the section heading when the selected card is removed by a re-read", () => {
    const { rerender } = render(
      <DeviceStoryCollection
        state={readyTwo}
        isRefreshing={false}
        selectedUuids={new Set(["u1"])}
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
          stories: [makeStory({ uuid: "u2", shortId: "0000BEEF" })],
        }}
        isRefreshing={false}
        selectedUuids={new Set(["u1"])}
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
        selectedUuids={new Set()}
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
        selectedUuids={new Set(["u1"])}
        onSelectStory={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(card).toHaveFocus();
    expect(heading).not.toHaveFocus();
  });
});
