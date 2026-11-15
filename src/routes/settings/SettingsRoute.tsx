import type React from "react";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { getVersion } from "@tauri-apps/api/app";

import {
  CatalogPanel,
  invalidateDeviceLibraryCache,
  invalidatePackCoverCache,
  useOfficialCatalog,
} from "../../features/device";
import type { SectionRead } from "../../features/settings/components/SupportProfileView";
import { AnnouncementVoiceSettings } from "../../features/settings/components/AnnouncementVoiceSettings";
import { SupportProfileView } from "../../features/settings/components/SupportProfileView";
import { UpdateApplyZone } from "../../features/settings/components/UpdateApplyZone";
import { UpdateCheckButton } from "../../features/settings/components/UpdateCheckButton";
import { UpdateStatusLine } from "../../features/settings/components/UpdateStatusLine";
import { readContentSourcePolicy } from "../../ipc/commands/import-export";
import { readSupportProfile } from "../../ipc/commands/settings";
import type { ContentSourcePolicy } from "../../shared/ipc-contracts/import-export";
import type { SupportProfile } from "../../shared/ipc-contracts/settings";
import { Button, Icon } from "../../shared/ui";
import { useUpdateShell } from "../../shell/state/update-shell-store";
import { useUpdateApplyShell } from "../../shell/state/update-apply-shell-store";

import "./SettingsRoute.css";

/**
 * `Aide et réglages` owns the catalogue, voices, updates and read-only
 * support profile in a standalone view on `/settings`.
 *
 * Two INDEPENDENT pure reads feed the sections (`read_support_profile`
 * for devices + artifacts, `read_content_source_policy` reused
 * VERBATIM for the sources): a failed read renders its sections in the
 * calm `unavailable` state without ever taking down the sections whose
 * read succeeded — fail-closed per section, never invented content, no
 * retry (a failed pure read is a contract drift, not a transient
 * failure). Catalogue network access requires an explicit user action.
 */
export function SettingsRoute(): React.JSX.Element {
  const navigate = useNavigate();
  const officialCatalog = useOfficialCatalog({
    onChanged: () => {
      // The collection routes remount after settings: discard both cached
      // titles and covers so the return shows the newly recognized packs.
      invalidateDeviceLibraryCache();
      invalidatePackCoverCache();
    },
  });
  const isCatalogBusy = officialCatalog.action !== "idle";
  const updateStatus = useUpdateShell((state) => state.availability?.status);
  const applyStatus = useUpdateApplyShell((state) => state.state?.status);
  const updatesRef = useRef<HTMLDetailsElement>(null);
  const updateRunning = applyStatus === "running";

  // Keep every update component mounted when the disclosure is folded.
  // Each new actionable state opens it again, without moving keyboard focus.
  useEffect(() => {
    if (
      updatesRef.current &&
      (updateStatus === "updateAvailable" ||
        applyStatus === "running" ||
        applyStatus === "readyToRestart" ||
        applyStatus === "failed")
    ) {
      updatesRef.current.open = true;
    }
  }, [updateStatus, applyStatus]);
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
  const supportUnavailable =
    profileRead.kind === "unavailable" || policyRead.kind === "unavailable";
  const updateNotice =
    applyStatus === "running"
      ? "Mise à jour en cours…"
      : applyStatus === "readyToRestart"
        ? "Redémarrage disponible"
        : applyStatus === "failed"
          ? "Mise à jour à réessayer"
          : updateStatus === "updateAvailable"
            ? "Nouvelle version disponible"
            : null;

  return (
    <main
      className="settings-route"
      aria-label="Aide et réglages"
      aria-busy={isLoading}
    >
      <header className="settings-route__header">
        <div className="settings-route__heading">
          <p className="settings-route__eyebrow">À ta façon</p>
          <h1 className="settings-route__title">Aide et réglages</h1>
          <p className="settings-route__intro">
            Les voix, les histoires et un coup de main quand tu en as besoin.
          </p>
        </div>
        <Button
          variant="secondary"
          disabled={isCatalogBusy}
          onClick={() => {
            // `replace` keeps the browser history a single in/out
            // transition for the library ↔ settings context (the
            // StoryEditRoute pattern) — the system back button never
            // bounces back to the profile just left.
            navigate("/library", { replace: true });
          }}
        >
          <Icon name="arrow-left" />
          Retour à la bibliothèque
        </Button>
      </header>
      <div className="settings-route__preferences">
        <AnnouncementVoiceSettings />
        <CatalogPanel catalog={officialCatalog} />
      </div>
      <section className="settings-route__welcome">
        <span className="settings-route__help-icon" aria-hidden="true">
          <Icon name="wand" />
        </span>
        <div>
          <h2>Besoin d'un coup de main ?</h2>
          <p>Retrouve les premiers pas pour utiliser Rustory et ta Lunii.</p>
        </div>
        <Button
          variant="secondary"
          disabled={isCatalogBusy}
          onClick={() => navigate("/welcome")}
        >
          Revoir l'accueil
        </Button>
      </section>
      <details className="settings-route__disclosure" ref={updatesRef}>
        <summary
          aria-disabled={updateRunning || undefined}
          onClick={(event) => {
            // A running download always keeps its progress in view.
            if (updateRunning) event.preventDefault();
          }}
        >
          <span className="settings-route__disclosure-icon" aria-hidden="true">
            <Icon name="download" />
          </span>
          <span className="settings-route__summary-copy">
            <span className="settings-route__summary-title">
              Version et mises à jour
            </span>
            {version !== null && (
              <span className="settings-route__version">Version {version}</span>
            )}
          </span>
          {updateNotice !== null && (
            <span className="settings-route__update-notice">{updateNotice}</span>
          )}
          <Icon name="chevron-down" className="settings-route__chevron" />
        </summary>
        <div className="settings-route__update-content">
          <UpdateStatusLine />
          <UpdateCheckButton />
          <UpdateApplyZone />
        </div>
      </details>
      <details className="settings-route__disclosure">
        <summary>
          <span className="settings-route__disclosure-icon" aria-hidden="true">
            <Icon name="device" />
          </span>
          <span className="settings-route__summary-copy">
            <span className="settings-route__summary-title">
              Compatibilité et informations
            </span>
            <span className="settings-route__summary-description">
              Appareils, formats de fichiers et sources de contenu.
            </span>
            {supportUnavailable && (
              <span className="settings-route__support-notice" role="status">
                Certaines informations ne sont pas disponibles.
              </span>
            )}
          </span>
          <Icon name="chevron-down" className="settings-route__chevron" />
        </summary>
        <section
          className="settings-route__support-content"
          aria-labelledby="support-profile-title"
        >
          <h2 id="support-profile-title" className="settings-route__section-title">
            Profil de support
          </h2>
          <SupportProfileView profileRead={profileRead} policyRead={policyRead} />
        </section>
      </details>
    </main>
  );
}
