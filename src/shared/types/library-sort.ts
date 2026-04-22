/**
 * Sort keys available for the local library collection. Kept in `shared/`
 * so both the shell store and the feature-level hook can depend on it
 * without violating the dependency direction `features/ → shared/` and
 * `shell/ → shared/`.
 */
export type LibrarySortKey = "titre-asc" | "titre-desc";

export const LIBRARY_SORT_VALUES = new Set<LibrarySortKey>([
  "titre-asc",
  "titre-desc",
]);

export const DEFAULT_LIBRARY_SORT: LibrarySortKey = "titre-asc";

export function isLibrarySortKey(value: string): value is LibrarySortKey {
  return (LIBRARY_SORT_VALUES as Set<string>).has(value);
}
