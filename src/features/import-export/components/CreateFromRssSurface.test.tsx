import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CreateFromRssSurface } from "./CreateFromRssSurface";
import type { RssCreationStatus } from "../hooks/use-rss-creation";

const FEED_URL = "https://exemple.fr/flux.xml";

const EXPLOITABLE_PREVIEW = {
  sourceHost: "exemple.fr",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      itemRef: {
        kind: "guid" as const,
        guid: "g-1",
        fingerprint: "a".repeat(64),
      },
    },
    {
      title: "Episode 2",
      summary: "Deuxième texte.",
      hasEnclosure: true,
      itemRef: {
        kind: "guid" as const,
        guid: "g-2",
        fingerprint: "b".repeat(64),
      },
    },
  ],
  findings: [
    {
      aspect: "source" as const,
      category: "ambiguous" as const,
      message:
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
    },
  ],
  state: "needsReview" as const,
  blocked: false,
};

const REVIEW: RssCreationStatus = {
  kind: "review",
  feedUrl: FEED_URL,
  preview: EXPLOITABLE_PREVIEW,
  selectedItemRef: null,
  sourceChanged: false,
};

const REVIEW_SELECTED: RssCreationStatus = {
  ...REVIEW,
  kind: "review",
  selectedItemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
};

const REVIEW_BLOCKED: RssCreationStatus = {
  kind: "review",
  feedUrl: FEED_URL,
  preview: {
    sourceHost: "exemple.fr",
    items: [],
    findings: [
      {
        aspect: "envelope" as const,
        category: "blocking" as const,
        message:
          "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux.",
      },
    ],
    state: "blocked" as const,
    blocked: true,
  },
  selectedItemRef: null,
  sourceChanged: false,
};

const REVIEW_SOURCE_CHANGED: RssCreationStatus = {
  ...REVIEW,
  kind: "review",
  sourceChanged: true,
};

function noopHandlers() {
  return {
    onFetch: vi.fn(),
    onSelectItem: vi.fn(),
    onAccept: vi.fn(),
    onAbandon: vi.fn(),
    onDismiss: vi.fn(),
  };
}

describe("CreateFromRssSurface", () => {
  it("renders nothing while closed, whatever the machine state", () => {
    const { container } = render(
      <CreateFromRssSurface
        open={false}
        status={REVIEW}
        {...noopHandlers()}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the posture line, the address field and the fetch CTA when open on idle", () => {
    render(
      <CreateFromRssSurface
        open
        status={{ kind: "idle" }}
        {...noopHandlers()}
      />,
    );
    expect(
      screen.getByText(
        "Utilise uniquement des contenus dont tu as les droits : tes contenus personnels ou des contenus libres.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Récupérer le flux" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByRole("button", { name: "Abandonner" }),
    ).toBeInTheDocument();
  });

  it("fetches the typed address on Récupérer le flux", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface open status={{ kind: "idle" }} {...handlers} />,
    );
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    await user.click(screen.getByRole("button", { name: "Récupérer le flux" }));
    expect(handlers.onFetch).toHaveBeenCalledWith(FEED_URL);
  });

  it("trims the typed address on fetch (a pasted leading space must not poison the send)", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface open status={{ kind: "idle" }} {...handlers} />,
    );
    const field = screen.getByLabelText("Adresse du flux RSS");
    // Paste-like input with a leading space (userEvent.type would strip
    // nothing — the value carries the space verbatim).
    await user.click(field);
    await user.paste(` ${FEED_URL}`);
    await user.click(screen.getByRole("button", { name: "Récupérer le flux" }));
    expect(handlers.onFetch).toHaveBeenCalledWith(FEED_URL);
  });

  it("renders the fetching progress with its frozen label, not announced", () => {
    render(
      <CreateFromRssSurface
        open
        status={{ kind: "fetching" }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Récupération du flux…")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders the review with host, findings, selectable items and a disabled CTA before selection", () => {
    render(<CreateFromRssSurface open status={REVIEW} {...noopHandlers()} />);
    expect(screen.getByText("exemple.fr")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Episode 1")).toBeInTheDocument();
    expect(screen.getByText("Deuxième texte.")).toBeInTheDocument();
    // The enclosure note renders on the item that references a remote media.
    expect(screen.getByText("Média distant non récupéré")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Créer le brouillon" }),
    ).toHaveAttribute("aria-disabled", "true");
    // No alert for a calm exploitable review.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("selects an item and accepts from the enabled CTA", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    const { rerender } = render(
      <CreateFromRssSurface open status={REVIEW} {...handlers} />,
    );
    // The field carries the reviewed address (the user fetched it from
    // here — the surface keeps the typed value across the state change).
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    await user.click(screen.getByRole("button", { name: /Episode 1/ }));
    expect(handlers.onSelectItem).toHaveBeenCalledWith({
      kind: "guid",
      guid: "g-1",
      fingerprint: "a".repeat(64),
    });

    rerender(
      <CreateFromRssSurface open status={REVIEW_SELECTED} {...handlers} />,
    );
    const selected = screen.getByRole("button", { name: /Episode 1/ });
    expect(selected).toHaveAttribute("aria-pressed", "true");
    await user.click(
      screen.getByRole("button", { name: "Créer le brouillon" }),
    );
    expect(handlers.onAccept).toHaveBeenCalledTimes(1);
  });

  it("renders a blocked verdict as an alert with only Abandonner (the field stays)", () => {
    render(
      <CreateFromRssSurface open status={REVIEW_BLOCKED} {...noopHandlers()} />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux.",
    );
    expect(
      screen.queryByRole("button", { name: "Créer le brouillon" }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Abandonner" }),
    ).toBeInTheDocument();
  });

  it("renders the sourceChanged refusal as an alert with the frozen verdict and drops the stale items", () => {
    render(
      <CreateFromRssSurface
        open
        status={REVIEW_SOURCE_CHANGED}
        {...noopHandlers()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("La source a changé depuis la récupération.");
    expect(alert).toHaveTextContent("Relance la récupération du flux.");
    expect(screen.queryByText("Episode 1")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Créer le brouillon" }),
    ).not.toBeInTheDocument();
    // The field + fetch CTA stay available for the re-fetch gesture.
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();
  });

  it("renders the creating progress with the shared frozen label", () => {
    render(
      <CreateFromRssSurface
        open
        status={{ kind: "creating" }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();
  });

  it("renders the success terminal with the created title and Fermer", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface
        open
        status={{
          kind: "created",
          story: { id: "s-1", title: "Episode 1", importState: "needsReview" },
        }}
        {...handlers}
      />,
    );
    // The success chip + the polite live region carry the frozen copy.
    expect(
      screen.getAllByText("Histoire créée dans ta bibliothèque").length,
    ).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Episode 1")).toBeInTheDocument();
    // The address form is gone on a terminal.
    expect(
      screen.queryByLabelText("Adresse du flux RSS"),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(handlers.onDismiss).toHaveBeenCalledTimes(1);
  });

  it("renders a transport failure as an alert with the canonical copy and Réessayer then Fermer", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "RSS_SOURCE_UNREACHABLE",
            message:
              "Récupération du flux impossible: la source est injoignable.",
            userAction:
              "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
            details: null,
          },
        }}
        {...handlers}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Récupération du flux impossible: la source est injoignable.",
    );
    expect(alert).toHaveTextContent(
      "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
    );
    // The address field STAYS on a transport failure (the gesture is
    // "correct the address, then retry" — in-context); the form's own
    // fetch CTA yields to the alert's Réessayer.
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual([
      "Réessayer",
      "Fermer",
    ]);
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(handlers.onDismiss).toHaveBeenCalledTimes(1);
  });

  it("Réessayer after a failure fetches the CORRECTED address typed in the visible field", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "RSS_SOURCE_UNREACHABLE",
            message:
              "Récupération du flux impossible: l'adresse du flux n'est pas valide.",
            userAction: "Saisis une adresse http(s) complète puis réessaie.",
            details: null,
          },
        }}
        {...handlers}
      />,
    );
    await user.type(
      screen.getByLabelText("Adresse du flux RSS"),
      "https://exemple.fr/flux.xml",
    );
    await user.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(handlers.onFetch).toHaveBeenCalledWith(
      "https://exemple.fr/flux.xml",
    );
  });

  it("refuses the accept while the typed address diverges from the reviewed one", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface open status={REVIEW_SELECTED} {...handlers} />,
    );
    // The reviewed feedUrl is FEED_URL but the visible field is empty →
    // diverged: the accept CTA is refused even with a selection.
    const accept = screen.getByRole("button", { name: "Créer le brouillon" });
    expect(accept).toHaveAttribute("aria-disabled", "true");
    await user.click(accept);
    expect(handlers.onAccept).not.toHaveBeenCalled();

    // Typing the reviewed address back restores the CTA.
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    const restored = screen.getByRole("button", {
      name: "Créer le brouillon",
    });
    expect(restored).not.toHaveAttribute("aria-disabled");
    await user.click(restored);
    expect(handlers.onAccept).toHaveBeenCalledTimes(1);
  });

  it("forgets the typed address when the surface closes", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    const { rerender } = render(
      <CreateFromRssSurface open status={{ kind: "idle" }} {...handlers} />,
    );
    await user.type(
      screen.getByLabelText("Adresse du flux RSS"),
      "https://exemple.fr/flux-prive.xml?token=secret",
    );
    rerender(
      <CreateFromRssSurface
        open={false}
        status={{ kind: "idle" }}
        {...handlers}
      />,
    );
    rerender(
      <CreateFromRssSurface open status={{ kind: "idle" }} {...handlers} />,
    );
    expect(screen.getByLabelText("Adresse du flux RSS")).toHaveValue("");
  });

  it("keeps Abandonner reachable during the long fetching and creating states", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    const { rerender } = render(
      <CreateFromRssSurface
        open
        status={{ kind: "fetching" }}
        {...handlers}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abandonner" }));
    expect(handlers.onAbandon).toHaveBeenCalledTimes(1);

    rerender(
      <CreateFromRssSurface
        open
        status={{ kind: "creating" }}
        {...handlers}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abandonner" }));
    expect(handlers.onAbandon).toHaveBeenCalledTimes(2);
  });

  // ===== Content-source activation mention + policy refusal =====

  const UNAVAILABLE: RssCreationStatus = {
    kind: "unavailable",
    error: {
      code: "CONTENT_SOURCE_UNAVAILABLE",
      message:
        "Cette source de contenu n'est pas activée dans la distribution officielle.",
      userAction:
        "Utilise une source activée ou consulte le profil de support de ta version.",
      details: { source: "content_source_policy", kind: "rss" },
    },
  };

  it("renders the frozen activation mention from the opening, next to the posture line (both visible)", () => {
    render(
      <CreateFromRssSurface
        open
        status={{ kind: "idle" }}
        {...noopHandlers()}
      />,
    );
    // The mention and the posture COEXIST as distinct lines — VERBATIM.
    expect(
      screen.getByText("Source activée par la distribution officielle."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Utilise uniquement des contenus dont tu as les droits : tes contenus personnels ou des contenus libres.",
      ),
    ).toBeInTheDocument();
  });

  it("keeps the activation mention through review, failed and created (surface-level, not state-level)", () => {
    const { rerender } = render(
      <CreateFromRssSurface open status={REVIEW} {...noopHandlers()} />,
    );
    expect(
      screen.getByText("Source activée par la distribution officielle."),
    ).toBeInTheDocument();
    rerender(
      <CreateFromRssSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "RSS_SOURCE_UNREACHABLE",
            message:
              "Récupération du flux impossible: la source est injoignable.",
            userAction:
              "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
            details: null,
          },
        }}
        {...noopHandlers()}
      />,
    );
    expect(
      screen.getByText("Source activée par la distribution officielle."),
    ).toBeInTheDocument();
    // The success terminal drops the address form but keeps the mention
    // (a surface-level line, not a form-level one).
    rerender(
      <CreateFromRssSurface
        open
        status={{
          kind: "created",
          story: { id: "s-1", title: "Episode 1" },
        }}
        {...noopHandlers()}
      />,
    );
    expect(
      screen.getByText("Source activée par la distribution officielle."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Adresse du flux RSS")).not.toBeInTheDocument();
  });

  it("renders the policy refusal as a CALM status region with the frozen copy and NO retry", async () => {
    const user = userEvent.setup();
    const handlers = noopHandlers();
    render(<CreateFromRssSurface open status={UNAVAILABLE} {...handlers} />);
    // A calm region — role="status", never an alert (a distribution
    // decision is not a breakage).
    const region = screen.getByRole("status");
    expect(region).toHaveTextContent(
      "Cette source de contenu n'est pas activée dans la distribution officielle.",
    );
    expect(region).toHaveTextContent(
      "Utilise une source activée ou consulte le profil de support de ta version.",
    );
    // NO retry gesture (a retry cannot change the policy), no address
    // field, no fetch CTA — the way out is Abandonner.
    expect(
      screen.queryByRole("button", { name: "Réessayer" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Adresse du flux RSS")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Récupérer le flux" }),
    ).not.toBeInTheDocument();
    // The activation mention would contradict the refusal: not rendered.
    expect(
      screen.queryByText("Source activée par la distribution officielle."),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Abandonner" }));
    expect(handlers.onAbandon).toHaveBeenCalledTimes(1);
  });

  it("announces the policy refusal through the persistent live region (mounted BEFORE the transition)", () => {
    const { container, rerender } = render(
      <CreateFromRssSurface open status={REVIEW} {...noopHandlers()} />,
    );
    // The persistent polite region exists BEFORE the transition (a live
    // region inserted already filled is not reliably announced — only
    // changes of an existing one are), and is empty during review.
    const live = container.querySelector('[aria-live="polite"][aria-atomic="true"]');
    expect(live).not.toBeNull();
    expect(live).toHaveTextContent("");
    rerender(
      <CreateFromRssSurface open status={UNAVAILABLE} {...noopHandlers()} />,
    );
    expect(
      container.querySelector('[aria-live="polite"][aria-atomic="true"]'),
    ).toHaveTextContent(
      "Cette source de contenu n'est pas activée dans la distribution officielle.",
    );
  });

  it("never renders the policy refusal as an alert (distinct from the transport failed state)", () => {
    render(
      <CreateFromRssSurface open status={UNAVAILABLE} {...noopHandlers()} />,
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("mounts a polite atomic live region, empty until a terminal announcement", () => {
    const { container, rerender } = render(
      <CreateFromRssSurface open status={REVIEW} {...noopHandlers()} />,
    );
    const live = container.querySelector('[aria-live="polite"]');
    expect(live).not.toBeNull();
    expect(live).toHaveAttribute("aria-atomic", "true");
    expect(live).toHaveTextContent("");
    rerender(
      <CreateFromRssSurface
        open
        status={{
          kind: "created",
          story: { id: "s-1", title: "Episode 1" },
        }}
        {...noopHandlers()}
      />,
    );
    expect(
      container.querySelector('[aria-live="polite"][aria-atomic="true"]'),
    ).toHaveTextContent("Histoire créée dans ta bibliothèque");
  });
});
