import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LibraryFiltersNav } from "./LibraryFiltersNav";

describe("<LibraryFiltersNav />", () => {
  it("navigates to the distinct local, device and catalogue contexts", async () => {
    const user = userEvent.setup();
    const onLibrary = vi.fn(), onDevice = vi.fn(), onCatalog = vi.fn();
    render(<LibraryFiltersNav onLibrary={onLibrary} onDevice={onDevice} onCatalog={onCatalog} />);
    await user.click(screen.getByRole("button", { name: "Ma bibliothèque" }));
    await user.click(screen.getByRole("button", { name: "Sur l'appareil" }));
    await user.click(screen.getByRole("button", { name: "Catalogue officiel" }));
    expect(onLibrary).toHaveBeenCalledOnce();
    expect(onDevice).toHaveBeenCalledOnce();
    expect(onCatalog).toHaveBeenCalledOnce();
  });
  it("identifies only the current collection", () => {
    render(<LibraryFiltersNav activeSection="device" />);
    expect(screen.getByRole("button", { name: "Sur l'appareil" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Ma bibliothèque" })).not.toHaveAttribute("aria-current");
  });
});
