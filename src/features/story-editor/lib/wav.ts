/**
 * The little audio knowledge a microphone recording needs, as PURE
 * functions: trimming the silence around a take and encoding PCM samples
 * as a 16-bit mono WAV — the format Rust stores as-is and transcodes to the
 * Lunii format at send time. No compressed browser format (WebM/Opus,
 * MP4/AAC) is ever produced: the Rust chain does not decode Opus.
 */

/** Amplitude under which a sample counts as silence (about −50 dBFS). */
const SILENCE_THRESHOLD = 0.003;
/** Silence kept on each side of the take, so words are not clipped. */
const PADDING_SECONDS = 0.15;

/**
 * Cut the leading and trailing silence of `samples`, keeping a short pad
 * on both sides. A take that is silent throughout comes back empty.
 */
export function trimSilence(samples: Float32Array, sampleRate: number): Float32Array {
  let start = 0;
  while (start < samples.length && Math.abs(samples[start] ?? 0) < SILENCE_THRESHOLD) {
    start += 1;
  }
  if (start >= samples.length) return new Float32Array(0);
  let end = samples.length - 1;
  while (end > start && Math.abs(samples[end] ?? 0) < SILENCE_THRESHOLD) {
    end -= 1;
  }
  const pad = Math.round(PADDING_SECONDS * sampleRate);
  return samples.slice(Math.max(0, start - pad), Math.min(samples.length, end + 1 + pad));
}

/** Encode mono float samples in [-1, 1] as a 16-bit PCM RIFF/WAVE file. */
export function encodeWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const dataLength = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataLength);
  const view = new DataView(buffer);
  const ascii = (offset: number, text: string): void => {
    for (let i = 0; i < text.length; i += 1) view.setUint8(offset + i, text.charCodeAt(i));
  };
  ascii(0, "RIFF");
  view.setUint32(4, 36 + dataLength, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  ascii(36, "data");
  view.setUint32(40, dataLength, true);
  let offset = 44;
  for (let i = 0; i < samples.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[i] ?? 0));
    view.setInt16(offset, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true);
    offset += 2;
  }
  return new Uint8Array(buffer);
}

/** Standard base64 of raw bytes (the IPC carries the recording as text). */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Duration of mono samples at `sampleRate`, in milliseconds. */
export function durationMs(sampleCount: number, sampleRate: number): number {
  return Math.round((sampleCount / sampleRate) * 1000);
}
