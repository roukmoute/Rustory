import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  applyRecovery,
  ApplyRecoveryContractDriftError,
  createStory,
  discardDraft,
  getStoryDetail,
  readRecoverableDraft,
  ReadRecoverableDraftContractDriftError,
  recordDraft,
  saveStory,
} from "./story";

const STORY_ID = "0197a5d0-0000-7000-8000-000000000000";

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

describe("saveStory", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the update_story command with the expected payload shape", async () => {
    const output = {
      id: STORY_ID,
      title: "Nouveau titre",
      updatedAt: "2026-04-23T10:00:00.000Z",
    };
    vi.mocked(invoke).mockResolvedValueOnce(output);
    const result = await saveStory({ id: STORY_ID, title: "Nouveau titre" });
    expect(invoke).toHaveBeenCalledWith("update_story", {
      input: { id: STORY_ID, title: "Nouveau titre" },
    });
    expect(result).toEqual(output);
  });

  it("propagates a LOCAL_STORAGE_UNAVAILABLE error verbatim", async () => {
    const rustError = {
      code: "LOCAL_STORAGE_UNAVAILABLE",
      message: "Rustory n'a pas pu enregistrer ta modification.",
      userAction:
        "Réessaie dans un instant ; si le problème persiste, consulte les traces locales.",
      details: {
        source: "sqlite_update",
        table: "stories",
        stage: "commit",
        kind: "busy",
      },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      saveStory({ id: STORY_ID, title: "Titre" }),
    ).rejects.toEqual(rustError);
  });

  it("propagates a LIBRARY_INCONSISTENT error verbatim when the story is missing", async () => {
    const rustError = {
      code: "LIBRARY_INCONSISTENT",
      message: "Histoire introuvable, recharge la bibliothèque.",
      userAction: "Retourne à la bibliothèque et recharge la liste.",
      details: { source: "story_missing", id: STORY_ID },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      saveStory({ id: STORY_ID, title: "Titre" }),
    ).rejects.toEqual(rustError);
  });
});

describe("getStoryDetail", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the get_story_detail command with the expected payload shape", async () => {
    const detail = {
      id: STORY_ID,
      title: "Un brouillon",
      schemaVersion: 1,
      structureJson: '{"schemaVersion":1,"nodes":[]}',
      contentChecksum: "a".repeat(64),
      createdAt: "2026-04-23T09:00:00.000Z",
      updatedAt: "2026-04-23T09:00:00.000Z",
    };
    vi.mocked(invoke).mockResolvedValueOnce(detail);
    const result = await getStoryDetail({ storyId: STORY_ID });
    expect(invoke).toHaveBeenCalledWith("get_story_detail", {
      storyId: STORY_ID,
    });
    expect(result).toEqual(detail);
  });

  it("returns null when the Rust core has no matching row", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    const result = await getStoryDetail({ storyId: "absent" });
    expect(result).toBeNull();
  });

  it("propagates a LIBRARY_INCONSISTENT error verbatim", async () => {
    const rustError = {
      code: "LIBRARY_INCONSISTENT",
      message: "La bibliothèque locale contient des histoires en double.",
      userAction: "Recharge Rustory pour reconstruire la vue cohérente.",
      details: null,
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(getStoryDetail({ storyId: STORY_ID })).rejects.toEqual(
      rustError,
    );
  });
});

describe("recordDraft", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes record_draft with the canonical wire shape (input wrapped)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await recordDraft({ storyId: STORY_ID, draftTitle: "Buffered" });
    expect(invoke).toHaveBeenCalledWith("record_draft", {
      input: { storyId: STORY_ID, draftTitle: "Buffered" },
    });
  });

  it("propagates an AppError verbatim on rejection", async () => {
    const rustError = {
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "Récupération indisponible.",
      userAction: "Vérifie le disque local.",
      details: { source: "sqlite_upsert", kind: "busy" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      recordDraft({ storyId: STORY_ID, draftTitle: "x" }),
    ).rejects.toEqual(rustError);
  });
});

describe("readRecoverableDraft", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes read_recoverable_draft with storyId payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    await readRecoverableDraft({ storyId: STORY_ID });
    expect(invoke).toHaveBeenCalledWith("read_recoverable_draft", {
      storyId: STORY_ID,
    });
  });

  it("returns canonical none when backend has no draft", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    const result = await readRecoverableDraft({ storyId: STORY_ID });
    expect(result).toEqual({ kind: "none" });
  });

  it("returns canonical recoverable when backend has a draft", async () => {
    const payload = {
      kind: "recoverable",
      storyId: STORY_ID,
      draftTitle: "Buffered",
      draftAt: "2026-04-25T12:00:00.000Z",
      persistedTitle: "Persisted",
    };
    vi.mocked(invoke).mockResolvedValueOnce(payload);
    const result = await readRecoverableDraft({ storyId: STORY_ID });
    expect(result).toEqual(payload);
  });

  it("throws ReadRecoverableDraftContractDriftError on drift", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "wrong" });
    await expect(
      readRecoverableDraft({ storyId: STORY_ID }),
    ).rejects.toBeInstanceOf(ReadRecoverableDraftContractDriftError);
  });

  it("captures the drifted payload on the error instance", async () => {
    const drift = { kind: "recoverable", storyId: "" };
    vi.mocked(invoke).mockResolvedValueOnce(drift);
    try {
      await readRecoverableDraft({ storyId: STORY_ID });
      throw new Error("expected throw");
    } catch (err) {
      expect(err).toBeInstanceOf(ReadRecoverableDraftContractDriftError);
      expect((err as ReadRecoverableDraftContractDriftError).raw).toEqual(drift);
    }
  });
});

describe("applyRecovery", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes apply_recovery with the canonical wire shape", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      id: STORY_ID,
      title: "Recovered",
      updatedAt: "2026-04-25T12:00:00.000Z",
    });
    await applyRecovery({ storyId: STORY_ID });
    expect(invoke).toHaveBeenCalledWith("apply_recovery", {
      input: { storyId: STORY_ID },
    });
  });

  it("returns the UpdateStoryOutput on success", async () => {
    const output = {
      id: STORY_ID,
      title: "Recovered",
      updatedAt: "2026-04-25T12:00:00.000Z",
    };
    vi.mocked(invoke).mockResolvedValueOnce(output);
    const result = await applyRecovery({ storyId: STORY_ID });
    expect(result).toEqual(output);
  });

  it("throws ApplyRecoveryContractDriftError on drift", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "x", updatedAt: "bad" });
    await expect(
      applyRecovery({ storyId: STORY_ID }),
    ).rejects.toBeInstanceOf(ApplyRecoveryContractDriftError);
  });

  it("propagates an INVALID_STORY_TITLE rejection verbatim", async () => {
    const rustError = {
      code: "INVALID_STORY_TITLE",
      message: "Création impossible: titre contient des caractères non autorisés",
      userAction: "Supprime les sauts de ligne, tabulations et caractères invisibles.",
      details: { source: "recovery_draft_invalid", id: STORY_ID },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(applyRecovery({ storyId: STORY_ID })).rejects.toEqual(
      rustError,
    );
  });
});

describe("discardDraft", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("invokes discard_draft with the wrapped input shape (no expectedDraftAt)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await discardDraft({ storyId: STORY_ID });
    expect(invoke).toHaveBeenCalledWith("discard_draft", {
      input: { storyId: STORY_ID, expectedDraftAt: null },
    });
  });

  it("forwards expectedDraftAt as a CAS guard when supplied", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await discardDraft({
      storyId: STORY_ID,
      expectedDraftAt: "2026-04-25T12:00:00.000Z",
    });
    expect(invoke).toHaveBeenCalledWith("discard_draft", {
      input: {
        storyId: STORY_ID,
        expectedDraftAt: "2026-04-25T12:00:00.000Z",
      },
    });
  });

  it("propagates a RECOVERY_DRAFT_UNAVAILABLE rejection verbatim", async () => {
    const rustError = {
      code: "RECOVERY_DRAFT_UNAVAILABLE",
      message: "Récupération indisponible.",
      userAction: "Vérifie le disque local.",
      details: { source: "sqlite_delete" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(discardDraft({ storyId: STORY_ID })).rejects.toEqual(
      rustError,
    );
  });
});
