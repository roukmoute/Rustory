import { describe, expect, it } from "vitest";

import {
  isContentSourceEntry,
  isContentSourcePolicy,
  isExportStoryDialogOutcome,
  isImportArtifactAnalysis,
  isImportFinding,
  isRssItemRef,
  isRssPreview,
  isStructuredCreationAnalysis,
  type ContentSourcePolicy,
  type ExportStoryDialogOutcome,
  type ImportArtifactAnalysis,
  type RssPreview,
  type StructuredCreationAnalysis,
} from "./import-export";

const VALID_EXPORTED: ExportStoryDialogOutcome = {
  kind: "exported",
  destinationPath: "/tmp/histoire.rustory",
  bytesWritten: 451,
  contentChecksum: "a".repeat(64),
};

describe("isExportStoryDialogOutcome", () => {
  it("accepts a canonical exported payload", () => {
    expect(isExportStoryDialogOutcome(VALID_EXPORTED)).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isExportStoryDialogOutcome({ kind: "cancelled" })).toBe(true);
  });

  it("rejects a cancelled payload that carries extra fields", () => {
    expect(
      isExportStoryDialogOutcome({ kind: "cancelled", leaked: true }),
    ).toBe(false);
  });

  it("rejects an unknown kind", () => {
    expect(isExportStoryDialogOutcome({ kind: "weird" })).toBe(false);
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isExportStoryDialogOutcome(value)).toBe(false);
    },
  );

  it("rejects an exported payload with a missing field", () => {
    const { bytesWritten: _b, ...rest } = VALID_EXPORTED;
    expect(isExportStoryDialogOutcome(rest)).toBe(false);
  });

  it("rejects an empty destinationPath", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, destinationPath: "" }),
    ).toBe(false);
  });

  it("rejects a negative bytesWritten", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, bytesWritten: -1 }),
    ).toBe(false);
  });

  it("rejects a non-integer bytesWritten", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, bytesWritten: 1.5 }),
    ).toBe(false);
  });

  it("rejects a short contentChecksum", () => {
    expect(
      isExportStoryDialogOutcome({ ...VALID_EXPORTED, contentChecksum: "abc" }),
    ).toBe(false);
  });

  it("rejects a contentChecksum with non-hex characters", () => {
    expect(
      isExportStoryDialogOutcome({
        ...VALID_EXPORTED,
        contentChecksum: "z".repeat(64),
      }),
    ).toBe(false);
  });
});

const IMPORTABLE_CONTENT = {
  title: "Le Soleil",
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-06-20T10:00:00.000Z",
  updatedAt: "2026-06-24T14:15:00.000Z",
};

const ANALYZED_PARTIAL: ImportArtifactAnalysis = {
  kind: "analyzed",
  quality: "partial",
  state: "needsReview",
  findings: [
    {
      aspect: "title",
      category: "ambiguous",
      message: "Le titre a été normalisé à l'import.",
    },
  ],
  importableContent: IMPORTABLE_CONTENT,
  sourceName: "histoire.rustory",
  artifactChecksum: "b".repeat(64),
};

const ANALYZED_BLOCKED: ImportArtifactAnalysis = {
  kind: "analyzed",
  quality: "unusable",
  state: "blocked",
  findings: [
    {
      aspect: "integrity",
      category: "blocking",
      message: "Corruption détectée.",
    },
  ],
  sourceName: "corrompu.rustory",
  artifactChecksum: "c".repeat(64),
};

describe("isImportFinding", () => {
  it("accepts a well-formed finding", () => {
    expect(
      isImportFinding({ aspect: "title", category: "ambiguous", message: "x" }),
    ).toBe(true);
  });

  it("rejects an unknown aspect or category", () => {
    expect(
      isImportFinding({ aspect: "weird", category: "ambiguous", message: "x" }),
    ).toBe(false);
    expect(
      isImportFinding({ aspect: "title", category: "weird", message: "x" }),
    ).toBe(false);
  });

  it("rejects an empty message", () => {
    expect(
      isImportFinding({ aspect: "title", category: "ambiguous", message: "" }),
    ).toBe(false);
  });
});

describe("isImportArtifactAnalysis", () => {
  it("accepts a partial (importable) verdict", () => {
    expect(isImportArtifactAnalysis(ANALYZED_PARTIAL)).toBe(true);
  });

  it("accepts a blocked verdict with no importable content", () => {
    expect(isImportArtifactAnalysis(ANALYZED_BLOCKED)).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isImportArtifactAnalysis({ kind: "cancelled" })).toBe(true);
  });

  it("rejects a cancelled payload with extra fields", () => {
    expect(isImportArtifactAnalysis({ kind: "cancelled", leaked: 1 })).toBe(
      false,
    );
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isImportArtifactAnalysis(value)).toBe(false);
    },
  );

  it("rejects an unknown quality or state", () => {
    expect(
      isImportArtifactAnalysis({ ...ANALYZED_PARTIAL, quality: "weird" }),
    ).toBe(false);
    expect(
      isImportArtifactAnalysis({ ...ANALYZED_PARTIAL, state: "weird" }),
    ).toBe(false);
  });

  it("rejects a non-unusable verdict that lacks importable content", () => {
    const { importableContent: _omit, ...rest } = ANALYZED_PARTIAL;
    expect(isImportArtifactAnalysis(rest)).toBe(false);
  });

  it("rejects an unusable verdict that carries importable content", () => {
    expect(
      isImportArtifactAnalysis({
        ...ANALYZED_BLOCKED,
        importableContent: IMPORTABLE_CONTENT,
      }),
    ).toBe(false);
  });

  it("rejects a malformed importable content (short checksum)", () => {
    expect(
      isImportArtifactAnalysis({
        ...ANALYZED_PARTIAL,
        importableContent: { ...IMPORTABLE_CONTENT, contentChecksum: "abc" },
      }),
    ).toBe(false);
  });

  it("rejects a malformed finding in the list", () => {
    expect(
      isImportArtifactAnalysis({
        ...ANALYZED_PARTIAL,
        findings: [{ aspect: "title" }],
      }),
    ).toBe(false);
  });

  it("rejects a non-hex artifactChecksum", () => {
    expect(
      isImportArtifactAnalysis({
        ...ANALYZED_PARTIAL,
        artifactChecksum: "z".repeat(64),
      }),
    ).toBe(false);
  });

  it("rejects an incoherent quality/state couple (clean + blocked)", () => {
    expect(
      isImportArtifactAnalysis({
        kind: "analyzed",
        quality: "clean",
        state: "blocked",
        findings: [
          { aspect: "envelope", category: "recognized", message: "ok" },
        ],
        importableContent: IMPORTABLE_CONTENT,
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).toBe(false);
  });

  it("rejects a quality not derived from the findings", () => {
    // An ambiguous finding ⟹ derived quality `partial`, but it claims `clean`.
    expect(
      isImportArtifactAnalysis({
        kind: "analyzed",
        quality: "clean",
        state: "recognized",
        findings: [
          { aspect: "title", category: "ambiguous", message: "ajusté" },
        ],
        importableContent: IMPORTABLE_CONTENT,
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).toBe(false);
  });

  it("rejects a verdict with a duplicated aspect", () => {
    expect(
      isImportArtifactAnalysis({
        ...ANALYZED_PARTIAL,
        findings: [
          { aspect: "title", category: "ambiguous", message: "ajusté" },
          { aspect: "title", category: "recognized", message: "ok" },
        ],
      }),
    ).toBe(false);
  });

  it("accepts a coherent multi-aspect verdict", () => {
    expect(
      isImportArtifactAnalysis({
        kind: "analyzed",
        quality: "partial",
        state: "needsReview",
        findings: [
          { aspect: "envelope", category: "recognized", message: "ok" },
          { aspect: "title", category: "ambiguous", message: "ajusté" },
        ],
        importableContent: IMPORTABLE_CONTENT,
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).toBe(true);
  });

  it("accepts an importable content with an empty (preserved) timestamp", () => {
    // Rust preserves any timestamp verbatim and classifies an empty one as
    // `Timestamps=Ambiguous` → importable (AC2). The guard MUST NOT reject it
    // as a transport drift.
    expect(
      isImportArtifactAnalysis({
        kind: "analyzed",
        quality: "partial",
        state: "needsReview",
        findings: [
          { aspect: "timestamps", category: "ambiguous", message: "date conservée" },
        ],
        importableContent: { ...IMPORTABLE_CONTENT, createdAt: "" },
        sourceName: "histoire.rustory",
        artifactChecksum: "b".repeat(64),
      }),
    ).toBe(true);
  });
});

// ===== Structured-folder creation =====

const FOLDER_FINDINGS_CLEAN = [
  { aspect: "envelope", category: "recognized", message: "Manifest lisible." },
  { aspect: "formatVersion", category: "recognized", message: "Version prise en charge." },
  { aspect: "title", category: "recognized", message: "Titre valide." },
  { aspect: "structure", category: "recognized", message: "Structure reconnue." },
  { aspect: "media", category: "recognized", message: "Médias présents." },
] as const;

type AnalyzedFolderVerdict = Extract<
  StructuredCreationAnalysis,
  { kind: "analyzed" }
>;

const FOLDER_ANALYZED_CLEAN: AnalyzedFolderVerdict = {
  kind: "analyzed",
  quality: "clean",
  state: "recognized",
  findings: [...FOLDER_FINDINGS_CLEAN],
  creatableSummary: {
    title: "Le voyage de Nour",
    nodeCount: 2,
    retainedMedia: ["couverture.png"],
    discardedMedia: [],
  },
  folderName: "mon-dossier",
  folderPath: "/home/user/mon-dossier",
};

const FOLDER_ANALYZED_PARTIAL: AnalyzedFolderVerdict = {
  kind: "analyzed",
  quality: "partial",
  state: "partial",
  findings: [
    FOLDER_FINDINGS_CLEAN[0],
    FOLDER_FINDINGS_CLEAN[1],
    FOLDER_FINDINGS_CLEAN[2],
    FOLDER_FINDINGS_CLEAN[3],
    { aspect: "media", category: "missing", message: "Des fichiers sont introuvables." },
  ],
  creatableSummary: {
    title: "Sans image",
    nodeCount: 1,
    retainedMedia: [],
    discardedMedia: ["absente.png"],
  },
  folderName: "manque",
  folderPath: "/home/user/manque",
};

const FOLDER_ANALYZED_BLOCKED: AnalyzedFolderVerdict = {
  kind: "analyzed",
  quality: "unusable",
  state: "blocked",
  findings: [
    { aspect: "envelope", category: "blocking", message: "Manifest illisible." },
  ],
  folderName: "casse",
  folderPath: "/home/user/casse",
};

describe("isStructuredCreationAnalysis", () => {
  it("accepts a clean creatable verdict", () => {
    expect(isStructuredCreationAnalysis(FOLDER_ANALYZED_CLEAN)).toBe(true);
  });

  it("accepts a partial verdict whose state names the missing content", () => {
    expect(isStructuredCreationAnalysis(FOLDER_ANALYZED_PARTIAL)).toBe(true);
  });

  it("accepts a blocked verdict with no creatable summary", () => {
    expect(isStructuredCreationAnalysis(FOLDER_ANALYZED_BLOCKED)).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isStructuredCreationAnalysis({ kind: "cancelled" })).toBe(true);
    expect(
      isStructuredCreationAnalysis({ kind: "cancelled", extra: 1 }),
    ).toBe(false);
  });

  it("rejects an analyzed verdict without folderPath", () => {
    const { folderPath: _dropped, ...rest } = FOLDER_ANALYZED_CLEAN;
    expect(isStructuredCreationAnalysis(rest)).toBe(false);
    expect(
      isStructuredCreationAnalysis({ ...FOLDER_ANALYZED_CLEAN, folderPath: "" }),
    ).toBe(false);
  });

  it("rejects an empty folderName", () => {
    expect(
      isStructuredCreationAnalysis({ ...FOLDER_ANALYZED_CLEAN, folderName: "" }),
    ).toBe(false);
  });

  it("rejects a partial state without a missing finding", () => {
    // The folder derivation is deterministic: `partial` REQUIRES missing
    // content; ambiguity alone must have derived `needsReview`.
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_PARTIAL,
        findings: [
          FOLDER_FINDINGS_CLEAN[0],
          FOLDER_FINDINGS_CLEAN[1],
          FOLDER_FINDINGS_CLEAN[2],
          { aspect: "structure", category: "ambiguous", message: "Champ inattendu." },
          FOLDER_FINDINGS_CLEAN[4],
        ],
      }),
    ).toBe(false);
  });

  it("rejects a needsReview state that hides a missing finding", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_PARTIAL,
        state: "needsReview",
      }),
    ).toBe(false);
  });

  it("rejects an aspect outside the folder set", () => {
    // `timestamps` belongs to the `.rustory` flow — a folder verdict must
    // never carry it.
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        findings: [
          ...FOLDER_FINDINGS_CLEAN.slice(0, 4),
          { aspect: "timestamps", category: "recognized", message: "Dates ok." },
        ],
      }),
    ).toBe(false);
  });

  it("rejects a duplicated aspect", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        findings: [...FOLDER_FINDINGS_CLEAN.slice(0, 4), FOLDER_FINDINGS_CLEAN[3]],
      }),
    ).toBe(false);
  });

  it("rejects a creatable verdict missing one of the five folder aspects", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        quality: "clean",
        state: "recognized",
        findings: FOLDER_FINDINGS_CLEAN.slice(0, 4),
      }),
    ).toBe(false);
  });

  it("rejects a creatable verdict without its summary", () => {
    const { creatableSummary: _dropped, ...rest } = FOLDER_ANALYZED_CLEAN;
    expect(isStructuredCreationAnalysis(rest)).toBe(false);
  });

  it("rejects a blocked verdict that carries a summary", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_BLOCKED,
        creatableSummary: {
          title: "X",
          nodeCount: 1,
          retainedMedia: [],
          discardedMedia: [],
        },
      }),
    ).toBe(false);
  });

  it("rejects a malformed summary (zero nodes, empty title)", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        creatableSummary: { ...FOLDER_ANALYZED_CLEAN.creatableSummary!, nodeCount: 0 },
      }),
    ).toBe(false);
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        creatableSummary: { ...FOLDER_ANALYZED_CLEAN.creatableSummary!, title: "" },
      }),
    ).toBe(false);
  });

  it("rejects a quality not derived from the findings", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        quality: "partial",
        state: "needsReview",
      }),
    ).toBe(false);
  });
});

// ===== RSS external-source creation =====

const RSS_PREVIEW_EXPLOITABLE: RssPreview = {
  sourceHost: "exemple.fr",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      itemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
    },
    {
      title: "",
      summary: "Sans titre.",
      hasEnclosure: true,
      itemRef: {
        kind: "titleLink",
        title: "",
        link: null,
        fingerprint: "b".repeat(64),
      },
    },
  ],
  findings: [
    { aspect: "envelope", category: "recognized", message: "Le flux RSS est lisible." },
    {
      aspect: "formatVersion",
      category: "recognized",
      message: "Le flux est au format RSS 2.0 supporté.",
    },
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

const RSS_PREVIEW_BLOCKED: RssPreview = {
  sourceHost: "exemple.fr",
  items: [],
  findings: [
    {
      aspect: "envelope",
      category: "blocking",
      message: "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux.",
    },
  ],
  state: "blocked",
  blocked: true,
};

describe("isRssItemRef", () => {
  const FP = "a".repeat(64);

  it("accepts the two documented variants (with their content proof)", () => {
    expect(isRssItemRef({ kind: "guid", guid: "g-1", fingerprint: FP })).toBe(
      true,
    );
    expect(
      isRssItemRef({
        kind: "titleLink",
        title: "Episode",
        link: null,
        fingerprint: FP,
      }),
    ).toBe(true);
    expect(
      isRssItemRef({
        kind: "titleLink",
        title: "Episode",
        link: "https://exemple.fr/ep",
        fingerprint: FP,
      }),
    ).toBe(true);
  });

  it("rejects an empty guid, a foreign kind and non-objects", () => {
    expect(isRssItemRef({ kind: "guid", guid: "", fingerprint: FP })).toBe(
      false,
    );
    expect(isRssItemRef({ kind: "index", index: 0, fingerprint: FP })).toBe(
      false,
    );
    expect(isRssItemRef(null)).toBe(false);
    expect(isRssItemRef("guid")).toBe(false);
  });

  it("rejects a reference without a well-formed content proof", () => {
    expect(isRssItemRef({ kind: "guid", guid: "g-1" })).toBe(false);
    expect(
      isRssItemRef({ kind: "guid", guid: "g-1", fingerprint: "court" }),
    ).toBe(false);
    expect(
      isRssItemRef({
        kind: "titleLink",
        title: "Episode",
        link: null,
        fingerprint: "Z".repeat(64),
      }),
    ).toBe(false);
  });
});

describe("isRssPreview", () => {
  it("accepts an exploitable preview with the nominal source ambiguity", () => {
    expect(isRssPreview(RSS_PREVIEW_EXPLOITABLE)).toBe(true);
  });

  it("accepts a blocked verdict with zero item", () => {
    expect(isRssPreview(RSS_PREVIEW_BLOCKED)).toBe(true);
  });

  it("rejects a blocked flag diverging from the state", () => {
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, blocked: true }),
    ).toBe(false);
    expect(isRssPreview({ ...RSS_PREVIEW_BLOCKED, blocked: false })).toBe(
      false,
    );
  });

  it("rejects an exploitable preview whose state is not the needsReview floor", () => {
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, state: "recognized" }),
    ).toBe(false);
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, state: "partial" }),
    ).toBe(false);
  });

  it("rejects an exploitable preview missing the nominal source finding", () => {
    expect(
      isRssPreview({
        ...RSS_PREVIEW_EXPLOITABLE,
        findings: RSS_PREVIEW_EXPLOITABLE.findings.filter(
          (f) => f.aspect !== "source",
        ),
      }),
    ).toBe(false);
  });

  it("rejects an exploitable preview with zero item", () => {
    expect(isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, items: [] })).toBe(
      false,
    );
  });

  it("rejects a blocked verdict that still carries items", () => {
    expect(
      isRssPreview({
        ...RSS_PREVIEW_BLOCKED,
        items: RSS_PREVIEW_EXPLOITABLE.items,
      }),
    ).toBe(false);
  });

  it("rejects a blocked verdict without a blocking finding", () => {
    expect(
      isRssPreview({
        ...RSS_PREVIEW_BLOCKED,
        findings: [
          {
            aspect: "envelope",
            category: "recognized",
            message: "Le flux RSS est lisible.",
          },
        ],
      }),
    ).toBe(false);
  });

  it("rejects a sourceHost that is not host-only (PII drift guard)", () => {
    for (const drifted of [
      "https://exemple.fr/flux.xml",
      "exemple.fr/chemin",
      "exemple.fr:8000",
      "exemple.fr?token=secret",
      "exemple.fr#frag",
      "user@exemple.fr",
      "exem ple.fr",
      "exemple.fr\u0000",
      "h".repeat(97),
    ]) {
      expect(
        isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, sourceHost: drifted }),
      ).toBe(false);
    }
  });

  it("rejects an empty source host and a selectable item with no text at all", () => {
    expect(isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, sourceHost: "" })).toBe(
      false,
    );
    expect(
      isRssPreview({
        ...RSS_PREVIEW_EXPLOITABLE,
        items: [
          {
            title: "",
            summary: "",
            hasEnclosure: false,
            itemRef: { kind: "guid", guid: "g", fingerprint: "a".repeat(64) },
          },
        ],
      }),
    ).toBe(false);
  });

  it("accepts the source aspect in the closed finding set", () => {
    expect(
      isImportFinding({
        aspect: "source",
        category: "ambiguous",
        message: "Contenu ingéré depuis une source externe (RSS).",
      }),
    ).toBe(true);
  });
});

// ===== Content-source activation policy =====

const OFFICIAL_POLICY: ContentSourcePolicy = {
  sources: [
    { kind: "rss", label: "Flux RSS", activation: "enabled" },
    {
      kind: "atom",
      label: "Flux Atom",
      activation: "notActivated",
      reason: "Source indisponible: non activée dans la distribution officielle",
    },
    {
      kind: "jsonFeed",
      label: "Flux JSON Feed",
      activation: "notActivated",
      reason: "Source indisponible: non activée dans la distribution officielle",
    },
  ],
};

describe("isContentSourceEntry", () => {
  it("accepts an enabled line without a reason", () => {
    expect(isContentSourceEntry(OFFICIAL_POLICY.sources[0])).toBe(true);
  });

  it("accepts a non-enabled line with its frozen reason", () => {
    expect(isContentSourceEntry(OFFICIAL_POLICY.sources[1])).toBe(true);
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "blockedByPolicy",
        reason: "Source indisponible: bloquée par la politique de distribution",
      }),
    ).toBe(true);
  });

  it("refuses a reason on an enabled line (the marker replaces it)", () => {
    expect(
      isContentSourceEntry({
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
        reason: "surnuméraire",
      }),
    ).toBe(false);
  });

  it("requires a non-empty reason on a non-enabled line", () => {
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
      }),
    ).toBe(false);
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        reason: "",
      }),
    ).toBe(false);
  });

  it("refuses an unknown kind, an unknown activation and an empty label", () => {
    expect(
      isContentSourceEntry({
        kind: "torrent",
        label: "Torrent",
        activation: "enabled",
      }),
    ).toBe(false);
    expect(
      isContentSourceEntry({
        kind: "rss",
        label: "Flux RSS",
        activation: "maybe",
      }),
    ).toBe(false);
    expect(
      isContentSourceEntry({ kind: "rss", label: "", activation: "enabled" }),
    ).toBe(false);
  });
});

describe("isContentSourcePolicy", () => {
  it("accepts the current official policy shape", () => {
    expect(isContentSourcePolicy(OFFICIAL_POLICY)).toBe(true);
  });

  it("refuses an empty or missing source list (fail-closed drift)", () => {
    expect(isContentSourcePolicy({ sources: [] })).toBe(false);
    expect(isContentSourcePolicy({})).toBe(false);
    expect(isContentSourcePolicy(null)).toBe(false);
    expect(isContentSourcePolicy("policy")).toBe(false);
  });

  it("refuses a duplicated kind (a malformed policy never renders)", () => {
    expect(
      isContentSourcePolicy({
        sources: [OFFICIAL_POLICY.sources[0], OFFICIAL_POLICY.sources[0]],
      }),
    ).toBe(false);
  });

  it("refuses a policy carrying one drifted line", () => {
    expect(
      isContentSourcePolicy({
        sources: [
          OFFICIAL_POLICY.sources[0],
          { kind: "atom", label: "Flux Atom", activation: "notActivated" },
        ],
      }),
    ).toBe(false);
  });

  it("refuses a drifted label or a drifted reason (frozen couples only)", () => {
    expect(
      isContentSourceEntry({ kind: "rss", label: "RSS", activation: "enabled" }),
    ).toBe(false);
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        reason: "indisponible",
      }),
    ).toBe(false);
    // A reason swapped between the two non-enabled states is a drift too.
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "blockedByPolicy",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
      }),
    ).toBe(false);
  });

  it("refuses an enabled line on a kind without an ingestion flow in this build", () => {
    // Only rss is actionable today: a policy enabling atom / jsonFeed is
    // ahead of the frontend and must fail closed (an explicit re-scope
    // ships the ingestion flow AND relaxes this guard together).
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "enabled",
      }),
    ).toBe(false);
    expect(
      isContentSourcePolicy({
        sources: [
          OFFICIAL_POLICY.sources[0],
          { kind: "atom", label: "Flux Atom", activation: "enabled" },
          OFFICIAL_POLICY.sources[2],
        ],
      }),
    ).toBe(false);
  });

  it("refuses a partial policy — every known kind must stay visible", () => {
    // The exact drift named by the contract: a lone enabled rss line would
    // silently drop Atom / JSON Feed from the dialog.
    expect(
      isContentSourcePolicy({
        sources: [{ kind: "rss", label: "Flux RSS", activation: "enabled" }],
      }),
    ).toBe(false);
    expect(
      isContentSourcePolicy({
        sources: [OFFICIAL_POLICY.sources[0], OFFICIAL_POLICY.sources[1]],
      }),
    ).toBe(false);
  });
});
