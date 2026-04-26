import { describe, expect, it } from "vitest";

import { formatRelativeTime } from "./format-relative-time";

const REF = new Date("2026-04-25T12:00:00.000Z");

describe("formatRelativeTime", () => {
  it("returns 'à l'instant' for deltas under 60 seconds", () => {
    expect(formatRelativeTime("2026-04-25T11:59:30.000Z", REF)).toBe(
      "à l'instant",
    );
  });

  it("returns 'il y a 5 minutes' for a 5-minute delta", () => {
    expect(formatRelativeTime("2026-04-25T11:55:00.000Z", REF)).toBe(
      "il y a 5 minutes",
    );
  });

  it("returns 'il y a 1 minute' for a single-minute delta (singular)", () => {
    expect(formatRelativeTime("2026-04-25T11:59:00.000Z", REF)).toBe(
      "il y a 1 minute",
    );
  });

  it("returns 'il y a 2 heures' for a 2-hour delta", () => {
    expect(formatRelativeTime("2026-04-25T10:00:00.000Z", REF)).toBe(
      "il y a 2 heures",
    );
  });

  it("returns 'il y a 1 heure' for a single-hour delta (singular)", () => {
    expect(formatRelativeTime("2026-04-25T11:00:00.000Z", REF)).toBe(
      "il y a 1 heure",
    );
  });

  it("returns an absolute date for deltas of 1 day or more", () => {
    // 26 hours ago.
    const result = formatRelativeTime("2026-04-24T10:00:00.000Z", REF);
    expect(result.startsWith("le ")).toBe(true);
    // The exact format depends on the JS runtime but must stay
    // human-readable French.
    expect(result).toMatch(/^le \d{2}\/\d{2}\/2026$/);
  });

  it("falls back to 'récemment' when the timestamp cannot be parsed", () => {
    expect(formatRelativeTime("not-a-date", REF)).toBe("récemment");
  });

  it("falls back to absolute date for negative deltas (clock rollback / future draft)", () => {
    // Reference is BEFORE the timestamp — a clock-skew or tampered
    // DB situation. Saying "à l'instant" would lie. Saying "il y a
    // -3 secondes" would be gibberish. The fallback is the absolute
    // date — the user sees an honest signal that something is off.
    const result = formatRelativeTime("2026-04-25T12:00:30.000Z", REF);
    expect(result.startsWith("le ")).toBe(true);
    expect(result).toMatch(/^le \d{2}\/\d{2}\/2026$/);
  });

  it("uses Math.floor at the 59.5s boundary (no round-up into the 1-minute band)", () => {
    // Reference - 59500ms = 59.5 seconds delta. Math.floor keeps it
    // in the "à l'instant" band; Math.round would have promoted it
    // to "il y a 1 minute" inconsistently with the other floor-based
    // bands.
    const past = new Date(REF.getTime() - 59500).toISOString();
    expect(formatRelativeTime(past, REF)).toBe("à l'instant");
  });
});
