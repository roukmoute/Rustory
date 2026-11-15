import { useId } from "react";
import type { DeviceBulkImportStatus } from "../../device/hooks/use-device-bulk-import";
import { Button, ProgressIndicator, SurfacePanel } from "../../../shared/ui";

export function RecoveryProgress({
  status,
  onDismiss,
}: {
  status: DeviceBulkImportStatus;
  onDismiss: () => void;
}) {
  const titleId = useId();
  if (status.kind === "idle") return null;
  return (
    <SurfacePanel
      as="section"
      elevation={0}
      className="library-route__recovery"
      ariaLabelledBy={titleId}
    >
      {status.kind === "running" ? (
        <>
          <h2 id={titleId}>Récupération de tes histoires</h2>
          <ProgressIndicator
            mode="determinate"
            value={status.total === 0 ? 0 : (status.done * 100) / status.total}
            label={`${status.done} sur ${status.total} histoires traitées`}
          />
          <p>
            Elles restent sur ton appareil. Ne le débranche pas pendant la
            copie.
          </p>
        </>
      ) : (
        <>
          <h2 id={titleId}>
            {status.failed === 0
              ? "Tes histoires ont rejoint la bibliothèque"
              : "La récupération est terminée"}
          </h2>
          <p role="status">
            {status.succeeded} histoire{status.succeeded > 1 ? "s" : ""} copiée
            {status.succeeded > 1 ? "s" : ""}
            {status.failed > 0
              ? ` · ${status.failed} copie${status.failed > 1 ? "s" : ""} en échec`
              : ""}
            .
          </p>
          {status.firstError && (
            <p role="alert">
              {status.firstError.message} {status.firstError.userAction}
            </p>
          )}
          <Button variant="quiet" onClick={onDismiss}>
            Fermer le résultat
          </Button>
        </>
      )}
    </SurfacePanel>
  );
}
