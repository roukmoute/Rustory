import { describe, expect, it } from "vitest";

import { isStoryValidationDto } from "./story-validation";

const VALID_ID = "0123456789abcdef0123456789abcdef";
const VALID_STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

function blocker(
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    axis: "structure",
    cause: "checksumMismatch",
    message: "Les données locales ont changé.",
    userAction: "Restaure une sauvegarde saine.",
    ...overrides,
  };
}

function ready(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    kind: "ready",
    deviceIdentifier: VALID_ID,
    story: { id: VALID_STORY_ID, title: "Mon histoire" },
    verdict: "presumedTransferable",
    blockers: [],
    ...overrides,
  };
}

describe("isStoryValidationDto", () => {
  it("accepts kind=noDevice", () => {
    expect(isStoryValidationDto({ kind: "noDevice" })).toBe(true);
  });

  it("accepts a ready presumedTransferable verdict with no blockers", () => {
    expect(isStoryValidationDto(ready())).toBe(true);
  });

  it("accepts a ready blocked verdict with a canonical + device-profile blocker", () => {
    expect(
      isStoryValidationDto(
        ready({
          verdict: "blocked",
          blockers: [
            blocker(),
            blocker({
              axis: "deviceProfile",
              cause: "metadataUnsupported",
              message: "Profil non pris en charge.",
              userAction: "Consulte le profil de support.",
            }),
          ],
        }),
      ),
    ).toBe(true);
  });

  it("accepts each verdict in the closed set with coherent blockers", () => {
    const cases: Array<[string, Record<string, unknown>[]]> = [
      ["presumedTransferable", []],
      [
        "toFix",
        [
          blocker({
            cause: "titleInvalid",
            message: "Titre invalide.",
            userAction: "Renomme.",
          }),
        ],
      ],
      ["blocked", [blocker()]],
    ];
    for (const [verdict, blockers] of cases) {
      expect(isStoryValidationDto(ready({ verdict, blockers }))).toBe(true);
    }
  });

  it("rejects an unknown kind", () => {
    expect(isStoryValidationDto({ kind: "weird" })).toBe(false);
  });

  it("returns false (never throws) on inherited Object.prototype kind keys", () => {
    for (const kind of ["constructor", "toString", "hasOwnProperty", "__proto__"]) {
      expect(() => isStoryValidationDto({ kind })).not.toThrow();
      expect(isStoryValidationDto({ kind })).toBe(false);
    }
  });

  it("rejects extra top-level keys (serializer drift)", () => {
    expect(isStoryValidationDto({ kind: "noDevice", extra: 1 })).toBe(false);
    expect(isStoryValidationDto(ready({ mountPath: "/sneaky" }))).toBe(false);
  });

  it("rejects a verdict outside the closed set", () => {
    expect(isStoryValidationDto(ready({ verdict: "ready" }))).toBe(false);
  });

  it("rejects a non-32-hex deviceIdentifier", () => {
    expect(isStoryValidationDto(ready({ deviceIdentifier: "abc" }))).toBe(false);
    expect(
      isStoryValidationDto(ready({ deviceIdentifier: VALID_ID.toUpperCase() })),
    ).toBe(false);
  });

  it("rejects a non-canonical story.id", () => {
    expect(isStoryValidationDto(ready({ story: { id: "s1", title: "X" } }))).toBe(
      false,
    );
  });

  it("rejects a blank story.title", () => {
    expect(
      isStoryValidationDto(ready({ story: { id: VALID_STORY_ID, title: "" } })),
    ).toBe(false);
  });

  it("rejects a blockers value that is not an array", () => {
    expect(isStoryValidationDto(ready({ blockers: "none" }))).toBe(false);
  });

  it("rejects a blocker with an unknown axis", () => {
    expect(
      isStoryValidationDto(ready({ blockers: [blocker({ axis: "network" })] })),
    ).toBe(false);
  });

  it("rejects a blocker with an unknown cause", () => {
    expect(
      isStoryValidationDto(ready({ blockers: [blocker({ cause: "somethingElse" })] })),
    ).toBe(false);
  });

  it("rejects an impossible axis × cause pair (closed taxonomy)", () => {
    // A device-profile axis paired with a structure cause, and vice-versa: each
    // value is individually known, but the PAIR is not in the closed taxonomy.
    expect(
      isStoryValidationDto(
        ready({
          verdict: "blocked",
          blockers: [
            blocker({ axis: "deviceProfile", cause: "checksumMismatch" }),
          ],
        }),
      ),
    ).toBe(false);
    expect(
      isStoryValidationDto(
        ready({
          verdict: "blocked",
          blockers: [
            blocker({ axis: "structure", cause: "metadataUnsupported" }),
          ],
        }),
      ),
    ).toBe(false);
  });

  it("rejects a blocker on a declared-but-causeless axis (media / filesystem)", () => {
    for (const axis of ["media", "filesystem"]) {
      expect(
        isStoryValidationDto(
          ready({
            verdict: "blocked",
            blockers: [blocker({ axis, cause: "checksumMismatch" })],
          }),
        ),
      ).toBe(false);
    }
  });

  it("accepts a coherent deviceProfile blocker pair (wire-ready taxonomy)", () => {
    expect(
      isStoryValidationDto(
        ready({
          verdict: "blocked",
          blockers: [
            blocker({
              axis: "deviceProfile",
              cause: "metadataUnsupported",
              message: "Profil non pris en charge.",
              userAction: "Consulte le profil de support.",
            }),
          ],
        }),
      ),
    ).toBe(true);
  });

  it("rejects a verdict incoherent with its blockers (derived in Rust)", () => {
    // blocked with no blocker
    expect(
      isStoryValidationDto(ready({ verdict: "blocked", blockers: [] })),
    ).toBe(false);
    // presumedTransferable carrying a blocker
    expect(
      isStoryValidationDto(
        ready({ verdict: "presumedTransferable", blockers: [blocker()] }),
      ),
    ).toBe(false);
    // toFix carrying a blocking cause (only a fixable cause may yield toFix)
    expect(
      isStoryValidationDto(
        ready({
          verdict: "toFix",
          blockers: [blocker({ cause: "checksumMismatch" })],
        }),
      ),
    ).toBe(false);
  });

  it("accepts toFix with only the fixable titleInvalid cause", () => {
    expect(
      isStoryValidationDto(
        ready({
          verdict: "toFix",
          blockers: [
            blocker({
              cause: "titleInvalid",
              message: "Le titre n'est pas valide.",
              userAction: "Renomme l'histoire.",
            }),
          ],
        }),
      ),
    ).toBe(true);
  });

  it("rejects a blocker with an empty message or userAction", () => {
    expect(
      isStoryValidationDto(ready({ blockers: [blocker({ message: "" })] })),
    ).toBe(false);
    expect(
      isStoryValidationDto(ready({ blockers: [blocker({ userAction: "" })] })),
    ).toBe(false);
  });

  it("rejects an extra key inside a blocker", () => {
    expect(
      isStoryValidationDto(ready({ blockers: [blocker({ leak: 1 })] })),
    ).toBe(false);
  });

  it("rejects null and non-objects", () => {
    expect(isStoryValidationDto(null)).toBe(false);
    expect(isStoryValidationDto("ready")).toBe(false);
    expect(isStoryValidationDto(42)).toBe(false);
  });
});
