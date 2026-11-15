import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LibraryLayout } from "./LibraryLayout";

describe("<LibraryLayout />", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("renders navigation and collection as two landmarks without an aside", () => {
    render(
      <LibraryLayout
        leftNav={<p>nav-content</p>}
        center={<p>center-content</p>}
      />,
    );

    const nav = screen.getByRole("navigation", {
      name: "Navigation bibliothèque",
    });
    const main = screen.getByRole("main", { name: "Collection d'histoires" });
    expect(nav).toHaveTextContent("nav-content");
    expect(main).toHaveTextContent("center-content");
    expect(screen.getAllByRole("navigation")).toHaveLength(1);
    expect(screen.getAllByRole("main")).toHaveLength(1);
    expect(
      screen.queryByRole("complementary", { hidden: true }),
    ).not.toBeInTheDocument();
    expect(document.querySelector("aside")).toBeNull();
  });

  it("keeps floating actions inside the named collection and keyboard accessible", async () => {
    const user = userEvent.setup();
    const send = vi.fn();
    render(
      <LibraryLayout
        leftNav={<p>Navigation</p>}
        mainLabel="Histoires sur l'appareil"
        center={<p>Histoires</p>}
        bottomBar={<button onClick={send}>Copier la sélection</button>}
      />,
    );

    const main = screen.getByRole("main", { name: "Histoires sur l'appareil" });
    const action = within(main).getByRole("button", {
      name: "Copier la sélection",
    });
    expect(
      within(screen.getByRole("navigation")).queryByRole("button"),
    ).toBeNull();
    expect(screen.queryByRole("complementary", { hidden: true })).toBeNull();
    await user.tab();
    expect(action).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("preserves the collection when its selection bar appears and disappears", async () => {
    const user = userEvent.setup();
    const center = <input aria-label="Recherche" defaultValue="" />;
    const { rerender } = render(<LibraryLayout leftNav={null} center={center} />);
    const search = screen.getByRole("textbox", { name: "Recherche" });
    await user.type(search, "soleil");
    rerender(
      <LibraryLayout
        leftNav={null}
        center={center}
        bottomBar={<button>Envoyer</button>}
      />,
    );
    expect(screen.getByRole("textbox")).toBe(search);
    expect(search).toHaveValue("soleil");
    rerender(<LibraryLayout leftNav={null} center={center} />);
    expect(screen.queryByRole("button", { name: "Envoyer" })).toBeNull();
    expect(screen.getByRole("textbox")).toBe(search);
    expect(search).toHaveValue("soleil");
  });

  it("updates reserved content space when the floating bar wraps and releases it on removal", () => {
    let resized!: ResizeObserverCallback;
    let height = 64;
    const disconnect = vi.fn();
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { resized = callback; }
      observe = vi.fn();
      disconnect = disconnect;
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ height }) as DOMRect,
    );
    const { rerender } = render(
      <LibraryLayout
        leftNav={null}
        center={<p>Histoires</p>}
        bottomBar={<button>Envoyer</button>}
      />,
    );
    const main = screen.getByRole("main");
    expect(main.style.getPropertyValue("--library-bottom-bar-height")).toBe("64px");
    height = 132;
    act(() => resized([], {} as ResizeObserver));
    expect(main.style.getPropertyValue("--library-bottom-bar-height")).toBe("132px");
    rerender(<LibraryLayout leftNav={null} center={<p>Histoires</p>} />);
    expect(main.style.getPropertyValue("--library-bottom-bar-height")).toBe("");
    expect(disconnect).toHaveBeenCalledTimes(1);
  });
});
