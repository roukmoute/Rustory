import type React from "react";
import { useId } from "react";

import { Button, Icon, type IconName } from "../../../shared/ui";

import "./SelectionActionBar.css";

export interface SelectionBarAction {
  label: string;
  onClick: () => void;
  icon?: IconName;
  disabled?: boolean;
  disabledReason?: string;
}

export interface SelectionActionBarProps {
  selectedCount: number;
  primaryAction?: SelectionBarAction;
  onDelete?: () => void;
  deleteLabel?: string;
  deleteDisabled?: boolean;
  deleteDisabledReason?: string;
  onClearSelection: () => void;
  clearDisabled?: boolean;
  clearDisabledReason?: string;
  onDetails?: () => void;
  /** While an operation runs, keep its selection and mutation actions stable.
   * Details remain reachable so the user can inspect the live operation. */
  busy?: boolean;
  busyLabel?: string;
  /** Real progress or outcome supplied by the owner; never a simulated timer. */
  progress?: React.ReactNode;
}

/** The collection layout owns anchoring and reserves the bar's measured height
 * so final cards remain reachable. This component only renders the pill and
 * never takes focus on selection; the owner restores focus when clearing a
 * selection removes the bar. */
export function SelectionActionBar({
  selectedCount,
  primaryAction,
  onDelete,
  deleteLabel = "Supprimer la sélection",
  deleteDisabled = false,
  deleteDisabledReason,
  onClearSelection,
  clearDisabled = false,
  clearDisabledReason,
  onDetails,
  busy = false,
  busyLabel = "Opération en cours…",
  progress,
}: SelectionActionBarProps): React.JSX.Element | null {
  const primaryReasonId = useId();
  const deleteReasonId = useId();
  const clearReasonId = useId();
  const progressId = useId();

  if (selectedCount <= 0) return null;

  const primaryReason = primaryAction?.disabled
    ? primaryAction.disabledReason
    : undefined;
  const deleteReason =
    onDelete && deleteDisabled ? deleteDisabledReason : undefined;
  const clearReason = clearDisabled ? clearDisabledReason : undefined;
  const hasProgress =
    progress !== undefined && progress !== null && progress !== false;
  const describe = (reasonId?: string): string | undefined =>
    [reasonId, busy ? progressId : undefined].filter(Boolean).join(" ") ||
    undefined;

  return (
    <section
      className="selection-action-bar"
      aria-label="Actions pour la sélection"
    >
      <div className="selection-action-bar__row">
        <p
          className="selection-action-bar__count"
          role="status"
          aria-atomic="true"
        >
          {selectedCount} sélectionnée{selectedCount > 1 ? "s" : ""}
        </p>
        {primaryAction && (
          <Button
            variant="primary"
            className="selection-action-bar__primary"
            onClick={primaryAction.onClick}
            aria-disabled={busy || primaryAction.disabled || undefined}
            aria-busy={busy || undefined}
            aria-describedby={describe(
              primaryReason ? primaryReasonId : undefined,
            )}
          >
            <Icon name={primaryAction.icon ?? "upload"} size={17} />
            <span>{primaryAction.label}</span>
          </Button>
        )}
        {onDetails && (
          <Button
            variant="quiet"
            className="selection-action-bar__details"
            onClick={onDetails}
          >
            Détails
          </Button>
        )}
        {onDelete && (
          <Button
            variant="quiet"
            className="selection-action-bar__icon selection-action-bar__delete"
            onClick={onDelete}
            aria-label={deleteLabel}
            title={deleteLabel}
            aria-disabled={busy || deleteDisabled || undefined}
            aria-describedby={describe(
              deleteReason ? deleteReasonId : undefined,
            )}
          >
            <Icon name="trash" size={18} />
          </Button>
        )}
        <Button
          variant="quiet"
          className="selection-action-bar__icon"
          onClick={onClearSelection}
          aria-label="Effacer la sélection"
          title="Effacer la sélection"
          aria-disabled={busy || clearDisabled || undefined}
          aria-describedby={describe(clearReason ? clearReasonId : undefined)}
        >
          <Icon name="close" size={18} />
        </Button>
      </div>
      {(primaryReason ||
        deleteReason ||
        clearReason ||
        busy ||
        hasProgress) && (
        <div className="selection-action-bar__feedback">
          {primaryReason && <p id={primaryReasonId}>{primaryReason}</p>}
          {deleteReason && <p id={deleteReasonId}>{deleteReason}</p>}
          {clearReason && <p id={clearReasonId}>{clearReason}</p>}
          {(busy || hasProgress) && (
            <div
              id={progressId}
              className="selection-action-bar__progress"
              role="status"
              aria-atomic="true"
            >
              {busy && <p>{busyLabel}</p>}
              {progress}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
