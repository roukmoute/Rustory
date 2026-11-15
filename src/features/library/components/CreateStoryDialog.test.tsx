import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

vi.mock("../../../ipc/commands/story", () => ({
  createStory: vi.fn(),
}));

import { createStory } from "../../../ipc/commands/story";

import { CreateStoryDialog } from "./CreateStoryDialog";
import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";

/** The official policy as of the web import (TDD-7): `web` is enabled
 *  alongside `rss`, exactly as `read_content_source_policy` serializes it. */
const WEB_ENABLED_POLICY: ContentSourcePolicy = {
  sources: [
    {
      kind: "rss",
      label: "Flux RSS",
      activation: "enabled",
      activationMarker: "Activée par la distribution officielle",
    },
    {
      kind: "atom",
      label: "Flux Atom",
      activation: "notActivated",
      reason:
        "Source indisponible: non activée dans la distribution officielle",
    },
    {
      kind: "jsonFeed",
      label: "Flux JSON Feed",
      activation: "notActivated",
      reason:
        "Source indisponible: non activée dans la distribution officielle",
    },
    {
      kind: "web",
      label: "Page web",
      activation: "enabled",
      activationMarker: "Activée par la distribution officielle",
    },
  ],
};

/** The current official policy, exactly as `read_content_source_policy`
 *  serializes it (rss enabled; atom / jsonFeed known but not activated). */
const RSS_ENABLED_POLICY: ContentSourcePolicy = {
  sources: [
    {
      kind: "rss",
      label: "Flux RSS",
      activation: "enabled",
      activationMarker: "Activée par la distribution officielle",
    },
    {
      kind: "atom",
      label: "Flux Atom",
      activation: "notActivated",
      reason:
        "Source indisponible: non activée dans la distribution officielle",
    },
    {
      kind: "jsonFeed",
      label: "Flux JSON Feed",
      activation: "notActivated",
      reason:
        "Source indisponible: non activée dans la distribution officielle",
    },
  ],
};

// happy-dom's HTMLDialogElement is a stub — mirror the spy pattern used in
// Dialog.test.tsx so this file never relies on a real modal lifecycle.
const showModalSpy = vi.spyOn(
  HTMLDialogElement.prototype,
  "showModal" as never,
);
const closeSpy = vi.spyOn(HTMLDialogElement.prototype, "close" as never);

beforeAll(() => {
  showModalSpy.mockImplementation(function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  });
  closeSpy.mockImplementation(function (this: HTMLDialogElement) {
    this.removeAttribute("open");
  });
});

afterAll(() => {
  showModalSpy.mockRestore();
  closeSpy.mockRestore();
});

describe("<CreateStoryDialog />", () => {
  beforeEach(() => {
    vi.mocked(createStory).mockReset();
  });

  function renderDialog(
    override?: Partial<React.ComponentProps<typeof CreateStoryDialog>>,
  ) {
    const onClose = vi.fn();
    const onCreated = vi.fn();
    const view = render(
      <CreateStoryDialog
        open
        initialStep="title"
        onClose={onClose}
        onCreated={onCreated}
        {...override}
      />,
    );
    return { onClose, onCreated, ...view };
  }

  function renderImportDialog(
    override?: Partial<React.ComponentProps<typeof CreateStoryDialog>>,
  ) {
    return renderDialog({ ...override, initialStep: "import" });
  }

  it("renders only the writing step with its heading, title and actions", () => {
    renderDialog();
    expect(
      screen.getByRole("heading", { name: "Comment s'appelle ton histoire ?" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retour" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^créer$/i }),
    ).toBeInTheDocument();
  });

  it("marks Créer as aria-disabled with the 'titre requis' reason while empty", () => {
    renderDialog();
    const submit = screen.getByRole("button", { name: /^créer$/i });
    expect(submit).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText(/création impossible: titre requis/i),
    ).toBeInTheDocument();
  });

  it("enables Créer once a valid title is typed", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.type(screen.getByLabelText(/^titre$/i), "Un titre");
    const submit = screen.getByRole("button", { name: /^créer$/i });
    expect(submit).not.toHaveAttribute("aria-disabled", "true");
  });

  it("shows the too-long reason when the title exceeds 120 code points", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.type(screen.getByLabelText(/^titre$/i), "a".repeat(121));
    expect(
      screen.getByText(
        /création impossible: titre trop long \(120 caractères maximum, 1 en trop\)/i,
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^créer$/i })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("shows the control-char reason when the title contains a tab", async () => {
    const user = userEvent.setup();
    renderDialog();
    // fireEvent via paste avoids userEvent filtering literal control chars
    const field = screen.getByLabelText(/^titre$/i) as HTMLInputElement;
    await user.click(field);
    field.focus();
    // Inject the value through the React onChange path so validation runs.
    await user.paste("a\tb");
    expect(
      screen.getByText(
        /création impossible: titre contient des caractères non autorisés/i,
      ),
    ).toBeInTheDocument();
  });

  it("submits the normalized title and calls onCreated + onClose on success", async () => {
    const user = userEvent.setup();
    vi.mocked(createStory).mockResolvedValueOnce({
      id: "id-1",
      title: "Mon histoire",
    });
    const { onCreated, onClose } = renderDialog();

    await user.type(screen.getByLabelText(/^titre$/i), "  Mon histoire  ");
    await user.click(screen.getByRole("button", { name: /^créer$/i }));

    await waitFor(() => expect(createStory).toHaveBeenCalledTimes(1));
    expect(createStory).toHaveBeenCalledWith({ title: "Mon histoire" });
    await waitFor(() =>
      expect(onCreated).toHaveBeenCalledWith({
        id: "id-1",
        title: "Mon histoire",
      }),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("surfaces Rust INVALID_STORY_TITLE errors as role=alert, returns focus to the field and keeps the typed value", async () => {
    const user = userEvent.setup();
    vi.mocked(createStory).mockRejectedValueOnce({
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre requis",
      userAction: "Saisis un titre non vide pour créer l'histoire.",
      details: null,
    });
    const { onCreated, onClose } = renderDialog();

    const field = screen.getByLabelText(/^titre$/i) as HTMLInputElement;
    await user.type(field, "X");
    await user.click(screen.getByRole("button", { name: /^créer$/i }));

    await waitFor(() => {
      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent(/titre requis/i);
      expect(alert).toHaveTextContent(/saisis un titre non vide/i);
    });
    expect(onCreated).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    // The typed content survives so the user can correct without retyping.
    expect(field.value).toBe("X");
    // Focus returns to the field so the user can correct without hunting
    // the next focus stop via keyboard.
    expect(field).toHaveFocus();
  });

  it("clears the stale server error banner as soon as the user edits the title", async () => {
    const user = userEvent.setup();
    vi.mocked(createStory).mockRejectedValueOnce({
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre requis",
      userAction: "Saisis un titre non vide pour créer l'histoire.",
      details: null,
    });
    renderDialog();

    const field = screen.getByLabelText(/^titre$/i) as HTMLInputElement;
    await user.type(field, "X");
    await user.click(screen.getByRole("button", { name: /^créer$/i }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());

    // One more keystroke means the user is correcting — the banner must
    // disappear immediately instead of shouting about a value that no
    // longer exists.
    await user.type(field, "Y");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("forwards a submission to Rust even when the local validator disagrees (race path, trusts Rust as authority)", async () => {
    // Simulates a race where the DOM value is actually valid but React's
    // last render still reflects the empty state. handleSubmit reads the
    // live DOM value, so the call still reaches createStory; Rust stays
    // authoritative for the outcome.
    const user = userEvent.setup();
    vi.mocked(createStory).mockResolvedValueOnce({ id: "id-3", title: "A" });
    const { onCreated } = renderDialog();

    const field = screen.getByLabelText(/^titre$/i) as HTMLInputElement;
    // Write directly to the DOM to sidestep React state, then press Enter.
    field.focus();
    field.value = "A";
    await user.keyboard("{Enter}");

    await waitFor(() =>
      expect(createStory).toHaveBeenCalledWith({ title: "A" }),
    );
    await waitFor(() =>
      expect(onCreated).toHaveBeenCalledWith({ id: "id-3", title: "A" }),
    );
  });

  it("normalizes unexpected IPC rejections via toAppError", async () => {
    const user = userEvent.setup();
    vi.mocked(createStory).mockRejectedValueOnce(new Error("boom"));
    renderDialog();

    await user.type(screen.getByLabelText(/^titre$/i), "X");
    await user.click(screen.getByRole("button", { name: /^créer$/i }));

    await waitFor(() => {
      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent(/erreur inattendue/i);
    });
  });

  it("clicks Annuler → calls onClose without any IPC", async () => {
    const user = userEvent.setup();
    const { onClose } = renderDialog({ initialStep: "choice" });
    await user.click(screen.getByRole("button", { name: /annuler/i }));
    expect(onClose).toHaveBeenCalled();
    expect(createStory).not.toHaveBeenCalled();
  });

  it("Enter in the title field submits when valid", async () => {
    const user = userEvent.setup();
    vi.mocked(createStory).mockResolvedValueOnce({ id: "id-2", title: "A" });
    const { onCreated } = renderDialog();
    const field = screen.getByLabelText(/^titre$/i);
    await user.type(field, "A{Enter}");
    await waitFor(() =>
      expect(onCreated).toHaveBeenCalledWith({ id: "id-2", title: "A" }),
    );
  });

  it("renders no story, BMAD or epic jargon in the dialog", () => {
    renderDialog();
    const dialog = screen.getByRole("dialog", {
      name: "Comment s'appelle ton histoire ?",
    });
    expect(dialog.textContent).not.toMatch(/\b(BMAD|épic|story\s*1)\b/i);
  });

  it("hides the folder entry when no folder handler is wired", () => {
    renderImportDialog();
    expect(
      screen.queryByRole("button", { name: /choisir un dossier/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/dossier préparé hors de Rustory/i),
    ).not.toBeInTheDocument();
  });

  it("renders the folder entry only within the import step", () => {
    renderImportDialog({ onCreateFromFolderRequest: vi.fn() });
    expect(screen.queryByLabelText(/^titre$/i)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^créer$/i }),
    ).not.toBeInTheDocument();
    // The secondary entry, with its frozen copy.
    expect(
      screen.getByText("Un dossier préparé hors de Rustory"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choisir un dossier…" }),
    ).toBeInTheDocument();
  });

  it("Choisir un dossier… closes the dialog THEN hands over to the folder flow, without any interactive IPC", async () => {
    const user = userEvent.setup();
    const onCreateFromFolderRequest = vi.fn();
    const { onClose } = renderImportDialog({ onCreateFromFolderRequest });
    await user.click(
      screen.getByRole("button", { name: "Choisir un dossier…" }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onCreateFromFolderRequest).toHaveBeenCalledTimes(1);
    // The dialog closes BEFORE the handover (the native picker must never
    // stack under a modal).
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(
      onCreateFromFolderRequest.mock.invocationCallOrder[0],
    );
    expect(createStory).not.toHaveBeenCalled();
  });

  it("disables the folder entry while a sibling import/creation flow is busy (cross-flow exclusivity)", async () => {
    const user = userEvent.setup();
    const onCreateFromFolderRequest = vi.fn();
    const { onClose } = renderImportDialog({
      onCreateFromFolderRequest,
      isCreateFromFolderUnavailable: true,
    });
    const folderButton = screen.getByRole("button", {
      name: "Choisir un dossier…",
    });
    expect(folderButton).toHaveAttribute("aria-disabled", "true");
    await user.click(folderButton);
    // Two native dialogs / review surfaces must never stack: the handover
    // is a no-op and the dialog stays open.
    expect(onCreateFromFolderRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("Choisir une archive de pack (.zip)… closes the dialog THEN hands over to the archive flow", async () => {
    const user = userEvent.setup();
    const onCreateFromArchiveRequest = vi.fn();
    const { onClose } = renderImportDialog({ onCreateFromArchiveRequest });
    expect(
      screen.getByText("Une archive de pack au format communautaire"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "Choisir une archive de pack (.zip)…",
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onCreateFromArchiveRequest).toHaveBeenCalledTimes(1);
    // The dialog closes BEFORE the handover (the native picker must never
    // stack under a modal).
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(
      onCreateFromArchiveRequest.mock.invocationCallOrder[0],
    );
    expect(createStory).not.toHaveBeenCalled();
  });

  it("disables the archive entry while a sibling flow is busy, and hides it when unwired", async () => {
    const user = userEvent.setup();
    const onCreateFromArchiveRequest = vi.fn();
    const { onClose } = renderImportDialog({
      onCreateFromArchiveRequest,
      isCreateFromArchiveUnavailable: true,
    });
    const archiveButton = screen.getByRole("button", {
      name: "Choisir une archive de pack (.zip)…",
    });
    expect(archiveButton).toHaveAttribute("aria-disabled", "true");
    await user.click(archiveButton);
    expect(onCreateFromArchiveRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("hides the archive entry when no archive handler is wired", () => {
    renderImportDialog();
    expect(
      screen.queryByRole("button", {
        name: "Choisir une archive de pack (.zip)…",
      }),
    ).not.toBeInTheDocument();
  });

  it("hides the RSS entry when no RSS handler is wired", () => {
    renderImportDialog();
    expect(
      screen.queryByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    ).not.toBeInTheDocument();
  });

  // RE-SCOPED with the content-source policy: the entry is ACTIVE only
  // when the read policy enables `rss` (fail-closed without one), so
  // every "active entry" journey now hands the enabled policy in.
  it("renders RSS beside the folder entry without mixing in the title step", () => {
    renderImportDialog({
      onCreateFromFolderRequest: vi.fn(),
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    expect(screen.queryByLabelText(/^titre$/i)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^créer$/i }),
    ).not.toBeInTheDocument();
    // The folder entry is INTACT.
    expect(
      screen.getByRole("button", { name: "Choisir un dossier…" }),
    ).toBeInTheDocument();
    // The third entry, with its frozen copy.
    expect(
      screen.getByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    ).toBeInTheDocument();
  });

  it("Démarrer depuis une source externe (RSS) closes the dialog THEN hands over, without any interactive IPC", async () => {
    const user = userEvent.setup();
    const onCreateFromRssRequest = vi.fn();
    const { onClose } = renderImportDialog({
      onCreateFromRssRequest,
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    await user.click(
      screen.getByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onCreateFromRssRequest).toHaveBeenCalledTimes(1);
    // The dialog closes BEFORE the handover (the in-context surface must
    // never sit under a modal).
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(
      onCreateFromRssRequest.mock.invocationCallOrder[0],
    );
    expect(createStory).not.toHaveBeenCalled();
  });

  it("disables the RSS entry while a sibling import/creation flow is busy (cross-flow exclusivity)", async () => {
    const user = userEvent.setup();
    const onCreateFromRssRequest = vi.fn();
    const { onClose } = renderImportDialog({
      onCreateFromRssRequest,
      isCreateFromRssUnavailable: true,
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
    await user.click(rssButton);
    expect(onCreateFromRssRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  // ===== Content-source section (policy-driven) =====

  it("renders the enabled RSS action with its frozen activation description", () => {
    renderImportDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).not.toHaveAttribute("aria-disabled");
    // The kind label and the frozen entry-level marker, VERBATIM.
    expect(rssButton).toHaveAccessibleDescription(
      "Activée par la distribution officielle",
    );
    expect(
      screen.getByText("Activée par la distribution officielle"),
    ).toBeInTheDocument();
  });

  it.each(["notActivated", "enabled", "blockedByPolicy"] as const)(
    "omits unimplemented source kinds even when their policy is %s",
    (activation) => {
      renderImportDialog({
        onCreateFromRssRequest: vi.fn(),
        onCreateFromWebRequest: vi.fn(),
        contentSourcePolicy: {
          sources: [
            RSS_ENABLED_POLICY.sources[0],
            {
              kind: "atom",
              label: "Flux Atom",
              activation,
              reason: "Source indisponible",
            },
            {
              kind: "jsonFeed",
              label: "Flux JSON Feed",
              activation,
              reason: "Source indisponible",
            },
            WEB_ENABLED_POLICY.sources[3],
          ],
        },
      });
      expect(
        screen.queryByRole("button", { name: "Flux Atom" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Flux JSON Feed" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: /page web/i }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", {
          name: "Démarrer depuis une source externe (RSS)",
        }),
      ).not.toHaveAttribute("aria-disabled", "true");
    },
  );

  it("renders the RSS entry disabled with the Rust reason when the policy does not enable it", async () => {
    const user = userEvent.setup();
    const onCreateFromRssRequest = vi.fn();
    renderImportDialog({
      onCreateFromRssRequest,
      contentSourcePolicy: {
        sources: [
          {
            kind: "rss",
            label: "Flux RSS",
            activation: "notActivated",
            reason:
              "Source indisponible: non activée dans la distribution officielle",
          },
        ],
      },
    });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
    await user.click(rssButton);
    expect(onCreateFromRssRequest).not.toHaveBeenCalled();
    expect(
      screen.getByText(
        "Source indisponible: non activée dans la distribution officielle",
      ),
    ).toBeInTheDocument();
  });

  // The previous behavior (callback prop alone ⇒ active entry) is
  // RE-SCOPED, never dropped: without a readable policy the entry renders
  // FAIL-CLOSED.
  it("renders the RSS entry disabled fail-closed when the callback is wired but no policy was read", async () => {
    const user = userEvent.setup();
    const onCreateFromRssRequest = vi.fn();
    const { onClose } = renderImportDialog({ onCreateFromRssRequest });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
    await user.click(rssButton);
    expect(onCreateFromRssRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    expect(
      screen.getByText("Sources externes indisponibles pour l'instant."),
    ).toBeInTheDocument();
    // The title path remains accessible through the choice step despite the policy failure.
    await user.click(screen.getByRole("button", { name: "Retour" }));
    await user.click(
      screen.getByRole("button", { name: "Écrire une histoire" }),
    );
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
  });

  it("renders the fail-closed entry when the read policy carries no rss line", () => {
    renderImportDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: {
        sources: [
          {
            kind: "atom",
            label: "Flux Atom",
            activation: "notActivated",
            reason:
              "Source indisponible: non activée dans la distribution officielle",
          },
        ],
      },
    });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText("Sources externes indisponibles pour l'instant."),
    ).toBeInTheDocument();
  });

  // Web navigation and submission are distinct: selecting the card never fetches.
  it("keeps the web choice fail-closed when its handler is absent", () => {
    renderDialog({
      initialStep: "choice",
      contentSourcePolicy: WEB_ENABLED_POLICY,
    });
    const web = screen.getByRole("button", { name: "Depuis une page web" });
    expect(web).toHaveAttribute("aria-disabled", "true");
    expect(web).toHaveAccessibleDescription(
      "Sources externes indisponibles pour l'instant.",
    );
    fireEvent.click(web);
    expect(
      screen.queryByLabelText("Adresse de la page web"),
    ).not.toBeInTheDocument();
  });

  it("opens the web address step without starting a fetch or closing the dialog", async () => {
    const user = userEvent.setup();
    const onCreateFromWebRequest = vi.fn();
    const { onClose } = renderDialog({
      initialStep: "choice",
      onCreateFromWebRequest,
      contentSourcePolicy: WEB_ENABLED_POLICY,
    });
    await user.click(
      screen.getByRole("button", { name: "Depuis une page web" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Depuis une page web" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Adresse de la page web")).toHaveFocus();
    expect(screen.queryByLabelText("Titre")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Choisir un dossier…" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Récupérer la page" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(onCreateFromWebRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes the dialog before requesting the trimmed web address, without creating a draft", async () => {
    const user = userEvent.setup();
    const onCreateFromWebRequest = vi.fn();
    const { onClose } = renderDialog({
      initialStep: "choice",
      onCreateFromWebRequest,
      contentSourcePolicy: WEB_ENABLED_POLICY,
    });
    await user.click(
      screen.getByRole("button", { name: "Depuis une page web" }),
    );
    await user.type(
      screen.getByLabelText("Adresse de la page web"),
      "  https://example.org/histoire  ",
    );
    expect(onCreateFromWebRequest).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Récupérer la page" }));
    expect(onCreateFromWebRequest).toHaveBeenCalledExactlyOnceWith(
      "https://example.org/histoire",
    );
    expect(onClose).toHaveBeenCalledOnce();
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(
      onCreateFromWebRequest.mock.invocationCallOrder[0],
    );
    expect(createStory).not.toHaveBeenCalled();
  });

  it("refuses whitespace-only web addresses for both button and Enter submission", async () => {
    const user = userEvent.setup();
    const onCreateFromWebRequest = vi.fn();
    const { onClose } = renderDialog({
      initialStep: "choice",
      onCreateFromWebRequest,
      contentSourcePolicy: WEB_ENABLED_POLICY,
    });
    await user.click(
      screen.getByRole("button", { name: "Depuis une page web" }),
    );
    await user.type(
      screen.getByLabelText("Adresse de la page web"),
      "   {Enter}",
    );
    await user.click(screen.getByRole("button", { name: "Récupérer la page" }));
    expect(onCreateFromWebRequest).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it.each(["busy", "policy"] as const)(
    "refuses web submission when %s changes after the address step opened",
    async (gate) => {
      const user = userEvent.setup();
      const onCreateFromWebRequest = vi.fn();
      const onClose = vi.fn();
      const props = {
        open: true,
        onClose,
        onCreated: vi.fn(),
        onCreateFromWebRequest,
        contentSourcePolicy: WEB_ENABLED_POLICY,
      };
      const { rerender } = render(<CreateStoryDialog {...props} />);
      await user.click(
        screen.getByRole("button", { name: "Depuis une page web" }),
      );
      await user.type(
        screen.getByLabelText("Adresse de la page web"),
        "https://example.org/histoire",
      );
      rerender(
        <CreateStoryDialog
          {...props}
          isCreateFromWebUnavailable={gate === "busy"}
          contentSourcePolicy={gate === "policy" ? null : WEB_ENABLED_POLICY}
        />,
      );
      await user.keyboard("{Enter}");
      const fetch = screen.getByRole("button", { name: "Récupérer la page" });
      expect(fetch).toHaveAttribute("aria-disabled", "true");
      await user.click(fetch);
      expect(onCreateFromWebRequest).not.toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();
    },
  );

  it("shows the Rust refusal reason on a web choice disabled by policy", () => {
    renderDialog({
      initialStep: "choice",
      onCreateFromWebRequest: vi.fn(),
      contentSourcePolicy: {
        sources: [
          {
            kind: "web",
            label: "Page web",
            activation: "notActivated",
            reason:
              "Source indisponible: non activée dans la distribution officielle",
          },
        ],
      },
    });
    const web = screen.getByRole("button", { name: "Depuis une page web" });
    expect(web).toHaveAttribute("aria-disabled", "true");
    expect(web).toHaveAccessibleDescription(
      "Source indisponible: non activée dans la distribution officielle",
    );
  });

  it("keeps Retour and duplicate submission inactive until the title creation settles", async () => {
    const user = userEvent.setup();
    let finish!: (story: { id: string; title: string }) => void;
    vi.mocked(createStory).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const { onClose, onCreated } = renderDialog();
    await user.type(screen.getByLabelText("Titre"), "Mon histoire");
    await user.click(screen.getByRole("button", { name: "Créer" }));
    const dialog = screen.getByRole("dialog", {
      name: "Comment s'appelle ton histoire ?",
    });
    await user.click(within(dialog).getByRole("button", { name: "Retour" }));
    fireEvent.keyDown(screen.getByLabelText("Titre"), { key: "Enter" });
    expect(createStory).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("dialog", { name: "Comment s'appelle ton histoire ?" }),
    ).toBe(dialog);
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => finish({ id: "created", title: "Mon histoire" }));
    expect(onCreated).toHaveBeenCalledExactlyOnceWith({
      id: "created",
      title: "Mon histoire",
    });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
