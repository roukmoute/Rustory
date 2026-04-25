/** Maximum number of code points kept in a sanitized filename base. First
 *  pass truncation before the byte-length cap — cheap on common ASCII
 *  titles. */
const MAX_BASENAME_CODEPOINTS = 80;

/** Hard cap on the UTF-8 byte length of the sanitized basename. Most POSIX
 *  filesystems limit a single path component to 255 bytes (ext4, HFS+,
 *  APFS); we leave 40 bytes of headroom for the caller-appended `.rustory`
 *  extension and for any future metadata suffix so the composed filename
 *  always fits. */
const MAX_BASENAME_BYTES = 200;

/** Fallback basename used when sanitization strips everything useful. */
const FALLBACK_BASENAME = "histoire";

/** Windows reserved basenames (case-insensitive). Windows refuses the
 *  name WITH OR WITHOUT extension (`CON.txt` is as reserved as `CON`),
 *  so we strip any trailing `.ext` before comparing. The sanitized
 *  output is prefixed with `_` to keep the artifact portable across all
 *  supported desktop platforms. */
const WINDOWS_RESERVED_BASENAMES = new Set([
  "CON",
  "PRN",
  "AUX",
  "NUL",
  "COM1",
  "COM2",
  "COM3",
  "COM4",
  "COM5",
  "COM6",
  "COM7",
  "COM8",
  "COM9",
  "LPT1",
  "LPT2",
  "LPT3",
  "LPT4",
  "LPT5",
  "LPT6",
  "LPT7",
  "LPT8",
  "LPT9",
]);

/** Unicode bidi-control / zero-width / directional override codepoints
 *  that render invisibly but affect how the filename is displayed by
 *  the OS file manager. Replacing them with `_` keeps the visual form
 *  honest — a filename that reads as `foo.rustory` cannot hide a
 *  right-to-left override that makes it appear as `yrotsur.foo` to the
 *  user. */
const UNICODE_INVISIBLE_DIRECTIONAL = new Set<number>([
  0x200e, // LEFT-TO-RIGHT MARK
  0x200f, // RIGHT-TO-LEFT MARK
  0x202a, // LEFT-TO-RIGHT EMBEDDING
  0x202b, // RIGHT-TO-LEFT EMBEDDING
  0x202c, // POP DIRECTIONAL FORMATTING
  0x202d, // LEFT-TO-RIGHT OVERRIDE
  0x202e, // RIGHT-TO-LEFT OVERRIDE
  0x2066, // LEFT-TO-RIGHT ISOLATE
  0x2067, // RIGHT-TO-LEFT ISOLATE
  0x2068, // FIRST STRONG ISOLATE
  0x2069, // POP DIRECTIONAL ISOLATE
  0xfeff, // ZERO WIDTH NO-BREAK SPACE (BOM)
]);

const UTF8_ENCODER = new TextEncoder();

function truncateToMaxBytes(value: string, maxBytes: number): string {
  if (UTF8_ENCODER.encode(value).byteLength <= maxBytes) return value;
  const codepoints = Array.from(value);
  let result = "";
  for (const cp of codepoints) {
    const next = result + cp;
    if (UTF8_ENCODER.encode(next).byteLength > maxBytes) break;
    result = next;
  }
  return result;
}

/**
 * Build a filesystem-safe basename from a user-provided title. The
 * output:
 *
 *   - is NFC-normalized + trimmed so the canonical `stories.title`
 *     variant is what the user sees in the save dialog,
 *   - replaces filesystem-unsafe characters (`\x00-\x1f`, `\x7f`, slash,
 *     backslash, colon, star, question mark, double quote, angle
 *     brackets, pipe) with a single `_`, which is safe on every
 *     supported desktop OS,
 *   - collapses runs of whitespace/`_` into a single `_`,
 *   - is truncated at [`MAX_BASENAME_CODEPOINTS`] code points to avoid
 *     accidental OS-level refusal on long titles,
 *   - never returns an empty string: when sanitization strips every
 *     character, returns the [`FALLBACK_BASENAME`] constant.
 *
 * No extension is appended here — the caller composes the final filename
 * (typically `` `${sanitizeFilename(title)}.rustory` ``).
 */
export function sanitizeFilename(rawTitle: string): string {
  const normalized = rawTitle.normalize("NFC").trim();

  const mapped = Array.from(normalized).map((codepoint) => {
    const code = codepoint.codePointAt(0) ?? 0;
    // C0 controls, DEL, and the classic Windows-unsafe set.
    if (code < 0x20 || code === 0x7f) return "_";
    if ("/\\:*?\"<>|".includes(codepoint)) return "_";
    // BiDi / directional override / zero-width codepoints.
    if (UNICODE_INVISIBLE_DIRECTIONAL.has(code)) return "_";
    return codepoint;
  });

  // Collapse consecutive underscores/whitespace so a title like
  // "Un / Deux ?" becomes "Un_Deux_" instead of "Un___Deux__".
  const collapsed = mapped
    .join("")
    .replace(/[\s_]+/g, "_")
    .replace(/^_+|_+$/g, "");

  const codepoints = Array.from(collapsed);
  const codepointTruncated =
    codepoints.length > MAX_BASENAME_CODEPOINTS
      ? codepoints.slice(0, MAX_BASENAME_CODEPOINTS).join("")
      : codepoints.join("");

  const byteTruncated = truncateToMaxBytes(
    codepointTruncated,
    MAX_BASENAME_BYTES,
  ).replace(/_+$/, "");

  if (byteTruncated.length === 0) {
    return FALLBACK_BASENAME;
  }

  // Windows reserves the name with OR without an extension — test
  // against the first `.`-delimited segment to catch `CON.txt`.
  const reservedProbe = byteTruncated.split(".", 1)[0].toUpperCase();
  if (WINDOWS_RESERVED_BASENAMES.has(reservedProbe)) {
    return `_${byteTruncated}`;
  }

  return byteTruncated;
}
