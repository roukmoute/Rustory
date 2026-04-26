import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AppError } from "../../../shared/errors/app-error";
import type { RecoverableDraft } from "../../../shared/ipc-contracts/story";

import { RecoveryBanner } from "./RecoveryBanner";

const DRAFT: Extract<RecoverableDraft, { kind: "recoverable" }> = {
  kind: "recoverable",
  storyId: "sid",
  draftTitle: "Buffered live",
  draftAt: "2026-04-25T12:00:00.000Z",
  persistedTitle: "Persisted save",
};

describe("RecoveryBanner", () => {
  it("renders the persisted and draft titles in distinct strong elements", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    // Use exact text rather than the previous brittle `/"/`-against-
    // `selector: "strong"` query: that pattern matched any strong
    // element containing a quote, and the escape pipeline now
    // surfaces literal escape sequences (e.g. `‮`) which would
    // shift the regex semantics. The exact-text query stays anchored
    // on the user-visible string and stays robust.
    expect(screen.getByText('"Buffered live"')).toBeInTheDocument();
    expect(screen.getByText('"Persisted save"')).toBeInTheDocument();
  });

  it("renders the section with role=region and aria-label='Brouillon récupéré'", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    const region = screen.getByRole("region", { name: "Brouillon récupéré" });
    expect(region).toBeInTheDocument();
  });

  it("renders both actions accessible by keyboard with focus order primary → secondary", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    const primary = screen.getByRole("button", {
      name: "Restaurer le brouillon",
    });
    const secondary = screen.getByRole("button", {
      name: "Conserver l'état enregistré",
    });
    expect(primary).toBeInTheDocument();
    expect(secondary).toBeInTheDocument();
    // Primary appears before secondary in tab order.
    const buttons = screen.getAllByRole("button");
    expect(buttons.indexOf(primary)).toBeLessThan(buttons.indexOf(secondary));
  });

  it("clicking Restaurer le brouillon calls onApply", async () => {
    const onApply = vi.fn();
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={onApply}
        onDiscard={vi.fn()}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Restaurer le brouillon" }),
    );
    expect(onApply).toHaveBeenCalledTimes(1);
  });

  it("clicking Conserver l'état enregistré calls onDiscard", async () => {
    const onDiscard = vi.fn();
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={onDiscard}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Conserver l'état enregistré" }),
    );
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("applying state disables both action buttons and adds aria-busy", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent="apply"
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    const primary = screen.getByRole("button", {
      name: "Restauration en cours…",
    });
    expect(primary).toBeDisabled();
    expect(primary).toHaveAttribute("aria-busy", "true");
    const secondary = screen.getByRole("button", {
      name: "Conserver l'état enregistré",
    });
    expect(secondary).toBeDisabled();
    const region = screen.getByRole("region", { name: "Brouillon récupéré" });
    expect(region).toHaveAttribute("aria-busy", "true");
  });

  it("applying state changes the primary label to Restauration en cours…", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent="apply"
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Restauration en cours…" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Restaurer le brouillon" }),
    ).not.toBeInTheDocument();
  });

  it('error state renders role="alert" with message and userAction', () => {
    const error: AppError = {
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "Récupération indisponible.",
      userAction: "Vérifie le disque local.",
      details: null,
    };
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        error={error}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
        onRetry={vi.fn()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Récupération indisponible.");
    expect(alert).toHaveTextContent("Vérifie le disque local.");
  });

  it("error state renders Réessayer la récupération button calling onRetry", async () => {
    const onRetry = vi.fn();
    const error: AppError = {
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "Boom",
      userAction: "Vérifie",
      details: null,
    };
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        error={error}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
        onRetry={onRetry}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Réessayer la récupération" }),
    );
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("formats draftAt with the relative-time helper", () => {
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    // The helper resolves to one of the predicted forms — find via
    // partial match so the test stays stable across clock jitter.
    expect(screen.getByText(/Brouillon enregistré /)).toBeInTheDocument();
  });

  it("renders an empty draft title with a (vide) fallback distinguishable from a missing line", () => {
    render(
      <RecoveryBanner
        draft={{ ...DRAFT, draftTitle: "" }}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    // An empty string used to render as `""` and was indistinguishable
    // from a missing field. The fallback now renders `(vide)` in
    // italic so the user sees they had erased everything.
    expect(screen.getByText("(vide)")).toBeInTheDocument();
  });

  it("renders a whitespace-only draft title with a distinct (espaces) fallback", () => {
    render(
      <RecoveryBanner
        draft={{ ...DRAFT, draftTitle: "   " }}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    expect(screen.getByText("(espaces)")).toBeInTheDocument();
  });

  it("escapes BiDi override U+202E in the rendered diff (anti-spoof)", () => {
    render(
      <RecoveryBanner
        draft={{ ...DRAFT, draftTitle: "abc‮def" }}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    // Raw U+202E would let the title visually flip. The escape
    // surfaces it as a visible ‮ sequence inside the quoted
    // strong element.
    expect(screen.getByText('"abc\\u202Edef"')).toBeInTheDocument();
  });

  it("escapes line breaks so a newline in the draft does not split visual layout", () => {
    render(
      <RecoveryBanner
        draft={{ ...DRAFT, draftTitle: "line1\nline2" }}
        applyingIntent={null}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    expect(screen.getByText('"line1\\nline2"')).toBeInTheDocument();
  });

  it("rephrases an INVALID_STORY_TITLE error from `Création impossible` to `Restauration impossible`", () => {
    const error: AppError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre contient des caractères non autorisés",
      userAction: "Supprime les sauts de ligne, tabulations et caractères invisibles.",
      details: { source: "recovery_draft_invalid" },
    };
    render(
      <RecoveryBanner
        draft={DRAFT}
        applyingIntent={null}
        error={error}
        onApply={vi.fn()}
        onDiscard={vi.fn()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Restauration impossible: titre contient des caractères non autorisés",
    );
    // The original Création prefix must NOT leak into the recovery surface.
    expect(alert).not.toHaveTextContent(/Création impossible/);
  });
});
