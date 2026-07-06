import type { StateChipTone } from "../../../shared/ui/StateChip";
import type {
  ImportCategory,
  ImportQuality,
} from "../../../shared/ipc-contracts/import-export";

/**
 * Shared recognition-report label helpers, kept in sync with
 * `product-language.md`. The `.rustory` import surface and the
 * structured-folder creation surface reuse THESE (one copy per label) —
 * never each other's orchestrator (an import-review flow is a context
 * orchestrator, not a generic component).
 */

/** Quality label (`Propre` / `Partiellement exploitable` / `Inexploitable`). */
export function qualityLabel(quality: ImportQuality): string {
  switch (quality) {
    case "clean":
      return "Propre";
    case "partial":
      return "Partiellement exploitable";
    case "unusable":
      return "Inexploitable";
  }
}

export function qualityTone(quality: ImportQuality): StateChipTone {
  switch (quality) {
    case "clean":
      return "success";
    case "partial":
      return "warning";
    case "unusable":
      return "error";
  }
}

/** Per-finding category label (`reconnu` / `ambiguïté` / `information
 *  manquante` / `blocage réel`) — never color-only (the StateChip ships a
 *  glyph). */
export function categoryLabel(category: ImportCategory): string {
  switch (category) {
    case "recognized":
      return "reconnu";
    case "ambiguous":
      return "ambiguïté";
    case "missing":
      return "information manquante";
    case "blocking":
      return "blocage réel";
  }
}

export function categoryTone(category: ImportCategory): StateChipTone {
  switch (category) {
    case "recognized":
      return "success";
    case "ambiguous":
      return "warning";
    case "missing":
      return "info";
    case "blocking":
      return "error";
  }
}
