import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LibraryFiltersNav } from "./LibraryFiltersNav";

describe("<LibraryFiltersNav />", () => {
  it("renders a Filtres heading and the three filter entries", () => {
    render(<LibraryFiltersNav />);
    expect(
      screen.getByRole("heading", { name: /filtres/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /toutes les histoires/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /brouillons locaux/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /histoires transférées/i }),
    ).toBeInTheDocument();
  });

  it("marks every filter disabled with the same canonical reason accessible from the keyboard", () => {
    render(<LibraryFiltersNav />);
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(3);

    for (const btn of buttons) {
      expect(btn).not.toBeDisabled();
      expect(btn).toHaveAttribute("aria-disabled", "true");
      const reasonId = btn.getAttribute("aria-describedby");
      expect(reasonId).toBeTruthy();
      const reason = document.getElementById(reasonId as string);
      expect(reason).toHaveTextContent(/filtres avancés à venir/i);
    }
  });

  it("does not combine aria-disabled with aria-pressed (ambiguous for assistive tech)", () => {
    render(<LibraryFiltersNav />);
    for (const btn of screen.getAllByRole("button")) {
      expect(btn).not.toHaveAttribute("aria-pressed");
    }
  });
});
