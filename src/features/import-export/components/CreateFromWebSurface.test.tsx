import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CreateFromWebSurface } from "./CreateFromWebSurface";
import type { WebCreationStatus } from "../hooks/use-web-creation";

const WEB_URL =
  "https://www.radiofrance.fr/radiofrance/podcasts/selection-pour-partir-a-l-aventure";

const PAGE_CHECKSUM = "c".repeat(64);

const WEB_PREVIEW = {
  sourceHost: "www.radiofrance.fr",
  pageChecksum: PAGE_CHECKSUM,
  items: [
    {
      title: "Épisode 1",
      summary: "Premier texte.",
      audioUrl: "https://media.exemple.fr/e1.mp3",
      imageUrl: null,
    },
    {
      title: "Épisode 2",
      summary: "Deuxième texte.",
      audioUrl: "https://media.exemple.fr/e2.mp3",
      imageUrl: "https://media.exemple.fr/e2.jpg",
    },
  ],
};

const REVIEW: WebCreationStatus = {
  kind: "review",
  webUrl: WEB_URL,
  preview: WEB_PREVIEW,
  sourceChanged: false,
};

const REVIEW_SOURCE_CHANGED: WebCreationStatus = {
  ...REVIEW,
  kind: "review",
  sourceChanged: true,
};

function noopHandlers() {
  return {
    onFetch: vi.fn(),
    onAccept: vi.fn(),
    onAbandon: vi.fn(),
    onDismiss: vi.fn(),
  };
}

describe("CreateFromWebSurface", () => {
  it("renders nothing while closed, whatever the machine state", () => {
    const { container } = render(
      <CreateFromWebSurface open={false} status={REVIEW} {...noopHandlers()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the posture line, the address field and the fetch CTA when open on idle", () => {
    render(
      <CreateFromWebSurface
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
    expect(
      screen.getByLabelText("Adresse de la page web"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Récupérer la page" }),
    ).toBeInTheDocument();
  });

  it("emits the fetch with the trimmed address on the CTA", async () => {
    const user = userEvent.setup();
    const onFetch = vi.fn();
    render(
      <CreateFromWebSurface
        open
        status={{ kind: "idle" }}
        onFetch={onFetch}
        onAccept={vi.fn()}
        onAbandon={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    await user.type(screen.getByLabelText("Adresse de la page web"), ` ${WEB_URL} `);
    await user.click(screen.getByRole("button", { name: "Récupérer la page" }));
    expect(onFetch).toHaveBeenCalledWith(WEB_URL);
  });

  it("shows the motivated reason in an alert for a malformed URL (S4)", () => {
    render(
      <CreateFromWebSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "RSS_SOURCE_UNREACHABLE",
            message:
              "Récupération de la page impossible: l'adresse n'est pas valide.",
            userAction: "Saisis une adresse http(s) complète puis réessaie.",
            details: { source: "network", stage: "url_invalid" },
          },
        }}
        {...noopHandlers()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "Récupération de la page impossible: l'adresse n'est pas valide.",
    );
    expect(alert).toHaveTextContent(
      "Saisis une adresse http(s) complète puis réessaie.",
    );
    // The address field STAYS on a transport failure (the gesture is
    // "correct the address, then retry" — in-context); the form's own
    // fetch CTA yields to the alert's Réessayer.
    expect(
      screen.getByLabelText("Adresse de la page web"),
    ).toBeInTheDocument();
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual([
      "Réessayer",
      "Fermer",
    ]);
  });

  it("shows the motivated reason in an alert for an unreachable page (S5)", () => {
    render(
      <CreateFromWebSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "RSS_SOURCE_UNREACHABLE",
            message: "La page est injoignable.",
            userAction: "Vérifie ta connexion puis réessaie.",
            details: { source: "network", stage: "request" },
          },
        }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "La page est injoignable.",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Vérifie ta connexion puis réessaie.",
    );
  });

  it("shows the motivated reason in an alert for a page without audio media (S6)", () => {
    render(
      <CreateFromWebSurface
        open
        status={{
          kind: "failed",
          error: {
            code: "IMPORT_FAILED",
            message: "Aucun média audio n'a été trouvé.",
            userAction:
              "Vérifie que la page contient des épisodes audio puis réessaie.",
            details: { source: "parsing", stage: "no_audio_media" },
          },
        }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Aucun média audio n'a été trouvé.",
    );
  });

  it("lists the preview episodes with their media markers and emits the accept on confirmation", async () => {
    const user = userEvent.setup();
    const onAccept = vi.fn();
    render(
      <CreateFromWebSurface
        open
        status={REVIEW}
        onFetch={vi.fn()}
        onAccept={onAccept}
        onAbandon={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    // The reviewed host (never the full address) leads the review.
    expect(screen.getByText("www.radiofrance.fr")).toBeInTheDocument();
    // Every previewed episode, in document order.
    expect(screen.getByText("Épisode 1")).toBeInTheDocument();
    expect(screen.getByText("Épisode 2")).toBeInTheDocument();
    // The audio marker on both, the image marker only where the page
    // provides one (S3: the missing image never hides the episode).
    expect(screen.getAllByText("Média audio")).toHaveLength(2);
    expect(screen.getAllByText("Image")).toHaveLength(1);
    // The field still holds the fetched address in the real flow —
    // type it back so the accept CTA is not refused (address not diverged).
    await user.type(screen.getByLabelText("Adresse de la page web"), WEB_URL);
    await user.click(screen.getByRole("button", { name: "Créer l'histoire" }));
    expect(onAccept).toHaveBeenCalledTimes(1);
  });

  it("refuses the accept while the typed address diverges from the reviewed one", async () => {
    const handlers = noopHandlers();
    const user = userEvent.setup();
    render(
      <CreateFromWebSurface open status={REVIEW} {...handlers} />,
    );
    // The reviewed webUrl is WEB_URL but the visible field is empty →
    // diverged: the accept CTA is refused even with a selection.
    const accept = screen.getByRole("button", { name: "Créer l'histoire" });
    expect(accept).toHaveAttribute("aria-disabled", "true");
    await user.click(accept);
    expect(handlers.onAccept).not.toHaveBeenCalled();

    // Typing the reviewed address back restores the CTA.
    await user.type(screen.getByLabelText("Adresse de la page web"), WEB_URL);
    const restored = screen.getByRole("button", {
      name: "Créer l'histoire",
    });
    expect(restored).not.toHaveAttribute("aria-disabled");
    await user.click(restored);
    expect(handlers.onAccept).toHaveBeenCalledTimes(1);
  });

  it("shows the frozen verdict when the page changed since the fetch", () => {
    render(
      <CreateFromWebSurface
        open
        status={REVIEW_SOURCE_CHANGED}
        {...noopHandlers()}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(
      "La page a changé depuis la récupération.",
    );
    expect(alert).toHaveTextContent("Relance la récupération de la page.");
    expect(
      screen.queryByRole("button", { name: "Créer l'histoire" }),
    ).not.toBeInTheDocument();
  });

  it("shows the fetching progress while the page is being fetched", () => {
    render(
      <CreateFromWebSurface
        open
        status={{ kind: "fetching" }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Récupération de la page…")).toBeInTheDocument();
  });

  it("shows the creating progress while the story is being built", () => {
    render(
      <CreateFromWebSurface
        open
        status={{ kind: "creating" }}
        {...noopHandlers()}
      />,
    );
    expect(screen.getByText("Création en cours…")).toBeInTheDocument();
  });

  it("shows the success chip and the story title when created", () => {
    render(
      <CreateFromWebSurface
        open
        status={{
          kind: "created",
          story: {
            id: "0197a5d0-0000-7000-8000-000000000000",
            title: "Sélection pour partir à l'aventure",
          },
        }}
        {...noopHandlers()}
      />,
    );
    // The success chip + the polite live region carry the frozen copy.
    expect(
      screen.getAllByText("Histoire créée dans ta bibliothèque").length,
    ).toBeGreaterThanOrEqual(1);
    expect(
      screen.getByText("Sélection pour partir à l'aventure"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Fermer" }),
    ).toBeInTheDocument();
  });
});
