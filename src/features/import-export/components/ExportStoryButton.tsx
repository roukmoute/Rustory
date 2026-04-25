import type React from "react";

import { Button } from "../../../shared/ui";
import { sanitizeFilename } from "../lib/sanitize-filename";
import type { UseStoryExport } from "../hooks/use-story-export";

export interface ExportStoryButtonProps {
  storyId: string;
  /** LIVE title for the suggested filename. The caller is expected to
   *  pass the currently typed draft (after normalization via
   *  `normalizeStoryTitle` or equivalent) rather than the last
   *  persisted value, so an export fired mid-edit suggests the user's
   *  current work, not a stale snapshot. */
  storyTitle: string;
  exporter: UseStoryExport;
  disabled?: boolean;
  /** Called BEFORE the export flow starts. Typically the route uses
   *  this to flush a pending autosave so the artifact written on disk
   *  reflects the live draft. Can return a Promise; the button awaits
   *  it before invoking the export. */
  onBeforeTrigger?: () => void | Promise<void>;
}

/**
 * CTA that kicks off a single-story export. The component composes a
 * filesystem-safe filename from the passed title and delegates the
 * dialog + IPC flow to the shared [`useStoryExport`] hook so the parent
 * can render a sibling `ExportStatusSurface` and keep the state
 * ownership in one place.
 */
export function ExportStoryButton({
  storyId,
  storyTitle,
  exporter,
  disabled,
  onBeforeTrigger,
}: ExportStoryButtonProps): React.JSX.Element {
  const suggestedTitle = sanitizeFilename(storyTitle);
  const isExporting = exporter.status.kind === "exporting";
  // Using `aria-disabled` + `aria-busy` (rather than the native
  // `disabled` attribute) preserves focus and keyboard tab order
  // while the export is in flight. The click handler no-ops when
  // aria-disabled so the behavior matches the attribute.
  const isSoftDisabled = disabled === true || isExporting;

  return (
    <Button
      variant="secondary"
      aria-disabled={isSoftDisabled || undefined}
      aria-busy={isExporting || undefined}
      onClick={async () => {
        if (isSoftDisabled) return;
        if (onBeforeTrigger) {
          await onBeforeTrigger();
        }
        await exporter.triggerExport(storyId, suggestedTitle);
      }}
    >
      Exporter l'histoire
    </Button>
  );
}
