import { describe, expect, it } from "vitest";

import {
  isSendPackToDeviceInput,
  isSendPackToDeviceOutcome,
} from "./device-send";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("isSendPackToDeviceInput", () => {
  it("accepts the canonical device identifier", () => {
    expect(isSendPackToDeviceInput({ deviceIdentifier: DEVICE_ID })).toBe(true);
  });

  it("rejects a non-hex device identifier", () => {
    expect(isSendPackToDeviceInput({ deviceIdentifier: "ZZZ" })).toBe(false);
  });

  it("rejects an unknown field so no path can be smuggled in", () => {
    // The archive is picked in a Rust-owned NATIVE dialog: an
    // `archivePath` crossing IPC would be a boundary breach.
    expect(
      isSendPackToDeviceInput({
        deviceIdentifier: DEVICE_ID,
        archivePath: "/sneaky.zip",
      }),
    ).toBe(false);
  });
});

describe("isSendPackToDeviceOutcome", () => {
  it("accepts a cancelled outcome (a dismissed picker is a non-event)", () => {
    expect(isSendPackToDeviceOutcome({ kind: "cancelled" })).toBe(true);
  });

  it("accepts a sent outcome with canonical uuid and counts", () => {
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID,
        imageCount: 117,
        audioCount: 223,
      }),
    ).toBe(true);
  });

  it("accepts zero counts (a text-only pack)", () => {
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID,
        imageCount: 0,
        audioCount: 0,
      }),
    ).toBe(true);
  });

  it("rejects an unknown kind", () => {
    expect(isSendPackToDeviceOutcome({ kind: "exploded" })).toBe(false);
  });

  it("rejects a hostile kind resolving through the prototype chain", () => {
    expect(isSendPackToDeviceOutcome({ kind: "constructor" })).toBe(false);
  });

  it("rejects a non-canonical or uppercase pack uuid", () => {
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID.toUpperCase(),
        imageCount: 1,
        audioCount: 1,
      }),
    ).toBe(false);
  });

  it("rejects negative or non-integer counts", () => {
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID,
        imageCount: -1,
        audioCount: 0,
      }),
    ).toBe(false);
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID,
        imageCount: 1.5,
        audioCount: 0,
      }),
    ).toBe(false);
  });

  it("rejects an extra key on either kind", () => {
    expect(
      isSendPackToDeviceOutcome({ kind: "cancelled", extra: true }),
    ).toBe(false);
    expect(
      isSendPackToDeviceOutcome({
        kind: "sent",
        packUuid: PACK_UUID,
        imageCount: 1,
        audioCount: 1,
        family: "lunii",
      }),
    ).toBe(false);
  });

  it("rejects a cancelled payload missing nothing but carrying sent fields", () => {
    expect(
      isSendPackToDeviceOutcome({
        kind: "cancelled",
        packUuid: PACK_UUID,
      }),
    ).toBe(false);
  });
});
