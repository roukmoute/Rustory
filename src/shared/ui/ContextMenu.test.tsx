import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ContextMenu } from "./ContextMenu";

function renderMenu(overrides?: {
  onClose?: () => void;
  edit?: () => void;
  del?: () => void;
  delDisabled?: boolean;
}) {
  const onClose = overrides?.onClose ?? vi.fn();
  const edit = overrides?.edit ?? vi.fn();
  const del = overrides?.del ?? vi.fn();
  render(
    <ContextMenu
      x={100}
      y={100}
      ariaLabel="Actions pour Mon histoire"
      onClose={onClose}
      items={[
        { label: "Éditer", onSelect: edit },
        {
          label: "Supprimer",
          onSelect: del,
          danger: true,
          disabled: overrides?.delDisabled,
        },
      ]}
    />,
  );
  return { onClose, edit, del };
}

describe("<ContextMenu />", () => {
  it("renders the items as a labelled menu", () => {
    renderMenu();
    const menu = screen.getByRole("menu", { name: "Actions pour Mon histoire" });
    expect(menu).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Éditer" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Supprimer" }),
    ).toBeInTheDocument();
  });

  it("activates an item then closes", async () => {
    const user = userEvent.setup();
    const { onClose, del } = renderMenu();
    await user.click(screen.getByRole("menuitem", { name: "Supprimer" }));
    expect(del).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not activate a disabled item", async () => {
    const user = userEvent.setup();
    const { del } = renderMenu({ delDisabled: true });
    await user.click(screen.getByRole("menuitem", { name: "Supprimer" }));
    expect(del).not.toHaveBeenCalled();
  });

  it("closes on Escape without acting", async () => {
    const user = userEvent.setup();
    const { onClose, edit, del } = renderMenu();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(edit).not.toHaveBeenCalled();
    expect(del).not.toHaveBeenCalled();
  });

  it("closes on an outside pointer press", async () => {
    const user = userEvent.setup();
    const { onClose } = renderMenu();
    await user.pointer({ keys: "[MouseLeft>]", target: document.body });
    expect(onClose).toHaveBeenCalled();
  });

  it("navigates with the arrow keys and activates with Enter", async () => {
    const user = userEvent.setup();
    const { edit } = renderMenu();
    // First ArrowDown lands on the first enabled item, Enter activates it.
    await user.keyboard("{ArrowDown}{Enter}");
    expect(edit).toHaveBeenCalledTimes(1);
  });
});
