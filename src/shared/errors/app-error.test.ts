import { describe, expect, it } from "vitest";

import { isAppError, toAppError, type AppErrorCode } from "./app-error";

/**
 * Every stable discriminant the Rust core can produce MUST appear in
 * the TS `AppErrorCode` union. If a new variant is added on either
 * side without the mirror, this test fails loudly instead of leaving
 * the UI fallback to `UNKNOWN` silently.
 *
 * Mirror of `src-tauri/src/domain/shared/error.rs::AppErrorCode` —
 * keep alphabetical for easy cross-check.
 */
const EXPECTED_RUST_CODES = [
  "EXPORT_DESTINATION_UNAVAILABLE",
  "INVALID_STORY_TITLE",
  "LIBRARY_INCONSISTENT",
  "LOCAL_STORAGE_UNAVAILABLE",
  "RECOVERY_DRAFT_UNAVAILABLE",
] as const satisfies readonly AppErrorCode[];

describe("AppErrorCode ↔ Rust mirror", () => {
  it.each(EXPECTED_RUST_CODES)(
    "recognizes Rust-produced code %s as a valid AppError discriminant",
    (code) => {
      const raw = {
        code,
        message: `sample message for ${code}`,
        userAction: null,
        details: null,
      };
      expect(isAppError(raw)).toBe(true);
      const narrowed = toAppError(raw);
      expect(narrowed.code).toBe(code);
    },
  );

  it("falls back to UNKNOWN for non-AppError inputs", () => {
    const narrowed = toAppError(new Error("boom"));
    expect(narrowed.code).toBe("UNKNOWN");
  });

  it("preserves an already-valid AppError verbatim (no re-wrap)", () => {
    const raw = {
      code: "EXPORT_DESTINATION_UNAVAILABLE" as const,
      message: "msg",
      userAction: "action",
      details: { source: "temp_create" },
    };
    expect(toAppError(raw)).toBe(raw);
  });
});

describe("isAppError", () => {
  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isAppError(value)).toBe(false);
    },
  );

  it("rejects objects missing the code field", () => {
    expect(isAppError({ message: "x", userAction: null, details: null })).toBe(
      false,
    );
  });

  it("rejects objects with a non-string code", () => {
    expect(
      isAppError({ code: 42, message: "x", userAction: null, details: null }),
    ).toBe(false);
  });

  it("requires the details key to be present (even as null)", () => {
    expect(isAppError({ code: "LIBRARY_INCONSISTENT", message: "x", userAction: null })).toBe(
      false,
    );
  });
});
