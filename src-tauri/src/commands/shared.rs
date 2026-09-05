use crate::domain::shared::AppError;
use crate::infrastructure::filesystem::MediaKind;

/// Bound the accepted story id length so a hostile or malformed payload
/// can never issue a multi-kilobyte SQL prepared-statement parameter.
/// UUIDv7 canonical form is 36 bytes; a generous ceiling leaves room for
/// future id schemes without becoming an attack surface.
pub const MAX_STORY_ID_LEN: usize = 256;

/// Validate an incoming story id at the IPC boundary, surfacing the same
/// `LIBRARY_INCONSISTENT` category regardless of which command the id
/// feeds — the UI recovers the same way in every case ("reload the
/// library"). Kept as a single source of truth so story-id validation
/// never drifts between commands.
pub fn validate_story_id(raw: &str) -> Result<(), AppError> {
    if raw.is_empty() {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "empty",
        })));
    }
    if raw.len() > MAX_STORY_ID_LEN {
        return Err(AppError::library_inconsistent(
            "Histoire introuvable, recharge la bibliothèque.",
            "Retourne à la bibliothèque et recharge la liste.",
        )
        .with_details(serde_json::json!({
            "source": "story_id_invalid",
            "cause": "too_long",
            "maxLen": MAX_STORY_ID_LEN,
        })));
    }
    Ok(())
}

/// Parse a wire media-slot discriminator (`image` / `audio`) into its
/// [`MediaKind`]. An unknown value is a `MEDIA_INVALID` block — the UI only
/// ever sends the two known slots, so anything else is a contract drift, not a
/// user-recoverable file issue.
pub fn parse_media_slot(raw: &str) -> Result<MediaKind, AppError> {
    match raw {
        "image" => Ok(MediaKind::Image),
        "audio" => Ok(MediaKind::Audio),
        _ => Err(AppError::media_invalid(
            "Emplacement de média inconnu.",
            "Recharge l'éditeur puis réessaie.",
        )
        .with_details(serde_json::json!({ "source": "media_invalid", "stage": "unknown_slot" }))),
    }
}

/// Minimal standard-alphabet base64 encoder (RFC 4648, full padding). Kept
/// dependency-free — wraps small cached images / node media into a `data:` URL
/// for the webview. Single source of truth shared by the catalog cover and
/// node-media preview commands.
pub fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Strict standard-alphabet base64 decoder (RFC 4648): the inverse of
/// [`base64_encode`], for the few payloads the webview PRODUCES (a
/// microphone recording). Refuses anything but the alphabet, misplaced
/// padding or a length that is not a multiple of 4 — never guesses.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let last = i + 1 == bytes.len() / 4;
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = (n << 6) | value(c)?;
        }
        n <<= 6 * pad as u32;
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;

    #[test]
    fn base64_decode_inverts_encode_and_refuses_garbage() {
        for data in [
            b"".to_vec(),
            b"f".to_vec(),
            b"fo".to_vec(),
            b"foo".to_vec(),
            b"foob".to_vec(),
            (0u8..=255).collect::<Vec<u8>>(),
        ] {
            assert_eq!(
                base64_decode(&base64_encode(&data)).as_deref(),
                Some(data.as_slice())
            );
        }
        assert_eq!(base64_decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(base64_decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("Zg=="), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Zg="), None, "length not a multiple of 4");
        assert_eq!(base64_decode("Zg==Zm9v"), None, "padding not at the end");
        assert_eq!(base64_decode("Zm9!"), None, "outside the alphabet");
        assert_eq!(base64_decode("===="), None);
    }

    #[test]
    fn parse_media_slot_maps_known_slots_and_rejects_others() {
        assert_eq!(parse_media_slot("image").unwrap(), MediaKind::Image);
        assert_eq!(parse_media_slot("audio").unwrap(), MediaKind::Audio);
        let err = parse_media_slot("video").expect_err("unknown slot");
        assert_eq!(err.code, AppErrorCode::MediaInvalid);
        assert_eq!(err.details.unwrap()["stage"], "unknown_slot");
    }

    #[test]
    fn base64_encode_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn accepts_a_canonical_uuid_v7() {
        assert!(validate_story_id("0197a5d0-0000-7000-8000-000000000000").is_ok());
    }

    #[test]
    fn rejects_empty_string() {
        let err = validate_story_id("").expect_err("must reject empty");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["source"], "story_id_invalid");
        assert_eq!(details["cause"], "empty");
    }

    #[test]
    fn rejects_oversize_string() {
        let huge = "a".repeat(MAX_STORY_ID_LEN + 1);
        let err = validate_story_id(&huge).expect_err("must reject oversize");
        assert_eq!(err.code, AppErrorCode::LibraryInconsistent);
        let details = err.details.as_ref().expect("details");
        assert_eq!(details["cause"], "too_long");
        assert_eq!(details["maxLen"], MAX_STORY_ID_LEN);
    }
}
