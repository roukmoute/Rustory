import {
  Navigate,
  type RouteObject,
  createBrowserRouter,
  createMemoryRouter,
} from "react-router-dom";

import { LibraryRoute } from "../routes/library/LibraryRoute";
import { StoryEditRoute } from "../routes/story-edit/StoryEditRoute";
import { AppShell } from "./AppShell";

/**
 * Canonical route table for the Rustory shell. Exported so tests can build
 * a MemoryRouter against the exact same tree the browser mounts — no drift
 * between production and test configurations.
 */
export const appRoutes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/library" replace /> },
      { path: "library", element: <LibraryRoute /> },
      { path: "story/:storyId/edit", element: <StoryEditRoute /> },
      { path: "*", element: <Navigate to="/library" replace /> },
    ],
  },
];

export function createAppRouter(initialEntries?: string[]) {
  if (initialEntries) {
    return createMemoryRouter(appRoutes, { initialEntries });
  }
  return createBrowserRouter(appRoutes);
}

export const router = createAppRouter();
