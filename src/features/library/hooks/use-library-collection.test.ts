import { describe, expect, it } from "vitest";

import { applyLibraryFilters } from "./use-library-collection";

const stories = [
  { id: "s1", title: "Le soleil d'Éloi" },
  { id: "s2", title: "La lune des chats" },
  { id: "s3", title: "étoile filante" },
];

describe("applyLibraryFilters", () => {
  it("returns all stories sorted ascending when the query is empty", () => {
    const out = applyLibraryFilters({
      stories,
      query: "",
      sort: "titre-asc",
    });
    expect(out.map((s) => s.id)).toEqual(["s3", "s2", "s1"]);
  });

  it("returns all stories sorted descending when sort=titre-desc", () => {
    const out = applyLibraryFilters({
      stories,
      query: "",
      sort: "titre-desc",
    });
    expect(out.map((s) => s.id)).toEqual(["s1", "s2", "s3"]);
  });

  it("matches case- and accent-insensitively", () => {
    const out = applyLibraryFilters({
      stories,
      query: "eloi",
      sort: "titre-asc",
    });
    expect(out.map((s) => s.id)).toEqual(["s1"]);
  });

  it("returns an empty list when nothing matches", () => {
    const out = applyLibraryFilters({
      stories,
      query: "xyz-nope",
      sort: "titre-asc",
    });
    expect(out).toEqual([]);
  });

  it("does not match a query that is longer than — but prefixes — a title", () => {
    // Regression guard: "Le soleil" must NOT match a user query like
    // "Le soleil d'Éloi et la galaxie"; a substring matcher would (correctly)
    // return empty, but the previous collator-based shortcut could leak.
    const out = applyLibraryFilters({
      stories: [{ id: "s1", title: "Le soleil" }],
      query: "Le soleil d'Éloi",
      sort: "titre-asc",
    });
    expect(out).toEqual([]);
  });

  it("does not mutate the input array", () => {
    const snapshot = stories.map((s) => s.id);
    applyLibraryFilters({ stories, query: "e", sort: "titre-desc" });
    expect(stories.map((s) => s.id)).toEqual(snapshot);
  });
});
