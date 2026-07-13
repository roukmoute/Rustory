import { describe, expect, it } from "vitest";

import { isConnectedDeviceDto } from "./device";

const validSupported = {
  kind: "supported",
  family: "lunii",
  firmwareCohort: "origineV1",
  metadataFormatVersion: 3,
  deviceIdentifier: "0123456789abcdef0123456789abcdef",
  supportedOperations: {
    readLibrary: true,
    inspectStory: true,
    importStory: true,
    writeStory: false,
  },
};

// REAL Rust wire for a supported FLAM Gen1 (see the byte-for-byte
// contract test src-tauri/tests/contracts/device.rs): the
// `metadataFormatVersion` KEY is absent — never null — and the matrix
// line carries the activated read capabilities (readLibrary /
// inspectStory / importStory true, writeStory false). A verdict emitted
// by Rust must NEVER be rejected by this guard.
const validSupportedFlam = JSON.parse(
  '{"kind":"supported","family":"flam","firmwareCohort":"flamGen1",' +
    '"deviceIdentifier":"fedcba9876543210fedcba9876543210",' +
    '"supportedOperations":{"readLibrary":true,"inspectStory":true,' +
    '"importStory":true,"writeStory":false}}',
) as Record<string, unknown>;

describe("isConnectedDeviceDto — valid payloads", () => {
  it("accepts kind=none", () => {
    expect(isConnectedDeviceDto({ kind: "none" })).toBe(true);
  });

  it("accepts a complete supported payload", () => {
    expect(isConnectedDeviceDto(validSupported)).toBe(true);
  });

  it("accepts the V3 cohort with import_story=false", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        firmwareCohort: "v3",
        metadataFormatVersion: 7,
        supportedOperations: {
          readLibrary: true,
          inspectStory: true,
          importStory: false,
          writeStory: false,
        },
      }),
    ).toBe(true);
  });

  it("accepts unsupported with typed reason and string hint", () => {
    expect(
      isConnectedDeviceDto({
        kind: "unsupported",
        reason: "metadataUnsupported",
        firmwareHint: "metadata_v99",
      }),
    ).toBe(true);
  });

  it("accepts unsupported with null hint", () => {
    expect(
      isConnectedDeviceDto({
        kind: "unsupported",
        reason: "metadataCorrupt",
        firmwareHint: null,
      }),
    ).toBe(true);
  });

  it("accepts ambiguous with candidateCount=2", () => {
    expect(
      isConnectedDeviceDto({ kind: "ambiguous", candidateCount: 2 }),
    ).toBe(true);
  });

  it("accepts the real FLAM Gen1 wire (no metadataFormatVersion key)", () => {
    expect(isConnectedDeviceDto(validSupportedFlam)).toBe(true);
  });

  it("imposes no capability rule on flam — FAMILY_CONTRACTS freezes family⇔cohort⇔version only", () => {
    // The guard must NEVER assume "flam ⇒ all operations false": the
    // activated read capabilities pass (the real wire above), and a
    // zero-capability payload stays representable for any future
    // zero-capability profile.
    expect(isConnectedDeviceDto(validSupportedFlam)).toBe(true);
    expect(
      isConnectedDeviceDto({
        ...validSupportedFlam,
        supportedOperations: {
          readLibrary: false,
          inspectStory: false,
          importStory: false,
          writeStory: false,
        },
      }),
    ).toBe(true);
  });
});

describe("isConnectedDeviceDto — FAMILY_CONTRACTS closed combinations", () => {
  // The five cross rejections: an illegal family⇔cohort⇔version
  // combination must be unrepresentable at the boundary, even though
  // each half would pass an independent set membership check.

  it("rejects lunii paired with the flamGen1 cohort", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, firmwareCohort: "flamGen1" }),
    ).toBe(false);
  });

  it("rejects flam paired with a lunii cohort", () => {
    expect(
      isConnectedDeviceDto({ ...validSupportedFlam, firmwareCohort: "origineV1" }),
    ).toBe(false);
  });

  it("rejects flam carrying a metadataFormatVersion key", () => {
    expect(
      isConnectedDeviceDto({ ...validSupportedFlam, metadataFormatVersion: 3 }),
    ).toBe(false);
  });

  it("rejects flam carrying a null metadataFormatVersion (absent means NO key)", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupportedFlam,
        metadataFormatVersion: null,
      }),
    ).toBe(false);
  });

  it("rejects lunii missing its metadataFormatVersion", () => {
    const { metadataFormatVersion: _dropped, ...withoutVersion } =
      validSupported;
    expect(isConnectedDeviceDto(withoutVersion)).toBe(false);
  });

  it("rejects an unknown family even with a known cohort", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, family: "tonies" }),
    ).toBe(false);
  });

  it("rejects Object.prototype member names as family with a boolean, never a TypeError", () => {
    // A plain `FAMILY_CONTRACTS[family]` indexation would walk the
    // prototype chain for these names and crash the guard instead of
    // rejecting the drift.
    for (const hostile of ["constructor", "toString", "__proto__", "hasOwnProperty"]) {
      expect(
        isConnectedDeviceDto({ ...validSupported, family: hostile }),
        hostile,
      ).toBe(false);
    }
  });

  it("rejects Object.prototype member names as kind with a boolean, never a TypeError", () => {
    // Same prototype-safety discipline on the `kind` discriminant
    // (ALLOWED_KEYS lookup).
    for (const hostile of ["constructor", "toString", "__proto__"]) {
      expect(isConnectedDeviceDto({ kind: hostile }), hostile).toBe(false);
    }
  });
});

describe("isConnectedDeviceDto — drift rejections", () => {
  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isConnectedDeviceDto(value)).toBe(false);
    },
  );

  it("rejects an unknown kind", () => {
    expect(
      isConnectedDeviceDto({ kind: "unknown_variant" } as never),
    ).toBe(false);
  });

  it("rejects supported with unknown family", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, family: "tonies" }),
    ).toBe(false);
  });

  it("rejects supported with unknown firmwareCohort", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, firmwareCohort: "v4" }),
    ).toBe(false);
  });

  it("rejects supported with non-integer metadata version", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        metadataFormatVersion: 3.5,
      }),
    ).toBe(false);
  });

  it("rejects supported with metadata version > 127", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        metadataFormatVersion: 200,
      }),
    ).toBe(false);
  });

  it("rejects supported with empty deviceIdentifier", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, deviceIdentifier: "" }),
    ).toBe(false);
  });

  it("rejects supported with non-hex deviceIdentifier", () => {
    expect(
      isConnectedDeviceDto({ ...validSupported, deviceIdentifier: "abc" }),
    ).toBe(false);
  });

  it("rejects supported with uppercase-hex deviceIdentifier", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        deviceIdentifier: "0123456789ABCDEF0123456789ABCDEF",
      }),
    ).toBe(false);
  });

  it("rejects supported with wrong-length deviceIdentifier", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        deviceIdentifier: "0123456789abcdef0123456789abcdef00",
      }),
    ).toBe(false);
  });

  it("rejects supported payload carrying an extra field", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        unexpected: "drift",
      } as never),
    ).toBe(false);
  });

  it("rejects unsupported payload carrying an extra field", () => {
    expect(
      isConnectedDeviceDto({
        kind: "unsupported",
        reason: "metadataCorrupt",
        firmwareHint: null,
        unexpected: "drift",
      } as never),
    ).toBe(false);
  });

  it("rejects none payload carrying an extra field", () => {
    expect(
      isConnectedDeviceDto({ kind: "none", unexpected: "drift" } as never),
    ).toBe(false);
  });

  it("rejects supported with non-boolean supportedOperations field", () => {
    expect(
      isConnectedDeviceDto({
        ...validSupported,
        supportedOperations: {
          ...validSupported.supportedOperations,
          readLibrary: "true",
        },
      }),
    ).toBe(false);
  });

  it("rejects supported missing supportedOperations entirely", () => {
    const { supportedOperations: _drop, ...rest } = validSupported;
    void _drop;
    expect(isConnectedDeviceDto(rest)).toBe(false);
  });

  it("rejects unsupported with unrecognized reason", () => {
    expect(
      isConnectedDeviceDto({
        kind: "unsupported",
        reason: "metadata_unsupported", // snake_case must not be accepted
        firmwareHint: null,
      }),
    ).toBe(false);
  });

  it("rejects ambiguous with candidateCount < 2", () => {
    expect(
      isConnectedDeviceDto({ kind: "ambiguous", candidateCount: 1 }),
    ).toBe(false);
  });

  it("rejects ambiguous with non-integer candidateCount", () => {
    expect(
      isConnectedDeviceDto({ kind: "ambiguous", candidateCount: 2.5 }),
    ).toBe(false);
  });

  it("rejects payloads where snake_case fields leak", () => {
    expect(
      isConnectedDeviceDto({
        kind: "supported",
        family: "lunii",
        firmware_cohort: "origineV1",
        metadata_format_version: 3,
        device_identifier: "abc",
        supported_operations: validSupported.supportedOperations,
      } as never),
    ).toBe(false);
  });
});
