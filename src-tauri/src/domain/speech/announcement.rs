//! The text a voice reads for each announcement. Titles come from web pages,
//! RSS feeds or the editor and are written for the eye: « Le trésor de
//! Moctezuma : épisode 1/10 » read aloud would give « un dixième ». The rules
//! here turn them into short, natural spoken sentences, deterministically
//! (the same label always yields the same text — a re-generation is a no-op).

/// The menu question every menu-layout pack asks before the wheel.
pub const MENU_QUESTION: &str = "Quelle histoire veux-tu écouter ?";

/// Upper bound on a spoken text (characters): a title is an announcement,
/// not a description — longer input is cut at a word boundary.
pub const MAX_SPOKEN_CHARS: usize = 200;

/// The spoken form of a series title: whitespace collapsed, colons read as a
/// pause, cut to [`MAX_SPOKEN_CHARS`], ended by a period. Empty stays empty.
pub fn spoken_series_title(title: &str) -> String {
    finish(&trim_separators(&pause_colons(&collapse_whitespace(title))))
}

/// The spoken form of an episode label. An « épisode N » marker (with or
/// without « /M », accent-insensitive, any case) is moved FIRST as
/// « Épisode N » and the « /M » total is dropped; the remaining title follows
/// after a pause. Colons read as a pause. Examples:
///
/// - « Le trésor de Moctezuma : épisode 1/10 » → « Épisode 1. Le trésor de Moctezuma. »
/// - « Épisode 3 – L'île des femmes » → « Épisode 3. L'île des femmes. »
/// - « La flûte de Quetzalcoatl » → « La flûte de Quetzalcoatl. »
pub fn spoken_episode_title(label: &str) -> String {
    let text = collapse_whitespace(label);
    let Some((number, rest)) = split_episode_marker(&text) else {
        return finish(&trim_separators(&pause_colons(&text)));
    };
    let rest = pause_colons(&trim_separators(&rest));
    let rest = finish(&rest);
    if rest.is_empty() {
        format!("Épisode {number}.")
    } else {
        format!("Épisode {number}. {rest}")
    }
}

/// Find « épisode <digits>[/<digits>] » (accent- and case-insensitive) and
/// return the episode number plus the text without the marker.
fn split_episode_marker(text: &str) -> Option<(String, String)> {
    let lower: Vec<char> = text
        .chars()
        .map(|c| match c.to_lowercase().next().unwrap_or(c) {
            'é' | 'è' | 'ê' => 'e',
            other => other,
        })
        .collect();
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = "episode".chars().collect();
    let mut start = 0;
    while start + needle.len() <= lower.len() {
        if lower[start..start + needle.len()] == needle[..] {
            // Word boundary before.
            let boundary_before = start == 0 || !lower[start - 1].is_alphanumeric();
            let mut i = start + needle.len();
            // Optional separators, then digits.
            while i < lower.len() && (lower[i] == ' ' || lower[i] == '.' || lower[i] == ':') {
                i += 1;
            }
            let digits_start = i;
            while i < lower.len() && lower[i].is_ascii_digit() {
                i += 1;
            }
            if boundary_before && i > digits_start {
                let number: String = chars[digits_start..i].iter().collect();
                // Drop an « /M » total right after the number.
                let mut end = i;
                if end < lower.len() && lower[end] == '/' {
                    let mut j = end + 1;
                    while j < lower.len() && lower[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j > end + 1 {
                        end = j;
                    }
                }
                let before: String = chars[..start].iter().collect();
                let after: String = chars[end..].iter().collect();
                let rest = collapse_whitespace(&format!("{before} {after}"));
                return Some((number.trim_start_matches('0').to_string(), rest))
                    .map(|(n, r)| (if n.is_empty() { "0".to_string() } else { n }, r));
            }
        }
        start += 1;
    }
    None
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip dangling separators the marker removal leaves at either end
/// (« : », « – », « - », « — », « , », « | », « . »).
fn trim_separators(text: &str) -> String {
    let is_sep = |c: char| matches!(c, ':' | '–' | '-' | '—' | ',' | '|' | '.' | '·');
    text.trim_matches(|c: char| c.is_whitespace() || is_sep(c))
        .to_string()
}

/// « A : B » is read as « A, B » — a colon is a pause, not a word.
fn pause_colons(text: &str) -> String {
    collapse_whitespace(&text.replace(" :", ",").replace(':', ","))
}

/// Cut at [`MAX_SPOKEN_CHARS`] on a word boundary and end with a period
/// (unless the text already ends with a sentence mark). Empty stays empty.
fn finish(text: &str) -> String {
    let mut text = text.trim().to_string();
    if text.chars().count() > MAX_SPOKEN_CHARS {
        let cut: String = text.chars().take(MAX_SPOKEN_CHARS).collect();
        text = match cut.rfind(' ') {
            Some(space) => cut[..space].to_string(),
            None => cut,
        };
        text = trim_separators(&text);
    }
    if text.is_empty() {
        return text;
    }
    if text.ends_with(['.', '!', '?']) {
        text
    } else {
        format!("{text}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_episode_marker_at_the_end_moves_first_and_loses_its_total() {
        assert_eq!(
            spoken_episode_title("Le trésor de Moctezuma : épisode 1/10"),
            "Épisode 1. Le trésor de Moctezuma."
        );
        assert_eq!(
            spoken_episode_title("La flûte de Quetzalcoatl : épisode 2/10"),
            "Épisode 2. La flûte de Quetzalcoatl."
        );
    }

    #[test]
    fn an_episode_marker_at_the_start_keeps_its_place_and_drops_the_dash() {
        assert_eq!(
            spoken_episode_title("Épisode 3 – L'île des femmes"),
            "Épisode 3. L'île des femmes."
        );
        assert_eq!(
            spoken_episode_title("episode 12: Le parangon"),
            "Épisode 12. Le parangon."
        );
        assert_eq!(spoken_episode_title("EPISODE 7"), "Épisode 7.");
        assert_eq!(
            spoken_episode_title("Ep. 07"),
            "Ep. 07.",
            "only the word « épisode »"
        );
    }

    #[test]
    fn a_plain_title_is_just_ended_by_a_period_and_colons_become_pauses() {
        assert_eq!(
            spoken_episode_title("  La   flûte de Quetzalcoatl "),
            "La flûte de Quetzalcoatl."
        );
        assert_eq!(
            spoken_episode_title("Tina : le retour !"),
            "Tina, le retour !"
        );
        assert_eq!(spoken_episode_title(""), "");
        assert_eq!(spoken_episode_title(" : "), "");
    }

    #[test]
    fn a_series_title_is_read_as_is_with_a_final_period() {
        assert_eq!(
            spoken_series_title("Tina et le serpent à plumes"),
            "Tina et le serpent à plumes."
        );
        assert_eq!(
            spoken_series_title("Les aventures de Tina : saison 2"),
            "Les aventures de Tina, saison 2."
        );
        assert_eq!(spoken_series_title("Fin ?"), "Fin ?");
    }

    #[test]
    fn overlong_text_is_cut_on_a_word_boundary() {
        let long = "mot ".repeat(100);
        let spoken = spoken_series_title(&long);
        assert!(spoken.chars().count() <= MAX_SPOKEN_CHARS + 1);
        assert!(spoken.ends_with("mot."));
    }

    #[test]
    fn the_same_label_always_speaks_the_same_text() {
        let a = spoken_episode_title("Le trésor : épisode 1/10");
        let b = spoken_episode_title("Le trésor : épisode 1/10");
        assert_eq!(a, b);
    }
}
