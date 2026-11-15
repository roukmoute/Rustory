import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";
import { LibraryFiltersNav } from "./LibraryFiltersNav";

describe("<LibraryFiltersNav />", () => {
  it("uses three distinct route links", async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter initialEntries={["/library"]}>
        <LibraryFiltersNav />
      </MemoryRouter>,
    );
    const library = screen.getByRole("link", { name: "Ma bibliothèque" });
    const device = screen.getByRole("link", { name: "Sur l'appareil" });
    expect(library).toHaveAttribute("href", "/library");
    expect(device).toHaveAttribute("href", "/device");
    expect(library).toHaveAttribute("aria-current", "page");
    await user.click(device);
    expect(device).toHaveAttribute("aria-current", "page");
    expect(library).not.toHaveAttribute("aria-current");
    const drafts = screen.getByRole("link", { name: "Mes brouillons" });
    expect(drafts).toHaveAttribute("href", "/drafts");
    await user.click(drafts);
    expect(drafts).toHaveAttribute("aria-current", "page");
    expect(device).not.toHaveAttribute("aria-current");
  });
  it("derives the active screen from the initial URL", () => {
    render(
      <MemoryRouter initialEntries={["/device"]}>
        <LibraryFiltersNav />
      </MemoryRouter>,
    );
    expect(
      screen.getByRole("link", { name: "Sur l'appareil" }),
    ).toHaveAttribute("aria-current", "page");
    expect(
      screen.getByRole("link", { name: "Ma bibliothèque" }),
    ).not.toHaveAttribute("aria-current");
  });
});
