import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CreateFromRssSurface } from "./CreateFromRssSurface";
import type { RssCreationStatus } from "../hooks/use-rss-creation";

const FEED_URL = "https://exemple.fr/flux.xml";

const EXPLOITABLE_PREVIEW = {
  sourceHost: "exemple.fr",
  channelTitle: "Mon flux",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      hasImage: false,
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
      hasImage: true,
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

const KEY_1 = JSON.stringify(["guid", "g-1"]);
const KEY_2 = JSON.stringify(["guid", "g-2"]);

/** The nominal review: every item ticked (the whole podcast is the story). */
const REVIEW: RssCreationStatus = {
  kind: "review",
  feedUrl: FEED_URL,
  preview: EXPLOITABLE_PREVIEW,
  selectedKeys: new Set([KEY_1, KEY_2]),
  sourceChanged: false,
};

const REVIEW_NONE_SELECTED: RssCreationStatus = {
  ...REVIEW,
  kind: "review",
  selectedKeys: new Set(),
};

const REVIEW_ONE_SELECTED: RssCreationStatus = {
  ...REVIEW,
  kind: "review",
  selectedKeys: new Set([KEY_1]),
};

const REVIEW_BLOCKED: RssCreationStatus = {
  kind: "review",
  feedUrl: FEED_URL,
  preview: {
    sourceHost: "exemple.fr",
    channelTitle: null,
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
  selectedKeys: new Set(),
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
    onToggleItem: vi.fn(),
    onSelectAll: vi.fn(),
    onSelectNone: vi.fn(),
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

  it("renders the review with host, feed name, findings, the ticked episode list and the count", () => {
    render(<CreateFromRssSurface open status={REVIEW} {...noopHandlers()} />);
    expect(screen.getByText("exemple.fr")).toBeInTheDocument();
    expect(screen.getByText("Mon flux")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
      ),
    ).toBeInTheDocument();
    // Every episode is ticked by default: the whole podcast is the story.
    const list = screen.getByRole("list", { name: "Épisodes du flux" });
    const boxes = within(list).getAllByRole("checkbox");
    expect(boxes.map((b) => b.getAttribute("aria-label"))).toEqual([
      "Episode 1",
      "Episode 2",
    ]);
    expect(boxes.every((b) => (b as HTMLInputElement).checked)).toBe(true);
    expect(screen.getByText("Deuxième texte.")).toBeInTheDocument();
    expect(screen.getByText("2 épisodes sélectionnés sur 2")).toBeInTheDocument();
    // The media markers say what the accept will download.
    expect(screen.getByText("Média audio")).toBeInTheDocument();
    expect(screen.getByText("Sans média audio")).toBeInTheDocument();
    expect(screen.getByText("Image")).toBeInTheDocument();
    // No alert for a calm exploitable review.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("ticks and unticks episodes, with the whole-list gestures, and accepts from the enabled CTA", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    const { rerender } = render(
      <CreateFromRssSurface open status={REVIEW} {...handlers} />,
    );
    // The field carries the reviewed address (the user fetched it from
    // here — the surface keeps the typed value across the state change).
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    await user.click(screen.getByRole("checkbox", { name: "Episode 2" }));
    expect(handlers.onToggleItem).toHaveBeenCalledWith({
      kind: "guid",
      guid: "g-2",
      fingerprint: "b".repeat(64),
    });
    // Everything ticked: only "Tout désélectionner" is live.
    expect(
      screen.getByRole("button", { name: "Tout sélectionner" }),
    ).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Tout désélectionner" }));
    expect(handlers.onSelectNone).toHaveBeenCalledTimes(1);

    rerender(
      <CreateFromRssSurface open status={REVIEW_ONE_SELECTED} {...handlers} />,
    );
    expect(screen.getByRole("checkbox", { name: "Episode 1" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Episode 2" })).not.toBeChecked();
    expect(screen.getByText("1 épisode sélectionné sur 2")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tout sélectionner" }));
    expect(handlers.onSelectAll).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Créer l'histoire" }));
    expect(handlers.onAccept).toHaveBeenCalledTimes(1);
  });

  it("refuses the accept with nothing ticked", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromRssSurface open status={REVIEW_NONE_SELECTED} {...handlers} />,
    );
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    expect(screen.getByText("0 épisode sélectionné sur 2")).toBeInTheDocument();
    const accept = screen.getByRole("button", { name: "Créer l'histoire" });
    expect(accept).toHaveAttribute("aria-disabled", "true");
    await user.click(accept);
    expect(handlers.onAccept).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Tout désélectionner" }),
    ).toBeDisabled();
  });

  it("names an untitled episode by its rank", () => {
    const untitled: RssCreationStatus = {
      ...REVIEW,
      kind: "review",
      preview: {
        ...EXPLOITABLE_PREVIEW,
        channelTitle: null,
        items: [{ ...EXPLOITABLE_PREVIEW.items[1], title: "" }],
      },
      selectedKeys: new Set([KEY_2]),
    };
    render(<CreateFromRssSurface open status={untitled} {...noopHandlers()} />);
    expect(screen.getByRole("checkbox", { name: "Épisode 1" })).toBeChecked();
    expect(screen.queryByText("Mon flux")).not.toBeInTheDocument();
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
      screen.queryByRole("button", { name: "Créer l'histoire" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
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
      screen.queryByRole("button", { name: "Créer l'histoire" }),
    ).not.toBeInTheDocument();
    // The field + fetch CTA stay available for the re-fetch gesture.
    expect(screen.getByLabelText("Adresse du flux RSS")).toBeInTheDocument();
  });

  it("renders the creating progress with the shared frozen label, then the streamed percent", () => {
    const { rerender } = render(
      <CreateFromRssSurface
        open
        status={{ kind: "creating", progress: null }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();
    rerender(
      <CreateFromRssSurface
        open
        status={{ kind: "creating", progress: 42 }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Création en cours… 42 %")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "42");
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
    render(<CreateFromRssSurface open status={REVIEW} {...handlers} />);
    // The reviewed feedUrl is FEED_URL but the visible field is empty →
    // diverged: the accept CTA is refused even with a selection.
    const accept = screen.getByRole("button", { name: "Créer l'histoire" });
    expect(accept).toHaveAttribute("aria-disabled", "true");
    await user.click(accept);
    expect(handlers.onAccept).not.toHaveBeenCalled();

    // Typing the reviewed address back restores the CTA.
    await user.type(screen.getByLabelText("Adresse du flux RSS"), FEED_URL);
    const restored = screen.getByRole("button", {
      name: "Créer l'histoire",
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
        status={{ kind: "creating", progress: null }}
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
