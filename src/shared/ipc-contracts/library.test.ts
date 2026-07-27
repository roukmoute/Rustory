import { describe, expect, it } from "vitest";

import { isStoryCardDto } from "./library";

const base = { id: "0197a5d0-0000-7000-8000-000000000000", title: "Titre" };

describe("isStoryCardDto — coverAssetId", () => {
  it("accepts a card with no coverAssetId (the minimal shape)", () => {
    expect(isStoryCardDto(base)).toBe(true);
  });

  it("accepts a non-empty coverAssetId string", () => {
    expect(isStoryCardDto({ ...base, coverAssetId: "asset-123" })).toBe(true);
  });

  it("rejects an empty or non-string coverAssetId (serializer drift)", () => {
    expect(isStoryCardDto({ ...base, coverAssetId: "" })).toBe(false);
    expect(isStoryCardDto({ ...base, coverAssetId: 42 })).toBe(false);
  });
});
