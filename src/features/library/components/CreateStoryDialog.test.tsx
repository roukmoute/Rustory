import { render, screen, waitFor } from "@testing-library/react";
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
        onClose={onClose}
        onCreated={onCreated}
        {...override}
      />,
    );
    return { onClose, onCreated, ...view };
  }

  it("renders the dialog heading, labelled title field and both CTAs", () => {
    renderDialog();
    expect(
      screen.getByRole("heading", { name: /créer une histoire/i }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /annuler/i }),
    ).toBeInTheDocument();
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
    const { onClose } = renderDialog();
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
      name: /créer une histoire/i,
    });
    expect(dialog.textContent).not.toMatch(/\b(BMAD|épic|story\s*1)\b/i);
  });

  it("hides the folder entry when no folder handler is wired", () => {
    renderDialog();
    expect(
      screen.queryByRole("button", { name: /choisir un dossier/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/dossier préparé hors de Rustory/i),
    ).not.toBeInTheDocument();
  });

  it("renders the secondary folder entry and keeps the interactive path primary", () => {
    renderDialog({ onCreateFromFolderRequest: vi.fn() });
    // The interactive path is INTACT: field + both CTAs still there.
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^créer$/i }),
    ).toBeInTheDocument();
    // The secondary entry, with its frozen copy.
    expect(
      screen.getByText("Ou démarre depuis un dossier préparé hors de Rustory"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choisir un dossier…" }),
    ).toBeInTheDocument();
  });

  it("Choisir un dossier… closes the dialog THEN hands over to the folder flow, without any interactive IPC", async () => {
    const user = userEvent.setup();
    const onCreateFromFolderRequest = vi.fn();
    const { onClose } = renderDialog({ onCreateFromFolderRequest });
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
    const { onClose } = renderDialog({
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

  it("hides the RSS entry when no RSS handler is wired", () => {
    renderDialog();
    expect(
      screen.queryByRole("button", {
        name: "Démarrer depuis une source externe (RSS)",
      }),
    ).not.toBeInTheDocument();
  });

  // RE-SCOPED with the content-source policy: the entry is ACTIVE only
  // when the read policy enables `rss` (fail-closed without one), so
  // every "active entry" journey now hands the enabled policy in.
  it("renders the third RSS entry without touching the title and folder paths", () => {
    renderDialog({
      onCreateFromFolderRequest: vi.fn(),
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    // The interactive path is INTACT: field + primary CTA still there.
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^créer$/i }),
    ).toBeInTheDocument();
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
    const { onClose } = renderDialog({
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
    const { onClose } = renderDialog({
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

  it("renders the enabled RSS entry with its label and the frozen activation marker", () => {
    renderDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    const rssButton = screen.getByRole("button", {
      name: "Démarrer depuis une source externe (RSS)",
    });
    expect(rssButton).not.toHaveAttribute("aria-disabled");
    // The kind label and the frozen entry-level marker, VERBATIM.
    expect(screen.getByText("Flux RSS")).toBeInTheDocument();
    expect(
      screen.getByText("Activée par la distribution officielle"),
    ).toBeInTheDocument();
  });

  it("renders the non-activated kinds visible but disabled with their Rust-carried reason, keyboard-reachable", () => {
    renderDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: RSS_ENABLED_POLICY,
    });
    for (const label of ["Flux Atom", "Flux JSON Feed"]) {
      const entry = screen.getByRole("button", { name: label });
      expect(entry).toHaveAttribute("aria-disabled", "true");
      // The reason is reachable from the entry (aria-describedby → the
      // frozen Rust-carried copy).
      const describedBy = entry.getAttribute("aria-describedby");
      expect(describedBy).toBeTruthy();
      const reason = document.getElementById(describedBy as string);
      expect(reason).toHaveTextContent(
        "Source indisponible: non activée dans la distribution officielle",
      );
    }
  });

  it("renders an enabled non-RSS kind (unguarded prop) disabled with the fail-closed reason, never the marker", () => {
    // The IPC guard refuses such a policy upstream; the component still
    // renders honestly if handed one directly: a disabled entry is never
    // "justified" by the activation marker.
    renderDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: {
        sources: [
          {
            kind: "rss",
            label: "Flux RSS",
            activation: "enabled",
            activationMarker: "Activée par la distribution officielle",
          },
          { kind: "atom", label: "Flux Atom", activation: "enabled" },
        ],
      },
    });
    const entry = screen.getByRole("button", { name: "Flux Atom" });
    expect(entry).toHaveAttribute("aria-disabled", "true");
    const describedBy = entry.getAttribute("aria-describedby");
    const subText = document.getElementById(describedBy as string);
    expect(subText).toHaveTextContent(
      "Sources externes indisponibles pour l'instant.",
    );
    expect(subText).not.toHaveTextContent(
      "Activée par la distribution officielle",
    );
  });

  it("renders a blocked-by-policy kind with its own frozen reason", () => {
    renderDialog({
      onCreateFromRssRequest: vi.fn(),
      contentSourcePolicy: {
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
            activation: "blockedByPolicy",
            reason:
              "Source indisponible: bloquée par la politique de distribution",
          },
        ],
      },
    });
    const entry = screen.getByRole("button", { name: "Flux Atom" });
    expect(entry).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText(
        "Source indisponible: bloquée par la politique de distribution",
      ),
    ).toBeInTheDocument();
  });

  it("renders the RSS entry disabled with the Rust reason when the policy does not enable it", async () => {
    const user = userEvent.setup();
    const onCreateFromRssRequest = vi.fn();
    renderDialog({
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
    const { onClose } = renderDialog({ onCreateFromRssRequest });
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
    // The primary title path is NEVER blocked by a policy failure.
    expect(screen.getByLabelText(/^titre$/i)).toBeInTheDocument();
  });

  it("renders the fail-closed entry when the read policy carries no rss line", () => {
    renderDialog({
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
});
