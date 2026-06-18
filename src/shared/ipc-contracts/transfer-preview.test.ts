import { describe, expect, it } from "vitest";

import { isTransferPreviewDto } from "./transfer-preview";

const VALID_ID = "0123456789abcdef0123456789abcdef";
const VALID_STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

function ready(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    kind: "ready",
    deviceIdentifier: VALID_ID,
    story: { id: VALID_STORY_ID, title: "Mon histoire" },
    onDevice: false,
    unchangedCount: 2,
    transferable: false,
    ...overrides,
  };
}

describe("isTransferPreviewDto", () => {
  it("accepts kind=noDevice", () => {
    expect(isTransferPreviewDto({ kind: "noDevice" })).toBe(true);
  });

  it("accepts kind=unsupported with a known reason", () => {
    expect(
      isTransferPreviewDto({ kind: "unsupported", reason: "multipleCandidates" }),
    ).toBe(true);
  });

  it("accepts a ready 'new' verdict", () => {
    expect(isTransferPreviewDto(ready())).toBe(true);
  });

  it("accepts a ready 'replace' verdict with unchangedCount=0", () => {
    expect(isTransferPreviewDto(ready({ onDevice: true, unchangedCount: 0 }))).toBe(
      true,
    );
  });

  it("rejects an unknown kind", () => {
    expect(isTransferPreviewDto({ kind: "weird" })).toBe(false);
  });

  it("returns false (never throws) on inherited Object.prototype kind keys", () => {
    // A plain-object key map would resolve these to a prototype value and make
    // the guard throw on `.has`; the Map-based lookup returns drift instead.
    for (const kind of ["constructor", "toString", "hasOwnProperty", "__proto__"]) {
      expect(() => isTransferPreviewDto({ kind })).not.toThrow();
      expect(isTransferPreviewDto({ kind })).toBe(false);
    }
  });

  it("rejects extra top-level keys (serializer drift)", () => {
    expect(isTransferPreviewDto({ kind: "noDevice", extra: 1 })).toBe(false);
    expect(isTransferPreviewDto(ready({ mountPath: "/sneaky" }))).toBe(false);
  });

  it("rejects an unsupported reason outside the closed set", () => {
    expect(
      isTransferPreviewDto({ kind: "unsupported", reason: "somethingElse" }),
    ).toBe(false);
  });

  it("rejects a non-32-hex deviceIdentifier", () => {
    expect(isTransferPreviewDto(ready({ deviceIdentifier: "abc" }))).toBe(false);
    expect(
      isTransferPreviewDto(ready({ deviceIdentifier: VALID_ID.toUpperCase() })),
    ).toBe(false);
  });

  it("rejects a non-canonical story.id", () => {
    expect(isTransferPreviewDto(ready({ story: { id: "s1", title: "X" } }))).toBe(
      false,
    );
    expect(
      isTransferPreviewDto(
        ready({ story: { id: VALID_STORY_ID.toUpperCase(), title: "X" } }),
      ),
    ).toBe(false);
  });

  it("rejects a blank story.title", () => {
    expect(
      isTransferPreviewDto(ready({ story: { id: VALID_STORY_ID, title: "" } })),
    ).toBe(false);
  });

  it("rejects an extra key inside story", () => {
    expect(
      isTransferPreviewDto(
        ready({ story: { id: VALID_STORY_ID, title: "X", leak: 1 } }),
      ),
    ).toBe(false);
  });

  it("rejects a non-integer / negative unchangedCount", () => {
    expect(isTransferPreviewDto(ready({ unchangedCount: 1.5 }))).toBe(false);
    expect(isTransferPreviewDto(ready({ unchangedCount: -1 }))).toBe(false);
    expect(isTransferPreviewDto(ready({ unchangedCount: "2" }))).toBe(false);
  });

  it("rejects non-boolean onDevice / transferable", () => {
    expect(isTransferPreviewDto(ready({ onDevice: "yes" }))).toBe(false);
    expect(isTransferPreviewDto(ready({ transferable: 0 }))).toBe(false);
  });

  it("rejects null and non-objects", () => {
    expect(isTransferPreviewDto(null)).toBe(false);
    expect(isTransferPreviewDto("ready")).toBe(false);
    expect(isTransferPreviewDto(42)).toBe(false);
  });
});
