import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { UseStoryExport } from "../hooks/use-story-export";
import { ExportStoryButton } from "./ExportStoryButton";

function makeExporter(
  overrides: Partial<UseStoryExport> = {},
): UseStoryExport {
  return {
    status: { kind: "idle" },
    triggerExport: vi.fn().mockResolvedValue(undefined),
    retryExport: vi.fn().mockResolvedValue(undefined),
    dismissStatus: vi.fn(),
    ...overrides,
  } as UseStoryExport;
}

describe("<ExportStoryButton />", () => {
  it("renders an enabled button labelled Exporter l'histoire when idle", () => {
    render(
      <ExportStoryButton
        storyId="story-1"
        storyTitle="Mon histoire"
        exporter={makeExporter()}
      />,
    );
    const button = screen.getByRole("button", {
      name: /Exporter l'histoire/i,
    });
    expect(button).toBeInTheDocument();
    expect(button).not.toHaveAttribute("aria-disabled", "true");
    expect(button).not.toHaveAttribute("aria-busy", "true");
  });

  it("uses aria-disabled + aria-busy (NOT the native disabled attribute) while exporting", () => {
    render(
      <ExportStoryButton
        storyId="story-1"
        storyTitle="Mon histoire"
        exporter={makeExporter({ status: { kind: "exporting" } })}
      />,
    );
    const button = screen.getByRole("button", {
      name: /Exporter l'histoire/i,
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("aria-busy", "true");
    // Focus + tab order MUST be preserved — `disabled` would remove
    // the element from the tab sequence and the user could no longer
    // discover the in-progress label by tabbing back to it.
    expect(button).not.toBeDisabled();
  });

  it("uses aria-disabled when the parent passes disabled=true even if exporter is idle", () => {
    render(
      <ExportStoryButton
        storyId="story-1"
        storyTitle="Mon histoire"
        exporter={makeExporter()}
        disabled
      />,
    );
    const button = screen.getByRole("button", {
      name: /Exporter l'histoire/i,
    });
    expect(button).toHaveAttribute("aria-disabled", "true");
    // Not exporting — should not advertise busy.
    expect(button).not.toHaveAttribute("aria-busy", "true");
  });

  it("does not call triggerExport when the user clicks while exporting (soft-disabled no-op)", async () => {
    const user = userEvent.setup();
    const exporter = makeExporter({ status: { kind: "exporting" } });
    render(
      <ExportStoryButton
        storyId="story-1"
        storyTitle="Mon histoire"
        exporter={exporter}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /Exporter l'histoire/i }),
    );
    expect(exporter.triggerExport).not.toHaveBeenCalled();
  });

  it("awaits onBeforeTrigger BEFORE invoking triggerExport (live title flush guarantee)", async () => {
    const user = userEvent.setup();
    const callOrder: string[] = [];
    const onBeforeTrigger = vi.fn().mockImplementation(async () => {
      // Simulate a non-trivial flush: yield the microtask queue to
      // prove the button actually awaits the promise instead of
      // firing triggerExport synchronously.
      await Promise.resolve();
      callOrder.push("before");
    });
    const triggerExport = vi.fn().mockImplementation(async () => {
      callOrder.push("trigger");
    });
    render(
      <ExportStoryButton
        storyId="story-1"
        storyTitle="Mon histoire"
        exporter={makeExporter({ triggerExport })}
        onBeforeTrigger={onBeforeTrigger}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /Exporter l'histoire/i }),
    );
    expect(callOrder).toEqual(["before", "trigger"]);
  });

  it("passes the sanitized filename built from the LIVE title to triggerExport", async () => {
    const user = userEvent.setup();
    const triggerExport = vi.fn().mockResolvedValue(undefined);
    render(
      <ExportStoryButton
        storyId="story-42"
        storyTitle="Un / Deux : Trois"
        exporter={makeExporter({ triggerExport })}
      />,
    );
    await user.click(
      screen.getByRole("button", { name: /Exporter l'histoire/i }),
    );
    expect(triggerExport).toHaveBeenCalledWith("story-42", "Un_Deux_Trois");
  });
});
