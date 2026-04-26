/**
 * Renderable form of a recovery-banner draft / persisted title.
 *
 * The banner shows two strings the user previously typed or had
 * persisted, surrounded by quotes. Two edge cases would produce
 * misleading or unsafe UI if they reached the DOM verbatim:
 *
 * 1. **Empty / whitespace-only** — `""` and `"   "` would both render
 *    as the empty-quotes glyph, indistinguishable from each other and
 *    from a missing field. The user must see what they actually had.
 *    We replace such values with a sentinel `"empty"` / `"whitespace"`
 *    outcome the component renders in an explicit italic phrasing.
 *
 * 2. **BiDi controls / zero-width joiners / control chars** — a draft
 *    captured from a clipboard paste can carry U+202E (right-to-left
 *    override), U+200B (zero-width space), \r, \n, etc. Rendering
 *    those raw lets a malicious title visually flip the surrounding
 *    layout (filename-spoofing pattern), and renders \n as a literal
 *    line break inside our `<strong>` element — a real user typed
 *    "abc\ndef" gets shown as two visual lines without the user
 *    realizing the original was a single string. We replace each
 *    offender with the visible escape sequence ("\\n", "\\u202E", …)
 *    so the user sees the raw shape they actually typed.
 */

export type FormattedRecoveryDisplay =
  | { kind: "value"; text: string }
  | { kind: "empty" }
  | { kind: "whitespace" };

// Forbidden codepoints declared via Unicode-escape strings, then
// compiled via `new RegExp`. Embedding the raw characters directly
// into a regex literal would let the transpiler see U+2028 / U+2029
// as line terminators and break parsing — going through a string
// keeps the source ASCII-only.
//
// Coverage:
// - C0 / C1 controls except space:    U+0000-U+001F (incl. tab), U+007F-U+009F
// - Zero-width / BiDi format:         U+200B-U+200F, U+202A-U+202E, U+2066-U+2069
// - Line / paragraph separators:      U+2028, U+2029
// - BOM / zero-width no-break space:  U+FEFF
const FORBIDDEN_CONTROL_RE = new RegExp(
  "[\\u0000-\\u001F\\u007F-\\u009F]",
  "g",
);
const FORBIDDEN_BIDI_RE = new RegExp(
  "[\\u200B-\\u200F\\u202A-\\u202E\\u2066-\\u2069\\uFEFF]",
  "g",
);
const FORBIDDEN_LINE_TERMINATORS_RE = new RegExp("[\\u2028\\u2029]", "g");

function escapeChar(ch: string): string {
  const code = ch.codePointAt(0) ?? 0;
  // \r, \n, \t are common enough to deserve a friendlier glyph.
  if (ch === "\n") return "\\n";
  if (ch === "\r") return "\\r";
  if (ch === "\t") return "\\t";
  return `\\u${code.toString(16).toUpperCase().padStart(4, "0")}`;
}

export function formatRecoveryDisplay(value: string): FormattedRecoveryDisplay {
  if (value.length === 0) return { kind: "empty" };
  if (value.trim().length === 0) return { kind: "whitespace" };
  const escaped = value
    .replace(FORBIDDEN_CONTROL_RE, escapeChar)
    .replace(FORBIDDEN_BIDI_RE, escapeChar)
    .replace(FORBIDDEN_LINE_TERMINATORS_RE, escapeChar);
  return { kind: "value", text: escaped };
}
