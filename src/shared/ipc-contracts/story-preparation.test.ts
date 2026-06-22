import { describe, expect, it } from "vitest";

import {
  isJobCompletedEvent,
  isJobFailedEvent,
  isJobProgressEvent,
  isPreparationStateDto,
  isStartPreparationAcceptedDto,
} from "./story-preparation";

const DEVICE = "0123456789abcdef0123456789abcdef";
const STORY = "0197a5d0-0000-7000-8000-000000000000";
const story = { id: STORY, title: "Mon histoire" };

const validProgress = {
  jobId: "j",
  jobType: "prepare_story",
  targetStoryId: STORY,
  phase: "preflight",
  progress: null,
  sequence: 1,
  message: null,
};

describe("isPreparationStateDto", () => {
  it("accepts each valid variant", () => {
    expect(isPreparationStateDto({ kind: "idle" })).toBe(true);
    expect(
      isPreparationStateDto({ kind: "preflight", deviceIdentifier: DEVICE, story }),
    ).toBe(true);
    expect(
      isPreparationStateDto({
        kind: "preparing",
        deviceIdentifier: DEVICE,
        story,
        progress: null,
      }),
    ).toBe(true);
    expect(
      isPreparationStateDto({
        kind: "prepared",
        deviceIdentifier: DEVICE,
        story,
        targetCohort: "v3",
      }),
    ).toBe(true);
    expect(
      isPreparationStateDto({
        kind: "retryable",
        story,
        cause: "preflightNotPassing",
        message: "m",
        userAction: "a",
        blockers: [],
      }),
    ).toBe(true);
  });

  it("rejects an unknown kind", () => {
    expect(isPreparationStateDto({ kind: "weird" })).toBe(false);
  });

  it("rejects an extra key on idle", () => {
    expect(isPreparationStateDto({ kind: "idle", extra: 1 })).toBe(false);
  });

  it("rejects a malformed deviceIdentifier", () => {
    expect(
      isPreparationStateDto({ kind: "preflight", deviceIdentifier: "nothex", story }),
    ).toBe(false);
  });

  it("rejects an unknown cause", () => {
    expect(
      isPreparationStateDto({
        kind: "retryable",
        story,
        cause: "boom",
        message: "m",
        userAction: "a",
        blockers: [],
      }),
    ).toBe(false);
  });

  it("rejects an empty userAction on retryable", () => {
    expect(
      isPreparationStateDto({
        kind: "retryable",
        story,
        cause: "interrupted",
        message: "m",
        userAction: "",
        blockers: [],
      }),
    ).toBe(false);
  });

  it("rejects an impossible (axis, cause) blocker pair", () => {
    expect(
      isPreparationStateDto({
        kind: "retryable",
        story,
        cause: "preflightNotPassing",
        message: "m",
        userAction: "a",
        blockers: [
          {
            axis: "deviceProfile",
            cause: "checksumMismatch",
            message: "m",
            userAction: "a",
          },
        ],
      }),
    ).toBe(false);
  });
});

describe("isStartPreparationAcceptedDto", () => {
  const JOB = "0197a5d0-0000-7000-8000-0000000000aa";

  it("accepts a valid acceptance with UUID ids", () => {
    expect(isStartPreparationAcceptedDto({ jobId: JOB, storyId: STORY })).toBe(
      true,
    );
  });
  it("rejects a non-UUID / empty jobId", () => {
    expect(isStartPreparationAcceptedDto({ jobId: "j", storyId: STORY })).toBe(
      false,
    );
    expect(isStartPreparationAcceptedDto({ jobId: "", storyId: STORY })).toBe(
      false,
    );
  });
  it("rejects a non-UUID storyId", () => {
    expect(isStartPreparationAcceptedDto({ jobId: JOB, storyId: "x" })).toBe(
      false,
    );
  });
  it("rejects an extra key", () => {
    expect(
      isStartPreparationAcceptedDto({ jobId: JOB, storyId: STORY, x: 1 }),
    ).toBe(false);
  });
});

describe("job event guards", () => {
  it("accepts a valid progress event", () => {
    expect(isJobProgressEvent(validProgress)).toBe(true);
  });
  it("rejects an out-of-scope phase (transfer / verify are not emitted here)", () => {
    expect(isJobProgressEvent({ ...validProgress, phase: "transfer" })).toBe(false);
  });
  it("rejects a negative sequence", () => {
    expect(isJobProgressEvent({ ...validProgress, sequence: -1 })).toBe(false);
  });
  it("rejects an extra key", () => {
    expect(isJobProgressEvent({ ...validProgress, extra: 1 })).toBe(false);
  });
  it("accepts a completed event", () => {
    expect(
      isJobCompletedEvent({
        jobId: "j",
        jobType: "prepare_story",
        targetStoryId: STORY,
        sequence: 3,
      }),
    ).toBe(true);
  });
  it("accepts a valid failed event", () => {
    expect(
      isJobFailedEvent({
        jobId: "j",
        jobType: "prepare_story",
        targetStoryId: STORY,
        sequence: 2,
        errorCode: "PREPARATION_FAILED",
        errorMessage: "m",
        userAction: "a",
      }),
    ).toBe(true);
  });
  it("rejects a failed event with an empty userAction", () => {
    expect(
      isJobFailedEvent({
        jobId: "j",
        jobType: "prepare_story",
        targetStoryId: STORY,
        sequence: 2,
        errorCode: "PREPARATION_FAILED",
        errorMessage: "m",
        userAction: "",
      }),
    ).toBe(false);
  });
});
