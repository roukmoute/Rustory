import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { flushSync } from "react-dom";
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
  it("returns focus to the keyboard trigger after Escape", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return <><button onClick={() => setOpen(true)}>Actions de l’histoire</button>{open ? <ContextMenu x={100} y={100} ariaLabel="Actions" onClose={() => setOpen(false)} items={[{label: "Éditer", onSelect: () => undefined}]} /> : null}</>;
    }
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Actions de l’histoire" });
    await user.click(trigger);
    expect(screen.getByRole("menu")).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

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

  /** A surface that opens the menu from its own right-click, committing
   *  the menu WITHIN the event — what React does in the app for a discrete
   *  event, effects included. */
  function RightClickHarness() {
    const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
    return (
      <div>
        <div
          data-testid="card"
          onContextMenu={(event) => {
            event.preventDefault();
            const { clientX, clientY } = event;
            flushSync(() => setMenu({ x: clientX, y: clientY }));
          }}
        >
          Carte
        </div>
        <p data-testid="elsewhere">Ailleurs</p>
        {menu ? (
          <ContextMenu
            x={menu.x}
            y={menu.y}
            ariaLabel="Actions"
            onClose={() => setMenu(null)}
            items={[{ label: "Éditer", onSelect: () => undefined }]}
          />
        ) : null}
      </div>
    );
  }

  it("stays open on the very right-click that opens it", () => {
    // The opening `contextmenu` is still bubbling toward `document` when
    // the menu mounts: it must not be taken for an outside gesture.
    render(<RightClickHarness />);
    fireEvent.contextMenu(screen.getByTestId("card"));
    expect(screen.getByRole("menu", { name: "Actions" })).toBeInTheDocument();
  });

  it("still closes on a later right-click or press elsewhere", async () => {
    render(<RightClickHarness />);
    fireEvent.contextMenu(screen.getByTestId("card"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    // Once the opening gesture is over, an outside right-click closes it…
    await waitFor(() => {
      fireEvent.contextMenu(screen.getByTestId("elsewhere"));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    });
    // …and so does an outside press after it was opened again.
    fireEvent.contextMenu(screen.getByTestId("card"));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await waitFor(() => {
      fireEvent.mouseDown(screen.getByTestId("elsewhere"));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    });
  });

  it("navigates with the arrow keys and activates with Enter", async () => {
    const user = userEvent.setup();
    const { edit } = renderMenu();
    // First ArrowDown lands on the first enabled item, Enter activates it.
    await user.keyboard("{ArrowDown}{Enter}");
    expect(edit).toHaveBeenCalledTimes(1);
  });
});
