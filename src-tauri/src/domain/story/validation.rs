use unicode_normalization::UnicodeNormalization;

use crate::domain::shared::AppError;
use crate::domain::story::schema::MAX_TITLE_CHARS;

/// Normalize a raw user-supplied title before running validation or writing
/// it to disk: compose into NFC, then trim ASCII whitespace. NFC keeps the
/// stored representation independent of the input method (precomposed vs
/// decomposed diacritics), so two visually-identical titles cannot collide
/// on `PRIMARY KEY` with different byte sequences.
pub fn normalize_title(raw: &str) -> String {
    raw.nfc().collect::<String>().trim().to_string()
}

/// Exhaustive list of title rejection causes. Each variant maps to one of
/// the canonical reasons documented in `docs/architecture/ui-states.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryTitleError {
    Empty,
    TooLong { chars: usize },
    ControlChars,
}

/// Validate a previously-normalized title. Returns `Ok(())` if the title is
/// acceptable for persistence.
pub fn validate_title(normalized: &str) -> Result<(), StoryTitleError> {
    if normalized.is_empty() {
        return Err(StoryTitleError::Empty);
    }
    let char_count = normalized.chars().count();
    if char_count > MAX_TITLE_CHARS {
        return Err(StoryTitleError::TooLong { chars: char_count });
    }
    for ch in normalized.chars() {
        if ch.is_control() || is_denied_formatting_char(ch) {
            return Err(StoryTitleError::ControlChars);
        }
    }
    Ok(())
}

/// Targeted denylist of format-category (Cf) and line-separator code points
/// that would render a title unsafe or ambiguous.
///
/// Rejected:
/// - `U+FEFF` — byte-order mark / zero-width no-break space (hidden prefix)
/// - `U+202A..=U+202E` — LRE/RLE/PDF/LRO/RLO bidi overrides
/// - `U+2066..=U+2069` — LRI/RLI/FSI/PDI bidi isolates
/// - `U+200E`, `U+200F` — LRM/RLM directional marks
/// - `U+061C` — Arabic letter mark
/// - `U+2028`, `U+2029` — line/paragraph separators (semantic newlines)
///
/// Intentionally NOT rejected: `U+200C` ZWNJ, `U+200D` ZWJ, and other
/// legitimate Cf code points required for correct rendering of several
/// scripts (Farsi, Indic, emoji ZWJ sequences).
fn is_denied_formatting_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{FEFF}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Canonical user-facing reasons. Kept in sync with
/// `docs/architecture/ui-states.md#Disabled Actions and Reasons`; a change
/// here without matching doc update is a bug — the test suite asserts the
/// exact strings.
pub const REASON_TITLE_REQUIRED: &str = "Création impossible: titre requis";
pub const REASON_TITLE_TOO_LONG: &str =
    "Création impossible: titre trop long (120 caractères maximum)";
pub const REASON_TITLE_CONTROL_CHARS: &str =
    "Création impossible: titre contient des caractères non autorisés";
pub const ACTION_TITLE_REQUIRED: &str = "Saisis un titre non vide pour créer l'histoire.";
pub const ACTION_TITLE_TOO_LONG: &str = "Raccourcis le titre à 120 caractères maximum.";
pub const ACTION_TITLE_CONTROL_CHARS: &str =
    "Supprime les sauts de ligne, tabulations et caractères invisibles.";

/// Map a [`StoryTitleError`] to the normalized [`AppError`] surfaced by the
/// IPC boundary. Each variant uses the canonical message documented in the
/// UI states contract so the UI never has to synthesize a wording.
pub fn map_error(err: StoryTitleError) -> AppError {
    match err {
        StoryTitleError::Empty => {
            AppError::invalid_story_title(REASON_TITLE_REQUIRED, ACTION_TITLE_REQUIRED)
        }
        StoryTitleError::TooLong { chars } => {
            AppError::invalid_story_title(REASON_TITLE_TOO_LONG, ACTION_TITLE_TOO_LONG)
                .with_details(serde_json::json!({
                    "cause": "too_long",
                    "maxChars": MAX_TITLE_CHARS,
                    "chars": chars,
                }))
        }
        StoryTitleError::ControlChars => {
            AppError::invalid_story_title(REASON_TITLE_CONTROL_CHARS, ACTION_TITLE_CONTROL_CHARS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_whitespace_and_composes_nfc() {
        assert_eq!(normalize_title("  Café  "), "Café");
        assert_eq!(normalize_title("cafe\u{0301}"), "café");
        assert_eq!(normalize_title(""), "");
    }

    #[test]
    fn validate_empty_title_is_rejected() {
        assert_eq!(validate_title(""), Err(StoryTitleError::Empty));
    }

    #[test]
    fn validate_title_length_boundary() {
        let ok = "a".repeat(MAX_TITLE_CHARS);
        let too_long = "a".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(validate_title(&ok), Ok(()));
        assert_eq!(
            validate_title(&too_long),
            Err(StoryTitleError::TooLong {
                chars: MAX_TITLE_CHARS + 1,
            })
        );
    }

    #[test]
    fn validate_title_rejects_control_chars() {
        assert_eq!(validate_title("a\nb"), Err(StoryTitleError::ControlChars));
        assert_eq!(validate_title("a\tb"), Err(StoryTitleError::ControlChars));
        assert_eq!(validate_title("a\0b"), Err(StoryTitleError::ControlChars));
    }

    #[test]
    fn validate_title_allows_unicode_letters_punctuation_spaces() {
        let title = "Aventure ①: 💫 été — L'île mystérieuse";
        assert_eq!(validate_title(title), Ok(()));
    }

    #[test]
    fn validate_title_rejects_byte_order_mark() {
        assert_eq!(
            validate_title("Titre\u{FEFF}caché"),
            Err(StoryTitleError::ControlChars)
        );
    }

    #[test]
    fn validate_title_rejects_bidi_overrides_and_isolates() {
        for ch in [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}',
        ] {
            let title = format!("a{ch}b");
            assert_eq!(
                validate_title(&title),
                Err(StoryTitleError::ControlChars),
                "U+{:04X} must be rejected",
                ch as u32
            );
        }
    }

    #[test]
    fn validate_title_rejects_directional_marks() {
        for ch in ['\u{200E}', '\u{200F}', '\u{061C}'] {
            let title = format!("a{ch}b");
            assert_eq!(validate_title(&title), Err(StoryTitleError::ControlChars));
        }
    }

    #[test]
    fn validate_title_rejects_line_and_paragraph_separators() {
        assert_eq!(
            validate_title("ligne1\u{2028}ligne2"),
            Err(StoryTitleError::ControlChars)
        );
        assert_eq!(
            validate_title("para1\u{2029}para2"),
            Err(StoryTitleError::ControlChars)
        );
    }

    #[test]
    fn validate_title_allows_legitimate_zwj_zwnj() {
        // ZWNJ and ZWJ are required for correct rendering in many scripts
        // and in emoji sequences — they must not trigger the denylist.
        assert_eq!(validate_title("a\u{200C}b"), Ok(()));
        assert_eq!(validate_title("a\u{200D}b"), Ok(()));
    }

    #[test]
    fn map_error_preserves_canonical_reasons() {
        let err = map_error(StoryTitleError::Empty);
        assert_eq!(err.message, REASON_TITLE_REQUIRED);

        let err = map_error(StoryTitleError::TooLong { chars: 121 });
        assert_eq!(err.message, REASON_TITLE_TOO_LONG);
        assert_eq!(err.details.as_ref().expect("details")["chars"], 121);

        let err = map_error(StoryTitleError::ControlChars);
        assert_eq!(err.message, REASON_TITLE_CONTROL_CHARS);
    }
}
