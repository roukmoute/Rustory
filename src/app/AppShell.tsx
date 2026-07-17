import type React from "react";
import { Outlet } from "react-router-dom";

import { DropOverlay } from "../features/import-export/components/DropOverlay";

import "./AppShell.css";

/**
 * Root application shell. Hosts routed contexts through `<Outlet />` — each
 * route owns its own layout (library uses the 3-column grid, the edit route
 * uses a single-column reading surface). The drop hover overlay is
 * APP-LEVEL by contract (`Drop Intent Contract`): the whole window is the
 * drop target, so its decorative feedback lives above the routed outlet,
 * owned by no route.
 */
export function AppShell(): React.JSX.Element {
  return (
    <div className="app-shell">
      <Outlet />
      <DropOverlay />
    </div>
  );
}
