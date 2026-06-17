import { describe, expect, it } from "vitest";

import {
  isDeviceStoryTitleDto,
  isSetDeviceStoryTitleInput,
} from "./device-title";

const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("isSetDeviceStoryTitleInput", () => {
  it("accepts a canonical input", () => {
    expect(
      isSetDeviceStoryTitleInput({ packUuid: PACK_UUID, title: "Mon histoire" }),
    ).toBe(true);
  });

  it("rejects a non-canonical pack uuid", () => {
    expect(
      isSetDeviceStoryTitleInput({ packUuid: "not-a-uuid", title: "x" }),
    ).toBe(false);
    expect(
      isSetDeviceStoryTitleInput({
        packUuid: PACK_UUID.toUpperCase(),
        title: "x",
      }),
    ).toBe(false);
  });

  it("rejects a blank or non-string title", () => {
    expect(isSetDeviceStoryTitleInput({ packUuid: PACK_UUID, title: "   " })).toBe(
      false,
    );
    expect(isSetDeviceStoryTitleInput({ packUuid: PACK_UUID, title: 42 })).toBe(
      false,
    );
  });

  it("rejects unknown / extra keys (provenance is Rust-owned)", () => {
    expect(
      isSetDeviceStoryTitleInput({
        packUuid: PACK_UUID,
        title: "x",
        source: "user",
      }),
    ).toBe(false);
  });
});

describe("isDeviceStoryTitleDto", () => {
  it("accepts a stored user title", () => {
    expect(isDeviceStoryTitleDto({ title: "Mon histoire", source: "user" })).toBe(
      true,
    );
  });

  it("rejects an unknown provenance token", () => {
    expect(isDeviceStoryTitleDto({ title: "x", source: "community" })).toBe(
      false,
    );
  });

  it("rejects an empty title", () => {
    expect(isDeviceStoryTitleDto({ title: "", source: "user" })).toBe(false);
  });

  it("rejects extra keys", () => {
    expect(
      isDeviceStoryTitleDto({ title: "x", source: "user", thumbnail: "c" }),
    ).toBe(false);
  });
});
