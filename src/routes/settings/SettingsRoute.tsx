import type React from "react";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { getVersion } from "@tauri-apps/api/app";

import type { SectionRead } from "../../features/settings/components/SupportProfileView";
import { SupportProfileView } from "../../features/settings/components/SupportProfileView";
import { UpdateStatusLine } from "../../features/settings/components/UpdateStatusLine";
import { readContentSourcePolicy } from "../../ipc/commands/import-export";
import { readSupportProfile } from "../../ipc/commands/settings";
import type { ContentSourcePolicy } from "../../shared/ipc-contracts/import-export";
import type { SupportProfile } from "../../shared/ipc-contracts/settings";
import { Button } from "../../shared/ui";

import "./SettingsRoute.css";

/**
 * The read-only `Profil de support` screen (`Support Profile Screen
 * Contract`): a standalone single-column view on `/settings` — the
 * `StoryEditRoute` pattern, never the three-column library grid.
 *
 * Two INDEPENDENT pure reads feed the sections (`read_support_profile`
 * for devices + artifacts, `read_content_source_policy` reused
 * VERBATIM for the sources): a failed read renders its sections in the
 * calm `unavailable` state without ever taking down the sections whose
 * read succeeded — fail-closed per section, never invented content, no
 * retry (a failed pure read is a contract drift, not a transient
 * failure). Zero network by construction (NFR14).
 */
export function SettingsRoute(): React.JSX.Element {
  const navigate = useNavigate();
  const [profileRead, setProfileRead] = useState<SectionRead<SupportProfile>>({
    kind: "loading",
  });
  const [policyRead, setPolicyRead] = useState<
    SectionRead<ContentSourcePolicy>
  >({ kind: "loading" });
  // The app version renders when the read lands; on failure the line
  // is OMITTED — never an invented value.
  const [version, setVersion] = useState<string | null>(null);

  // Mount token: only the reads issued by the LATEST mount may apply
  // their result (the `policyReadTokenRef` pattern, StrictMode-safe —
  // the cleanup invalidates the aborted first pass).
  const readTokenRef = useRef(0);

  useEffect(() => {
    readTokenRef.current += 1;
    const token = readTokenRef.current;
    setProfileRead({ kind: "loading" });
    setPolicyRead({ kind: "loading" });
    void readSupportProfile().then(
      (profile) => {
        if (readTokenRef.current === token) {
          setProfileRead({ kind: "loaded", data: profile });
        }
      },
      () => {
        if (readTokenRef.current === token) {
          setProfileRead({ kind: "unavailable" });
        }
      },
    );
    void readContentSourcePolicy().then(
      (policy) => {
        if (readTokenRef.current === token) {
          setPolicyRead({ kind: "loaded", data: policy });
        }
      },
      () => {
        if (readTokenRef.current === token) {
          setPolicyRead({ kind: "unavailable" });
        }
      },
    );
    void getVersion().then(
      (value) => {
        if (readTokenRef.current === token) {
          setVersion(value);
        }
      },
      () => {
        // Version read failed: the header line stays omitted.
      },
    );
    return () => {
      readTokenRef.current += 1;
    };
  }, []);

  const isLoading =
    profileRead.kind === "loading" || policyRead.kind === "loading";

  return (
    <main
      className="settings-route"
      aria-label="Profil de support"
      aria-busy={isLoading}
    >
      <header className="settings-route__header">
        <div className="settings-route__heading">
          <h1 className="settings-route__title">Profil de support</h1>
          {version !== null && (
            <p className="settings-route__version">Version {version}</p>
          )}
          {/* The launch's update-availability verdict, UNDER the
              installed-version line (which never moves): renders when a
              verdict exists, NOTHING before (`Update Availability
              Contract`). */}
          <UpdateStatusLine />
        </div>
        <Button
          variant="secondary"
          onClick={() => {
            // `replace` keeps the browser history a single in/out
            // transition for the library ↔ settings context (the
            // StoryEditRoute pattern) — the system back button never
            // bounces back to the profile just left.
            navigate("/library", { replace: true });
          }}
        >
          Retour à la bibliothèque
        </Button>
      </header>
      <SupportProfileView profileRead={profileRead} policyRead={policyRead} />
    </main>
  );
}
