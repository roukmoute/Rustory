import { describe, expect, it } from "vitest";

import { formatRecoveryDisplay } from "./format-recovery-display";

// Build offending strings via escape sequences only — see the source
// file for the rationale on keeping U+2028 / U+2029 / BiDi out of
// raw literals.
const RLO = "‮"; // right-to-left override
const ZWSP = "​"; // zero-width space
const BOM = "﻿";
const LRI = "⁦";
const PDI = "⁩";
const LSEP = " ";

describe("formatRecoveryDisplay", () => {
  it("returns kind=empty for an empty string", () => {
    expect(formatRecoveryDisplay("")).toEqual({ kind: "empty" });
  });

  it("returns kind=whitespace for a whitespace-only string", () => {
    expect(formatRecoveryDisplay("   ")).toEqual({ kind: "whitespace" });
    expect(formatRecoveryDisplay("\t\t")).toEqual({ kind: "whitespace" });
  });

  it("passes regular printable text through unchanged", () => {
    expect(formatRecoveryDisplay("Le Petit Prince")).toEqual({
      kind: "value",
      text: "Le Petit Prince",
    });
  });

  it("escapes \\n into the visible \\n sequence", () => {
    expect(formatRecoveryDisplay("abc\ndef")).toEqual({
      kind: "value",
      text: "abc\\ndef",
    });
  });

  it("escapes \\r and \\t with friendly glyphs", () => {
    expect(formatRecoveryDisplay("a\rb\tc")).toEqual({
      kind: "value",
      text: "a\\rb\\tc",
    });
  });

  it("escapes BiDi override U+202E (filename-spoofing pattern)", () => {
    expect(formatRecoveryDisplay(`a${RLO}b`)).toEqual({
      kind: "value",
      text: "a\\u202Eb",
    });
  });

  it("escapes zero-width space U+200B and BOM U+FEFF", () => {
    expect(formatRecoveryDisplay(`a${ZWSP}b`)).toEqual({
      kind: "value",
      text: "a\\u200Bb",
    });
    expect(formatRecoveryDisplay(`${BOM}a`)).toEqual({
      kind: "value",
      text: "\\uFEFFa",
    });
  });

  it("escapes the LRI/RLI/PDI isolation block", () => {
    expect(formatRecoveryDisplay(`${LRI}rtl${PDI}`)).toEqual({
      kind: "value",
      text: "\\u2066rtl\\u2069",
    });
  });

  it("escapes the U+2028 line separator (would break inline layout)", () => {
    expect(formatRecoveryDisplay(`a${LSEP}b`)).toEqual({
      kind: "value",
      text: "a\\u2028b",
    });
  });

  it("preserves accented characters and emoji unchanged", () => {
    expect(formatRecoveryDisplay("café 🦀")).toEqual({
      kind: "value",
      text: "café 🦀",
    });
  });

  it("escapes multiple offenders in the same string", () => {
    expect(formatRecoveryDisplay(`a\nb${RLO}c`)).toEqual({
      kind: "value",
      text: "a\\nb\\u202Ec",
    });
  });
});
