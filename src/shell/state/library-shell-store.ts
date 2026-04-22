import { create } from "zustand";

import {
  DEFAULT_LIBRARY_SORT,
  type LibrarySortKey,
} from "../../shared/types/library-sort";

export type SelectionMode = "replace" | "toggle";

export interface LibraryShellState {
  selectedStoryIds: ReadonlySet<string>;
  query: string;
  sort: LibrarySortKey;
  selectStory: (id: string, mode: SelectionMode) => void;
  clearSelection: () => void;
  pruneSelection: (presentIds: ReadonlySet<string>) => void;
  setQuery: (query: string) => void;
  setSort: (sort: LibrarySortKey) => void;
  resetFilters: () => void;
}

const EMPTY_SELECTION: ReadonlySet<string> = new Set();

export { DEFAULT_LIBRARY_SORT };

/**
 * Library shell slice. Holds UI continuity only — selection, search query
 * and sort key that must survive navigation to the edit route and back.
 *
 * No persistence: a fresh app launch never restores a stale selection or an
 * invisible filter, per the architecture persistence rule.
 */
export const useLibraryShell = create<LibraryShellState>((set) => ({
  selectedStoryIds: EMPTY_SELECTION,
  query: "",
  sort: DEFAULT_LIBRARY_SORT,

  selectStory: (id, mode) =>
    set((state) => {
      if (mode === "replace") {
        return { selectedStoryIds: new Set([id]) };
      }
      const next = new Set(state.selectedStoryIds);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return { selectedStoryIds: next };
    }),

  clearSelection: () => set({ selectedStoryIds: new Set() }),

  pruneSelection: (presentIds) =>
    set((state) => {
      let removed = false;
      const next = new Set<string>();
      for (const id of state.selectedStoryIds) {
        if (presentIds.has(id)) {
          next.add(id);
        } else {
          removed = true;
        }
      }
      if (!removed) {
        return state;
      }
      return { selectedStoryIds: next };
    }),

  setQuery: (query) => set({ query }),
  setSort: (sort) => set({ sort }),
  resetFilters: () => set({ query: "", sort: DEFAULT_LIBRARY_SORT }),
}));
