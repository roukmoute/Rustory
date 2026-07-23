import { describe, expect, it } from "vitest";

import {
  isSendPackToDeviceInput,
  isSendPackToDeviceOutcome,
} from "./device-send";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("isSendPackToDeviceInput", () => {
  it("accepts the two canonical identifiers", () => {
    expect(
      isSendPackToDeviceInput({ deviceIdentifier: DEVICE_ID, storyId: STORY_ID }),
    ).toBe(true);
  });

  it("rejects a non-hex device identifier", () => {
    expect(
      isSendPackToDeviceInput({ deviceIdentifier: "ZZZ", storyId: STORY_ID }),
    ).toBe(false);
  });

  it("rejects a non-canonical story id", () => {
    expect(
      isSendPackToDeviceInput({ deviceIdentifier: DEVICE_ID, storyId: "nope" }),
    ).toBe(false);
  });

  it("rejects an unknown field so no path can be smuggled in", () => {
    // The archive is resolved by Rust from the story id: an `archivePath`
    // crossing IPC would be a boundary breach.
    expect(
      isSendPackToDeviceInput({
        deviceIdentifier: DEVICE_ID,
        storyId: STORY_ID,
        archivePath: "/sneaky.zip",
      }),
    ).toBe(false);
  });
});

describe("isSendPackToDeviceOutcome", () => {
  it("accepts a sent outcome with canonical uuid and counts", () => {
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID,
        imageCount: 117,
        audioCount: 223,
      }),
    ).toBe(true);
  });

  it("accepts zero counts (a text-only pack)", () => {
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID,
        imageCount: 0,
        audioCount: 0,
      }),
    ).toBe(true);
  });

  it("rejects a non-canonical or uppercase pack uuid", () => {
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID.toUpperCase(),
        imageCount: 1,
        audioCount: 1,
      }),
    ).toBe(false);
  });

  it("rejects negative or non-integer counts", () => {
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID,
        imageCount: -1,
        audioCount: 0,
      }),
    ).toBe(false);
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID,
        imageCount: 1.5,
        audioCount: 0,
      }),
    ).toBe(false);
  });

  it("rejects an extra key", () => {
    expect(
      isSendPackToDeviceOutcome({
        packUuid: PACK_UUID,
        imageCount: 1,
        audioCount: 1,
        family: "lunii",
      }),
    ).toBe(false);
  });

  it("rejects a missing count", () => {
    expect(isSendPackToDeviceOutcome({ packUuid: PACK_UUID, imageCount: 1 })).toBe(
      false,
    );
  });
});
