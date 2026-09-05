import { describe, expect, it } from "vitest";

import {
  isReorderDeviceStoriesInput,
  isReorderDeviceStoriesOutcome,
  movePackUuid,
} from "./device-reorder";

const A = "11111111-1111-1111-1111-1111aaaaaaaa";
const B = "22222222-2222-2222-2222-2222bbbbbbbb";
const C = "33333333-3333-3333-3333-3333cccccccc";
const DEVICE = "0123456789abcdef0123456789abcdef";

describe("reorder contract guards", () => {
  it("accepts distinct canonical uuids on a device identifier and refuses drifts", () => {
    expect(isReorderDeviceStoriesInput({ deviceIdentifier: DEVICE, orderedPackUuids: [A, B] })).toBe(true);
    expect(isReorderDeviceStoriesInput({ deviceIdentifier: "nope", orderedPackUuids: [A] })).toBe(false);
    expect(isReorderDeviceStoriesInput({ deviceIdentifier: DEVICE, orderedPackUuids: [] })).toBe(false);
    expect(isReorderDeviceStoriesInput({ deviceIdentifier: DEVICE, orderedPackUuids: [A, A] })).toBe(false);
    expect(isReorderDeviceStoriesInput({ deviceIdentifier: DEVICE, orderedPackUuids: [A.toUpperCase()] })).toBe(false);
    expect(isReorderDeviceStoriesOutcome({ count: 2, changed: true })).toBe(true);
    expect(isReorderDeviceStoriesOutcome({ count: "2", changed: true })).toBe(false);
  });

  it("moves a pack one step and leaves edge moves harmless", () => {
    expect(movePackUuid([A, B, C], B, -1)).toEqual([B, A, C]);
    expect(movePackUuid([A, B, C], B, 1)).toEqual([A, C, B]);
    expect(movePackUuid([A, B, C], A, -1)).toEqual([A, B, C]);
    expect(movePackUuid([A, B, C], C, 1)).toEqual([A, B, C]);
    expect(movePackUuid([A, B, C], "zz", 1)).toEqual([A, B, C]);
  });
});
