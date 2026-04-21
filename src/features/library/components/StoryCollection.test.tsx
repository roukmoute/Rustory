import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { StoryCollection } from "./StoryCollection";

const sampleStories = [
  { id: "s1", title: "Le soleil d'Éloi" },
  { id: "s2", title: "La lune des chats" },
  { id: "s3", title: "Étoile filante" },
];

describe("<StoryCollection />", () => {
  it("renders all cards and announces the total count", () => {
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

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
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

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
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

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

    // All three stories come back after the reset.
    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^3 histoires$/)).toBeInTheDocument();
  });

  it("shows the loaded-empty state with a disabled create CTA and its reason when the collection is really empty", () => {
    render(<StoryCollection stories={[]} isLoading={false} />);

    expect(
      screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^0 histoire$/)).toBeInTheDocument();

    const create = screen.getByRole("button", {
      name: /créer une histoire/i,
    });
    // Stays focusable — keyboard users must reach it and read the reason.
    expect(create).not.toBeDisabled();
    expect(create).toHaveAttribute("aria-disabled", "true");

    const reasonId = create.getAttribute("aria-describedby");
    expect(reasonId).toBeTruthy();
    const reason = document.getElementById(reasonId as string);
    expect(reason).toHaveTextContent(/création d'histoire indisponible/i);
  });

  it("shows the pending state with a progress indicator while loading", () => {
    render(<StoryCollection stories={[]} isLoading={true} />);

    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    // The empty state must NOT leak through while we're still fetching.
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
  });

  it("sorts alphabetically ascending by default and descending when toggled", async () => {
    const user = userEvent.setup();
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

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
    render(<StoryCollection stories={sampleStories} isLoading={false} />);
    const counter = screen.getByText(/^3 histoires$/);
    expect(counter).not.toHaveAttribute("aria-live");
  });

  it("exposes a disabled filter placeholder with the canonical 'Filtres avancés à venir' reason", () => {
    render(<StoryCollection stories={sampleStories} isLoading={false} />);
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
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

    await user.tab();
    expect(screen.getByLabelText(/rechercher une histoire/i)).toHaveFocus();

    await user.tab();
    expect(screen.getByLabelText(/trier par/i)).toHaveFocus();

    await user.tab();
    expect(
      screen.getByRole("button", { name: /toutes les histoires/i }),
    ).toHaveFocus();

    await user.tab();
    // The first Story Card in ascending order is "Étoile filante".
    expect(
      screen.getByRole("group", { name: /étoile filante/i }),
    ).toHaveFocus();
  });

  it("resets both the query and the sort when Réinitialiser les filtres is pressed", async () => {
    const user = userEvent.setup();
    render(<StoryCollection stories={sampleStories} isLoading={false} />);

    // Move to descending sort first so we can assert it reverts.
    await user.selectOptions(
      screen.getByLabelText(/trier par/i),
      "titre-desc",
    );
    // Narrow the collection to trigger filtered-empty.
    await user.type(
      screen.getByLabelText(/rechercher une histoire/i),
      "xyz-nope",
    );

    await user.click(
      screen.getByRole("button", { name: /réinitialiser les filtres/i }),
    );

    // Sort must be back to ascending.
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

    const { rerender } = render(
      <StoryCollection stories={[]} isLoading={true} />,
    );
    const pending = screen.getAllByRole("status");
    expect(
      pending.some((node) => node.getAttribute("aria-live") === "polite"),
    ).toBe(true);

    rerender(<StoryCollection stories={[]} isLoading={false} />);
    const emptyStatuses = screen.getAllByRole("status");
    // The counter AND the empty-state region both carry role=status — at
    // least one announces the empty state.
    expect(
      emptyStatuses.some((node) =>
        node.textContent?.includes("Ta bibliothèque est vide"),
      ),
    ).toBe(true);

    rerender(<StoryCollection stories={sampleStories} isLoading={false} />);
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
});
