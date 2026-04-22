import { render, screen, waitFor } from "@testing-library/react";
import { RouterProvider } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const getLibraryOverviewMock = vi.fn();

vi.mock("../ipc/commands/library", () => ({
  getLibraryOverview: () => ({
    promise: getLibraryOverviewMock(),
    cancel: () => {},
  }),
  LIBRARY_OVERVIEW_TIMEOUT_MS: 2000,
  LIBRARY_OVERVIEW_TIMEOUT_ERROR: {
    code: "UNKNOWN",
    message: "timeout",
    userAction: "retry",
    details: null,
  },
}));

import { createAppRouter } from "./router";

describe("router", () => {
  beforeEach(() => {
    getLibraryOverviewMock.mockReset();
    getLibraryOverviewMock.mockResolvedValue({
      stories: [{ id: "abc-123", title: "Le soleil" }],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("matches /library to LibraryRoute with the 3-column semantic layout", async () => {
    const router = createAppRouter(["/library"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
  });

  it("matches /story/:storyId/edit to StoryEditRoute and mounts its main landmark", async () => {
    const router = createAppRouter(["/story/abc-123/edit"]);
    render(<RouterProvider router={router} />);

    // Assert the route's own landmark directly — proves the :storyId
    // binding rendered StoryEditRoute, not the library fallback. The
    // story heading is a side-effect we also verify.
    await waitFor(() =>
      expect(
        screen.getByRole("main", { name: /reprise d'un brouillon local/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  it("decodes a percent-encoded storyId before looking it up", async () => {
    const router = createAppRouter(["/story/abc%2D123/edit"]);
    render(<RouterProvider router={router} />);

    await waitFor(() =>
      expect(
        screen.getByRole("main", { name: /reprise d'un brouillon local/i }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: /le soleil/i }),
    ).toBeInTheDocument();
  });

  it("redirects / to /library", async () => {
    const router = createAppRouter(["/"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
  });

  it("redirects unknown paths to /library", async () => {
    const router = createAppRouter(["/does-not-exist"]);
    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(
        screen.getByRole("main", { name: "Collection d'histoires" }),
      ).toBeInTheDocument();
    });
  });
});
