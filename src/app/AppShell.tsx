import type React from "react";
import { Outlet } from "react-router-dom";

import "./AppShell.css";

/**
 * Root application shell. Hosts routed contexts through `<Outlet />` — each
 * route owns its own layout (library uses the 3-column grid, the edit route
 * uses a single-column reading surface).
 */
export function AppShell(): React.JSX.Element {
  return (
    <div className="app-shell">
      <Outlet />
    </div>
  );
}
