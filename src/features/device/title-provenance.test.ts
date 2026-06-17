import { describe, expect, it } from "vitest";

import {
  titleProvenanceChip,
  titleProvenancePhrase,
} from "./title-provenance";

describe("titleProvenanceChip", () => {
  it("labels the official catalog distinctly with the info tone", () => {
    expect(titleProvenanceChip("official")).toEqual({
      tone: "info",
      label: "Titre officiel",
    });
  });

  it("never reuses the 'officiel' wording for user or community titles (honesty)", () => {
    const user = titleProvenanceChip("user");
    const unofficial = titleProvenanceChip("unofficial");
    expect(user.label).toBe("Titre saisi");
    expect(unofficial.label).toBe("Titre non-officiel");
    // Only the official badge gets the info tone.
    expect(user.tone).toBe("neutral");
    expect(unofficial.tone).toBe("neutral");
  });
});

describe("titleProvenancePhrase", () => {
  it("returns a lowercase phrase for each provenance", () => {
    expect(titleProvenancePhrase("user")).toBe("titre saisi");
    expect(titleProvenancePhrase("official")).toBe("titre officiel");
    expect(titleProvenancePhrase("unofficial")).toBe("titre non-officiel");
  });
});
