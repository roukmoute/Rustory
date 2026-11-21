import { describe, expect, it } from "vitest";

import {
  isArchiveCreationAnalysis,
  isContentSourceEntry,
  isContentSourcePolicy,
  isDropAnalysis,
  isExportStoryArchiveOutcome,
  isExportStoryDialogOutcome,
  isImportArtifactAnalysis,
  isImportFinding,
  isOsOpenAnalysis,
  isRssItemRef,
  isRssPreview,
  isStructuredCreationAnalysis,
  type ContentSourcePolicy,
  type DropAnalysis,
  type ExportStoryArchiveOutcome,
  type ExportStoryDialogOutcome,
  type ImportArtifactAnalysis,
  type OsOpenAnalysis,
  type RssPreview,
  type StructuredCreationAnalysis,
  rssItemRefKey,
} from "./import-export";

const VALID_EXPORTED: ExportStoryDialogOutcome = {
  kind: "exported",
  destinationPath: "/tmp/histoire.rustory",
  bytesWritten: 451,
  contentChecksum: "a".repeat(64),
};

const VALID_EXPORTED_ARCHIVE: ExportStoryArchiveOutcome = {
  kind: "exported",
  destinationPath: "/home/moi/Mon histoire.zip",
  bytesWritten: 12_345_678,
  imageCount: 1,
  audioCount: 2,
};

describe("isExportStoryArchiveOutcome", () => {
  it("accepts a canonical exported payload", () => {
    expect(isExportStoryArchiveOutcome(VALID_EXPORTED_ARCHIVE)).toBe(true);
  });

  it("accepts an archive without any picture", () => {
    expect(
      isExportStoryArchiveOutcome({ ...VALID_EXPORTED_ARCHIVE, imageCount: 0 }),
    ).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isExportStoryArchiveOutcome({ kind: "cancelled" })).toBe(true);
  });

  it("rejects a cancelled payload that carries extra fields", () => {
    expect(
      isExportStoryArchiveOutcome({ kind: "cancelled", leaked: true }),
    ).toBe(false);
  });

  it("rejects an unknown kind", () => {
    expect(isExportStoryArchiveOutcome({ kind: "weird" })).toBe(false);
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isExportStoryArchiveOutcome(value)).toBe(false);
    },
  );

  it.each(["destinationPath", "bytesWritten", "imageCount", "audioCount"])(
    "rejects an exported payload missing %s",
    (field) => {
      const { [field]: _omitted, ...rest } = VALID_EXPORTED_ARCHIVE as Record<
        string,
        unknown
      >;
      expect(isExportStoryArchiveOutcome(rest)).toBe(false);
    },
  );

  it("rejects an empty destinationPath", () => {
    expect(
      isExportStoryArchiveOutcome({
        ...VALID_EXPORTED_ARCHIVE,
        destinationPath: "",
      }),
    ).toBe(false);
  });

  it.each(["bytesWritten", "imageCount", "audioCount"])(
    "rejects a negative, fractional or non-numeric %s",
    (field) => {
      for (const value of [-1, 1.5, "3", null]) {
        expect(
          isExportStoryArchiveOutcome({
            ...VALID_EXPORTED_ARCHIVE,
            [field]: value,
          }),
        ).toBe(false);
      }
    },
  );

  it("rejects an exported payload carrying a field outside the contract", () => {
    // The `.rustory` export's checksum has no meaning for an archive: a
    // payload that mixes the two shapes is a drift, never rendered.
    expect(
      isExportStoryArchiveOutcome({
        ...VALID_EXPORTED_ARCHIVE,
        contentChecksum: "a".repeat(64),
      }),
    ).toBe(false);
  });

  it("does not accept the `.rustory` export outcome", () => {
    expect(isExportStoryArchiveOutcome(VALID_EXPORTED)).toBe(false);
  });
});

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

  it("accepts the media-size aspect in the closed finding set", () => {
    expect(
      isImportFinding({
        aspect: "mediaSize",
        category: "ambiguous",
        message:
          "Certains fichiers audio ou image référencés dépassent la taille autorisée (134 Mo par fichier).",
      }),
    ).toBe(true);
    // The wire tag is camelCase — never the Rust variant nor snake_case.
    for (const drifted of ["MediaSize", "media_size", "size"]) {
      expect(
        isImportFinding({ aspect: drifted, category: "ambiguous", message: "x" }),
      ).toBe(false);
    }
  });

  it("accepts the media-count aspect in the closed finding set", () => {
    expect(
      isImportFinding({
        aspect: "mediaCount",
        category: "blocking",
        message:
          "Les fichiers audio et image référencés dépassent le nombre autorisé (64 fichiers différents).",
      }),
    ).toBe(true);
    // The wire tag is camelCase — never the Rust variant nor snake_case.
    for (const drifted of ["MediaCount", "media_count", "count"]) {
      expect(
        isImportFinding({ aspect: drifted, category: "blocking", message: "x" }),
      ).toBe(false);
    }
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
          {
            aspect: "timestamps",
            category: "ambiguous",
            message: "date conservée",
          },
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
  {
    aspect: "formatVersion",
    category: "recognized",
    message: "Version prise en charge.",
  },
  { aspect: "title", category: "recognized", message: "Titre valide." },
  {
    aspect: "structure",
    category: "recognized",
    message: "Structure reconnue.",
  },
  { aspect: "media", category: "recognized", message: "Médias présents." },
  {
    aspect: "mediaSize",
    category: "recognized",
    message: "Aucun fichier ne dépasse la taille autorisée.",
  },
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
    {
      aspect: "media",
      category: "missing",
      message: "Des fichiers sont introuvables.",
    },
    FOLDER_FINDINGS_CLEAN[5],
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
    {
      aspect: "envelope",
      category: "blocking",
      message: "Manifest illisible.",
    },
  ],
  folderName: "casse",
  folderPath: "/home/user/casse",
};

describe("isOsOpenAnalysis", () => {
  it("accepts an archive settlement re-tagged from the picker verdict", () => {
    const { kind: _k, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    expect(isOsOpenAnalysis({ kind: "archive", ...fields })).toBe(true);
  });

  const MULTIPLE_FILES: OsOpenAnalysis = {
    kind: "multipleFiles",
    message:
      "Rustory ouvre un fichier à la fois. Rouvre chaque fichier séparément.",
  };

  it("accepts a none payload with only the kind discriminant", () => {
    expect(isOsOpenAnalysis({ kind: "none" })).toBe(true);
  });

  it("rejects a none payload that carries extra fields", () => {
    expect(isOsOpenAnalysis({ kind: "none", leaked: 1 })).toBe(false);
  });

  it("accepts the multipleFiles calm limit with its Rust-carried copy", () => {
    expect(isOsOpenAnalysis(MULTIPLE_FILES)).toBe(true);
  });

  it("rejects a multipleFiles payload with a missing or empty message (fail-closed)", () => {
    expect(isOsOpenAnalysis({ kind: "multipleFiles" })).toBe(false);
    expect(isOsOpenAnalysis({ kind: "multipleFiles", message: "" })).toBe(
      false,
    );
  });

  it("rejects a multipleFiles payload with extra fields", () => {
    expect(
      isOsOpenAnalysis({ ...MULTIPLE_FILES, leaked: true }),
    ).toBe(false);
  });

  it("accepts an analyzed verdict — importable and blocked alike (the dialog-import guard)", () => {
    expect(isOsOpenAnalysis(ANALYZED_PARTIAL)).toBe(true);
    expect(isOsOpenAnalysis(ANALYZED_BLOCKED)).toBe(true);
  });

  it("rejects an altered analyzed verdict exactly like the dialog-import guard", () => {
    // Quality incoherent with its findings — the reused guard refuses it.
    expect(
      isOsOpenAnalysis({ ...ANALYZED_PARTIAL, quality: "clean" }),
    ).toBe(false);
    // A blocked verdict must not carry importable content.
    expect(
      isOsOpenAnalysis({
        ...ANALYZED_BLOCKED,
        importableContent: IMPORTABLE_CONTENT,
      }),
    ).toBe(false);
    // A drifted field type never renders.
    expect(
      isOsOpenAnalysis({ ...ANALYZED_PARTIAL, sourceName: "" }),
    ).toBe(false);
  });

  it("rejects the dialog-only cancelled kind and unknown kinds", () => {
    // The OS channel has no dialog, hence no cancelled variant.
    expect(isOsOpenAnalysis({ kind: "cancelled" })).toBe(false);
    expect(isOsOpenAnalysis({ kind: "weird" })).toBe(false);
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isOsOpenAnalysis(value)).toBe(false);
    },
  );
});

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
    expect(isStructuredCreationAnalysis({ kind: "cancelled", extra: 1 })).toBe(
      false,
    );
  });

  it("rejects an analyzed verdict without folderPath", () => {
    const { folderPath: _dropped, ...rest } = FOLDER_ANALYZED_CLEAN;
    expect(isStructuredCreationAnalysis(rest)).toBe(false);
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        folderPath: "",
      }),
    ).toBe(false);
  });

  it("rejects an empty folderName", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        folderName: "",
      }),
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
          {
            aspect: "structure",
            category: "ambiguous",
            message: "Champ inattendu.",
          },
          FOLDER_FINDINGS_CLEAN[4],
          FOLDER_FINDINGS_CLEAN[5],
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
          ...FOLDER_FINDINGS_CLEAN,
          {
            aspect: "timestamps",
            category: "recognized",
            message: "Dates ok.",
          },
        ],
      }),
    ).toBe(false);
  });

  it("rejects a duplicated aspect", () => {
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        findings: [...FOLDER_FINDINGS_CLEAN, FOLDER_FINDINGS_CLEAN[3]],
      }),
    ).toBe(false);
  });

  it("rejects a creatable verdict missing one of the six folder aspects", () => {
    for (const dropped of FOLDER_FINDINGS_CLEAN) {
      expect(
        isStructuredCreationAnalysis({
          ...FOLDER_ANALYZED_CLEAN,
          findings: FOLDER_FINDINGS_CLEAN.filter(
            (finding) => finding.aspect !== dropped.aspect,
          ),
        }),
      ).toBe(false);
    }
  });

  it("accepts a media left out for its size: an ambiguity, the review is pending", () => {
    // The file is THERE — never the `partial` state of a missing media.
    const verdict = {
      ...FOLDER_ANALYZED_CLEAN,
      quality: "partial",
      state: "needsReview",
      findings: [
        ...FOLDER_FINDINGS_CLEAN.slice(0, 5),
        {
          aspect: "mediaSize",
          category: "ambiguous",
          message:
            "Certains fichiers audio ou image référencés dépassent la taille autorisée (34 Mo par fichier). L'histoire sera créée sans eux ; tu pourras ajouter des fichiers plus légers dans l'éditeur.",
        },
      ],
    };
    expect(isStructuredCreationAnalysis(verdict)).toBe(true);
    expect(isStructuredCreationAnalysis({ ...verdict, state: "partial" })).toBe(
      false,
    );
  });

  it("accepts a verdict blocked on the media size, every aspect judged and nothing to create", () => {
    const { creatableSummary: _dropped, ...rest } = FOLDER_ANALYZED_CLEAN;
    const blocked = {
      ...rest,
      quality: "unusable",
      state: "blocked",
      findings: [
        ...FOLDER_FINDINGS_CLEAN.slice(0, 5),
        {
          aspect: "mediaSize",
          category: "blocking",
          message:
            "Les fichiers audio et image référencés dépassent ensemble la taille autorisée (268 Mo). Allège le dossier (moins de médias ou des fichiers plus légers) puis relance l'analyse.",
        },
      ],
    };
    expect(isStructuredCreationAnalysis(blocked)).toBe(true);
    // A size block creates nothing: a summary on it is a drift.
    expect(
      isStructuredCreationAnalysis({
        ...blocked,
        creatableSummary: FOLDER_ANALYZED_CLEAN.creatableSummary,
      }),
    ).toBe(false);
  });

  it("rejects a verdict judging the media without their size, or the reverse", () => {
    // The two are judged together or not at all (an unlisted format never
    // reads a single media) — even on a blocked verdict.
    const blockedOnFormat = {
      ...FOLDER_ANALYZED_BLOCKED,
      findings: [
        FOLDER_FINDINGS_CLEAN[0],
        {
          aspect: "formatVersion",
          category: "blocking",
          message: "Version non prise en charge.",
        },
        FOLDER_FINDINGS_CLEAN[2],
        FOLDER_FINDINGS_CLEAN[3],
      ],
    };
    expect(isStructuredCreationAnalysis(blockedOnFormat)).toBe(true);
    expect(
      isStructuredCreationAnalysis({
        ...blockedOnFormat,
        findings: [...blockedOnFormat.findings, FOLDER_FINDINGS_CLEAN[4]],
      }),
    ).toBe(false);
    expect(
      isStructuredCreationAnalysis({
        ...blockedOnFormat,
        findings: [...blockedOnFormat.findings, FOLDER_FINDINGS_CLEAN[5]],
      }),
    ).toBe(false);
  });

  // The count copy below is the one Rust sends for a folder (the guard
  // never reads a message — it is here so the fixture is the real wire).
  const FOLDER_COUNT_BLOCKED = {
    kind: "analyzed",
    quality: "unusable",
    state: "blocked",
    findings: [
      ...FOLDER_FINDINGS_CLEAN.slice(0, 4),
      {
        aspect: "mediaCount",
        category: "blocking",
        message:
          "Les fichiers audio et image référencés dépassent le nombre autorisé (64 fichiers différents). Réduis le nombre de médias référencés par le manifest puis relance l'analyse ; tu pourras ajouter les autres dans l'éditeur.",
      },
    ],
    folderName: "longue-histoire",
    folderPath: "/home/user/longue-histoire",
  };

  it("accepts a verdict blocked on the media count: no media line, nothing to create", () => {
    // Too many referenced media: the manifest, its format, its title and
    // its structure are judged; the media were never read, so neither
    // `media` nor `mediaSize` is there — only the count line, which blocks.
    expect(isStructuredCreationAnalysis(FOLDER_COUNT_BLOCKED)).toBe(true);
    // A count block creates nothing: a summary on it is a drift.
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_COUNT_BLOCKED,
        creatableSummary: FOLDER_ANALYZED_CLEAN.creatableSummary,
      }),
    ).toBe(false);
    // The same verdict settles the drop channel.
    expect(isDropAnalysis({ ...FOLDER_COUNT_BLOCKED, kind: "folder" })).toBe(
      true,
    );
  });

  it("accepts the count block next to the other blockers of the same manifest", () => {
    // The report names everything to fix: an invalid title blocks on its
    // own line, beside the count.
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_COUNT_BLOCKED,
        findings: [
          FOLDER_FINDINGS_CLEAN[0],
          FOLDER_FINDINGS_CLEAN[1],
          {
            aspect: "title",
            category: "blocking",
            message: "Le titre du manifest est manquant ou n'est pas valide.",
          },
          FOLDER_FINDINGS_CLEAN[3],
          FOLDER_COUNT_BLOCKED.findings[4],
        ],
      }),
    ).toBe(true);
  });

  it("rejects a count block that still judges the media or their size", () => {
    // Beyond the reference bound no media is read: a verdict that says so
    // AND words their presence or their size claims work that was not done.
    for (const extra of [
      [FOLDER_FINDINGS_CLEAN[4]],
      [FOLDER_FINDINGS_CLEAN[5]],
      [FOLDER_FINDINGS_CLEAN[4], FOLDER_FINDINGS_CLEAN[5]],
    ]) {
      expect(
        isStructuredCreationAnalysis({
          ...FOLDER_COUNT_BLOCKED,
          findings: [...FOLDER_COUNT_BLOCKED.findings, ...extra],
        }),
      ).toBe(false);
    }
  });

  it("rejects a count line that does not block", () => {
    // Within the bound the count has NO line: a creatable verdict carries
    // exactly its six aspects, never a seventh that says "the count is fine".
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        findings: [
          ...FOLDER_FINDINGS_CLEAN,
          {
            aspect: "mediaCount",
            category: "recognized",
            message:
              "Les fichiers audio et image référencés ne dépassent pas le nombre autorisé.",
          },
        ],
      }),
    ).toBe(false);
    // Nor is it ever an ambiguity or a missing information.
    for (const [category, state] of [
      ["ambiguous", "needsReview"],
      ["missing", "partial"],
    ] as const) {
      expect(
        isStructuredCreationAnalysis({
          ...FOLDER_ANALYZED_CLEAN,
          quality: "partial",
          state,
          findings: [
            ...FOLDER_FINDINGS_CLEAN.slice(0, 4),
            { aspect: "mediaCount", category, message: "x" },
          ],
        }),
      ).toBe(false);
    }
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
        creatableSummary: {
          ...FOLDER_ANALYZED_CLEAN.creatableSummary!,
          nodeCount: 0,
        },
      }),
    ).toBe(false);
    expect(
      isStructuredCreationAnalysis({
        ...FOLDER_ANALYZED_CLEAN,
        creatableSummary: {
          ...FOLDER_ANALYZED_CLEAN.creatableSummary!,
          title: "",
        },
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

// ===== Structured-archive creation =====

const ARCHIVE_FINDINGS_CLEAN = [
  {
    aspect: "envelope",
    category: "recognized",
    message: "Le descripteur story.json est présent et lisible.",
  },
  {
    aspect: "title",
    category: "recognized",
    message: "Le titre de l'histoire est valide.",
  },
  {
    aspect: "structure",
    category: "recognized",
    message: "La structure du pack est reconnue et convertie en histoire.",
  },
  {
    aspect: "media",
    category: "recognized",
    message:
      "Tous les fichiers audio et image référencés par le pack sont présents dans l'archive et reconnus.",
  },
  {
    aspect: "mediaSize",
    category: "recognized",
    message:
      "Aucun fichier audio ou image référencé ne dépasse la taille autorisée.",
  },
] as const;

const ARCHIVE_ANALYZED_CLEAN = {
  kind: "analyzed",
  quality: "clean",
  state: "recognized",
  findings: [...ARCHIVE_FINDINGS_CLEAN],
  creatableSummary: {
    title: "Le pack du soir",
    nodeCount: 3,
    retainedMedia: ["cover.png"],
    discardedMedia: [],
  },
  archiveName: "Le pack du soir.zip",
  archivePath: "/home/user/Le pack du soir.zip",
};

const ARCHIVE_ANALYZED_BLOCKED = {
  kind: "analyzed",
  quality: "unusable",
  state: "blocked",
  findings: [
    {
      aspect: "envelope",
      category: "blocking",
      message: "L'archive ne contient pas de descripteur story.json lisible.",
    },
  ],
  archiveName: "casse.zip",
  archivePath: "/home/user/casse.zip",
};

describe("isArchiveCreationAnalysis", () => {
  it("accepts a clean creatable verdict", () => {
    expect(isArchiveCreationAnalysis(ARCHIVE_ANALYZED_CLEAN)).toBe(true);
  });

  it("accepts a blocked verdict with no creatable summary", () => {
    expect(isArchiveCreationAnalysis(ARCHIVE_ANALYZED_BLOCKED)).toBe(true);
  });

  it("accepts a cancelled payload with only the kind discriminant", () => {
    expect(isArchiveCreationAnalysis({ kind: "cancelled" })).toBe(true);
    expect(isArchiveCreationAnalysis({ kind: "cancelled", extra: 1 })).toBe(
      false,
    );
  });

  it("rejects an analyzed verdict without archivePath or archiveName", () => {
    const { archivePath: _dropped, ...rest } = ARCHIVE_ANALYZED_CLEAN;
    expect(isArchiveCreationAnalysis(rest)).toBe(false);
    expect(
      isArchiveCreationAnalysis({ ...ARCHIVE_ANALYZED_CLEAN, archiveName: "" }),
    ).toBe(false);
  });

  it("rejects the folder flow's formatVersion aspect — never analyzed here", () => {
    expect(
      isArchiveCreationAnalysis({
        ...ARCHIVE_ANALYZED_CLEAN,
        findings: [
          ...ARCHIVE_FINDINGS_CLEAN,
          {
            aspect: "formatVersion",
            category: "recognized",
            message: "Version ok.",
          },
        ],
      }),
    ).toBe(false);
  });

  it("rejects a creatable verdict missing one of the five archive aspects", () => {
    for (const dropped of ARCHIVE_FINDINGS_CLEAN) {
      expect(
        isArchiveCreationAnalysis({
          ...ARCHIVE_ANALYZED_CLEAN,
          findings: ARCHIVE_FINDINGS_CLEAN.filter(
            (finding) => finding.aspect !== dropped.aspect,
          ),
        }),
      ).toBe(false);
    }
  });

  it("accepts a media left out for its size: an ambiguity, the review is pending", () => {
    const verdict = {
      ...ARCHIVE_ANALYZED_CLEAN,
      quality: "partial",
      state: "needsReview",
      findings: [
        ...ARCHIVE_FINDINGS_CLEAN.slice(0, 4),
        {
          aspect: "mediaSize",
          category: "ambiguous",
          message:
            "Certains fichiers audio ou image référencés dépassent la taille autorisée (134 Mo par fichier). L'histoire sera créée sans eux ; pour les garder, remplace-les dans l'archive par des fichiers plus légers puis relance l'analyse.",
        },
      ],
      creatableSummary: {
        ...ARCHIVE_ANALYZED_CLEAN.creatableSummary,
        discardedMedia: ["trop.mp3"],
      },
    };
    expect(isArchiveCreationAnalysis(verdict)).toBe(true);
    // Present but too large is never the `partial` state of a missing media.
    expect(isArchiveCreationAnalysis({ ...verdict, state: "partial" })).toBe(
      false,
    );
  });

  it("accepts a media absent from the archive: a missing information, the durable state is partial", () => {
    // Symmetric twin of the Rust producer test
    // (`structured_archive_media_findings_use_the_archive_copy`): the copy
    // is the archive's own — a pack and its archive, never a folder.
    const verdict = {
      ...ARCHIVE_ANALYZED_CLEAN,
      quality: "partial",
      state: "partial",
      findings: [
        ...ARCHIVE_FINDINGS_CLEAN.slice(0, 3),
        {
          aspect: "media",
          category: "missing",
          message:
            "Certains fichiers audio ou image référencés par le pack sont introuvables dans l'archive. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.",
        },
        ARCHIVE_FINDINGS_CLEAN[4],
      ],
      creatableSummary: {
        ...ARCHIVE_ANALYZED_CLEAN.creatableSummary,
        retainedMedia: [],
        discardedMedia: ["cover.png"],
      },
    };
    expect(isArchiveCreationAnalysis(verdict)).toBe(true);
    // A missing media is never the `needsReview` state of an ambiguity.
    expect(
      isArchiveCreationAnalysis({ ...verdict, state: "needsReview" }),
    ).toBe(false);
    // The same verdict settles the drop and the OS-open channels.
    expect(isDropAnalysis({ ...verdict, kind: "archive" })).toBe(true);
    expect(isOsOpenAnalysis({ ...verdict, kind: "archive" })).toBe(true);
  });

  it("accepts a verdict blocked on the media size, every aspect judged and nothing to create", () => {
    const { creatableSummary: _dropped, ...rest } = ARCHIVE_ANALYZED_CLEAN;
    const blocked = {
      ...rest,
      quality: "unusable",
      state: "blocked",
      findings: [
        ...ARCHIVE_FINDINGS_CLEAN.slice(0, 4),
        {
          aspect: "mediaSize",
          category: "blocking",
          message:
            "Les fichiers audio et image référencés dépassent ensemble la taille autorisée (34,4 Go). Allège l'archive (moins de médias ou des fichiers plus légers) puis relance l'analyse.",
        },
      ],
    };
    expect(isArchiveCreationAnalysis(blocked)).toBe(true);
    expect(
      isArchiveCreationAnalysis({
        ...blocked,
        creatableSummary: ARCHIVE_ANALYZED_CLEAN.creatableSummary,
      }),
    ).toBe(false);
    // The same verdict settles the drop and the OS-open channels.
    expect(isDropAnalysis({ ...blocked, kind: "archive" })).toBe(true);
    expect(isOsOpenAnalysis({ ...blocked, kind: "archive" })).toBe(true);
  });

  it("rejects a verdict judging the media without their size, or the reverse", () => {
    // The two lines are judged together or not at all — on a blocked
    // verdict too: here the structure blocks, and the media were read.
    const blockedOnStructure = {
      ...ARCHIVE_ANALYZED_BLOCKED,
      findings: [
        ARCHIVE_FINDINGS_CLEAN[0],
        ARCHIVE_FINDINGS_CLEAN[1],
        {
          aspect: "structure",
          category: "blocking",
          message: "La structure du descripteur est incomplète ou incohérente.",
        },
        ARCHIVE_FINDINGS_CLEAN[3],
        ARCHIVE_FINDINGS_CLEAN[4],
      ],
    };
    expect(isArchiveCreationAnalysis(blockedOnStructure)).toBe(true);
    for (const dropped of ["media", "mediaSize"]) {
      expect(
        isArchiveCreationAnalysis({
          ...blockedOnStructure,
          findings: blockedOnStructure.findings.filter(
            (finding) => finding.aspect !== dropped,
          ),
        }),
      ).toBe(false);
    }
  });

  // The count copy below is the one Rust sends for an archive (the guard
  // never reads a message — it is here so the fixture is the real wire).
  const ARCHIVE_COUNT_BLOCKED = {
    kind: "analyzed",
    quality: "unusable",
    state: "blocked",
    findings: [
      ...ARCHIVE_FINDINGS_CLEAN.slice(0, 3),
      {
        aspect: "mediaCount",
        category: "blocking",
        message:
          "Les fichiers audio et image référencés dépassent le nombre autorisé (8192 fichiers différents). Réduis le nombre de médias de l'archive (par exemple en répartissant le pack sur plusieurs archives) puis relance l'analyse.",
      },
    ],
    archiveName: "pack-sans-fin.zip",
    archivePath: "/home/user/pack-sans-fin.zip",
  };

  it("accepts a verdict blocked on the media count: no media line, nothing to create", () => {
    // A descriptor referencing more files than the reference bound blocks
    // on their NUMBER without a single media read: the descriptor, its
    // title and its structure are judged, neither media line exists.
    expect(isArchiveCreationAnalysis(ARCHIVE_COUNT_BLOCKED)).toBe(true);
    expect(
      isArchiveCreationAnalysis({
        ...ARCHIVE_COUNT_BLOCKED,
        creatableSummary: ARCHIVE_ANALYZED_CLEAN.creatableSummary,
      }),
    ).toBe(false);
    // The same verdict settles the drop and the OS-open channels.
    expect(isDropAnalysis({ ...ARCHIVE_COUNT_BLOCKED, kind: "archive" })).toBe(
      true,
    );
    expect(
      isOsOpenAnalysis({ ...ARCHIVE_COUNT_BLOCKED, kind: "archive" }),
    ).toBe(true);
  });

  it("rejects a count block that still judges the media or their size", () => {
    // Beyond the reference bound no media is read: a verdict that says so
    // AND words their presence or their size claims work that was not done.
    for (const extra of [
      [ARCHIVE_FINDINGS_CLEAN[3]],
      [ARCHIVE_FINDINGS_CLEAN[4]],
      [ARCHIVE_FINDINGS_CLEAN[3], ARCHIVE_FINDINGS_CLEAN[4]],
    ]) {
      const drifted = {
        ...ARCHIVE_COUNT_BLOCKED,
        findings: [...ARCHIVE_COUNT_BLOCKED.findings, ...extra],
      };
      expect(isArchiveCreationAnalysis(drifted)).toBe(false);
      expect(isDropAnalysis({ ...drifted, kind: "archive" })).toBe(false);
      expect(isOsOpenAnalysis({ ...drifted, kind: "archive" })).toBe(false);
    }
  });

  it("rejects a count line that does not block", () => {
    // Within the bound the count has NO line: a creatable verdict carries
    // exactly its five aspects.
    expect(
      isArchiveCreationAnalysis({
        ...ARCHIVE_ANALYZED_CLEAN,
        findings: [
          ...ARCHIVE_FINDINGS_CLEAN,
          {
            aspect: "mediaCount",
            category: "recognized",
            message:
              "Les fichiers audio et image référencés ne dépassent pas le nombre autorisé.",
          },
        ],
      }),
    ).toBe(false);
    for (const [category, state] of [
      ["ambiguous", "needsReview"],
      ["missing", "partial"],
    ] as const) {
      expect(
        isArchiveCreationAnalysis({
          ...ARCHIVE_ANALYZED_CLEAN,
          quality: "partial",
          state,
          findings: [
            ...ARCHIVE_FINDINGS_CLEAN.slice(0, 3),
            { aspect: "mediaCount", category, message: "x" },
          ],
        }),
      ).toBe(false);
    }
  });

  it("rejects a creatable verdict without its summary", () => {
    const { creatableSummary: _dropped, ...rest } = ARCHIVE_ANALYZED_CLEAN;
    expect(isArchiveCreationAnalysis(rest)).toBe(false);
  });

  it("rejects a blocked verdict that carries a summary", () => {
    expect(
      isArchiveCreationAnalysis({
        ...ARCHIVE_ANALYZED_BLOCKED,
        creatableSummary: ARCHIVE_ANALYZED_CLEAN.creatableSummary,
      }),
    ).toBe(false);
  });
});

// ===== RSS external-source creation =====

const RSS_PREVIEW_EXPLOITABLE: RssPreview = {
  sourceHost: "exemple.fr",
  channelTitle: "Mon flux",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      hasImage: false,
      itemRef: { kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) },
    },
    {
      title: "",
      summary: "Sans titre.",
      hasEnclosure: true,
      hasImage: true,
      itemRef: {
        kind: "titleLink",
        title: "",
        link: null,
        fingerprint: "b".repeat(64),
      },
    },
  ],
  findings: [
    {
      aspect: "envelope",
      category: "recognized",
      message: "Le flux RSS est lisible.",
    },
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
  channelTitle: null,
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
    expect(isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, blocked: true })).toBe(
      false,
    );
    expect(isRssPreview({ ...RSS_PREVIEW_BLOCKED, blocked: false })).toBe(
      false,
    );
  });

  it("rejects an exploitable preview whose state is not the needsReview floor", () => {
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, state: "recognized" }),
    ).toBe(false);
    expect(isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, state: "partial" })).toBe(
      false,
    );
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
    expect(isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, items: [] })).toBe(false);
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
            hasImage: false,
            itemRef: { kind: "guid", guid: "g", fingerprint: "a".repeat(64) },
          },
        ],
      }),
    ).toBe(false);
  });

  it("requires the artwork flag on every item and a null-or-string channel title", () => {
    const [first, second] = RSS_PREVIEW_EXPLOITABLE.items;
    const { hasImage: _dropped, ...withoutImageFlag } = first;
    void _dropped;
    expect(
      isRssPreview({
        ...RSS_PREVIEW_EXPLOITABLE,
        items: [withoutImageFlag, second],
      }),
    ).toBe(false);
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, channelTitle: null }),
    ).toBe(true);
    expect(
      isRssPreview({ ...RSS_PREVIEW_EXPLOITABLE, channelTitle: 42 }),
    ).toBe(false);
    const { channelTitle: _title, ...withoutChannelTitle } =
      RSS_PREVIEW_EXPLOITABLE;
    void _title;
    expect(isRssPreview(withoutChannelTitle)).toBe(false);
  });

  it("keys an item reference by its identity, never by its fingerprint", () => {
    expect(
      rssItemRefKey({ kind: "guid", guid: "g-1", fingerprint: "a".repeat(64) }),
    ).toBe(
      rssItemRefKey({ kind: "guid", guid: "g-1", fingerprint: "b".repeat(64) }),
    );
    expect(
      rssItemRefKey({
        kind: "titleLink",
        title: "T",
        link: null,
        fingerprint: "a".repeat(64),
      }),
    ).not.toBe(
      rssItemRefKey({
        kind: "titleLink",
        title: "T",
        link: "https://exemple.fr/t",
        fingerprint: "a".repeat(64),
      }),
    );
    expect(
      rssItemRefKey({ kind: "guid", guid: "T", fingerprint: "a".repeat(64) }),
    ).not.toBe(
      rssItemRefKey({
        kind: "titleLink",
        title: "T",
        link: null,
        fingerprint: "a".repeat(64),
      }),
    );
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
    {
      kind: "web",
      label: "Page web",
      activation: "enabled",
      activationMarker: "Activée par la distribution officielle",
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
        activationMarker: "Activée par la distribution officielle",
        reason: "surnuméraire",
      }),
    ).toBe(false);
  });

  it("requires the exact Rust-owned activation marker on an enabled line", () => {
    // Missing marker: the enabled line lost its Rust-carried copy.
    expect(
      isContentSourceEntry({
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
      }),
    ).toBe(false);
    // Drifted marker: never rendered as authoritative.
    expect(
      isContentSourceEntry({
        kind: "rss",
        label: "Flux RSS",
        activation: "enabled",
        activationMarker: "Activée",
      }),
    ).toBe(false);
  });

  it("refuses a marker on a non-enabled line (the reason replaces it)", () => {
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "notActivated",
        activationMarker: "Activée par la distribution officielle",
        reason:
          "Source indisponible: non activée dans la distribution officielle",
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
      isContentSourceEntry({
        kind: "rss",
        label: "RSS",
        activation: "enabled",
      }),
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

  it("accepts the enabled web line and the full 4-line official policy (TDD-7)", () => {
    // The web ingestion flow ships in this build: the official matrix
    // activates it, and the guard must accept the Rust-carried line.
    expect(
      isContentSourceEntry({
        kind: "web",
        label: "Page web",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      }),
    ).toBe(true);
    expect(
      isContentSourcePolicy({
        sources: [
          OFFICIAL_POLICY.sources[0],
          OFFICIAL_POLICY.sources[1],
          OFFICIAL_POLICY.sources[2],
          {
            kind: "web",
            label: "Page web",
            activation: "enabled",
            activationMarker: "Activée par la distribution officielle",
          },
        ],
      }),
    ).toBe(true);
    // A drifted web label is a drift (the frozen couple is refused), and
    // an enabled line without its marker stays refused.
    expect(
      isContentSourceEntry({
        kind: "web",
        label: "Web",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      }),
    ).toBe(false);
    expect(
      isContentSourceEntry({
        kind: "web",
        label: "Page web",
        activation: "enabled",
      }),
    ).toBe(false);
    // atom / jsonFeed keep failing closed on an enabled line: only rss
    // and web have an ingestion flow in this build.
    expect(
      isContentSourceEntry({
        kind: "atom",
        label: "Flux Atom",
        activation: "enabled",
        activationMarker: "Activée par la distribution officielle",
      }),
    ).toBe(false);
  });

  it("refuses a partial policy — every known kind must stay visible", () => {
    // The exact drift named by the contract: a lone enabled rss line would
    // silently drop Atom / JSON Feed from the dialog.
    expect(
      isContentSourcePolicy({
        sources: [
          {
            kind: "rss",
            label: "Flux RSS",
            activation: "enabled",
            activationMarker: "Activée par la distribution officielle",
          },
        ],
      }),
    ).toBe(false);
    expect(
      isContentSourcePolicy({
        sources: [OFFICIAL_POLICY.sources[0], OFFICIAL_POLICY.sources[1]],
      }),
    ).toBe(false);
  });
});

// ===== Drop channel =====

const DROP_ARTIFACT: DropAnalysis = {
  ...ANALYZED_PARTIAL,
  kind: "artifact",
} as DropAnalysis;

const DROP_ARTIFACT_BLOCKED: DropAnalysis = {
  ...ANALYZED_BLOCKED,
  kind: "artifact",
} as DropAnalysis;

const DROP_FOLDER: DropAnalysis = {
  ...FOLDER_ANALYZED_CLEAN,
  kind: "folder",
} as DropAnalysis;

const DROP_FOLDER_BLOCKED: DropAnalysis = {
  ...FOLDER_ANALYZED_BLOCKED,
  kind: "folder",
} as DropAnalysis;

const DROP_MULTIPLE_ITEMS: DropAnalysis = {
  kind: "multipleItems",
  message:
    "Rustory traite un seul élément déposé à la fois. Dépose chaque élément séparément.",
};

describe("isDropAnalysis", () => {
  it("accepts an archive settlement re-tagged from the picker verdict", () => {
    const { kind: _k, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    expect(isDropAnalysis({ kind: "archive", ...fields })).toBe(true);
  });

  it("rejects an archive settlement whose fields drift from the picker contract", () => {
    const { kind: _k, archivePath: _p, ...fields } = ARCHIVE_ANALYZED_CLEAN;
    expect(isDropAnalysis({ kind: "archive", ...fields })).toBe(false);
  });

  it("accepts the silent none with only the kind discriminant", () => {
    expect(isDropAnalysis({ kind: "none" })).toBe(true);
  });

  it("rejects a none payload with extra fields", () => {
    expect(isDropAnalysis({ kind: "none", leaked: 1 })).toBe(false);
  });

  it("accepts the multipleItems calm limit with its copy", () => {
    expect(isDropAnalysis(DROP_MULTIPLE_ITEMS)).toBe(true);
  });

  it("rejects a multipleItems without a usable copy (fail-closed)", () => {
    expect(isDropAnalysis({ kind: "multipleItems" })).toBe(false);
    expect(isDropAnalysis({ kind: "multipleItems", message: "" })).toBe(false);
  });

  it("rejects a multipleItems with extra fields", () => {
    expect(isDropAnalysis({ ...DROP_MULTIPLE_ITEMS, leaked: true })).toBe(
      false,
    );
  });

  it("accepts an artifact verdict through the dialog-import guard", () => {
    expect(isDropAnalysis(DROP_ARTIFACT)).toBe(true);
    expect(isDropAnalysis(DROP_ARTIFACT_BLOCKED)).toBe(true);
  });

  it("rejects a drifted artifact verdict exactly like the import guard", () => {
    expect(isDropAnalysis({ ...DROP_ARTIFACT, quality: "clean" })).toBe(false);
    expect(isDropAnalysis({ ...DROP_ARTIFACT, sourceName: "" })).toBe(false);
    expect(
      isDropAnalysis({
        ...DROP_ARTIFACT_BLOCKED,
        importableContent: IMPORTABLE_CONTENT,
      }),
    ).toBe(false);
  });

  it("accepts a folder verdict through the picker-folder guard", () => {
    expect(isDropAnalysis(DROP_FOLDER)).toBe(true);
    expect(isDropAnalysis(DROP_FOLDER_BLOCKED)).toBe(true);
  });

  it("rejects a drifted folder verdict exactly like the picker guard", () => {
    expect(isDropAnalysis({ ...DROP_FOLDER, folderPath: "" })).toBe(false);
    expect(isDropAnalysis({ ...DROP_FOLDER, folderName: "" })).toBe(false);
    const { creatableSummary: _omit, ...withoutSummary } = DROP_FOLDER as {
      creatableSummary?: unknown;
    } & Record<string, unknown>;
    expect(isDropAnalysis(withoutSummary)).toBe(false);
  });

  it("rejects the sibling channels' own kinds (closed union)", () => {
    expect(isDropAnalysis({ kind: "analyzed" })).toBe(false);
    expect(isDropAnalysis({ ...ANALYZED_PARTIAL })).toBe(false);
    expect(isDropAnalysis({ kind: "multipleFiles", message: "x" })).toBe(
      false,
    );
    expect(isDropAnalysis({ kind: "cancelled" })).toBe(false);
    expect(isDropAnalysis({ kind: "weird" })).toBe(false);
  });

  it.each([null, undefined, 42, "string", []])(
    "rejects non-objects (%s)",
    (value) => {
      expect(isDropAnalysis(value)).toBe(false);
    },
  );
});
