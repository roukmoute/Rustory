import { describe, expect, it } from "vitest";

import { isStoryDetailDto, type StoryDetailDto } from "./story";

const VALID_DETAIL: StoryDetailDto = {
  id: "0197a5d0-0000-7000-8000-000000000000",
  title: "Un brouillon",
  schemaVersion: 1,
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-04-23T09:00:00.000Z",
  updatedAt: "2026-04-23T10:00:00.000Z",
};

describe("isStoryDetailDto", () => {
  it("accepts a canonical payload", () => {
    expect(isStoryDetailDto(VALID_DETAIL)).toBe(true);
  });

  it.each([null, undefined, "string", 42, [] as unknown])(
    "rejects a non-object payload: %p",
    (value) => {
      expect(isStoryDetailDto(value)).toBe(false);
    },
  );

  it("rejects a missing id", () => {
    const { id: _omit, ...rest } = VALID_DETAIL;
    expect(isStoryDetailDto(rest)).toBe(false);
  });

  it("rejects an empty id", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, id: "" })).toBe(false);
  });

  it("rejects a blank title", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, title: "   " })).toBe(false);
  });

  it("rejects a non-integer schemaVersion", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, schemaVersion: 1.5 })).toBe(
      false,
    );
  });

  it("rejects schemaVersion < 1", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, schemaVersion: 0 })).toBe(false);
  });

  it("rejects a non-string structureJson", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, structureJson: 42 })).toBe(
      false,
    );
  });

  it("rejects a short contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "a".repeat(63) }),
    ).toBe(false);
  });

  it("rejects an uppercase contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "A".repeat(64) }),
    ).toBe(false);
  });

  it("rejects a non-hex contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "z".repeat(64) }),
    ).toBe(false);
  });

  it("rejects a createdAt that has no UTC marker at all", () => {
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        createdAt: "2026-04-23T09:00:00.000",
      }),
    ).toBe(false);
  });

  it("rejects an updatedAt with a non-UTC offset", () => {
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        updatedAt: "2026-04-23T10:00:00.000+02:00",
      }),
    ).toBe(false);
  });

  it("rejects an explicit +00:00 UTC offset (contract mandates Z suffix)", () => {
    // Rust serializes with `Z`. Accepting `+00:00` silently would
    // let a contract drift go unnoticed; the guard must stay strict.
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        createdAt: "2026-04-23T09:00:00.000+00:00",
      }),
    ).toBe(false);
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        updatedAt: "2026-04-23T10:00:00.000+00:00",
      }),
    ).toBe(false);
  });
});
