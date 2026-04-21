import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Button } from "./Button";

describe("<Button />", () => {
  it("renders children and fires onClick", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Envoyer</Button>);

    await user.click(screen.getByRole("button", { name: /envoyer/i }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("defaults to type=button to avoid accidental form submissions", () => {
    render(<Button>Envoyer</Button>);
    expect(screen.getByRole("button")).toHaveAttribute("type", "button");
  });

  it("stays focusable when aria-disabled and blocks onClick without native disabled", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      <Button aria-disabled="true" onClick={onClick}>
        Envoyer
      </Button>,
    );
    const btn = screen.getByRole("button");

    // Native `disabled` would strip the button from the tab order and hide any
    // `aria-describedby` reason from assistive tech — we rely on aria-disabled.
    expect(btn).not.toBeDisabled();
    expect(btn).toHaveAttribute("aria-disabled", "true");

    await user.click(btn);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("applies the variant class", () => {
    render(<Button variant="destructive">Supprimer</Button>);
    expect(screen.getByRole("button")).toHaveClass("ds-button--destructive");
  });
});
