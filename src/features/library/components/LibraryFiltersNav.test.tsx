import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { LibraryFiltersNav } from "./LibraryFiltersNav";

describe("<LibraryFiltersNav />", () => {
  it("uses distinct route links and keeps catalogue access available", async () => {
    const user = userEvent.setup();
    const onCatalog = vi.fn();
    render(
      <MemoryRouter initialEntries={["/library"]}>
        <LibraryFiltersNav onCatalog={onCatalog} />
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
    await user.click(
      screen.getByRole("button", { name: "Catalogue officiel" }),
    );
    expect(onCatalog).toHaveBeenCalledOnce();
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
