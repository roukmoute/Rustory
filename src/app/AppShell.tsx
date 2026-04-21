import type React from "react";

import { LibraryRoute } from "../routes/library/LibraryRoute";
import "./AppShell.css";

/**
 * Root application shell — a single desktop-only layout hosting the library
 * route. React Router and shell stores will layer on top when needed.
 */
export function AppShell(): React.JSX.Element {
  return (
    <div className="app-shell">
      <main className="app-shell__main">
        <LibraryRoute />
      </main>
    </div>
  );
}
