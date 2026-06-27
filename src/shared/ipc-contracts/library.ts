/**
 * Wire contract for the `get_library_overview` Tauri command.
 *
 * This file is the frontend mirror of `src-tauri/src/ipc/dto/library.rs`.
 * Any change here MUST match the Rust side — contract tests on both sides
 * validate the shape.
 */
import {
  isImportFinding,
  type ImportFinding,
  type ImportState,
} from "./import-export";

export interface StoryCardDto {
  id: string;
  title: string;
  /** Present iff the story came from a local artifact import. Drives the
   *  `Importée` origin marker (any value) and the `Import Issue Marker`
   *  chip (`partial` / `needsReview`). Absent on native / device-copied
   *  stories, which keep the bare `{ id, title }` shape. */
  importState?: ImportState;
  /** The FULL per-aspect report (recognized elements + points of attention)
   *  backing the on-demand `Import Review Flow`. Present only for a
   *  `partial` / `needsReview` import. */
  importReport?: ImportFinding[];
}

const CARD_IMPORT_STATES: ReadonlySet<string> = new Set([
  "recognized",
  "partial",
  "needsReview",
]);

export interface LibraryOverviewDto {
  stories: StoryCardDto[];
}

export function isStoryCardDto(value: unknown): value is StoryCardDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.id !== "string" || typeof candidate.title !== "string") {
    return false;
  }
  // An empty id collapses under `key={id}` in React lists and cannot be
  // used as a lookup key in pruneSelection or /story/:storyId/edit routing.
  if (candidate.id.length === 0) return false;
  // A blank title produces a card with no accessible name (StoryCard uses
  // `aria-label={title}`); refuse the payload instead of rendering a
  // focusable button keyboard users cannot identify.
  if (candidate.title.trim().length === 0) return false;
  // Optional import provenance: when present it must be a card-persistable
  // state (`recognized` / `partial` / `needsReview`) and, if issues are
  // attached, a well-formed finding list — a drift never reaches the marker.
  if (candidate.importState !== undefined) {
    if (
      typeof candidate.importState !== "string" ||
      !CARD_IMPORT_STATES.has(candidate.importState)
    ) {
      return false;
    }
  }
  if (candidate.importReport !== undefined) {
    if (
      !Array.isArray(candidate.importReport) ||
      !candidate.importReport.every(isImportFinding)
    ) {
      return false;
    }
  }
  return true;
}

/**
 * Runtime guard against a malformed IPC payload. Rust is authoritative, but
 * we still refuse to trust a wire shape that drifts from the contract — the
 * UI must never render against an arbitrary object.
 *
 * In addition to shape validation, duplicate ids are rejected: they would
 * silently collide on `key={id}` in React lists and make selection /
 * routing ambiguous.
 */
export function isLibraryOverviewDto(
  value: unknown,
): value is LibraryOverviewDto {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  if (!Array.isArray(candidate.stories)) return false;
  const seen = new Set<string>();
  for (const entry of candidate.stories) {
    if (!isStoryCardDto(entry)) return false;
    if (seen.has(entry.id)) return false;
    seen.add(entry.id);
  }
  return true;
}
