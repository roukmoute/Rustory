import { describe, expect, it } from "vitest";

import { isDeviceLibraryDto } from "./device-library";

const VALID_ID = "0123456789abcdef0123456789abcdef";

function story(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    uuid: "00000000-0000-0000-0000-00000000abcd",
    shortId: "0000ABCD",
    hidden: false,
    contentPresent: true,
    alreadyImported: false,
    title: null,
    titleSource: null,
    thumbnail: null,
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

  it("accepts a readable FLAM inventory (real wire — the DTO is family-neutral)", () => {
    // What the FLAM reader actually emits: a real story UUID from the
    // text index, its uppercase 8-hex tail, the same flags as a Lunii
    // entry. No family field exists on this wire — the guard passes it
    // exactly like a Lunii inventory.
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: "fedcba9876543210fedcba9876543210",
        stories: [
          story({
            uuid: "12345678-9abc-def0-1122-334455667788",
            shortId: "55667788",
            hidden: true,
          }),
        ],
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
        stories: [story({ unexpected: "leak" })],
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
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ alreadyImported: "yes" })],
      }),
    ).toBe(false);
  });

  it("rejects a story missing the alreadyImported stamp (Rust composes it)", () => {
    const incomplete = story();
    delete (incomplete as Record<string, unknown>).alreadyImported;
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [incomplete],
      }),
    ).toBe(false);
  });

  it("accepts a story stamped alreadyImported: true", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ alreadyImported: true })],
      }),
    ).toBe(true);
  });

  // --- Title recognition fields (story 2.6) ---

  it("accepts a recognized title with a known provenance and a cover", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [
          story({
            title: "Le Loup",
            titleSource: "official",
            thumbnail: "cover.png",
          }),
        ],
      }),
    ).toBe(true);
  });

  it("accepts a recognized title with a null cover (user / local-library)", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ title: "Mon histoire", titleSource: "user" })],
      }),
    ).toBe(true);
  });

  it("rejects a title without a provenance (coupling: both null or both set)", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ title: "Orphan", titleSource: null })],
      }),
    ).toBe(false);
  });

  it("rejects a provenance without a title (coupling)", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ title: null, titleSource: "official" })],
      }),
    ).toBe(false);
  });

  it("rejects an unknown provenance token", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ title: "X", titleSource: "community" })],
      }),
    ).toBe(false);
  });

  it("rejects a cover on an unrecognized pack (no title)", () => {
    expect(
      isDeviceLibraryDto({
        kind: "readable",
        deviceIdentifier: VALID_ID,
        stories: [story({ thumbnail: "cover.png" })],
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
