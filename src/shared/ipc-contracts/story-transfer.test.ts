import { describe, expect, it } from "vitest";

import {
  isStartTransferAcceptedDto,
  isTransferOutcomeDto,
  isTransferStateDto,
} from "./story-transfer";

const DEVICE = "0123456789abcdef0123456789abcdef";
const STORY = "0197a5d0-0000-7000-8000-000000000000";
const story = { id: STORY, title: "Mon histoire" };
const summary = {
  changed: "« Mon histoire » est maintenant sur la Lunii.",
  unchanged: "2 autres histoires de l'appareil restent inchangées.",
};

describe("isTransferStateDto", () => {
  it("accepts each valid variant", () => {
    expect(isTransferStateDto({ kind: "idle" })).toBe(true);
    expect(
      isTransferStateDto({
        kind: "transferring",
        deviceIdentifier: DEVICE,
        story,
        progress: null,
      }),
    ).toBe(true);
    expect(
      isTransferStateDto({
        kind: "verified",
        deviceIdentifier: DEVICE,
        story,
        summary,
      }),
    ).toBe(true);
    expect(
      isTransferStateDto({
        kind: "retryable",
        story,
        cause: "writeNotAuthorized",
        message: "m",
        userAction: "a",
      }),
    ).toBe(true);
  });

  it("rejects an unknown kind", () => {
    expect(isTransferStateDto({ kind: "weird" })).toBe(false);
  });

  it("rejects an extra key on verified", () => {
    expect(
      isTransferStateDto({
        kind: "verified",
        deviceIdentifier: DEVICE,
        story,
        summary,
        extra: 1,
      }),
    ).toBe(false);
  });

  it("rejects a verified with a malformed summary (missing line / empty / extra key)", () => {
    const base = { kind: "verified", deviceIdentifier: DEVICE, story };
    expect(isTransferStateDto(base)).toBe(false); // missing summary
    expect(
      isTransferStateDto({ ...base, summary: { changed: "c" } }),
    ).toBe(false); // missing `unchanged`
    expect(
      isTransferStateDto({ ...base, summary: { changed: "", unchanged: "u" } }),
    ).toBe(false); // empty line
    expect(
      isTransferStateDto({
        ...base,
        summary: { changed: "c", unchanged: "u", extra: 1 },
      }),
    ).toBe(false); // extra key
  });

  it("rejects a malformed deviceIdentifier", () => {
    expect(
      isTransferStateDto({
        kind: "transferring",
        deviceIdentifier: "nothex",
        story,
        progress: null,
      }),
    ).toBe(false);
  });

  it("rejects an out-of-range progress", () => {
    expect(
      isTransferStateDto({
        kind: "transferring",
        deviceIdentifier: DEVICE,
        story,
        progress: 1.5,
      }),
    ).toBe(false);
  });

  it("rejects an unknown cause", () => {
    expect(
      isTransferStateDto({
        kind: "retryable",
        story,
        cause: "boom",
        message: "m",
        userAction: "a",
      }),
    ).toBe(false);
  });

  it("accepts the devicePackUnprovable cause (the protective update refusal)", () => {
    expect(
      isTransferStateDto({
        kind: "retryable",
        story,
        cause: "devicePackUnprovable",
        message:
          "Envoi interrompu : la copie présente sur l'appareil est dans un état que Rustory ne reconnaît pas, rien n'a été modifié.",
        userAction:
          "Vérifie l'appareil, débranche-le puis rebranche-le, puis relance l'envoi.",
        completeness: "failed",
      }),
    ).toBe(true);
  });

  it("rejects an empty userAction on retryable", () => {
    expect(
      isTransferStateDto({
        kind: "retryable",
        story,
        cause: "interrupted",
        message: "m",
        userAction: "",
      }),
    ).toBe(false);
  });

  it("accepts an optional completeness on retryable", () => {
    for (const completeness of ["failed", "incomplete"]) {
      expect(
        isTransferStateDto({
          kind: "retryable",
          story,
          cause: "writeRejected",
          message: "m",
          userAction: "a",
          completeness,
        }),
      ).toBe(true);
    }
  });

  it("rejects an unknown completeness on retryable", () => {
    expect(
      isTransferStateDto({
        kind: "retryable",
        story,
        cause: "writeRejected",
        message: "m",
        userAction: "a",
        completeness: "partial",
      }),
    ).toBe(false);
  });

  it("rejects a malformed story id", () => {
    expect(
      isTransferStateDto({
        kind: "verified",
        deviceIdentifier: DEVICE,
        story: { id: "x", title: "t" },
        summary,
      }),
    ).toBe(false);
  });
});

describe("isStartTransferAcceptedDto", () => {
  const JOB = "0197a5d0-0000-7000-8000-0000000000aa";

  it("accepts a valid acceptance with UUID ids", () => {
    expect(isStartTransferAcceptedDto({ jobId: JOB, storyId: STORY })).toBe(
      true,
    );
  });
  it("rejects a non-UUID / empty jobId", () => {
    expect(isStartTransferAcceptedDto({ jobId: "j", storyId: STORY })).toBe(
      false,
    );
    expect(isStartTransferAcceptedDto({ jobId: "", storyId: STORY })).toBe(
      false,
    );
  });
  it("rejects a non-UUID storyId", () => {
    expect(isStartTransferAcceptedDto({ jobId: JOB, storyId: "x" })).toBe(false);
  });
  it("rejects an extra key", () => {
    expect(
      isStartTransferAcceptedDto({ jobId: JOB, storyId: STORY, x: 1 }),
    ).toBe(false);
  });
});

describe("isTransferOutcomeDto", () => {
  const RECORDED_AT = "2026-06-23T00:00:00.000Z";
  const base = {
    storyId: STORY,
    message: "Un message.",
    userAction: "Une action.",
    recordedAt: RECORDED_AT,
  };

  it("accepts a verified outcome with a summary and no cause", () => {
    expect(
      isTransferOutcomeDto({ ...base, terminalKind: "verified", summary }),
    ).toBe(true);
  });

  it("accepts a write retryable outcome with a cause and no summary", () => {
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "retryable",
        cause: "deviceChanged",
      }),
    ).toBe(true);
  });

  it("accepts a remembered devicePackUnprovable retryable outcome", () => {
    // The durable memory re-hydrates the protective FR23 refusal like any other
    // write-phase cause.
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "retryable",
        cause: "devicePackUnprovable",
      }),
    ).toBe(true);
  });

  it("accepts a verify retryable outcome with neither cause nor summary", () => {
    // The verify `failed` verdict folds onto `retryable` carrying no write cause.
    expect(isTransferOutcomeDto({ ...base, terminalKind: "retryable" })).toBe(
      true,
    );
  });

  it("accepts a partial outcome with neither cause nor summary", () => {
    expect(isTransferOutcomeDto({ ...base, terminalKind: "partial" })).toBe(
      true,
    );
  });

  it("requires a cause on an incomplete outcome", () => {
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "incomplete",
        cause: "writeRejected",
      }),
    ).toBe(true);
    expect(isTransferOutcomeDto({ ...base, terminalKind: "incomplete" })).toBe(
      false,
    );
  });

  it("rejects an unknown terminalKind", () => {
    expect(isTransferOutcomeDto({ ...base, terminalKind: "transferring" })).toBe(
      false,
    );
  });

  it("rejects a summary on a non-verified terminal and a missing summary on verified", () => {
    expect(
      isTransferOutcomeDto({ ...base, terminalKind: "partial", summary }),
    ).toBe(false);
    expect(isTransferOutcomeDto({ ...base, terminalKind: "verified" })).toBe(
      false,
    );
  });

  it("rejects a cause on a verified or partial terminal", () => {
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "verified",
        summary,
        cause: "writeRejected",
      }),
    ).toBe(false);
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "partial",
        cause: "writeRejected",
      }),
    ).toBe(false);
  });

  it("rejects an unrecognized cause", () => {
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "retryable",
        cause: "bogus",
      }),
    ).toBe(false);
  });

  it("rejects a non-UTC-millisecond recordedAt", () => {
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "partial",
        recordedAt: "2026-06-23T00:00:00Z",
      }),
    ).toBe(false);
    expect(
      isTransferOutcomeDto({
        ...base,
        terminalKind: "partial",
        recordedAt: "yesterday",
      }),
    ).toBe(false);
  });

  it("rejects an empty message / userAction and an extra key", () => {
    expect(
      isTransferOutcomeDto({ ...base, terminalKind: "partial", message: "" }),
    ).toBe(false);
    expect(
      isTransferOutcomeDto({ ...base, terminalKind: "partial", userAction: "" }),
    ).toBe(false);
    expect(
      isTransferOutcomeDto({ ...base, terminalKind: "partial", extra: 1 }),
    ).toBe(false);
  });

  it("rejects a malformed storyId", () => {
    expect(
      isTransferOutcomeDto({ ...base, storyId: "x", terminalKind: "partial" }),
    ).toBe(false);
  });
});
