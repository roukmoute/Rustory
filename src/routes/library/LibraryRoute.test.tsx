import { StrictMode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../ipc/commands/library", () => ({
  getLibraryOverview: vi.fn(),
}));

import { getLibraryOverview } from "../../ipc/commands/library";
import { LibraryRoute } from "./LibraryRoute";

const mockGet = vi.mocked(getLibraryOverview);

describe("<LibraryRoute />", () => {
  beforeEach(() => {
    mockGet.mockReset();
  });

  it("shows the loading state before the IPC call resolves", async () => {
    // Never resolve: forces the initial synchronous render.
    mockGet.mockImplementation(() => new Promise(() => {}));

    render(<LibraryRoute />);

    const section = screen.getByText(/chargement de la bibliothèque/i)
      .closest("section");
    expect(section).toHaveAttribute("aria-busy", "true");
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an actionable empty state with a keyboard-reachable disabled CTA", async () => {
    mockGet.mockResolvedValueOnce({ stories: [] });

    render(<LibraryRoute />);

    expect(
      await screen.findByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();

    const primary = screen.getByRole("button", { name: /créer une histoire/i });
    // The button stays focusable (no `disabled` attr) so keyboard users can
    // reach it and read the inline reason.
    expect(primary).not.toBeDisabled();
    expect(primary).toHaveAttribute("aria-disabled", "true");

    const describedBy = primary.getAttribute("aria-describedby");
    expect(describedBy).toBeTruthy();
    const reason = document.getElementById(describedBy as string);
    expect(reason).toHaveTextContent(/création d'histoire indisponible/i);
    // No internal jargon leaks in user-facing copy.
    expect(reason?.textContent).not.toMatch(/story\s*1/i);
  });

  it("shows a localized error and a Réessayer button when storage init fails", async () => {
    mockGet.mockRejectedValueOnce({
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Le stockage local est inaccessible.",
      userAction: "Vérifie les permissions puis relance.",
      details: null,
    });

    render(<LibraryRoute />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Le stockage local est inaccessible.");
    expect(alert).toHaveTextContent("Vérifie les permissions puis relance.");

    // Empty state MUST NOT appear when the call failed — AC3 guardrail.
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();

    expect(screen.getByRole("button", { name: /réessayer/i })).toBeEnabled();
  });

  it("wraps non-AppError rejections as UNKNOWN instead of fabricating a storage failure", async () => {
    mockGet.mockRejectedValueOnce(new Error("kaboom"));

    render(<LibraryRoute />);

    const alert = await screen.findByRole("alert");
    // Must stay generic: never lie about which subsystem failed.
    expect(alert).toHaveTextContent(/une erreur inattendue/i);
    expect(alert).not.toHaveTextContent(/stockage local/i);
  });

  it("rejects a malformed overview payload instead of rendering it", async () => {
    // Simulate an IPC drift: the Rust side returns something that does not
    // match `LibraryOverviewDto`.
    mockGet.mockResolvedValueOnce({ unexpected: true } as never);

    render(<LibraryRoute />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/réponse inattendue/i);
    expect(
      screen.queryByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).not.toBeInTheDocument();
  });

  it("retries the IPC call when Réessayer is pressed and recovers on success", async () => {
    const user = userEvent.setup();
    mockGet
      .mockRejectedValueOnce({
        code: "LOCAL_STORAGE_UNAVAILABLE",
        message: "Le stockage local est inaccessible.",
        userAction: "Vérifie les permissions puis relance.",
        details: null,
      })
      .mockResolvedValueOnce({ stories: [] });

    render(<LibraryRoute />);

    await user.click(await screen.findByRole("button", { name: /réessayer/i }));

    await waitFor(() =>
      expect(
        screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
      ).toBeInTheDocument(),
    );

    expect(mockGet).toHaveBeenCalledTimes(2);
  });

  it("ignores a late response from a superseded IPC call (StrictMode race)", async () => {
    // In StrictMode the effect fires twice; if a late response from the
    // first (now-superseded) call landed in state, we would flash a ghost
    // result. The route must pin to the latest call only.
    let resolveFirst!: (v: { stories: unknown[] }) => void;
    mockGet
      .mockImplementationOnce(
        () => new Promise((res) => (resolveFirst = res as never)),
      )
      .mockResolvedValueOnce({ stories: [] });

    render(
      <StrictMode>
        <LibraryRoute />
      </StrictMode>,
    );

    // Second (StrictMode-induced) call must have landed the empty state.
    expect(
      await screen.findByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();

    // Now the first call resolves LATE with a ghost result — the guard
    // must drop it silently.
    resolveFirst({ stories: [{ id: "GHOST", title: "Fantôme" }] });

    // Give React a microtask turn; the empty state must still be visible.
    await waitFor(() =>
      expect(screen.queryByText(/fantôme/i)).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("heading", { name: /ta bibliothèque est vide/i }),
    ).toBeInTheDocument();
  });

  it("surfaces a timeout-shaped error from the IPC facade as UNKNOWN", async () => {
    mockGet.mockRejectedValueOnce({
      code: "UNKNOWN",
      message: "Rustory a mis trop de temps à charger la bibliothèque.",
      userAction: "Relance l'application.",
      details: null,
    });

    render(<LibraryRoute />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/mis trop de temps/i);
  });
});
