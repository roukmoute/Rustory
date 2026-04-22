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
});
