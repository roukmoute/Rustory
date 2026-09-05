//! The little WAV knowledge the speech layer needs: recognizing a RIFF/WAVE
//! file an engine wrote (fail-closed on anything else) and reading its
//! duration, for the voice preview and the diagnostics.

/// True iff `bytes` start with a RIFF/WAVE header and carry a `fmt ` chunk.
pub fn is_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 44
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WAVE"
        && find_chunk(bytes, b"fmt ").is_some()
}

/// Approximate duration in milliseconds from the `fmt ` byte rate and the
/// `data` chunk size. `None` when the file is not a readable WAV.
pub fn duration_ms(bytes: &[u8]) -> Option<u64> {
    let (fmt_start, _) = find_chunk(bytes, b"fmt ")?;
    let byte_rate = u32::from_le_bytes([
        *bytes.get(fmt_start + 8)?,
        *bytes.get(fmt_start + 9)?,
        *bytes.get(fmt_start + 10)?,
        *bytes.get(fmt_start + 11)?,
    ]);
    if byte_rate == 0 {
        return None;
    }
    let (_, data_len) = find_chunk(bytes, b"data")?;
    Some((data_len as u64 * 1000) / byte_rate as u64)
}

/// `(payload offset, payload length)` of the first chunk tagged `id`.
fn find_chunk(bytes: &[u8], id: &[u8; 4]) -> Option<(usize, usize)> {
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let tag = &bytes[offset..offset + 4];
        let len = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        if tag == id {
            let available = bytes.len().saturating_sub(offset + 8);
            return Some((offset + 8, len.min(available)));
        }
        // Chunks are word-aligned.
        offset = offset.checked_add(8 + len + (len & 1))?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::device::audio_transcode::test_support::wav_sine;

    #[test]
    fn recognizes_a_wav_and_reads_its_duration() {
        let wav = wav_sine(22_050, 1, 440.0, 1.5, 0.3);
        assert!(is_wav(&wav));
        let ms = duration_ms(&wav).expect("duration");
        assert!((1_490..=1_510).contains(&ms), "{ms} ms");
    }

    #[test]
    fn refuses_non_wav_bytes() {
        assert!(!is_wav(b""));
        assert!(!is_wav(b"RIFF....WAVEjunk"));
        assert!(!is_wav(&[0u8; 64]));
        assert_eq!(duration_ms(b"OggS"), None);
    }
}
