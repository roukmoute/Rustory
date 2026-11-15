import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ProgressIndicator } from "../../../shared/ui";
import {
  SelectionActionBar,
  type SelectionActionBarProps,
} from "./SelectionActionBar";

function props(
  overrides: Partial<SelectionActionBarProps> = {},
): SelectionActionBarProps {
  return {
    selectedCount: 1,
    primaryAction: { label: "Ajouter à la Lunii", onClick: vi.fn() },
    onDelete: vi.fn(),
    onClearSelection: vi.fn(),
    ...overrides,
  };
}

describe("SelectionActionBar", () => {
  it("only exists for a selection and announces the real count", () => {
    const initial = props({ selectedCount: 0 });
    const { rerender } = render(<SelectionActionBar {...initial} />);
    expect(
      screen.queryByRole("region", { name: "Actions pour la sélection" }),
    ).not.toBeInTheDocument();
    rerender(<SelectionActionBar {...initial} selectedCount={1} />);
    expect(screen.getByRole("status")).toHaveTextContent("1 sélectionnée");
    rerender(<SelectionActionBar {...initial} selectedCount={12} />);
    expect(screen.getByRole("status")).toHaveTextContent("12 sélectionnées");
  });

  it("provides named keyboard actions without stealing focus from the selected card", async () => {
    const user = userEvent.setup();
    const initial = props({ onDetails: vi.fn() });
    const { rerender } = render(
      <>
        <button>Une histoire</button>
        <SelectionActionBar {...initial} selectedCount={0} />
      </>,
    );
    const card = screen.getByRole("button", { name: "Une histoire" });
    card.focus();
    rerender(
      <>
        <button>Une histoire</button>
        <SelectionActionBar {...initial} />
      </>,
    );
    expect(card).toHaveFocus();
    await user.tab();
    expect(
      screen.getByRole("button", { name: "Ajouter à la Lunii" }),
    ).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(initial.primaryAction!.onClick).toHaveBeenCalledOnce();
    await user.tab();
    await user.keyboard(" ");
    expect(initial.onDetails).toHaveBeenCalledOnce();
    await user.tab();
    expect(
      screen.getByRole("button", { name: "Supprimer la sélection" }),
    ).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(initial.onDelete).toHaveBeenCalledOnce();
    await user.tab();
    expect(
      screen.getByRole("button", { name: "Effacer la sélection" }),
    ).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(initial.onClearSelection).toHaveBeenCalledOnce();
  });

  it("explains disabled actions visibly and keeps their reasons reachable by keyboard", async () => {
    const user = userEvent.setup();
    const onPrimary = vi.fn();
    const initial = props({
      primaryAction: {
        label: "Copier dans ma bibliothèque",
        icon: "download",
        onClick: onPrimary,
        disabled: true,
        disabledReason: "La copie n'est pas disponible pour cet appareil.",
      },
      deleteDisabled: true,
      deleteDisabledReason:
        "La suppression n'est pas disponible pour cet appareil.",
      clearDisabled: true,
      clearDisabledReason:
        "La sélection est utilisée par une opération en cours.",
    });
    render(<SelectionActionBar {...initial} />);
    const primary = screen.getByRole("button", {
      name: "Copier dans ma bibliothèque",
    });
    expect(primary).toHaveAccessibleDescription(
      initial.primaryAction!.disabledReason,
    );
    expect(primary).not.toBeDisabled();
    await user.tab();
    expect(primary).toHaveFocus();
    await user.keyboard("{Enter}");
    const remove = screen.getByRole("button", {
      name: "Supprimer la sélection",
    });
    expect(remove).toHaveAccessibleDescription(initial.deleteDisabledReason);
    fireEvent.click(remove);
    const clear = screen.getByRole("button", { name: "Effacer la sélection" });
    expect(clear).toHaveAccessibleDescription(initial.clearDisabledReason);
    fireEvent.click(clear);
    expect(onPrimary).not.toHaveBeenCalled();
    expect(initial.onDelete).not.toHaveBeenCalled();
    expect(initial.onClearSelection).not.toHaveBeenCalled();
  });

  it("locks mutations while busy, exposes real progress, and keeps details available", () => {
    const initial = props({
      busy: true,
      busyLabel: "Copie en cours…",
      onDetails: vi.fn(),
      progress: (
        <ProgressIndicator
          mode="determinate"
          label="2 sur 3 histoires traitées"
          value={(2 * 100) / 3}
        />
      ),
    });
    render(<SelectionActionBar {...initial} />);
    for (const name of [
      "Ajouter à la Lunii",
      "Supprimer la sélection",
      "Effacer la sélection",
    ]) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("aria-disabled", "true");
      expect(button).toHaveAccessibleDescription(/Copie en cours/);
      fireEvent.click(button);
    }
    expect(initial.primaryAction!.onClick).not.toHaveBeenCalled();
    expect(initial.onDelete).not.toHaveBeenCalled();
    expect(initial.onClearSelection).not.toHaveBeenCalled();
    expect(
      screen.getByRole("progressbar", { name: "2 sur 3 histoires traitées" }),
    ).toHaveAttribute("aria-valuenow", String((2 * 100) / 3));
    fireEvent.click(screen.getByRole("button", { name: "Détails" }));
    expect(initial.onDetails).toHaveBeenCalledOnce();
  });

  it("omits unavailable optional actions instead of inventing callbacks", () => {
    render(
      <SelectionActionBar
        {...props({ primaryAction: undefined, onDelete: undefined })}
      />,
    );
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "Effacer la sélection" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Détails" }),
    ).not.toBeInTheDocument();
  });
});
