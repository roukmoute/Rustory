import { describe, expect, it } from "vitest";

import {
  isExportStoryDialogOutcome,
  type ExportStoryDialogOutcome,
} from "./import-export";

const VALID_EXPORTED: ExportStoryDialogOutcome = {
  kind: "exported",
  destinationPath: "/tmp/histoire.rustory",
  bytesWritten: 451,
  contentChecksum: "a".repeat(64),
};

describe("isExportStoryDialogOutcome", () => {
  it("accepts a canonical exported payload", () => {
    expect(isExportStoryDialogOutcome(VALID_EXPORTED)).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isExportStoryDialogOutcome({ kind: "cancelled" })).toBe(true);
  });

  it("rejects a cancelled payload that carries extra fields", () => {
    expect(
      isExportStoryDialogOutcome({ kind: "cancelled", leaked: true }),
    ).toBe(false);
  });

  it("rejects an unknown kind", () => {
    expect(isExportStoryDialogOutcome({ kind: "weird" })).toBe(false);
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isExportStoryDialogOutcome(value)).toBe(false);
    },
  );

  it("rejects an exported payload with a missing field", () => {
    const { bytesWritten: _b, ...rest } = VALID_EXPORTED;
    expect(isExportStoryDialogOutcome(rest)).toBe(false);
  });

  it("rejects an empty destinationPath", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, destinationPath: "" }),
    ).toBe(false);
  });

  it("rejects a negative bytesWritten", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, bytesWritten: -1 }),
    ).toBe(false);
  });

  it("rejects a non-integer bytesWritten", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, bytesWritten: 1.5 }),
    ).toBe(false);
  });

  it("rejects a short contentChecksum", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, contentChecksum: "abc" }),
    ).toBe(false);
  });

  it("rejects a contentChecksum with non-hex characters", () => {
    expect(
      isExportStoryDialogOutcome({
        ...VALID_EXPORTED,
        contentChecksum: "z".repeat(64),
      }),
    ).toBe(false);
  });
});
