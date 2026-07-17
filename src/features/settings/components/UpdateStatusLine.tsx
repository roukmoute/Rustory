import type React from "react";

import { StateChip } from "../../../shared/ui";
import { useUpdateShell } from "../../../shell/state/update-shell-store";

import "./UpdateStatusLine.css";

/**
 * The settings header's update status line (`Update Availability
 * Contract`): renders the launch's verdict WHEN IT EXISTS, under the
 * `Version {version}` line (which stays untouched — this line gives the
 * "your version / the published version" context, never an ambiguity
 * about the installed version). Renders NOTHING while no verdict exists
 * — never a spinner, never a waiting state (nobody waits for a
 * background check). The copies are Rust-carried and render VERBATIM;
 * the `info` chip accompanies the `updateAvailable` state ONLY (glyph
 * included — color alone never carries the distinction), the three
 * other states read in a calm neutral tone. NO button, NO retry.
 */
export function UpdateStatusLine(): React.JSX.Element | null {
  const availability = useUpdateShell((s) => s.availability);
  if (availability === null) {
    return null;
  }
  return (
    <p className="update-status-line" role="status">
      {availability.status === "updateAvailable" ? (
        <StateChip tone="info" label={availability.headline} />
      ) : (
        <span className="update-status-line__headline">
          {availability.headline}
        </span>
      )}
      <span className="update-status-line__notice">{availability.notice}</span>
    </p>
  );
}
