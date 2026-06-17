import type { PackTitleSource } from "../../shared/ipc-contracts/device-library";
import type { StateChipTone } from "../../shared/ui";

/** Visual provenance badge for a recognized title. */
export interface TitleProvenanceChip {
  tone: StateChipTone;
  label: string;
}

/**
 * Map a title's provenance to its badge. Honesty rule (UX): the official
 * catalog is the only "officiel" wording; a user-typed or community title
 * is labelled distinctly so it is NEVER mistaken for official. Only the
 * official badge gets the `info` tone; the others stay neutral.
 */
export function titleProvenanceChip(
  source: PackTitleSource,
): TitleProvenanceChip {
  switch (source) {
    case "user":
      return { tone: "neutral", label: "Titre saisi" };
    case "official":
      return { tone: "info", label: "Titre officiel" };
    case "unofficial":
      return { tone: "neutral", label: "Titre non-officiel" };
  }
}

/** Lowercase phrase woven into accessible names / inspector prose so a
 *  screen-reader user hears the provenance, not just sees the chip. */
export function titleProvenancePhrase(source: PackTitleSource): string {
  switch (source) {
    case "user":
      return "titre saisi";
    case "official":
      return "titre officiel";
    case "unofficial":
      return "titre non-officiel";
  }
}
