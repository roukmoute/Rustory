import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StoryCard } from "./StoryCard";

describe("<StoryCard />", () => {
  it("renders the title as the primary content and a truncated id as metadata", () => {
    render(
      <StoryCard story={{ id: "abcd1234-efgh", title: "Le soleil d'Éloi" }} />,
    );
    expect(
      screen.getByRole("heading", { name: /le soleil d'éloi/i, level: 3 }),
    ).toBeInTheDocument();
    expect(screen.getByText("abcd1234")).toBeInTheDocument();
  });

  it("does not expose any interactive button — selection/edit belong to a future iteration", () => {
    render(
      <StoryCard story={{ id: "id-1", title: "Titre" }} />,
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("exposes a keyboard-reachable group so Tab can reach the card from the library controls", () => {
    render(<StoryCard story={{ id: "id-1", title: "Le soleil" }} />);
    const group = screen.getByRole("group", { name: /le soleil/i });
    expect(group).toHaveAttribute("tabindex", "0");
  });
});
