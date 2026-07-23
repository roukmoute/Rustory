import type React from "react";
import { useEffect } from "react";
import { Outlet } from "react-router-dom";

import { DropOverlay } from "../features/import-export/components/DropOverlay";

import "./AppShell.css";

/** True for an element where the OS/browser context menu is USEFUL (text
 *  editing: copy / paste / select). Everywhere else the default menu is the
 *  useless browser one, suppressed so it can be replaced by app menus. */
function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  return el.closest("input, textarea, [contenteditable=''], [contenteditable='true']") !== null;
}

/**
 * Root application shell. Hosts routed contexts through `<Outlet />` — each
 * route owns its own layout (library uses the 3-column grid, the edit route
 * uses a single-column reading surface). The drop hover overlay is
 * APP-LEVEL by contract (`Drop Intent Contract`): the whole window is the
 * drop target, so its decorative feedback lives above the routed outlet,
 * owned by no route.
 */
export function AppShell(): React.JSX.Element {
  // Suppress the default webview context menu everywhere EXCEPT text fields
  // (where copy/paste is genuinely useful). Surfaces that want a real menu
  // (a library card) render their own <ContextMenu> and preventDefault
  // first; this only removes the bare browser menu elsewhere.
  useEffect(() => {
    const onContextMenu = (event: MouseEvent): void => {
      if (!isEditableTarget(event.target)) event.preventDefault();
    };
    document.addEventListener("contextmenu", onContextMenu);
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, []);

  return (
    <div className="app-shell">
      <Outlet />
      <DropOverlay />
    </div>
  );
}
