import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import { createStory } from "./story";

describe("createStory", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the create_story command with the expected payload shape", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "id-1", title: "Un titre" });
    const result = await createStory({ title: "Un titre" });
    expect(invoke).toHaveBeenCalledWith("create_story", {
      input: { title: "Un titre" },
    });
    expect(result).toEqual({ id: "id-1", title: "Un titre" });
  });

  it("propagates Rust AppError rejections verbatim so the UI can switch on code", async () => {
    const rustError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre requis",
      userAction: "Saisis un titre non vide pour créer l'histoire.",
      details: null,
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(createStory({ title: "" })).rejects.toEqual(rustError);
  });
});
