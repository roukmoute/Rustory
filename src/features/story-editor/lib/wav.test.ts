import { describe, expect, it } from "vitest";

import { bytesToBase64, durationMs, encodeWav, trimSilence } from "./wav";

describe("wav helpers", () => {
  it("encodes mono 16-bit PCM with a valid RIFF header", () => {
    const samples = new Float32Array([0, 0.5, -0.5, 1, -1]);
    const wav = encodeWav(samples, 22050);
    const text = (offset: number, length: number) =>
      String.fromCharCode(...wav.subarray(offset, offset + length));
    expect(text(0, 4)).toBe("RIFF");
    expect(text(8, 4)).toBe("WAVE");
    expect(text(12, 4)).toBe("fmt ");
    expect(text(36, 4)).toBe("data");
    const view = new DataView(wav.buffer);
    expect(view.getUint16(22, true)).toBe(1); // mono
    expect(view.getUint32(24, true)).toBe(22050);
    expect(view.getUint16(34, true)).toBe(16);
    expect(view.getUint32(40, true)).toBe(10);
    expect(wav.length).toBe(44 + 10);
    // DataView truncates: 0.5 · 0x7fff = 16383.5 → 16383.
    expect(view.getInt16(44 + 2, true)).toBe(Math.trunc(0.5 * 0x7fff));
    expect(view.getInt16(44 + 6, true)).toBe(0x7fff);
    expect(view.getInt16(44 + 8, true)).toBe(-0x8000);
  });

  it("trims leading and trailing silence but keeps a short pad", () => {
    const rate = 1000;
    const samples = new Float32Array(3000);
    for (let i = 1000; i < 2000; i += 1) samples[i] = 0.4;
    const trimmed = trimSilence(samples, rate);
    // 1000 loud samples + 150 ms pad on each side.
    expect(trimmed.length).toBe(1000 + 2 * 150);
    expect(trimSilence(new Float32Array(500), rate).length).toBe(0);
    // Sound right at the edges: no pad beyond the buffer.
    const edge = new Float32Array(100).fill(0.2);
    expect(trimSilence(edge, rate).length).toBe(100);
  });

  it("converts bytes to base64 and computes durations", () => {
    expect(bytesToBase64(new Uint8Array([102, 111, 111]))).toBe("Zm9v");
    expect(bytesToBase64(new Uint8Array(0))).toBe("");
    expect(durationMs(22050, 22050)).toBe(1000);
    expect(durationMs(11025, 44100)).toBe(250);
  });
});
