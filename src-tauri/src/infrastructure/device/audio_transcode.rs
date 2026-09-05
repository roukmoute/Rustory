//! Transcode ANY decodable audio to the device MP3 format — bare MPEG-1
//! Layer III, 44100 Hz, MONO, 128 kbps CBR (the parameters observed on real
//! Lunii packs). The only step the device audio path needs beyond the
//! lossless MP3 handling of [`asset_convert`](super::asset_convert): podcast
//! episodes arrive as m4a/AAC or 48 kHz stereo MP3, which the device cannot
//! play.
//!
//! Pipeline (streaming — the input bytes are held once, the PCM never whole):
//! Symphonia probe + decode (AAC/MP4, MP3, WAV, Ogg Vorbis — exactly the
//! media-store formats) → per packet: interleaved f32 → channel average
//! (mono) → [`Resampler`] to 44100 Hz → `rusty_mp3` encoder → MP3 frames.
//! PURE beyond CPU: no I/O, no environment, deterministic for a given input.
//!
//! Fail-closed: an input Symphonia cannot probe/decode, or whose decode
//! yields no audio, is refused — never a half-transcoded file on the device.

use std::io::Cursor;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use super::resample::Resampler;

/// The device sample rate.
pub const DEVICE_SAMPLE_RATE: u32 = 44_100;
/// CBR bitrate of the produced MP3 (kbps) — the value real packs carry.
pub const DEVICE_MP3_KBPS: u32 = 128;

/// Why an input could not be transcoded. Closed, PII-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTranscodeError {
    /// No container/codec recognized the bytes, or no audio track.
    Unrecognized,
    /// The stream was recognized but produced no decodable audio.
    NoAudio,
    /// The encoder refused the stream parameters (a Symphonia-reported rate
    /// the resampler cannot bridge, an empty frame layout) — never expected
    /// for the store's formats, kept fail-closed.
    Encode,
}

/// Transcode `bytes` (any media-store audio) to a device MP3.
pub fn transcode_to_device_mp3(bytes: &[u8]) -> Result<Vec<u8>, AudioTranscodeError> {
    let source = Box::new(Cursor::new(bytes.to_vec()));
    let stream = MediaSourceStream::new(source, Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| AudioTranscodeError::Unrecognized)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(AudioTranscodeError::Unrecognized)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .cloned()
        .ok_or(AudioTranscodeError::Unrecognized)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| AudioTranscodeError::Unrecognized)?;
    let track_id = track.id;

    let mut encoder = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig {
        bitrate_kbps: DEVICE_MP3_KBPS,
        vbr_quality: None,
    });
    let mut resampler: Option<Resampler> = None;
    let mut interleaved: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut resampled: Vec<f32> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let mut decoded_any = false;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // End of stream (a clean EOF) — or an I/O shape error on a
            // truncated file: what was decoded so far is the audio.
            Ok(None) | Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            // A corrupt packet is skipped (the decoder resynchronizes), like
            // every player does; a structural error ends the stream.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = audio.spec();
        let channels = spec.channels().count().max(1);
        let rate = spec.rate();
        if rate == 0 {
            return Err(AudioTranscodeError::Encode);
        }
        interleaved.resize(audio.samples_interleaved(), f32::MID);
        audio.copy_to_slice_interleaved(&mut interleaved);
        if interleaved.is_empty() {
            continue;
        }
        decoded_any = true;

        mono.clear();
        mono.extend(
            interleaved
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        let resampler = resampler.get_or_insert_with(|| Resampler::new(rate, DEVICE_SAMPLE_RATE));
        resampled.clear();
        resampler.push(&mono, &mut resampled);
        push_pcm(&mut encoder, &resampled, &mut out)?;
    }

    if !decoded_any {
        return Err(AudioTranscodeError::NoAudio);
    }
    if let Some(resampler) = resampler.as_mut() {
        resampled.clear();
        resampler.finish(&mut resampled);
        push_pcm(&mut encoder, &resampled, &mut out)?;
    }
    encoder.finish();
    drain(&mut encoder, &mut out);
    if out.is_empty() {
        return Err(AudioTranscodeError::NoAudio);
    }
    Ok(out)
}

fn push_pcm(
    encoder: &mut rusty_mp3::Mp3Encoder,
    pcm: &[f32],
    out: &mut Vec<u8>,
) -> Result<(), AudioTranscodeError> {
    if pcm.is_empty() {
        return Ok(());
    }
    encoder
        .push_pcm_f32(pcm, 1, DEVICE_SAMPLE_RATE)
        .map_err(|_| AudioTranscodeError::Encode)?;
    drain(encoder, out);
    Ok(())
}

fn drain(encoder: &mut rusty_mp3::Mp3Encoder, out: &mut Vec<u8>) {
    while let Ok(packet) = encoder.next_packet() {
        out.extend_from_slice(&packet);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    /// A 16-bit PCM WAV of a sine tone — the decodable, non-MP3 input the
    /// transcoder must bridge (WAV is one of the media-store formats).
    pub fn wav_sine(rate: u32, channels: u16, hz: f64, seconds: f64, amp: f64) -> Vec<u8> {
        let frames = (rate as f64 * seconds) as usize;
        let data_len = frames * channels as usize * 2;
        let mut out = Vec::with_capacity(44 + data_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for i in 0..frames {
            let v = (amp
                * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()
                * i16::MAX as f64) as i16;
            for _ in 0..channels {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::wav_sine;
    use super::*;

    /// Decode an MP3 back to mono f32 PCM with Symphonia (the independent
    /// decoder — rusty_mp3 is only trusted to ENCODE here).
    fn decode_mp3_mono(bytes: &[u8]) -> (u32, usize, Vec<f32>) {
        let stream =
            MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .expect("probe mp3");
        let track = format.default_track(TrackType::Audio).expect("track");
        let params = track
            .codec_params
            .as_ref()
            .unwrap()
            .audio()
            .unwrap()
            .clone();
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .expect("decoder");
        let mut pcm = Vec::new();
        let mut rate = 0;
        let mut channels = 0;
        let mut buf = Vec::new();
        while let Ok(Some(packet)) = format.next_packet() {
            if let Ok(audio) = decoder.decode(&packet) {
                rate = audio.spec().rate();
                channels = audio.spec().channels().count();
                buf.resize(audio.samples_interleaved(), f32::MID);
                audio.copy_to_slice_interleaved(&mut buf);
                pcm.extend_from_slice(&buf);
            }
        }
        (rate, channels, pcm)
    }

    fn first_frame_header(mp3: &[u8]) -> [u8; 4] {
        [mp3[0], mp3[1], mp3[2], mp3[3]]
    }

    #[test]
    fn a_48k_stereo_wav_becomes_a_bare_mono_44k1_mpeg1_layer3_stream() {
        let wav = wav_sine(48_000, 2, 440.0, 2.0, 0.5);
        let mp3 = transcode_to_device_mp3(&wav).expect("transcode");
        // Frame sync + MPEG-1 Layer III, 44100 Hz, mono — the exact device
        // conformance `to_device_audio` enforces on the first header.
        let h = first_frame_header(&mp3);
        assert_eq!(h[0], 0xFF);
        assert_eq!(h[1] & 0xE0, 0xE0, "frame sync");
        assert_eq!((h[1] >> 3) & 0b11, 3, "MPEG-1");
        assert_eq!((h[1] >> 1) & 0b11, 1, "Layer III");
        assert_eq!((h[2] >> 2) & 0b11, 0, "44100 Hz");
        assert_eq!((h[3] >> 6) & 0b11, 3, "mono");
        assert!(!mp3.starts_with(b"ID3"), "bare, no ID3 wrapper");

        // Independent decode: mono, 44100 Hz, ~2 s, and the 440 Hz tone is
        // there (zero-crossing count ≈ 2 · 440 · 2 s) at roughly its level.
        let (rate, channels, pcm) = decode_mp3_mono(&mp3);
        assert_eq!(rate, 44_100);
        assert_eq!(channels, 1);
        let seconds = pcm.len() as f64 / 44_100.0;
        assert!((1.95..2.15).contains(&seconds), "duration {seconds:.3} s");
        let steady = &pcm[4410..pcm.len() - 4410];
        let crossings = steady
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let expected = 2.0 * 440.0 * (steady.len() as f64 / 44_100.0);
        assert!(
            (crossings as f64 - expected).abs() < expected * 0.02,
            "zero crossings {crossings} vs {expected:.0}"
        );
        let peak = steady.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((0.4..0.6).contains(&peak), "peak {peak}");
    }

    #[test]
    fn a_mono_44k1_wav_is_encoded_without_resampling_and_keeps_its_length() {
        let wav = wav_sine(44_100, 1, 1000.0, 1.0, 0.3);
        let mp3 = transcode_to_device_mp3(&wav).expect("transcode");
        let (rate, channels, pcm) = decode_mp3_mono(&mp3);
        assert_eq!((rate, channels), (44_100, 1));
        // One second, ± the encoder's frame padding and decoder delay.
        assert!(
            (pcm.len() as i64 - 44_100).abs() < 4_000,
            "samples {}",
            pcm.len()
        );
    }

    #[test]
    fn garbage_and_empty_input_are_refused() {
        assert_eq!(
            transcode_to_device_mp3(b"OggS this is not an ogg page really"),
            Err(AudioTranscodeError::Unrecognized)
        );
        assert_eq!(
            transcode_to_device_mp3(b""),
            Err(AudioTranscodeError::Unrecognized)
        );
        assert_eq!(
            transcode_to_device_mp3(&[0x00; 4096]),
            Err(AudioTranscodeError::Unrecognized)
        );
    }

    #[test]
    fn a_wav_header_with_no_samples_yields_no_audio() {
        let wav = wav_sine(44_100, 1, 1000.0, 0.0, 0.3);
        assert!(matches!(
            transcode_to_device_mp3(&wav),
            Err(AudioTranscodeError::NoAudio) | Err(AudioTranscodeError::Unrecognized)
        ));
    }
}
