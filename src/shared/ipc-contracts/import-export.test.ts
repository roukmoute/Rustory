import { describe, expect, it } from "vitest";

import {
  isExportStoryDialogOutcome,
  isImportArtifactAnalysis,
  isImportFinding,
  type ExportStoryDialogOutcome,
  type ImportArtifactAnalysis,
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
