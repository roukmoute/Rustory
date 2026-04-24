// Ergonomic mirror of `src-tauri/src/domain/story/validation.rs`. The Rust
// side stays authoritative: this file only exists to wire `aria-disabled`
// and the visible reason text at interactive speed. A bug that breaks the
// equivalence will be caught by the Rust validation refusing the payload.

export const MAX_STORY_TITLE_CHARS = 120;

export type StoryTitleIssue = "empty" | "too-long" | "control-chars";

export function normalizeStoryTitle(raw: string): string {
  return raw.normalize("NFC").trim();
}

export function validateStoryTitle(normalized: string): StoryTitleIssue | null {
  if (normalized.length === 0) return "empty";
  // String.length counts UTF-16 code units. To match Rust's char count we
  // iterate on code points via Array.from (each entry is one scalar value).
  if (Array.from(normalized).length > MAX_STORY_TITLE_CHARS) return "too-long";
  for (const ch of normalized) {
    const code = ch.codePointAt(0);
    if (code === undefined) continue;
    // C0 control (U+0000–U+001F) and C1 control (U+007F–U+009F) match
    // Rust's `char::is_control` for the ranges we care about.
    if ((code >= 0x0 && code <= 0x1f) || (code >= 0x7f && code <= 0x9f)) {
      return "control-chars";
    }
    if (isDeniedFormattingCodePoint(code)) {
      return "control-chars";
    }
  }
  return null;
}

/**
 * Targeted denylist mirroring `is_denied_formatting_char` in
 * `src-tauri/src/domain/story/validation.rs`. Keep the two sides in lockstep
 * — the Rust validator stays authoritative, but UI feedback must agree so
 * `aria-disabled` does not lie to the user.
 *
 * Rejected: BOM, RTL overrides, bidi isolates, LRM/RLM, Arabic letter mark,
 * line/paragraph separators. Not rejected: ZWJ / ZWNJ (legitimate in many
 * scripts and emoji sequences).
 */
function isDeniedFormattingCodePoint(code: number): boolean {
  return (
    code === 0xfeff ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2066 && code <= 0x2069) ||
    code === 0x200e ||
    code === 0x200f ||
    code === 0x061c ||
    code === 0x2028 ||
    code === 0x2029
  );
}

export interface ReasonContext {
  /** Number of code points in the normalized title. Used to compute the
   *  exact excess for the `too-long` reason so the user knows how many
   *  characters they need to trim. */
  charCount?: number;
}

export function reasonFor(
  issue: StoryTitleIssue,
  context: ReasonContext = {},
): string {
  switch (issue) {
    case "empty":
      return "Création impossible: titre requis";
    case "too-long": {
      const excess = Math.max(
        (context.charCount ?? MAX_STORY_TITLE_CHARS + 1) - MAX_STORY_TITLE_CHARS,
        1,
      );
      return `Création impossible: titre trop long (${MAX_STORY_TITLE_CHARS} caractères maximum, ${excess} en trop)`;
    }
    case "control-chars":
      return "Création impossible: titre contient des caractères non autorisés";
  }
}
