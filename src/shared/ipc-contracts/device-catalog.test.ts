import { describe, expect, it } from "vitest";

import {
  isCatalogStatusDto,
  isImportOfficialCatalogOutcome,
  isPackCoverDto,
} from "./device-catalog";

describe("isCatalogStatusDto", () => {
  it("accepts a non-negative integer count", () => {
    expect(isCatalogStatusDto({ count: 0 })).toBe(true);
    expect(isCatalogStatusDto({ count: 1200 })).toBe(true);
  });

  it("rejects a negative, fractional or non-number count", () => {
    expect(isCatalogStatusDto({ count: -1 })).toBe(false);
    expect(isCatalogStatusDto({ count: 1.5 })).toBe(false);
    expect(isCatalogStatusDto({ count: "12" })).toBe(false);
  });

  it("rejects extra keys", () => {
    expect(isCatalogStatusDto({ count: 1, lastUpdated: "x" })).toBe(false);
  });
});

describe("isImportOfficialCatalogOutcome", () => {
  it("accepts a cancelled outcome", () => {
    expect(isImportOfficialCatalogOutcome({ kind: "cancelled" })).toBe(true);
  });

  it("accepts an imported outcome with a count", () => {
    expect(isImportOfficialCatalogOutcome({ kind: "imported", count: 5 })).toBe(
      true,
    );
  });

  it("rejects an imported outcome without a valid count", () => {
    expect(isImportOfficialCatalogOutcome({ kind: "imported" })).toBe(false);
    expect(
      isImportOfficialCatalogOutcome({ kind: "imported", count: -2 }),
    ).toBe(false);
  });

  it("rejects an unknown kind and extra keys", () => {
    expect(isImportOfficialCatalogOutcome({ kind: "weird" })).toBe(false);
    expect(
      isImportOfficialCatalogOutcome({ kind: "cancelled", count: 1 }),
    ).toBe(false);
  });
});

describe("isPackCoverDto", () => {
  it("accepts a data: URL", () => {
    expect(isPackCoverDto({ dataUrl: "data:image/png;base64,AAAA" })).toBe(true);
  });

  it("rejects a non-data URL (no implicit remote reference)", () => {
    expect(isPackCoverDto({ dataUrl: "https://example/cover.png" })).toBe(false);
    expect(isPackCoverDto({ dataUrl: "" })).toBe(false);
  });

  it("rejects extra keys", () => {
    expect(
      isPackCoverDto({ dataUrl: "data:image/png;base64,AAAA", uuid: "x" }),
    ).toBe(false);
  });
});
