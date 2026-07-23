import type React from "react";
import { useId } from "react";

import { Button, ProgressIndicator, StateChip, SurfacePanel } from "../../../shared/ui";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import type { DeviceBulkImportStatus } from "../hooks/use-device-bulk-import";

import "./DeviceBulkImportPanel.css";

export interface DeviceBulkImportPanelProps {
  /** The device stories currently multi-selected (2 or more). */
  stories: DeviceStoryDto[];
  /** Whether the connected device's profile positively allows importing —
   *  the same capability gate the single inspector honors. When false, the
   *  whole selection is un-importable and the panel says so plainly. */
  canImport: boolean;
  /** Progress/outcome of an in-flight or just-finished batch. */
  status: DeviceBulkImportStatus;
  /** Start a batch copy of exactly these importable pack UUIDs. */
  onImport: (packUuids: string[]) => void;
  /** Clear the whole device selection. */
  onClearSelection: () => void;
  /** Dismiss the terminal batch summary back to idle. */
  onDismissStatus: () => void;
  /** Open the support-profile screen (context for a profile that blocks
   *  import). Hidden when the route wires no handler. */
  onConsultSupportProfile?: () => void;
}

/**
 * Bulk surface for a MULTI-selection of device stories (shown in place of the
 * single-story inspector when 2+ are selected). It triages the selection with
 * the SAME honesty as the single inspector — an importable pack needs a
 * present payload, must not already be in the library, and the device profile
 * must allow the copy — then offers one batch action over exactly the
 * importable subset. Packs that can't be copied are counted, never silently
 * dropped, so the tally always adds up for the user.
 */
export function DeviceBulkImportPanel({
  stories,
  canImport,
  status,
  onImport,
  onClearSelection,
  onDismissStatus,
  onConsultSupportProfile,
}: DeviceBulkImportPanelProps): React.JSX.Element {
  const titleId = useId();

  const importable = canImport
    ? stories.filter((s) => s.contentPresent && !s.alreadyImported)
    : [];
  const alreadyCount = stories.filter((s) => s.alreadyImported).length;
  const incompleteCount = stories.filter((s) => !s.contentPresent).length;

  const running = status.kind === "running";
  const total = stories.length;

  const handleImport = (): void => {
    if (importable.length === 0 || running) return;
    onImport(importable.map((s) => s.uuid));
  };

  return (
    <SurfacePanel
      elevation={1}
      as="section"
      ariaLabelledBy={titleId}
      className="device-bulk"
    >
      <h2 id={titleId} className="device-bulk__title">
        {total} histoires sélectionnées
      </h2>

      <div className="device-bulk__provenance">
        <StateChip tone="info" label="Sur l'appareil" />
      </div>

      {/* Honest triage of the selection: how many can actually be copied, and
          why the rest can't. Each line is a single string so the tally reads
          as one phrase. */}
      <ul className="device-bulk__triage">
        <li className="device-bulk__triage-importable">
          {`${importable.length} ${
            importable.length > 1 ? "importables" : "importable"
          }`}
        </li>
        {alreadyCount > 0 ? (
          <li className="device-bulk__triage-skip">
            {`${alreadyCount} déjà dans ta bibliothèque`}
          </li>
        ) : null}
        {incompleteCount > 0 ? (
          <li className="device-bulk__triage-skip">
            {`${incompleteCount} au contenu incomplet`}
          </li>
        ) : null}
      </ul>

      {!canImport ? (
        <p className="device-bulk__note">
          L'import n'est pas disponible pour le profil de cet appareil.
          {onConsultSupportProfile ? (
            <>
              {" "}
              <Button variant="quiet" onClick={onConsultSupportProfile}>
                Consulter le profil de support
              </Button>
            </>
          ) : null}
        </p>
      ) : importable.length === 0 ? (
        <p className="device-bulk__note">
          Aucune des histoires sélectionnées n'est importable pour le moment.
        </p>
      ) : null}

      {/* Live region: progress while running, tally when done. Mounted so a
          transition is announced, never inserted already-filled. */}
      <div className="device-bulk__status" role="status" aria-live="polite">
        {running ? (
          <ProgressIndicator
            mode="determinate"
            value={Math.round((status.done / status.total) * 100)}
            label={`Import en cours… ${status.done} / ${status.total}`}
          />
        ) : status.kind === "done" ? (
          <div className="device-bulk__summary">
            <StateChip
              tone={status.failed > 0 ? "warning" : "success"}
              label={
                status.failed > 0
                  ? `${status.succeeded} importée${
                      status.succeeded > 1 ? "s" : ""
                    }, ${status.failed} en échec`
                  : `${status.succeeded} importée${
                      status.succeeded > 1 ? "s" : ""
                    }`
              }
            />
            {status.firstError ? (
              <p className="device-bulk__summary-error">
                {status.firstError.message}
                {status.firstError.userAction
                  ? ` ${status.firstError.userAction}`
                  : ""}
              </p>
            ) : null}
            <Button variant="quiet" onClick={onDismissStatus}>
              Fermer
            </Button>
          </div>
        ) : null}
      </div>

      {!running && status.kind !== "done" ? (
        <div className="device-bulk__actions">
          <Button
            variant="primary"
            onClick={handleImport}
            disabled={importable.length === 0}
          >
            {importable.length > 1
              ? `Importer les ${importable.length} histoires`
              : "Importer l'histoire"}
          </Button>
          <Button variant="quiet" onClick={onClearSelection}>
            Effacer la sélection
          </Button>
        </div>
      ) : null}
    </SurfacePanel>
  );
}
