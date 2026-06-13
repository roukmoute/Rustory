import { describe, expect, it } from "vitest";

import {
  isImportDeviceStoryInput,
  isImportDeviceStoryOutcome,
} from "./device-import";

function outcome(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    story: {
      id: "0197a5d0-0000-7000-8000-000000000000",
      title: "Histoire de ma Lunii (FAC5562D)",
    },
    packShortId: "FAC5562D",
    importedAt: "2026-06-10T12:00:00.000Z",
    ...overrides,
  };
}

describe("isImportDeviceStoryOutcome", () => {
  it("accepts the canonical outcome shape", () => {
    expect(isImportDeviceStoryOutcome(outcome())).toBe(true);
  });

  it("rejects null and non-objects", () => {
    expect(isImportDeviceStoryOutcome(null)).toBe(false);
    expect(isImportDeviceStoryOutcome("imported")).toBe(false);
    expect(isImportDeviceStoryOutcome(42)).toBe(false);
  });

  it("rejects an extra top-level key (serializer drift)", () => {
    expect(isImportDeviceStoryOutcome(outcome({ mountPath: "/leak" }))).toBe(
      false,
    );
  });

  it("rejects a missing field", () => {
    const missing = outcome();
    delete missing.packShortId;
    expect(isImportDeviceStoryOutcome(missing)).toBe(false);
  });

  it("rejects a story card with a non-canonical id or blank title", () => {
    expect(
      isImportDeviceStoryOutcome(outcome({ story: { id: "", title: "T" } })),
    ).toBe(false);
    // A non-UUID id is a drift: the route feeds it to /story/:id/edit.
    expect(
      isImportDeviceStoryOutcome(
        outcome({ story: { id: "local-1", title: "T" } }),
      ),
    ).toBe(false);
    expect(
      isImportDeviceStoryOutcome(
        outcome({
          story: {
            id: "0197A5D0-0000-7000-8000-000000000000", // uppercase ⇒ drift
            title: "T",
          },
        }),
      ),
    ).toBe(false);
    expect(
      isImportDeviceStoryOutcome(
        outcome({
          story: { id: "0197a5d0-0000-7000-8000-000000000000", title: "   " },
        }),
      ),
    ).toBe(false);
  });

  it("rejects a story card with an extra key", () => {
    expect(
      isImportDeviceStoryOutcome(
        outcome({ story: { id: "x", title: "T", packUuid: "leak" } }),
      ),
    ).toBe(false);
  });

  it("rejects a malformed packShortId (lowercase, wrong length, non-hex)", () => {
    expect(isImportDeviceStoryOutcome(outcome({ packShortId: "fac5562d" }))).toBe(
      false,
    );
    expect(isImportDeviceStoryOutcome(outcome({ packShortId: "FAC55" }))).toBe(
      false,
    );
    expect(
      isImportDeviceStoryOutcome(outcome({ packShortId: "GGGGGGGG" })),
    ).toBe(false);
  });

  it("rejects a non-ISO importedAt", () => {
    expect(
      isImportDeviceStoryOutcome(outcome({ importedAt: "yesterday" })),
    ).toBe(false);
    expect(
      isImportDeviceStoryOutcome(outcome({ importedAt: "2026-06-10T12:00:00Z" })),
    ).toBe(false);
  });
});

describe("isImportDeviceStoryInput", () => {
  const validInput = {
    deviceIdentifier: "0123456789abcdef0123456789abcdef",
    packUuid: "abababab-abab-abab-abab-ababfac5562d",
  };

  it("accepts the canonical input shape", () => {
    expect(isImportDeviceStoryInput(validInput)).toBe(true);
  });

  it("rejects null, non-objects and extra keys", () => {
    expect(isImportDeviceStoryInput(null)).toBe(false);
    expect(isImportDeviceStoryInput("input")).toBe(false);
    expect(
      isImportDeviceStoryInput({ ...validInput, mountPath: "/leak" }),
    ).toBe(false);
  });

  it("rejects a malformed deviceIdentifier (length, case, non-hex)", () => {
    expect(
      isImportDeviceStoryInput({ ...validInput, deviceIdentifier: "abc" }),
    ).toBe(false);
    expect(
      isImportDeviceStoryInput({
        ...validInput,
        deviceIdentifier: validInput.deviceIdentifier.toUpperCase(),
      }),
    ).toBe(false);
    expect(
      isImportDeviceStoryInput({
        ...validInput,
        deviceIdentifier: "g".repeat(32),
      }),
    ).toBe(false);
  });

  it("rejects a non-canonical packUuid", () => {
    expect(
      isImportDeviceStoryInput({ ...validInput, packUuid: "not-a-uuid" }),
    ).toBe(false);
    expect(
      isImportDeviceStoryInput({
        ...validInput,
        packUuid: validInput.packUuid.toUpperCase(),
      }),
    ).toBe(false);
    expect(
      isImportDeviceStoryInput({
        ...validInput,
        packUuid: "ababababababababababababfac5562d", // no hyphens
      }),
    ).toBe(false);
  });
});
