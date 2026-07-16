import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  ContentSourcePolicyContractDriftError,
  ExportStoryContractDriftError,
  ImportArtifactContractDriftError,
  OsOpenContractDriftError,
  RssCreationContractDriftError,
  StructuredCreationContractDriftError,
  acceptArtifactImport,
  acceptRssStoryCreation,
  acceptStructuredCreation,
  analyzeArtifactForImport,
  analyzeOsOpenRequest,
  analyzeStructuredFolderForCreation,
  discardOsOpenRequest,
  exportStoryWithSaveDialog,
  fetchRssSourcePreview,
  readContentSourcePolicy,
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
      exportStoryWithSaveDialog({
        storyId: STORY_ID,
        suggestedFilename: SUGGESTED,
      }),
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
      exportStoryWithSaveDialog({
        storyId: STORY_ID,
        suggestedFilename: SUGGESTED,
      }),
    ).rejects.toEqual(rustError);
  });
});

const IMPORTABLE_CONTENT = {
  title: "Le Soleil",
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-06-20T10:00:00.000Z",
  updatedAt: "2026-06-24T14:15:00.000Z",
};

const ANALYZED = {
  kind: "analyzed",
  quality: "partial",
  state: "needsReview",
  findings: [
    { aspect: "title", category: "ambiguous", message: "Titre normalisé." },
  ],
  importableContent: IMPORTABLE_CONTENT,
  sourceName: "histoire.rustory",
  artifactChecksum: "b".repeat(64),
};

describe("analyzeArtifactForImport", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls analyze_artifact_for_import and returns the verdict", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(ANALYZED);
    const result = await analyzeArtifactForImport();
    expect(invoke).toHaveBeenCalledWith("analyze_artifact_for_import");
    expect(result.kind).toBe("analyzed");
  });

  it("returns a cancelled verdict when the dialog was dismissed", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "cancelled" });
    expect(await analyzeArtifactForImport()).toEqual({ kind: "cancelled" });
  });

  it("rejects with ImportArtifactContractDriftError on a shape drift", async () => {
    const raw = { kind: "analyzed", quality: "weird" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await analyzeArtifactForImport().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as ImportArtifactContractDriftError;
    expect(err).toBeInstanceOf(ImportArtifactContractDriftError);
    expect(err.raw).toEqual(raw);
  });

  it("normalizes a non-AppError transport rejection", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("boom");
    await expect(analyzeArtifactForImport()).rejects.toMatchObject({
      code: "UNKNOWN",
    });
  });
});

describe("acceptArtifactImport", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls accept_artifact_import with the input payload and returns the card", async () => {
    const card = {
      id: "0197a5d0-0000-7000-8000-000000000001",
      title: "Le Soleil",
      importState: "needsReview",
    };
    vi.mocked(invoke).mockResolvedValueOnce(card);
    const input = {
      content: IMPORTABLE_CONTENT,
      sourceName: "histoire.rustory",
      artifactChecksum: "b".repeat(64),
    };
    const result = await acceptArtifactImport(input);
    expect(invoke).toHaveBeenCalledWith("accept_artifact_import", { input });
    expect(result.id).toBe("0197a5d0-0000-7000-8000-000000000001");
  });

  it("rejects with a drift error when the returned card is malformed", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "", title: "" });
    await expect(
      acceptArtifactImport({
        content: IMPORTABLE_CONTENT,
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).rejects.toBeInstanceOf(ImportArtifactContractDriftError);
  });

  it("propagates an IMPORT_FAILED error verbatim", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: enregistrement local refusé.",
      userAction:
        "Réessaie ; si le problème persiste, consulte les traces locales.",
      details: { source: "db_commit", stage: "insert_story" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      acceptArtifactImport({
        content: IMPORTABLE_CONTENT,
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).rejects.toEqual(rustError);
  });
});

describe("analyzeOsOpenRequest", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls analyze_os_open_request and returns the validated verdict", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(ANALYZED);
    const result = await analyzeOsOpenRequest();
    expect(invoke).toHaveBeenCalledWith("analyze_os_open_request");
    expect(result.kind).toBe("analyzed");
  });

  it("resolves the none and multipleFiles kinds as-is", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "none" });
    expect(await analyzeOsOpenRequest()).toEqual({ kind: "none" });

    const limit = {
      kind: "multipleFiles",
      message:
        "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément.",
    };
    vi.mocked(invoke).mockResolvedValueOnce(limit);
    expect(await analyzeOsOpenRequest()).toEqual(limit);
  });

  it("rejects with OsOpenContractDriftError (carrying the raw payload) on a shape drift", async () => {
    const raw = { kind: "multipleFiles", message: "" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await analyzeOsOpenRequest().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as OsOpenContractDriftError;
    expect(err).toBeInstanceOf(OsOpenContractDriftError);
    expect(err.raw).toEqual(raw);
  });

  it("propagates an IMPORT_FAILED read error verbatim (the intent stays pending Rust-side)", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Import impossible: fichier illisible.",
      userAction:
        "Vérifie que le fichier existe, qu'il s'agit bien d'un artefact Rustory, puis réessaie.",
      details: { source: "file_read", stage: "metadata" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(analyzeOsOpenRequest()).rejects.toEqual(rustError);
  });

  it("normalizes a non-AppError transport rejection", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("boom");
    await expect(analyzeOsOpenRequest()).rejects.toMatchObject({
      code: "UNKNOWN",
    });
  });
});

describe("discardOsOpenRequest", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls discard_os_open_request and resolves", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(discardOsOpenRequest()).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("discard_os_open_request");
  });

  it("normalizes a transport rejection", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("boom");
    await expect(discardOsOpenRequest()).rejects.toMatchObject({
      code: "UNKNOWN",
    });
  });
});

// ===== Structured-folder creation facades =====

const FOLDER_ANALYZED = {
  kind: "analyzed",
  quality: "clean",
  state: "recognized",
  findings: [
    {
      aspect: "envelope",
      category: "recognized",
      message: "Manifest lisible.",
    },
    { aspect: "formatVersion", category: "recognized", message: "Version ok." },
    { aspect: "title", category: "recognized", message: "Titre valide." },
    {
      aspect: "structure",
      category: "recognized",
      message: "Structure reconnue.",
    },
    { aspect: "media", category: "recognized", message: "Médias présents." },
  ],
  creatableSummary: {
    title: "Le voyage de Nour",
    nodeCount: 2,
    retainedMedia: ["couverture.png"],
    discardedMedia: [],
  },
  folderName: "mon-dossier",
  folderPath: "/home/user/mon-dossier",
};

describe("analyzeStructuredFolderForCreation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the analyze_structured_folder_for_creation command with no payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(FOLDER_ANALYZED);
    const result = await analyzeStructuredFolderForCreation();
    expect(invoke).toHaveBeenCalledWith(
      "analyze_structured_folder_for_creation",
    );
    expect(result.kind).toBe("analyzed");
  });

  it("returns a cancelled outcome as a silent value", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "cancelled" });
    await expect(analyzeStructuredFolderForCreation()).resolves.toEqual({
      kind: "cancelled",
    });
  });

  it("rejects with a drift error when the verdict lacks folderPath", async () => {
    const { folderPath: _dropped, ...raw } = FOLDER_ANALYZED;
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await analyzeStructuredFolderForCreation().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as StructuredCreationContractDriftError;
    expect(err).toBeInstanceOf(StructuredCreationContractDriftError);
    expect(err.raw).toEqual(raw);
  });

  it("propagates an IMPORT_FAILED transport error verbatim", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message:
        "Création impossible: la fenêtre de sélection n'a pas pu s'ouvrir.",
      userAction:
        "Relance Rustory ; si le problème persiste, consulte les traces locales.",
      details: { source: "dialog_failed" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(analyzeStructuredFolderForCreation()).rejects.toEqual(
      rustError,
    );
  });
});

describe("acceptStructuredCreation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls the accept_structured_creation command with the folder pointer wrapped in input", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      id: "0197a5d0-0000-7000-8000-000000000001",
      title: "Le voyage de Nour",
      importState: "recognized",
    });
    const card = await acceptStructuredCreation({
      folderPath: "/home/user/mon-dossier",
    });
    expect(invoke).toHaveBeenCalledWith("accept_structured_creation", {
      input: { folderPath: "/home/user/mon-dossier" },
    });
    expect(card.title).toBe("Le voyage de Nour");
  });

  it("rejects with a drift error when the returned card is malformed", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ id: "", title: "" });
    await expect(
      acceptStructuredCreation({ folderPath: "/home/user/mon-dossier" }),
    ).rejects.toBeInstanceOf(StructuredCreationContractDriftError);
  });

  it("propagates a Rust refusal verbatim (revalidation)", async () => {
    const rustError = {
      code: "IMPORT_FAILED",
      message: "Création impossible: le dossier n'a pas pu être revalidé.",
      userAction:
        "Le contenu du dossier a peut-être changé. Relance l'analyse du dossier puis réessaie.",
      details: { source: "other", cause: "revalidation" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(rustError);
    await expect(
      acceptStructuredCreation({ folderPath: "/home/user/mon-dossier" }),
    ).rejects.toEqual(rustError);
  });
});

// ===== RSS external-source facades =====

const RSS_FEED_URL = "https://exemple.fr/flux.xml";

const RSS_PREVIEW_WIRE = {
  sourceHost: "exemple.fr",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      itemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
    },
  ],
  findings: [
    {
      aspect: "source",
      category: "ambiguous",
      message:
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
    },
  ],
  state: "needsReview",
  blocked: false,
};

describe("fetchRssSourcePreview", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls fetch_rss_source_preview with the feed address and returns the validated preview", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(RSS_PREVIEW_WIRE);
    const preview = await fetchRssSourcePreview(RSS_FEED_URL);
    expect(invoke).toHaveBeenCalledWith("fetch_rss_source_preview", {
      feedUrl: RSS_FEED_URL,
    });
    expect(preview.sourceHost).toBe("exemple.fr");
    expect(preview.items).toHaveLength(1);
  });

  it("resolves a blocked verdict (typed content problem, never a rejection)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      sourceHost: "exemple.fr",
      items: [],
      findings: [
        {
          aspect: "envelope",
          category: "blocking",
          message:
            "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux.",
        },
      ],
      state: "blocked",
      blocked: true,
    });
    const preview = await fetchRssSourcePreview(RSS_FEED_URL);
    expect(preview.blocked).toBe(true);
    expect(preview.items).toHaveLength(0);
  });

  it("normalizes a transport rejection into an AppError shape", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({
      code: "RSS_SOURCE_UNREACHABLE",
      message: "Récupération du flux impossible: la source est injoignable.",
      userAction: "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
      details: { source: "network", stage: "request" },
    });
    const err = (await fetchRssSourcePreview(RSS_FEED_URL).then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("RSS_SOURCE_UNREACHABLE");
  });

  it("rejects with RssCreationContractDriftError on a shape drift", async () => {
    const raw = { ...RSS_PREVIEW_WIRE, state: "recognized" };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await fetchRssSourcePreview(RSS_FEED_URL).then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as RssCreationContractDriftError;
    expect(err).toBeInstanceOf(RssCreationContractDriftError);
    expect(err.raw).toBe(raw);
  });
});

describe("acceptRssStoryCreation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("calls accept_rss_story_creation with the address + item reference and returns the created card", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      kind: "created",
      story: {
        id: "0197a5d0-0000-7000-8000-000000000000",
        title: "Episode 1",
        importState: "needsReview",
        importReport: [
          {
            aspect: "source",
            category: "ambiguous",
            message:
              "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
          },
        ],
      },
      report: [
        {
          aspect: "source",
          category: "ambiguous",
          message:
            "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
        },
      ],
    });
    const outcome = await acceptRssStoryCreation(RSS_FEED_URL, {
      kind: "guid",
      guid: "g-1",
      fingerprint: "a".repeat(64),
    });
    expect(invoke).toHaveBeenCalledWith("accept_rss_story_creation", {
      feedUrl: RSS_FEED_URL,
      itemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
    });
    expect(outcome.kind).toBe("created");
    if (outcome.kind === "created") {
      expect(outcome.story.title).toBe("Episode 1");
      expect(outcome.story.importState).toBe("needsReview");
    }
  });

  it("resolves the honest sourceChanged refusal (typed, never a rejection)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "sourceChanged" });
    const outcome = await acceptRssStoryCreation(RSS_FEED_URL, {
      kind: "titleLink",
      title: "Episode",
      link: null,
      fingerprint: "a".repeat(64),
    });
    expect(outcome).toEqual({ kind: "sourceChanged" });
  });

  it("rejects with RssCreationContractDriftError on a drifted story payload", async () => {
    const raw = { kind: "created", story: { id: "x" }, report: [] };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await acceptRssStoryCreation(RSS_FEED_URL, {
      kind: "guid",
      guid: "g-1",
      fingerprint: "a".repeat(64),
    }).then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as RssCreationContractDriftError;
    expect(err).toBeInstanceOf(RssCreationContractDriftError);
    expect(err.raw).toBe(raw);
  });

  it("rejects a sourceChanged refusal that leaks extra fields", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      kind: "sourceChanged",
      leaked: true,
    });
    const err = (await acceptRssStoryCreation(RSS_FEED_URL, {
      kind: "guid",
      guid: "g-1",
      fingerprint: "a".repeat(64),
    }).then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as RssCreationContractDriftError;
    expect(err).toBeInstanceOf(RssCreationContractDriftError);
  });
});

describe("readContentSourcePolicy", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  const OFFICIAL_POLICY = {
    sources: [
      {
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      },
      {
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
      {
        kind: "jsonFeed",
        label: "Flux JSON Feed",
        activation: "notActivated",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      },
    ],
  };

  it("resolves the validated policy from the pure read", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(OFFICIAL_POLICY);
    const policy = await readContentSourcePolicy();
    expect(invoke).toHaveBeenCalledWith("read_content_source_policy");
    expect(policy.sources).toHaveLength(3);
    expect(policy.sources[0]).toEqual({
      kind: "rss",
      label: "Flux RSS",
      activation: "enabled",
      activationMarker: "Activée par la distribution officielle",
    });
    expect(policy.sources[1].reason).toBe(
      "Source indisponible: non activée dans la distribution officielle",
    );
  });

  it("rejects with ContentSourcePolicyContractDriftError on a drifted payload", async () => {
    const raw = { sources: [{ kind: "torrent", activation: "enabled" }] };
    vi.mocked(invoke).mockResolvedValueOnce(raw);
    const err = (await readContentSourcePolicy().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as ContentSourcePolicyContractDriftError;
    expect(err).toBeInstanceOf(ContentSourcePolicyContractDriftError);
    expect(err.raw).toBe(raw);
  });

  it("normalizes an IPC rejection into an AppError (fail-closed upstream)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("ipc down"));
    const err = (await readContentSourcePolicy().then(
      () => {
        throw new Error("expected rejection");
      },
      (e: unknown) => e,
    )) as { code: string };
    expect(err.code).toBe("UNKNOWN");
  });
});
