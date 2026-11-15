import {
  Navigate,
  type RouteObject,
  createBrowserRouter,
  createMemoryRouter,
} from "react-router-dom";

import { LibraryRoute } from "../routes/library/LibraryRoute";
import { CollectionScreenRoute } from "../routes/library/CollectionScreenRoute";
import { SettingsRoute } from "../routes/settings/SettingsRoute";
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
      {
        // Keep device jobs and collection state alive between these screens.
        element: <LibraryRoute />,
        children: [
          {
            path: "library",
            element: <CollectionScreenRoute screen="library" />,
          },
          {
            path: "device",
            element: <CollectionScreenRoute screen="device" />,
          },
        ],
      },
      { path: "settings", element: <SettingsRoute /> },
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
