import type React from "react";

import { LibraryRoute } from "../routes/library/LibraryRoute";
import "./AppShell.css";

/**
 * Root application shell. `LibraryRoute` owns its own layout (three-column
 * grid with semantic nav/main/aside regions), so this container stays
 * minimal and future routes will plug in here.
 */
export function AppShell(): React.JSX.Element {
  return (
    <div className="app-shell">
      <LibraryRoute />
    </div>
  );
}
