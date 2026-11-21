import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  // The progress-streaming facades construct `new Channel<number>()`; the
  // command mock ignores it, so a bare stand-in is enough.
  Channel: class {
    onmessage: ((msg: number) => void) | null = null;
  },
}));

import { invoke } from "@tauri-apps/api/core";

import {
  DropContractDriftError,
  OsOpenContractDriftError,
  StructuredCreationContractDriftError,
  analyzeDropRequest,
  analyzeOsOpenRequest,
  analyzeStructuredArchiveForCreation,
  analyzeStructuredFolderForCreation,
} from "../commands/import-export";

// Symmetric twins of the Rust producer tests in
// `src-tauri/tests/contracts/import_export.rs`
// (`a_folder_referencing_65_media_blocks_on_their_number_and_names_the_limit_of_64_on_the_wire`,
// `an_archive_referencing_8193_media_blocks_on_their_number_and_names_the_limit_of_8192`,
// `the_drop_and_os_open_channels_carry_the_media_count_finding_unchanged`):
// the two payloads below are, field for field, what those tests prove Rust
// serializes. A source that references more media than its bound blocks on
// the `mediaCount` line — the limit named, the gesture given —, its
// structure reads as recognized, and no `media` nor `mediaSize` line exists
// (beyond the bound no media is read).

const FOLDER_OVER_ITS_MEDIA_COUNT = {
  kind: "analyzed",
  quality: "unusable",
  state: "blocked",
  findings: [
    {
      aspect: "envelope",
      category: "recognized",
      message: "Le manifest histoire.json est présent et lisible.",
    },
    {
      aspect: "formatVersion",
      category: "recognized",
      message: "La version de format du manifest est prise en charge.",
    },
    {
      aspect: "title",
      category: "recognized",
      message: "Le titre de l'histoire est valide.",
    },
    {
      aspect: "structure",
      category: "recognized",
      message: "La structure de l'histoire est reconnue.",
    },
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

const ARCHIVE_OVER_ITS_MEDIA_COUNT = {
  kind: "analyzed",
  quality: "unusable",
  state: "blocked",
  findings: [
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
      aspect: "mediaCount",
      category: "blocking",
      message:
        "Les fichiers audio et image référencés dépassent le nombre autorisé (8192 fichiers différents). Réduis le nombre de médias de l'archive (par exemple en répartissant le pack sur plusieurs archives) puis relance l'analyse.",
    },
  ],
  archiveName: "pack-sans-fin.zip",
  archivePath: "/home/user/pack-sans-fin.zip",
};

/** Await a facade call that must reject, and hand back what it threw. */
async function rejectionOf(call: Promise<unknown>): Promise<unknown> {
  return call.then(
    () => {
      throw new Error("expected a rejection");
    },
    (error: unknown) => error,
  );
}

describe("creation review IPC contract — the media count", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("resolves the verdict of a folder referencing 65 media: a block that names the limit of 64", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(FOLDER_OVER_ITS_MEDIA_COUNT);
    const verdict = await analyzeStructuredFolderForCreation();
    // A typed content verdict the review renders verbatim — never a drift.
    expect(verdict).toEqual(FOLDER_OVER_ITS_MEDIA_COUNT);
    expect(invoke).toHaveBeenCalledWith(
      "analyze_structured_folder_for_creation",
    );
  });

  it("resolves the verdict of an archive referencing 8193 media: a block that names the limit of 8192", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(ARCHIVE_OVER_ITS_MEDIA_COUNT);
    const verdict = await analyzeStructuredArchiveForCreation();
    expect(verdict).toEqual(ARCHIVE_OVER_ITS_MEDIA_COUNT);
    expect(invoke).toHaveBeenCalledWith(
      "analyze_structured_archive_for_creation",
    );
  });

  it("settles the drop and the OS-open channels with the same verdicts, re-tagged", async () => {
    const droppedFolder = { ...FOLDER_OVER_ITS_MEDIA_COUNT, kind: "folder" };
    vi.mocked(invoke).mockResolvedValueOnce(droppedFolder);
    await expect(analyzeDropRequest()).resolves.toEqual(droppedFolder);

    const archive = { ...ARCHIVE_OVER_ITS_MEDIA_COUNT, kind: "archive" };
    vi.mocked(invoke).mockResolvedValueOnce(archive);
    await expect(analyzeDropRequest()).resolves.toEqual(archive);
    vi.mocked(invoke).mockResolvedValueOnce(archive);
    await expect(analyzeOsOpenRequest()).resolves.toEqual(archive);
  });

  it("refuses a count block that still words media it never read", async () => {
    // Beyond the reference bound no media is read: a verdict carrying the
    // count line AND a media line claims work that was not done.
    const media = {
      aspect: "media",
      category: "recognized",
      message:
        "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
    };
    const size = {
      aspect: "mediaSize",
      category: "recognized",
      message:
        "Aucun fichier audio ou image référencé ne dépasse la taille autorisée.",
    };
    const driftedFolder = {
      ...FOLDER_OVER_ITS_MEDIA_COUNT,
      findings: [...FOLDER_OVER_ITS_MEDIA_COUNT.findings, media, size],
    };
    vi.mocked(invoke).mockResolvedValueOnce(driftedFolder);
    const folderError = await rejectionOf(analyzeStructuredFolderForCreation());
    expect(folderError).toBeInstanceOf(StructuredCreationContractDriftError);
    expect(folderError).toMatchObject({ raw: driftedFolder });

    const driftedArchive = {
      ...ARCHIVE_OVER_ITS_MEDIA_COUNT,
      findings: [...ARCHIVE_OVER_ITS_MEDIA_COUNT.findings, media, size],
    };
    vi.mocked(invoke).mockResolvedValueOnce(driftedArchive);
    const archiveError = await rejectionOf(
      analyzeStructuredArchiveForCreation(),
    );
    expect(archiveError).toBeInstanceOf(StructuredCreationContractDriftError);

    vi.mocked(invoke).mockResolvedValueOnce({
      ...driftedArchive,
      kind: "archive",
    });
    expect(await rejectionOf(analyzeDropRequest())).toBeInstanceOf(
      DropContractDriftError,
    );
    vi.mocked(invoke).mockResolvedValueOnce({
      ...driftedArchive,
      kind: "archive",
    });
    expect(await rejectionOf(analyzeOsOpenRequest())).toBeInstanceOf(
      OsOpenContractDriftError,
    );
  });

  it("refuses a count line that does not block: within the bound the count has no line", async () => {
    // What a creatable verdict must never carry: a seventh aspect saying
    // the count is fine. Rust emits the line only when the bound is crossed.
    const drifted = {
      kind: "analyzed",
      quality: "clean",
      state: "recognized",
      findings: [
        ...FOLDER_OVER_ITS_MEDIA_COUNT.findings.slice(0, 4),
        {
          aspect: "media",
          category: "recognized",
          message:
            "Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.",
        },
        {
          aspect: "mediaSize",
          category: "recognized",
          message:
            "Aucun fichier audio ou image référencé ne dépasse la taille autorisée.",
        },
        {
          aspect: "mediaCount",
          category: "recognized",
          message:
            "Les fichiers audio et image référencés ne dépassent pas le nombre autorisé.",
        },
      ],
      creatableSummary: {
        title: "Longue histoire",
        nodeCount: 32,
        retainedMedia: ["image-0.png"],
        discardedMedia: [],
      },
      folderName: "longue-histoire",
      folderPath: "/home/user/longue-histoire",
    };
    vi.mocked(invoke).mockResolvedValueOnce(drifted);
    expect(
      await rejectionOf(analyzeStructuredFolderForCreation()),
    ).toBeInstanceOf(StructuredCreationContractDriftError);
  });
});
