import { beforeEach, describe, expect, it } from "vitest";

import { useArchiveExportShell } from "./archive-export-shell-store";

describe("archive-export-shell-store", () => {
  beforeEach(() => {
    // The store is a module singleton: reset it between tests.
    useArchiveExportShell.setState({ status: { kind: "idle" } });
  });

  it("starts idle", () => {
    expect(useArchiveExportShell.getState().status).toEqual({ kind: "idle" });
  });

  it("refreshes the percent of the export in flight", () => {
    const shell = useArchiveExportShell.getState();
    shell.setStatus({
      kind: "exporting",
      storyId: "s1",
      storyTitle: "Les Bestioles",
      percent: null,
    });
    shell.setProgress("s1", 42);
    expect(useArchiveExportShell.getState().status).toEqual({
      kind: "exporting",
      storyId: "s1",
      storyTitle: "Les Bestioles",
      percent: 42,
    });
  });

  it("ignores a tick for another story", () => {
    const shell = useArchiveExportShell.getState();
    shell.setStatus({
      kind: "exporting",
      storyId: "s1",
      storyTitle: "Les Bestioles",
      percent: 10,
    });
    shell.setProgress("s2", 80);
    expect(useArchiveExportShell.getState().status).toMatchObject({
      storyId: "s1",
      percent: 10,
    });
  });

  it("never lets a late tick resurrect a settled export", () => {
    const shell = useArchiveExportShell.getState();
    const settled = {
      kind: "exported" as const,
      storyTitle: "Les Bestioles",
      destinationPath: "/tmp/Les Bestioles.zip",
      bytesWritten: 1024,
      imageCount: 1,
      audioCount: 2,
    };
    shell.setStatus(settled);
    shell.setProgress("s1", 99);
    expect(useArchiveExportShell.getState().status).toEqual(settled);

    shell.setStatus({ kind: "idle" });
    shell.setProgress("s1", 99);
    expect(useArchiveExportShell.getState().status).toEqual({ kind: "idle" });
  });
});
