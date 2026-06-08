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
