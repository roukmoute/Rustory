import { describe, expect, it } from "vitest";

import {
  isStartTransferAcceptedDto,
  isTransferStateDto,
} from "./story-transfer";

const DEVICE = "0123456789abcdef0123456789abcdef";
const STORY = "0197a5d0-0000-7000-8000-000000000000";
const story = { id: STORY, title: "Mon histoire" };

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
        kind: "transferred",
        deviceIdentifier: DEVICE,
        story,
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

  it("rejects an extra key on transferred", () => {
    expect(
      isTransferStateDto({
        kind: "transferred",
        deviceIdentifier: DEVICE,
        story,
        extra: 1,
      }),
    ).toBe(false);
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
        kind: "transferred",
        deviceIdentifier: DEVICE,
        story: { id: "x", title: "t" },
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
