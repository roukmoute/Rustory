import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CatalogPanel } from "./CatalogPanel";
import type { UseOfficialCatalog } from "../hooks/use-official-catalog";

function makeCatalog(
  overrides: Partial<UseOfficialCatalog> = {},
): UseOfficialCatalog {
  return {
    state: { kind: "ready", count: 0 },
    action: "idle",
    actionError: null,
    refresh: vi.fn().mockResolvedValue(undefined),
    importFile: vi.fn().mockResolvedValue(undefined),
    dismissError: vi.fn(),
    ...overrides,
  };
}

describe("<CatalogPanel />", () => {
  it("states the offline-first guardrail explicitly", () => {
    render(<CatalogPanel catalog={makeCatalog()} />);
    expect(
      screen.getByText(/ne contacte aucun serveur sans une action de ta part/i),
    ).toBeInTheDocument();
  });

  it("shows the cached count and pluralizes it", () => {
    render(
      <CatalogPanel catalog={makeCatalog({ state: { kind: "ready", count: 1200 } })} />,
    );
    expect(screen.getByText(/1200 titres officiels en cache/i)).toBeInTheDocument();
  });

  it("shows the empty-cache state distinctly", () => {
    render(<CatalogPanel catalog={makeCatalog({ state: { kind: "ready", count: 0 } })} />);
    expect(screen.getByText(/aucun titre officiel en cache/i)).toBeInTheDocument();
  });

  it("triggers the network refresh on click", async () => {
    const user = userEvent.setup();
    const refresh = vi.fn().mockResolvedValue(undefined);
    render(<CatalogPanel catalog={makeCatalog({ refresh })} />);
    await user.click(
      screen.getByRole("button", { name: /récupérer \/ mettre à jour/i }),
    );
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("triggers the offline file import on click", async () => {
    const user = userEvent.setup();
    const importFile = vi.fn().mockResolvedValue(undefined);
    render(<CatalogPanel catalog={makeCatalog({ importFile })} />);
    await user.click(
      screen.getByRole("button", { name: /importer depuis un fichier/i }),
    );
    expect(importFile).toHaveBeenCalledTimes(1);
  });

  it("soft-disables both actions and swallows clicks while refreshing", async () => {
    const user = userEvent.setup();
    const refresh = vi.fn().mockResolvedValue(undefined);
    const importFile = vi.fn().mockResolvedValue(undefined);
    render(
      <CatalogPanel
        catalog={makeCatalog({ action: "refreshing", refresh, importFile })}
      />,
    );
    const refreshButton = screen.getByRole("button", {
      name: /récupérer \/ mettre à jour/i,
    });
    expect(refreshButton).toHaveAttribute("aria-disabled", "true");
    expect(refreshButton).toHaveAttribute("aria-busy", "true");
    await user.click(refreshButton);
    await user.click(
      screen.getByRole("button", { name: /importer depuis un fichier/i }),
    );
    expect(refresh).not.toHaveBeenCalled();
    expect(importFile).not.toHaveBeenCalled();
  });

  it("surfaces an action error in-context (alert) with an explicit dismiss", async () => {
    const user = userEvent.setup();
    const dismissError = vi.fn();
    render(
      <CatalogPanel
        catalog={makeCatalog({
          dismissError,
          actionError: {
            code: "OFFICIAL_CATALOG_UNAVAILABLE",
            message: "Récupération du catalogue officiel impossible: le service est injoignable.",
            userAction: "Vérifie ta connexion puis réessaie.",
            details: null,
          },
        })}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/service est injoignable/i);
    expect(alert).toHaveTextContent(/vérifie ta connexion/i);
    await user.click(within(alert).getByRole("button", { name: /fermer/i }));
    expect(dismissError).toHaveBeenCalledTimes(1);
  });
});
