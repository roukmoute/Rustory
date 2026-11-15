import type { SupportedOperationsDto } from "../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../shared/ipc-contracts/device-library";
import type { DeviceStoryImportStatus } from "./hooks/use-device-story-import";

export type DeviceStoryCopyRefusal =
  | "alreadyImported"
  | "unsupportedProfile"
  | "incompleteContent"
  | "handlerUnavailable";

export interface DeviceStoryCopyAvailabilityInput {
  story: Pick<DeviceStoryDto, "alreadyImported" | "contentPresent">;
  supportedOperations?: Pick<SupportedOperationsDto, "importStory">;
  hasImportHandler: boolean;
  /** Only the status belonging to this story; an unrelated result is not its truth. */
  status?: DeviceStoryImportStatus;
}

export interface DeviceStoryCopyAvailability {
  enabled: boolean;
  /** Profile refusal and missing handler share a label but not a recovery action. */
  refusalKind: DeviceStoryCopyRefusal | null;
  /** A snapshot refusal only; progress and success have their own status surfaces. */
  reason: string | null;
  isImporting: boolean;
  isImported: boolean;
}

/** Shared presentation gate for the inspector and selection bar. Rust remains
 * authoritative. Refusal priority is deliberate: existing local copy, profile,
 * missing content, then unavailable handler. Hidden entries are not excluded. */
export function getDeviceStoryCopyAvailability({
  story,
  supportedOperations,
  hasImportHandler,
  status,
}: DeviceStoryCopyAvailabilityInput): DeviceStoryCopyAvailability {
  const refusalKind: DeviceStoryCopyRefusal | null = story.alreadyImported
    ? "alreadyImported"
    : supportedOperations?.importStory !== true
      ? "unsupportedProfile"
      : !story.contentPresent
        ? "incompleteContent"
        : !hasImportHandler
          ? "handlerUnavailable"
          : null;
  const isImporting = status?.kind === "importing";
  const isImported = status?.kind === "imported";
  // Keep a just-completed copy disabled until the inventory re-read marks it
  // alreadyImported, so a second click cannot turn success into a duplicate error.
  return {
    enabled: refusalKind === null && !isImporting && !isImported,
    refusalKind,
    reason: refusalKind === null ? null : formatCopyReason(refusalKind),
    isImporting,
    isImported,
  };
}

function formatCopyReason(kind: DeviceStoryCopyRefusal): string {
  switch (kind) {
    case "alreadyImported":
      return "Copie indisponible: déjà dans ta bibliothèque";
    case "incompleteContent":
      return "Copie indisponible: contenu incomplet sur l'appareil";
    case "unsupportedProfile":
    case "handlerUnavailable":
      return "Copie indisponible: profil non supporté";
  }
}
