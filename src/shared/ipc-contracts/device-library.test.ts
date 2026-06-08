import { describe, expect, it } from "vitest";

import { isDeviceLibraryDto } from "./device-library";

const VALID_ID = "0123456789abcdef0123456789abcdef";

function story(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    uuid: "00000000-0000-0000-0000-00000000abcd",
    shortId: "0000ABCD",
    hidden: false,
    contentPresent: true,
    ...overrides,
  };
}

describe("isDeviceLibraryDto", () => {
  it("accepts kind=none", () => {
    expect(isDeviceLibraryDto({ kind: "none" })).toBe(true);
  });

  it("accepts kind=unsupported with a known reason and a null hint", () => {
    expect(
      isDeviceLibraryDto({
        kind: "unsupported",
        reason: "multipleCandidates",
        firmwareHint: null,
      }),
    ).toBe(true);
  });

  it("accepts kind=readable with a 32-hex identifier and a story list", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story(), story({ shortId: "0000BEEF", hidden: true })],
      }),
    ).toBe(true);
  });

  it("accepts kind=readable with an empty story list (empty device)", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [],
      }),
    ).toBe(true);
  });

  it("rejects an unknown kind", () => {
    expect(isDeviceLibraryDto({ kind: "weird" })).toBe(false);
  });

  it("rejects extra top-level keys (serializer drift)", () => {
    expect(isDeviceLibraryDto({ kind: "none", extra: 1 })).toBe(false);
  });

  it("rejects a non-32-hex deviceIdentifier", () => {
    expect(
      isDeviceLibraryDto({ kind: "readable", deviceIdentifier: "abc", stories: [] }),
    ).toBe(false);
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID.toUpperCase(),
        stories: [],
      }),
    ).toBe(false);
  });

  it("rejects stories that is not an array", () => {
    expect(
      isDeviceLibraryDto({ kind: "readable", deviceIdentifier: VALID_ID, stories: {} }),
    ).toBe(false);
  });

  it("rejects a story with an extra key", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ title: "leak" })],
      }),
    ).toBe(false);
  });

  it("rejects a story with an empty uuid or shortId", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ uuid: "" })],
      }),
    ).toBe(false);
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ shortId: "" })],
      }),
    ).toBe(false);
  });

  it("rejects a story with a non-boolean flag", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ hidden: "yes" })],
      }),
    ).toBe(false);
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ contentPresent: 1 })],
      }),
    ).toBe(false);
  });

  it("rejects an unsupported reason outside the closed set", () => {
    expect(
      isDeviceLibraryDto({
        kind: "unsupported",
        reason: "somethingElse",
        firmwareHint: null,
      }),
    ).toBe(false);
  });

  it("rejects null and non-objects", () => {
    expect(isDeviceLibraryDto(null)).toBe(false);
    expect(isDeviceLibraryDto("readable")).toBe(false);
    expect(isDeviceLibraryDto(42)).toBe(false);
  });
});
