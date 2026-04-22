import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import {
  DEFAULT_LIBRARY_SORT,
} from "../../../shell/state/library-shell-store";
import type { LibrarySortKey } from "../hooks/use-library-collection";
import { StoryCollection } from "./StoryCollection";

const sampleStories = [
  { id: "s1", title: "Le soleil d'Éloi" },
  { id: "s2", title: "La lune des chats" },
  { id: "s3", title: "Étoile filante" },
];

interface HarnessProps {
  stories?: typeof sampleStories;
  isLoading?: boolean;
  initialQuery?: string;
  initialSort?: LibrarySortKey;
  selectedStoryIds?: ReadonlySet<string>;
  onSelectStory?: (id: string, mode: "replace" | "toggle") => void;
  onOpenStory?: (id: string) => void;
}

/** Testing harness that owns query/sort state locally so existing
 *  user-event-driven assertions keep working against a controlled collection. */
function Harness(props: HarnessProps) {
  const [query, setQuery] = useState(props.initialQuery ?? "");
  const [sort, setSort] = useState<LibrarySortKey>(
    props.initialSort ?? DEFAULT_LIBRARY_SORT,
  );
  return (
    <StoryCollection
      stories={props.stories ?? sampleStories}
      isLoading={props.isLoading ?? false}
      query={query}
      sort={sort}
      onQueryChange={setQuery}
      onSortChange={setSort}
      onResetFilters={() => {
        setQuery("");
        setSort(DEFAULT_LIBRARY_SORT);
      }}
      selectedStoryIds={props.selectedStoryIds}
      onSelectStory={props.onSelectStory}
      onOpenStory={props.onOpenStory}
    />
  );
}

describe("<StoryCollection />", () => {
  it("renders all cards and announces the total count", () => {
    render(<Harness />);

    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /la lune des chats/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /étoile filante/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^3 histoires$/)).toBeInTheDocument();
  });

  it("filters by search query and narrows the counter to 'X sur Y'", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const search = screen.getByLabelText(/rechercher une histoire/i);
    await user.type(search, "soleil");

    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: /la lune des chats/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/^1 sur 3$/)).toBeInTheDocument();
  });

  it("shows the filtered-empty state with a Réinitialiser les filtres button when a query matches nothing", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "xyz-nope",
    );

    expect(
      screen.getByRole("heading", { name: /aucun résultat/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^0 sur 3$/)).toBeInTheDocument();

    const reset = screen.getByRole("button", {
      name: /réinitialiser les filtres/i,
    });
    await user.click(reset);

    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^3 histoires$/)).toBeInTheDocument();
  });

  it("shows the loaded-empty state with a disabled create CTA and its reason when the collection is really empty", () => {
    render(<Harness stories={[]} />);

    expect(
      screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^0 histoire$/)).toBeInTheDocument();

    const create = screen.getByRole("button", {
      name: /créer une histoire/i,
    });
    expect(create).not.toBeDisabled();
    expect(create).toHaveAttribute("aria-disabled", "true");

    const reasonId = create.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(/création d'histoire indisponible/i);
  });

  it("shows the pending state with a progress indicator while loading", () => {
    render(<Harness stories={[]} isLoading={true} />);

    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
  });

  it("sorts alphabetically ascending by default and descending when toggled", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const headingsAsc = screen
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headingsAsc).toEqual([
      "Étoile filante",
      "La lune des chats",
      "Le soleil d'Éloi",
    ]);

    const select = screen.getByLabelText(/trier par/i);
    await user.selectOptions(select, "titre-desc");

    const headingsDesc = screen
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headingsDesc).toEqual([
      "Le soleil d'Éloi",
      "La lune des chats",
      "Étoile filante",
    ]);
  });

  it("does not put aria-live on the counter (status regions announce enough; avoids double-announce on keystroke)", () => {
    render(<Harness />);
    const counter = screen.getByText(/^3 histoires$/);
    expect(counter).not.toHaveAttribute("aria-live");
  });

  it("exposes a disabled filter placeholder with the canonical 'Filtres avancés à venir' reason", () => {
    render(<Harness />);
    const filter = screen.getByRole("button", {
      name: /toutes les histoires/i,
    });
    expect(filter).not.toBeDisabled();
    expect(filter).toHaveAttribute("aria-disabled", "true");

    const reasonId = filter.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(/filtres avancés à venir/i);
  });

  it("Tab walks through search → sort → filter placeholder → first Story Card", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.tab();
    expect(screen.getByLabelText(/rechercher une histoire/i)).toHaveFocus();

    await user.tab();
    expect(screen.getByLabelText(/trier par/i)).toHaveFocus();

    await user.tab();
    expect(
      screen.getByRole("button", { name: /toutes les histoires/i }),
    ).toHaveFocus();

    await user.tab();
    expect(
      screen.getByRole("button", { name: /étoile filante/i }),
    ).toHaveFocus();
  });

  it("resets both the query and the sort when Réinitialiser les filtres is pressed", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.selectOptions(
      screen.getByLabelText(/trier par/i),
      "titre-desc",
    );
    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "xyz-nope",
    );

    await user.click(
      screen.getByRole("button", { name: /réinitialiser les filtres/i }),
    );

    expect(screen.getByLabelText(/trier par/i)).toHaveValue("titre-asc");
    const headings = screen
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headings).toEqual([
      "Étoile filante",
      "La lune des chats",
      "Le soleil d'Éloi",
    ]);
  });

  it("announces loaded-empty, filtered-empty and pending states as polite status regions", async () => {
    const user = userEvent.setup();

    const { rerender } = render(<Harness stories={[]} isLoading={true} />);
    const pending = screen.getAllByRole("status");
    expect(
      pending.some((node) => node.getAttribute("aria-live") === "polite"),
    ).toBe(true);

    rerender(<Harness stories={[]} isLoading={false} />);
    const emptyStatuses = screen.getAllByRole("status");
    expect(
      emptyStatuses.some((node) =>
        node.textContent?.includes("Ta bibliothèque est vide"),
      ),
    ).toBe(true);

    rerender(<Harness stories={sampleStories} isLoading={false} />);
    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "xyz-nope",
    );
    const filteredStatuses = screen.getAllByRole("status");
    expect(
      filteredStatuses.some((node) =>
        node.textContent?.includes("Aucun résultat"),
      ),
    ).toBe(true);
  });

  it("is controlled by props: initial query/sort are honored and change callbacks fire", async () => {
    const user = userEvent.setup();
    const onQueryChange = vi.fn();
    const onSortChange = vi.fn();
    render(
      <StoryCollection
        stories={sampleStories}
        isLoading={false}
        query="soleil"
        sort="titre-desc"
        onQueryChange={onQueryChange}
        onSortChange={onSortChange}
        onResetFilters={vi.fn()}
      />,
    );

    expect(screen.getByLabelText(/rechercher une histoire/i)).toHaveValue(
      "soleil",
    );
    expect(screen.getByLabelText(/trier par/i)).toHaveValue("titre-desc");
    // Only the filtered story renders.
    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: /la lune/i }),
    ).not.toBeInTheDocument();

    await user.type(screen.getByLabelText(/rechercher une histoire/i), "X");
    expect(onQueryChange).toHaveBeenCalled();

    await user.selectOptions(
      screen.getByLabelText(/trier par/i),
      "titre-asc",
    );
    expect(onSortChange).toHaveBeenCalledWith("titre-asc");
  });

  // --- Selection wiring ---

  it("renders a card as selected when its id is in selectedStoryIds", () => {
    render(<Harness selectedStoryIds={new Set(["s1"])} />);

    const selected = screen.getByRole("button", { name: /le soleil d'éloi/i });
    expect(selected).toHaveAttribute("aria-pressed", "true");
    const other = screen.getByRole("button", { name: /étoile filante/i });
    expect(other).toHaveAttribute("aria-pressed", "false");
  });

  it("click on a card calls onSelectStory with replace", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(<Harness onSelectStory={onSelectStory} />);

    await user.click(screen.getByRole("button", { name: /la lune des chats/i }));
    expect(onSelectStory).toHaveBeenCalledWith("s2", "replace");
  });

  it("Ctrl+click on a card calls onSelectStory with toggle", async () => {
    const user = userEvent.setup();
    const onSelectStory = vi.fn();
    render(
      <Harness
        selectedStoryIds={new Set(["s1"])}
        onSelectStory={onSelectStory}
      />,
    );

    await user.keyboard("{Control>}");
    await user.click(
      screen.getByRole("button", { name: /la lune des chats/i }),
    );
    await user.keyboard("{/Control}");
    expect(onSelectStory).toHaveBeenCalledWith("s2", "toggle");
  });

  it("double-click on a card calls onOpenStory", async () => {
    const user = userEvent.setup();
    const onOpenStory = vi.fn();
    render(<Harness onOpenStory={onOpenStory} />);

    await user.dblClick(
      screen.getByRole("button", { name: /étoile filante/i }),
    );
    expect(onOpenStory).toHaveBeenCalledWith("s3");
  });

  it("counter appends '— X sélectionnée(s)' when the selection is non-empty", () => {
    const { rerender } = render(<Harness selectedStoryIds={new Set(["s1"])} />);
    expect(
      screen.getByText(/^3 histoires — 1 sélectionnée$/),
    ).toBeInTheDocument();

    rerender(<Harness selectedStoryIds={new Set(["s1", "s2"])} />);
    expect(
      screen.getByText(/^3 histoires — 2 sélectionnées$/),
    ).toBeInTheDocument();
  });

  it("counter shows 'X sur Y' without selected-clause when selection is empty", async () => {
    const user = userEvent.setup();
    render(<Harness selectedStoryIds={new Set()} />);

    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "soleil",
    );

    expect(screen.getByText(/^1 sur 3$/)).toBeInTheDocument();
    expect(screen.queryByText(/sélectionnée/)).not.toBeInTheDocument();
  });

  it("filtered-empty counter keeps the '— X sélectionnée(s)' clause so the selection stays visible", async () => {
    const user = userEvent.setup();
    render(<Harness selectedStoryIds={new Set(["s1"])} />);

    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "xyz-nope",
    );

    expect(
      screen.getByText(/^0 sur 3 — 1 sélectionnée$/),
    ).toBeInTheDocument();
  });
});
