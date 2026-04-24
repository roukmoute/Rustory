import { describe, expect, it } from "vitest";

import {
  MAX_STORY_TITLE_CHARS,
  normalizeStoryTitle,
  reasonFor,
  validateStoryTitle,
} from "./story-title";

describe("normalizeStoryTitle", () => {
  it("trims whitespace and composes NFC", () => {
    expect(normalizeStoryTitle("  Café  ")).toBe("Café");
    expect(normalizeStoryTitle("cafe\u{0301}")).toBe("café");
    expect(normalizeStoryTitle("")).toBe("");
  });
});

describe("validateStoryTitle", () => {
  it("accepts a reasonable title", () => {
    expect(validateStoryTitle("Un titre")).toBeNull();
  });

  it("rejects empty or whitespace-only titles (caller must normalize first)", () => {
    expect(validateStoryTitle("")).toBe("empty");
  });

  it("accepts a title with exactly MAX_STORY_TITLE_CHARS code points", () => {
    const title = "a".repeat(MAX_STORY_TITLE_CHARS);
    expect(validateStoryTitle(title)).toBeNull();
  });

  it("rejects a title with one code point above the limit", () => {
    const title = "a".repeat(MAX_STORY_TITLE_CHARS + 1);
    expect(validateStoryTitle(title)).toBe("too-long");
  });

  it("counts code points, not UTF-16 code units", () => {
    // An emoji uses two UTF-16 code units but one code point — the count
    // must stay below the boundary for emoji-heavy titles the same way
    // the Rust side counts chars.
    const emojiTitle = "💫".repeat(MAX_STORY_TITLE_CHARS);
    expect(validateStoryTitle(emojiTitle)).toBeNull();
    const tooMany = "💫".repeat(MAX_STORY_TITLE_CHARS + 1);
    expect(validateStoryTitle(tooMany)).toBe("too-long");
  });

  it("rejects C0 control characters", () => {
    expect(validateStoryTitle("a\nb")).toBe("control-chars");
    expect(validateStoryTitle("a\tb")).toBe("control-chars");
    expect(validateStoryTitle("a\0b")).toBe("control-chars");
  });

  it("rejects C1 control characters", () => {
    expect(validateStoryTitle("a\u{007f}b")).toBe("control-chars");
    expect(validateStoryTitle("a\u{0085}b")).toBe("control-chars");
  });

  it("accepts unicode letters, punctuation, emoji", () => {
    expect(
      validateStoryTitle("Aventure ①: 💫 été — L'île mystérieuse"),
    ).toBeNull();
  });

  it("rejects byte-order mark (U+FEFF)", () => {
    expect(validateStoryTitle("Titre\u{FEFF}caché")).toBe("control-chars");
  });

  it("rejects RTL overrides and bidi isolates", () => {
    for (const cp of [
      0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068, 0x2069,
    ]) {
      expect(validateStoryTitle(`a${String.fromCodePoint(cp)}b`)).toBe(
        "control-chars",
      );
    }
  });

  it("rejects directional marks (LRM/RLM/ALM)", () => {
    for (const cp of [0x200e, 0x200f, 0x061c]) {
      expect(validateStoryTitle(`a${String.fromCodePoint(cp)}b`)).toBe(
        "control-chars",
      );
    }
  });

  it("rejects line (U+2028) and paragraph (U+2029) separators", () => {
    expect(validateStoryTitle("ligne1\u{2028}ligne2")).toBe("control-chars");
    expect(validateStoryTitle("para1\u{2029}para2")).toBe("control-chars");
  });

  it("allows ZWJ (U+200D) and ZWNJ (U+200C) — legitimate formatting", () => {
    expect(validateStoryTitle("a\u{200C}b")).toBeNull();
    expect(validateStoryTitle("a\u{200D}b")).toBeNull();
  });
});

describe("reasonFor", () => {
  it("returns the canonical reason strings", () => {
    expect(reasonFor("empty")).toBe("Création impossible: titre requis");
    expect(reasonFor("too-long", { charCount: 125 })).toBe(
      "Création impossible: titre trop long (120 caractères maximum, 5 en trop)",
    );
    // Fallback when charCount is omitted: the reason still states at least
    // "1 en trop" so the wording never lies about the overage.
    expect(reasonFor("too-long")).toBe(
      "Création impossible: titre trop long (120 caractères maximum, 1 en trop)",
    );
    expect(reasonFor("control-chars")).toBe(
      "Création impossible: titre contient des caractères non autorisés",
    );
  });
});
