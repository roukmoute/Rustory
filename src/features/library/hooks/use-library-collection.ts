import type { StoryCardDto } from "../../../shared/ipc-contracts/library";

export type LibrarySortKey = "titre-asc" | "titre-desc";

export interface LibraryProjectionInput {
  stories: StoryCardDto[];
  query: string;
  sort: LibrarySortKey;
}

const collator = new Intl.Collator("fr", {
  sensitivity: "base",
  usage: "sort",
});

/**
 * Pure projection: given the raw library overview and the current UI query /
 * sort, returns the visible subset. Case- and accent-insensitive substring
 * match on `title`, stable alphabetical sort.
 */
export function applyLibraryFilters({
  stories,
  query,
  sort,
}: LibraryProjectionInput): StoryCardDto[] {
  const normalizedQuery = normalize(query.trim());
  const matcher =
    normalizedQuery.length === 0
      ? () => true
      : (story: StoryCardDto) =>
          normalize(story.title).includes(normalizedQuery);

  const filtered = stories.filter(matcher);
  const sorted = [...filtered].sort((a, b) => {
    const raw = collator.compare(a.title, b.title);
    return sort === "titre-asc" ? raw : -raw;
  });
  return sorted;
}

function normalize(value: string): string {
  return value
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();
}
