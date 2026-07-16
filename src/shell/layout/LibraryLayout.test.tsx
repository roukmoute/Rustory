import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LibraryLayout } from "./LibraryLayout";

describe("<LibraryLayout />", () => {
  it("renders the three slots inside semantic regions (nav/main/aside)", () => {
    render(
      <LibraryLayout
        leftNav={<p>nav-content</p>}
        center={<p>center-content</p>}
        rightPanel={<p>panel-content</p>}
      />,
    );

    // Re-scoped with the support-profile screen: the left column now
    // carries filters AND navigation entries, so its accessible name is
    // the generalized `Navigation bibliothèque` (Library Layout Contract).
    const nav = screen.getByRole("navigation", {
      name: /navigation bibliothèque/i,
    });
    const main = screen.getByRole("main", { name: /collection d'histoires/i });
    const panel = screen.getByRole("complementary", {
      name: /panneau de décision/i,
    });

    expect(nav).toHaveTextContent("nav-content");
    expect(main).toHaveTextContent("center-content");
    expect(panel).toHaveTextContent("panel-content");
  });

  it("does not add extra wrapper roles between the grid and its slots", () => {
    render(
      <LibraryLayout
        leftNav={<span>n</span>}
        center={<span>c</span>}
        rightPanel={<span>p</span>}
      />,
    );
    // Only three regions should be directly announced — no duplicate
    // landmarks leaking from the layout container.
    expect(screen.getAllByRole("navigation")).toHaveLength(1);
    expect(screen.getAllByRole("main")).toHaveLength(1);
    expect(screen.getAllByRole("complementary")).toHaveLength(1);
  });
});
