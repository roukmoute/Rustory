import { beforeEach, describe, expect, it } from "vitest";

import { DEFAULT_LIBRARY_SORT, useLibraryShell } from "./library-shell-store";

function reset(): void {
  useLibraryShell.setState({
    selectedStoryIds: new Set(),
    query: "",
    sort: DEFAULT_LIBRARY_SORT,
  });
}

describe("libraryShell store", () => {
  beforeEach(() => {
    reset();
  });

  it("starts with an empty selection and default filters", () => {
    const s = useLibraryShell.getState();
    expect(s.selectedStoryIds.size).toBe(0);
    expect(s.query).toBe("");
    expect(s.sort).toBe(DEFAULT_LIBRARY_SORT);
  });

  it("replace collapses to a singleton", () => {
    useLibraryShell.getState().selectStory("a", "replace");
    expect([...useLibraryShell.getState().selectedStoryIds]).toEqual(["a"]);

    useLibraryShell.getState().selectStory("b", "replace");
    expect([...useLibraryShell.getState().selectedStoryIds]).toEqual(["b"]);
  });

  it("toggle adds then removes the id", () => {
    useLibraryShell.getState().selectStory("a", "toggle");
    expect([...useLibraryShell.getState().selectedStoryIds]).toEqual(["a"]);

    useLibraryShell.getState().selectStory("b", "toggle");
    expect(new Set(useLibraryShell.getState().selectedStoryIds)).toEqual(
      new Set(["a", "b"]),
    );

    useLibraryShell.getState().selectStory("a", "toggle");
    expect([...useLibraryShell.getState().selectedStoryIds]).toEqual(["b"]);
  });

  it("clearSelection empties the set", () => {
    useLibraryShell.getState().selectStory("a", "replace");
    useLibraryShell.getState().clearSelection();
    expect(useLibraryShell.getState().selectedStoryIds.size).toBe(0);
  });

  it("pruneSelection drops ids missing from the present set", () => {
    useLibraryShell.getState().selectStory("a", "replace");
    useLibraryShell.getState().selectStory("b", "toggle");
    useLibraryShell.getState().pruneSelection(new Set(["a"]));
    expect([...useLibraryShell.getState().selectedStoryIds]).toEqual(["a"]);
  });

  it("pruneSelection keeps the same reference when nothing is removed", () => {
    useLibraryShell.getState().selectStory("a", "replace");
    const before = useLibraryShell.getState().selectedStoryIds;
    useLibraryShell.getState().pruneSelection(new Set(["a", "b"]));
    expect(useLibraryShell.getState().selectedStoryIds).toBe(before);
  });

  it("every selection mutation produces a new Set reference", () => {
    const before = useLibraryShell.getState().selectedStoryIds;
    useLibraryShell.getState().selectStory("a", "toggle");
    const after = useLibraryShell.getState().selectedStoryIds;
    expect(after).not.toBe(before);
  });

  it("setQuery and setSort update the filters", () => {
    useLibraryShell.getState().setQuery("soleil");
    expect(useLibraryShell.getState().query).toBe("soleil");

    useLibraryShell.getState().setSort("titre-desc");
    expect(useLibraryShell.getState().sort).toBe("titre-desc");
  });

  it("resetFilters restores defaults and leaves selection untouched", () => {
    useLibraryShell.getState().selectStory("a", "replace");
    useLibraryShell.getState().setQuery("x");
    useLibraryShell.getState().setSort("titre-desc");
    useLibraryShell.getState().resetFilters();

    const s = useLibraryShell.getState();
    expect(s.query).toBe("");
    expect(s.sort).toBe(DEFAULT_LIBRARY_SORT);
    expect([...s.selectedStoryIds]).toEqual(["a"]);
  });
});
