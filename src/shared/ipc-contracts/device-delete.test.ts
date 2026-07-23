import { describe, expect, it } from "vitest";

import {
  isDeleteDeviceStoryInput,
  isDeleteDeviceStoryOutcome,
} from "./device-delete";

const DEVICE_ID = "0123456789abcdef0123456789abcdef";
const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("isDeleteDeviceStoryInput", () => {
  it("accepts the two canonical identifiers", () => {
    expect(
      isDeleteDeviceStoryInput({ deviceIdentifier: DEVICE_ID, packUuid: PACK_UUID }),
    ).toBe(true);
  });

  it("rejects a non-hex device identifier", () => {
    expect(
      isDeleteDeviceStoryInput({ deviceIdentifier: "ZZZ", packUuid: PACK_UUID }),
    ).toBe(false);
  });

  it("rejects a non-canonical pack uuid", () => {
    expect(
      isDeleteDeviceStoryInput({ deviceIdentifier: DEVICE_ID, packUuid: "nope" }),
    ).toBe(false);
  });

  it("rejects an unknown field so no path can be smuggled in", () => {
    expect(
      isDeleteDeviceStoryInput({
        deviceIdentifier: DEVICE_ID,
        packUuid: PACK_UUID,
        mountPath: "/sneaky",
      }),
    ).toBe(false);
  });
});

describe("isDeleteDeviceStoryOutcome", () => {
  it("accepts a present-delete outcome", () => {
    expect(isDeleteDeviceStoryOutcome({ packUuid: PACK_UUID, wasPresent: true })).toBe(
      true,
    );
  });

  it("accepts an idempotent no-op outcome (wasPresent=false)", () => {
    expect(
      isDeleteDeviceStoryOutcome({ packUuid: PACK_UUID, wasPresent: false }),
    ).toBe(true);
  });

  it("rejects a missing / non-boolean wasPresent", () => {
    expect(isDeleteDeviceStoryOutcome({ packUuid: PACK_UUID })).toBe(false);
    expect(
      isDeleteDeviceStoryOutcome({ packUuid: PACK_UUID, wasPresent: "yes" }),
    ).toBe(false);
  });

  it("rejects an extra key", () => {
    expect(
      isDeleteDeviceStoryOutcome({
        packUuid: PACK_UUID,
        wasPresent: true,
        family: "lunii",
      }),
    ).toBe(false);
  });
});
