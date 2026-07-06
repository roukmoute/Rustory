import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { StoryCardDto } from "../../../shared/ipc-contracts/library";
import { StoryCard } from "./StoryCard";

const STORY: StoryCardDto = {
  id: "abc123def456",
  title: "Le soleil couchant",
};

describe("StoryCard", () => {
  it("renders the title and is exposed as a button", () => {
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    expect(button).toHaveAttribute("tabindex", "0");
    expect(button).toHaveAttribute("aria-pressed", "false");
    // The card is title-centric; no short id is shown — the identifier
    // stays an internal concern exposed through the URL only.
    expect(screen.getByText(STORY.title)).toBeInTheDocument();
  });

  it("click without modifier calls onSelect with replace", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={onSelect}
        onOpen={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: STORY.title }));
    expect(onSelect).toHaveBeenCalledWith(STORY.id, "replace");
  });

  it("Ctrl+click calls onSelect with toggle", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={onSelect}
        onOpen={vi.fn()}
      />,
    );

    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("button", { name: STORY.title }));
    await user.keyboard("{/Control}");
    expect(onSelect).toHaveBeenCalledWith(STORY.id, "toggle");
  });

  it("double-click opens the story", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={onOpen}
      />,
    );

    await user.dblClick(screen.getByRole("button", { name: STORY.title }));
    expect(onOpen).toHaveBeenCalledWith(STORY.id);
  });

  it("Space on the focused card toggles selection", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={onSelect}
        onOpen={vi.fn()}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    button.focus();
    await user.keyboard(" ");
    expect(onSelect).toHaveBeenCalledWith(STORY.id, "toggle");
  });

  it("Enter on the focused card opens the story", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={onOpen}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    button.focus();
    await user.keyboard("{Enter}");
    expect(onOpen).toHaveBeenCalledWith(STORY.id);
  });

  it("selected state ships aria-pressed + visible check glyph", () => {
    render(
      <StoryCard
        story={STORY}
        isSelected={true}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    expect(button).toHaveAttribute("aria-pressed", "true");
    // The marker glyph lives in the DOM (not only as ::before) so a grayscale
    // render still shows the selection without relying on color.
    expect(button.textContent).toContain("✓");
  });

  it("Shift+click does NOT call onSelect (range selection is out of MVP)", () => {
    const onSelect = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={onSelect}
        onOpen={vi.fn()}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    // Dispatch a real MouseEvent with shiftKey=true via the DOM — the
    // userEvent v14 keyboard-modifier path does not consistently propagate
    // shiftKey onto the synthesized pointer event in our setup.
    button.dispatchEvent(
      new MouseEvent("click", {
        bubbles: true,
        cancelable: true,
        shiftKey: true,
      }),
    );
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("double-click never mutates a multi-selection (stays intact across the two clicks)", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const onOpen = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={true}
        selectionSize={3}
        onSelect={onSelect}
        onOpen={onOpen}
      />,
    );

    await user.dblClick(screen.getByRole("button", { name: STORY.title }));

    // dblClick emits two click events; with a multi-selection in play,
    // neither may call onSelect — otherwise a double-click would collapse
    // the selection to a singleton before navigating.
    expect(onSelect).not.toHaveBeenCalled();
    expect(onOpen).toHaveBeenCalledWith(STORY.id);
  });

  it("click on the single selected card toggles it off (intuitive inverse of the first click)", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const onOpen = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={true}
        selectionSize={1}
        onSelect={onSelect}
        onOpen={onOpen}
      />,
    );

    await user.click(screen.getByRole("button", { name: STORY.title }));

    // Single-selection re-click sends a toggle, letting the store drop
    // the id — users don't have to learn Ctrl+click just to deselect.
    expect(onSelect).toHaveBeenCalledWith(STORY.id, "toggle");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("held Space / Enter (key repeat) fires at most once — no flicker", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const onOpen = vi.fn();
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={onSelect}
        onOpen={onOpen}
      />,
    );

    const button = screen.getByRole("button", { name: STORY.title });
    button.focus();
    // First, real keyboard event fires onSelect once.
    await user.keyboard(" ");
    expect(onSelect).toHaveBeenCalledTimes(1);

    // Simulate an OS-level auto-repeat by dispatching a keydown with
    // `repeat: true` — userEvent does not synthesize this flag, so we go
    // through the raw DOM event path.
    const repeatEvent = new KeyboardEvent("keydown", {
      key: " ",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(repeatEvent, "repeat", { value: true });
    button.dispatchEvent(repeatEvent);
    expect(onSelect).toHaveBeenCalledTimes(1);

    const repeatEnter = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(repeatEnter, "repeat", { value: true });
    button.dispatchEvent(repeatEnter);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("tolerates very long titles without breaking the grid", () => {
    const longTitle = "x".repeat(500);
    render(
      <StoryCard
        story={{ id: "x1", title: longTitle }}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );

    const title = screen.getByRole("heading", { level: 3 });
    expect(title).toHaveClass("story-card__title");
  });

  it("shows no import marker on a native story", () => {
    render(
      <StoryCard
        story={STORY}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );
    expect(screen.queryByText("Importée")).not.toBeInTheDocument();
    expect(screen.queryByText("à revoir")).not.toBeInTheDocument();
  });

  it("shows the provenance marker but NO issue chip on a clean import (AC3)", () => {
    render(
      <StoryCard
        story={{ ...STORY, importState: "recognized" }}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByText("Importée")).toBeInTheDocument();
    // A clean import carries no attention chip.
    expect(screen.queryByText("partiel")).not.toBeInTheDocument();
    expect(screen.queryByText("à revoir")).not.toBeInTheDocument();
  });

  it("renders a SETTLED review exactly like a recognized import (quiet card, AC3)", () => {
    render(
      <StoryCard
        story={{ ...STORY, importState: "resolved" }}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );
    // The provenance survives; the chip's disappearance IS the feedback —
    // no chip, no on-demand report, no success announcement.
    expect(screen.getByText("Importée")).toBeInTheDocument();
    expect(screen.queryByText("partiel")).not.toBeInTheDocument();
    expect(screen.queryByText("à revoir")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Voir le rapport d'import"),
    ).not.toBeInTheDocument();
    expect(document.querySelector("details")).toBeNull();
  });

  it("shows a dedicated 'à revoir' marker distinct from the transfer badge (AC2)", () => {
    render(
      <StoryCard
        story={{
          ...STORY,
          importState: "needsReview",
          importReport: [
            {
              aspect: "envelope",
              category: "recognized",
              message: "L'enveloppe de l'artefact est valide.",
            },
            {
              aspect: "title",
              category: "ambiguous",
              message: "Le titre a été normalisé à l'import.",
            },
          ],
        }}
        // The transfer/verification badge coexists and stays distinct.
        preparationBadge="partial"
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );
    expect(screen.getByText("Importée")).toBeInTheDocument();
    // The import marker uses its dedicated label, never the transfer
    // `état partiel` wording.
    expect(screen.getByText("à revoir")).toBeInTheDocument();
    expect(screen.getByText("état partiel")).toBeInTheDocument();
    expect(screen.queryByText("partiel", { exact: true })).not.toBeInTheDocument();
  });

  it("discloses the full on-demand import report (both groups) on request", async () => {
    const user = userEvent.setup();
    render(
      <StoryCard
        story={{
          ...STORY,
          importState: "needsReview",
          importReport: [
            {
              aspect: "envelope",
              category: "recognized",
              message: "L'enveloppe de l'artefact est valide.",
            },
            {
              aspect: "title",
              category: "ambiguous",
              message: "Le titre a été normalisé à l'import.",
            },
          ],
        }}
        isSelected={false}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
      />,
    );
    // Collapsed by default — the detail is behind a disclosure.
    const summary = screen.getByText("Voir le rapport d'import");
    await user.click(summary);
    // Both groups are restored from the durable report (§5).
    expect(screen.getByText("Ce que Rustory a reconnu")).toBeInTheDocument();
    expect(screen.getByText("L'enveloppe de l'artefact est valide.")).toBeInTheDocument();
    expect(screen.getByText("Points d'attention")).toBeInTheDocument();
    expect(
      screen.getByText("Le titre a été normalisé à l'import."),
    ).toBeInTheDocument();
  });
});
