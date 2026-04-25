import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  ExportStoryContractDriftError,
  exportStoryWithSaveDialog,
} from "./import-export";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";
const SUGGESTED = "Mon histoire.rustory";

describe("exportStoryWithSaveDialog", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the export_story_with_save_dialog command with the expected payload shape", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      kind: "exported",
      destinationPath: "/tmp/histoire.rustory",
      bytesWritten: 451,
      contentChecksum: "a".repeat(64),
    });
    const result = await exportStoryWithSaveDialog({
      storyId: STORY_ID,
      suggestedFilename: SUGGESTED,
    });
    expect(invoke).toHaveBeenCalledWith("export_story_with_save_dialog", {
      input: { storyId: STORY_ID, suggestedFilename: SUGGESTED },
    });
    expect(result.kind).toBe("exported");
  });

  it("returns a cancelled outcome when Rust reports the user cancelled the dialog", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "cancelled" });
    const result = await exportStoryWithSaveDialog({
      storyId: STORY_ID,
      suggestedFilename: SUGGESTED,
    });
    expect(result).toEqual({ kind: "cancelled" });
  });

  it("rejects with ExportStoryContractDriftError (carrying the raw payload) on a shape drift", async () => {
    const raw = {
      kind: "exported",
      destinationPath: "/tmp/histoire.rustory",
      // bytesWritten missing
      contentChecksum: "a".repeat(64),
    };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await exportStoryWithSaveDialog({
      storyId: STORY_ID,
      suggestedFilename: SUGGESTED,
    }).then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as ExportStoryContractDriftError;
    expect(err).toBeInstanceOf(ExportStoryContractDriftError);
    expect(err.raw).toEqual(raw);
  });

  it("propagates an EXPORT_DESTINATION_UNAVAILABLE error verbatim", async () => {
    const rustError = {
      code: "EXPORT_DESTINATION_UNAVAILABLE",
      message: "Écriture refusée par le système pour ce dossier.",
      userAction: "Choisis un dossier où tu as les droits en écriture.",
      details: { source: "temp_create", kind: "permission_denied" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      exportStoryWithSaveDialog({ storyId: STORY_ID, suggestedFilename: SUGGESTED }),
    ).rejects.toEqual(rustError);
  });

  it("propagates a LIBRARY_INCONSISTENT error verbatim", async () => {
    const rustError = {
      code: "LIBRARY_INCONSISTENT",
      message: "Export impossible: histoire introuvable.",
      userAction: "Retourne à la bibliothèque et recharge la liste.",
      details: { source: "story_missing", id: STORY_ID },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      exportStoryWithSaveDialog({ storyId: STORY_ID, suggestedFilename: SUGGESTED }),
    ).rejects.toEqual(rustError);
  });
});
