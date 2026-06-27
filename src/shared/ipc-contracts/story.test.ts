import { describe, expect, it } from "vitest";

import {
  isAttachNodeMediaOutcome,
  isNodeContentDto,
  isNodeMediaPreview,
  isNodeMediaSlot,
  isNodeWriteOutput,
  isRecoverableDraft,
  isRecoverableNodeDraft,
  isStoryDetailDto,
  isUpdateStoryOutput,
  type NodeContentDto,
  type StoryDetailDto,
} from "./story";

const VALID_DETAIL: StoryDetailDto = {
  id: "0197a5d0-0000-7000-8000-000000000000",
  title: "Un brouillon",
  schemaVersion: 1,
  structureJson: '{"schemaVersion":1,"nodes":[]}',
  contentChecksum: "a".repeat(64),
  createdAt: "2026-04-23T09:00:00.000Z",
  updatedAt: "2026-04-23T10:00:00.000Z",
  editable: true,
  node: { id: "n1", text: "", label: "", image: null, audio: null },
};

describe("isStoryDetailDto", () => {
  it("accepts a canonical payload", () => {
    expect(isStoryDetailDto(VALID_DETAIL)).toBe(true);
  });

  it.each([null, undefined, "string", 42, [] as unknown])(
    "rejects a non-object payload: %p",
    (value) => {
      expect(isStoryDetailDto(value)).toBe(false);
    },
  );

  it("rejects a missing id", () => {
    const { id: _omit, ...rest } = VALID_DETAIL;
    expect(isStoryDetailDto(rest)).toBe(false);
  });

  it("rejects an empty id", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, id: "" })).toBe(false);
  });

  it("rejects a blank title", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, title: "   " })).toBe(false);
  });

  it("rejects a non-integer schemaVersion", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, schemaVersion: 1.5 })).toBe(
      false,
    );
  });

  it("rejects schemaVersion < 1", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, schemaVersion: 0 })).toBe(false);
  });

  it("rejects a non-string structureJson", () => {
    expect(isStoryDetailDto({ ...VALID_DETAIL, structureJson: 42 })).toBe(
      false,
    );
  });

  it("rejects a short contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "a".repeat(63) }),
    ).toBe(false);
  });

  it("rejects an uppercase contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "A".repeat(64) }),
    ).toBe(false);
  });

  it("rejects a non-hex contentChecksum", () => {
    expect(
      isStoryDetailDto({ ...VALID_DETAIL, contentChecksum: "z".repeat(64) }),
    ).toBe(false);
  });

  it("rejects a createdAt that has no UTC marker at all", () => {
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        createdAt: "2026-04-23T09:00:00.000",
      }),
    ).toBe(false);
  });

  it("rejects an updatedAt with a non-UTC offset", () => {
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        updatedAt: "2026-04-23T10:00:00.000+02:00",
      }),
    ).toBe(false);
  });

  it("rejects an explicit +00:00 UTC offset (contract mandates Z suffix)", () => {
    // Rust serializes with `Z`. Accepting `+00:00` silently would
    // let a contract drift go unnoticed; the guard must stay strict.
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        createdAt: "2026-04-23T09:00:00.000+00:00",
      }),
    ).toBe(false);
    expect(
      isStoryDetailDto({
        ...VALID_DETAIL,
        updatedAt: "2026-04-23T10:00:00.000+00:00",
      }),
    ).toBe(false);
  });
});

describe("isUpdateStoryOutput", () => {
  const VALID = {
    id: "sid",
    title: "Saved",
    updatedAt: "2026-04-25T12:00:00.000Z",
  };

  it("accepts a canonical payload", () => {
    expect(isUpdateStoryOutput(VALID)).toBe(true);
  });

  it.each([null, undefined, 42, "string", []] as unknown[])(
    "rejects a non-object payload: %p",
    (value) => {
      expect(isUpdateStoryOutput(value)).toBe(false);
    },
  );

  it("rejects an empty id", () => {
    expect(isUpdateStoryOutput({ ...VALID, id: "" })).toBe(false);
  });

  it("rejects a blank title", () => {
    expect(isUpdateStoryOutput({ ...VALID, title: "   " })).toBe(false);
  });

  it("rejects an updatedAt without Z suffix", () => {
    expect(
      isUpdateStoryOutput({ ...VALID, updatedAt: "2026-04-25T12:00:00.000+00:00" }),
    ).toBe(false);
  });
});

describe("isRecoverableDraft", () => {
  const VALID_RECOVERABLE = {
    kind: "recoverable" as const,
    storyId: "sid",
    draftTitle: "Buffered",
    draftAt: "2026-04-25T12:00:00.000Z",
    persistedTitle: "Persisted",
  };

  it("accepts canonical none payload", () => {
    expect(isRecoverableDraft({ kind: "none" })).toBe(true);
  });

  it("accepts canonical recoverable payload", () => {
    expect(isRecoverableDraft(VALID_RECOVERABLE)).toBe(true);
  });

  it("accepts recoverable with empty draftTitle (user erased everything)", () => {
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, draftTitle: "" }),
    ).toBe(true);
  });

  it.each([null, undefined, 42, "string", []] as unknown[])(
    "rejects a non-object payload: %p",
    (value) => {
      expect(isRecoverableDraft(value)).toBe(false);
    },
  );

  it("rejects payload with neither kind nor branches", () => {
    expect(isRecoverableDraft({ storyId: "x" })).toBe(false);
  });

  it("rejects unknown kind value", () => {
    expect(isRecoverableDraft({ kind: "wrong" })).toBe(false);
  });

  it("rejects none variant carrying extra fields (drift signal)", () => {
    expect(
      isRecoverableDraft({ kind: "none", storyId: "leak" }),
    ).toBe(false);
  });

  it("rejects recoverable with missing storyId", () => {
    const { storyId: _omit, ...rest } = VALID_RECOVERABLE;
    expect(isRecoverableDraft(rest)).toBe(false);
  });

  it("rejects recoverable with empty storyId", () => {
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, storyId: "" }),
    ).toBe(false);
  });

  it("rejects recoverable with persistedTitle empty after trim", () => {
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, persistedTitle: "   " }),
    ).toBe(false);
  });

  it("rejects recoverable with draftTitle longer than 4096 chars", () => {
    expect(
      isRecoverableDraft({
        ...VALID_RECOVERABLE,
        draftTitle: "a".repeat(4097),
      }),
    ).toBe(false);
  });

  it("counts the cap by Unicode scalars to match Rust (emoji surrogate pairs)", () => {
    // 🦀 occupies 2 UTF-16 code units but 1 scalar. With UTF-16 length
    // 4096 emoji would falsely trigger the cap; the iterator form
    // matches Rust's `chars().count()`.
    const emojiDraft = "🦀".repeat(4096);
    expect(emojiDraft.length).toBe(8192); // sanity: UTF-16 doubled
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, draftTitle: emojiDraft }),
    ).toBe(true);
  });

  it("rejects 4097 unicode-scalar draftTitle even when UTF-16 length passes", () => {
    const emojiDraft = "🦀".repeat(4097);
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, draftTitle: emojiDraft }),
    ).toBe(false);
  });

  it("rejects recoverable with draftAt not ending with Z", () => {
    expect(
      isRecoverableDraft({
        ...VALID_RECOVERABLE,
        draftAt: "2026-04-25T12:00:00.000+00:00",
      }),
    ).toBe(false);
  });

  it("rejects recoverable with draftAt malformed", () => {
    expect(
      isRecoverableDraft({ ...VALID_RECOVERABLE, draftAt: "yesterday" }),
    ).toBe(false);
  });
});

const VALID_NODE: NodeContentDto = {
  id: "n1",
  text: "Bonjour",
  label: "Début",
  image: { assetId: "a1", mediaType: "image", state: "ready", format: "png", byteSize: 9 },
  audio: null,
};

describe("node guards", () => {
  it("isStoryDetailDto requires editable + a valid (or null) node", () => {
    expect(isStoryDetailDto(VALID_DETAIL)).toBe(true);
    // editable missing.
    const { editable: _e, ...noEditable } = VALID_DETAIL;
    expect(isStoryDetailDto(noEditable)).toBe(false);
    // node may be null.
    expect(isStoryDetailDto({ ...VALID_DETAIL, node: null })).toBe(true);
    // a drifted node object is rejected.
    expect(isStoryDetailDto({ ...VALID_DETAIL, node: { id: "" } })).toBe(false);
  });

  it("isNodeMediaSlot is strict on the state↔fields coupling (F11)", () => {
    const ready = {
      assetId: "a1",
      mediaType: "image",
      state: "ready",
      format: "png",
      byteSize: 9,
    };
    expect(isNodeMediaSlot(ready)).toBe(true);
    // ready WITHOUT format / size is a drift, not a `média · 0 o` fallback.
    expect(isNodeMediaSlot({ ...ready, format: undefined })).toBe(false);
    expect(isNodeMediaSlot({ ...ready, byteSize: undefined })).toBe(false);
    expect(isNodeMediaSlot({ ...ready, byteSize: -1 })).toBe(false);
    expect(isNodeMediaSlot({ ...ready, byteSize: 1.5 })).toBe(false);
    // attention MUST carry neither format nor size.
    expect(
      isNodeMediaSlot({ assetId: "a1", mediaType: "audio", state: "attention" }),
    ).toBe(true);
    expect(
      isNodeMediaSlot({
        assetId: "a1",
        mediaType: "audio",
        state: "attention",
        format: "mp3",
      }),
    ).toBe(false);
  });

  it("isNodeContentDto validates the projected node and rejects a bad slot", () => {
    expect(isNodeContentDto(VALID_NODE)).toBe(true);
    expect(
      isNodeContentDto({ ...VALID_NODE, image: { assetId: "a1", mediaType: "video", state: "ready" } }),
    ).toBe(false);
    expect(
      isNodeContentDto({ ...VALID_NODE, image: { assetId: "a1", mediaType: "image", state: "weird" } }),
    ).toBe(false);
  });

  it("isNodeWriteOutput validates the write outcome shape", () => {
    const valid = {
      id: "s1",
      updatedAt: "2026-06-27T10:00:00.000Z",
      contentChecksum: "a".repeat(64),
      node: VALID_NODE,
    };
    expect(isNodeWriteOutput(valid)).toBe(true);
    expect(isNodeWriteOutput({ ...valid, contentChecksum: "short" })).toBe(false);
    expect(isNodeWriteOutput({ ...valid, node: { id: "" } })).toBe(false);
  });

  it("isAttachNodeMediaOutcome accepts cancelled + attached, rejects drift", () => {
    expect(isAttachNodeMediaOutcome({ kind: "cancelled" })).toBe(true);
    expect(
      isAttachNodeMediaOutcome({
        kind: "attached",
        output: {
          id: "s1",
          updatedAt: "2026-06-27T10:00:00.000Z",
          contentChecksum: "a".repeat(64),
          node: VALID_NODE,
        },
      }),
    ).toBe(true);
    expect(isAttachNodeMediaOutcome({ kind: "attached" })).toBe(false);
    expect(isAttachNodeMediaOutcome({ kind: "other" })).toBe(false);
  });

  it("isNodeMediaPreview requires a data URL", () => {
    expect(isNodeMediaPreview({ dataUrl: "data:image/png;base64,AA" })).toBe(true);
    expect(isNodeMediaPreview({ dataUrl: "https://example.com/x.png" })).toBe(false);
    expect(isNodeMediaPreview({})).toBe(false);
  });

  it("isRecoverableNodeDraft accepts none + recoverable, rejects drift", () => {
    expect(isRecoverableNodeDraft({ kind: "none" })).toBe(true);
    expect(
      isRecoverableNodeDraft({
        kind: "recoverable",
        storyId: "s1",
        nodeId: "n1",
        draftText: "x",
        draftLabel: "",
        draftAt: "2026-06-27T12:00:00.000Z",
        persistedText: "",
        persistedLabel: "",
      }),
    ).toBe(true);
    expect(isRecoverableNodeDraft({ kind: "none", extra: 1 })).toBe(false);
    expect(
      isRecoverableNodeDraft({ kind: "recoverable", storyId: "s1" }),
    ).toBe(false);
  });
});

