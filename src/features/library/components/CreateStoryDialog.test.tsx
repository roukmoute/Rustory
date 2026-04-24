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
    expect(screen.getByRole("button", { name: /annuler/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^créer$/i })).toBeInTheDocument();
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

    await waitFor(() => expect(createStory).toHaveBeenCalledWith({ title: "A" }));
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
});
