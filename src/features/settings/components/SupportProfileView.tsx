import type React from "react";

import type { ContentSourcePolicy } from "../../../shared/ipc-contracts/import-export";
import type {
  DeviceSupportLine,
  SupportProfile,
} from "../../../shared/ipc-contracts/settings";
import { StateChip, SurfacePanel } from "../../../shared/ui";

import "./SupportProfileView.css";

/** One independent pure read feeding a section of the screen. */
export type SectionRead<T> =
  { kind: "loading" } | { kind: "loaded"; data: T } | { kind: "unavailable" };

export interface SupportProfileViewProps {
  /** Devices + local artifacts (`read_support_profile`). */
  profileRead: SectionRead<SupportProfile>;
  /** Content sources (`read_content_source_policy`, reused VERBATIM). */
  policyRead: SectionRead<ContentSourcePolicy>;
}

// Frontend-frozen literals of the screen (product-language.md). The
// matrix CONTENT (families, cohorts, kinds, channels, labels, reasons)
// is never composed here — it renders verbatim from the Rust-carried
// DTOs.
const SECTION_DEVICES = "Appareils";
const SECTION_ARTIFACTS = "Artefacts locaux";
const SECTION_FILE_ASSOCIATION = "Association de fichiers";
const SECTION_SOURCES = "Sources de contenu";
const SECTION_POSTURE = "Politique de distribution";
/** Calm chip labels of the FOURTH vocabulary — a durable distribution
 *  limit, never a runtime error. */
const CHIP_AVAILABLE = "Disponible";
const CHIP_NOT_AVAILABLE = "Non disponible dans cette version";
/** The node-media formats line — the canonical copy VERBATIM. */
const NODE_MEDIA_FORMATS =
  "Formats acceptés : images PNG, JPEG, BMP ; sons MP3, WAV, OGG";
/** The distribution posture, derived from the PRD official
 *  distribution policy (frozen copy). */
const DISTRIBUTION_POSTURE =
  "La distribution officielle autorise par défaut les histoires créées dans Rustory, tes contenus personnels et les contenus explicitement libres. Elle n'active jamais de flux orientés vers des contenus protégés non autorisés et n'intègre aucun contournement de protections techniques.";
/** Calm honest copies of a section whose pure read failed —
 *  `role="status"`, no retry (a failed pure read is a contract drift,
 *  not a transient failure). */
const PROFILE_UNAVAILABLE = "Le profil de support n'a pas pu être lu.";
const SOURCES_UNAVAILABLE = "Les sources de contenu n'ont pas pu être lues.";

/** Group the flat device lines by family, preserving the wire order of
 *  both the groups and their lines (pure projection — no hardcoded
 *  family list). Identity is the STABLE wire tag (`family`), never the
 *  human copy: `familyLabel` travels along for the heading only. */
function groupByFamily(
  devices: DeviceSupportLine[],
): { family: string; familyLabel: string; lines: DeviceSupportLine[] }[] {
  const groups: {
    family: string;
    familyLabel: string;
    lines: DeviceSupportLine[];
  }[] = [];
  for (const line of devices) {
    const group = groups.find((g) => g.family === line.family);
    if (group) {
      group.lines.push(line);
    } else {
      groups.push({
        family: line.family,
        familyLabel: line.familyLabel,
        lines: [line],
      });
    }
  }
  return groups;
}

function SectionUnavailable({ copy }: { copy: string }): React.JSX.Element {
  return (
    <p className="support-profile__unavailable" role="status">
      {copy}
    </p>
  );
}

/**
 * The five read-only sections of the `Profil de support` screen
 * (`Support Profile Screen Contract`). Everything the matrix says
 * renders VERBATIM from the DTOs; a non-available capability renders
 * the calm neutral chip plus its frozen reason — never a bare ✗, never
 * an error tone: a distribution limit is durable information.
 */
export function SupportProfileView({
  profileRead,
  policyRead,
}: SupportProfileViewProps): React.JSX.Element {
  return (
    <div className="support-profile">
      <SurfacePanel
        as="section"
        elevation={1}
        className="support-profile__section"
      >
        <h2 className="support-profile__section-title">{SECTION_DEVICES}</h2>
        <div
          className="support-profile__section-body"
          aria-busy={profileRead.kind === "loading" || undefined}
        >
          {profileRead.kind === "unavailable" && (
            <SectionUnavailable copy={PROFILE_UNAVAILABLE} />
          )}
          {profileRead.kind === "loaded" &&
            groupByFamily(profileRead.data.devices).map((group) => (
              <div key={group.family} className="support-profile__family">
                <h3 className="support-profile__family-title">
                  {group.familyLabel}
                </h3>
                {group.lines.map((line) => (
                  <div key={line.cohort} className="support-profile__cohort">
                    <div className="support-profile__cohort-head">
                      <span className="support-profile__cohort-name">
                        {line.cohortLabel}
                      </span>
                      {line.metadataFormatLabel !== undefined && (
                        <span className="support-profile__cohort-format">
                          {line.metadataFormatLabel}
                        </span>
                      )}
                    </div>
                    <ul className="support-profile__capabilities">
                      {line.capabilities.map((capability) => (
                        <li
                          key={capability.operation}
                          className="support-profile__capability"
                        >
                          <span className="support-profile__capability-label">
                            {capability.label}
                          </span>
                          <StateChip
                            tone={capability.available ? "success" : "neutral"}
                            label={
                              capability.available
                                ? CHIP_AVAILABLE
                                : CHIP_NOT_AVAILABLE
                            }
                          />
                          {capability.reason !== undefined && (
                            <span className="support-profile__reason">
                              {capability.reason}
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            ))}
        </div>
      </SurfacePanel>

      <SurfacePanel
        as="section"
        elevation={1}
        className="support-profile__section"
      >
        <h2 className="support-profile__section-title">{SECTION_ARTIFACTS}</h2>
        <div
          className="support-profile__section-body"
          aria-busy={profileRead.kind === "loading" || undefined}
        >
          {profileRead.kind === "unavailable" && (
            <SectionUnavailable copy={PROFILE_UNAVAILABLE} />
          )}
          {profileRead.kind === "loaded" && (
            <>
              <ul className="support-profile__artifacts">
                {profileRead.data.localArtifacts.map((line) => (
                  <li key={line.kind} className="support-profile__artifact">
                    <div className="support-profile__artifact-head">
                      <span className="support-profile__artifact-label">
                        {line.label}
                      </span>
                      {line.formatLabel !== undefined && (
                        <span className="support-profile__artifact-format">
                          {line.formatLabel}
                        </span>
                      )}
                    </div>
                    <div className="support-profile__artifact-state">
                      <StateChip
                        tone={line.available ? "success" : "neutral"}
                        label={
                          line.available ? CHIP_AVAILABLE : CHIP_NOT_AVAILABLE
                        }
                      />
                      {line.capabilitiesLabel !== undefined && (
                        <span className="support-profile__artifact-capabilities">
                          {line.capabilitiesLabel}
                        </span>
                      )}
                      {line.reason !== undefined && (
                        <span className="support-profile__reason">
                          {line.reason}
                        </span>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
              <p className="support-profile__media-formats">
                {NODE_MEDIA_FORMATS}
              </p>
            </>
          )}
        </div>
      </SurfacePanel>

      <SurfacePanel
        as="section"
        elevation={1}
        className="support-profile__section"
      >
        <h2 className="support-profile__section-title">
          {SECTION_FILE_ASSOCIATION}
        </h2>
        <div
          className="support-profile__section-body"
          aria-busy={profileRead.kind === "loading" || undefined}
        >
          {profileRead.kind === "unavailable" && (
            <SectionUnavailable copy={PROFILE_UNAVAILABLE} />
          )}
          {profileRead.kind === "loaded" && (
            <>
              {profileRead.data.fileAssociation.currentInstall !==
                undefined && (
                <p
                  className="support-profile__association-notice"
                  role="status"
                >
                  {profileRead.data.fileAssociation.currentInstall.notice}
                </p>
              )}
              <p className="support-profile__association-extension">
                {profileRead.data.fileAssociation.extensionLabel}
              </p>
              <ul className="support-profile__associations">
                {profileRead.data.fileAssociation.channels.map((channel) => (
                  <li
                    key={channel.channel}
                    className="support-profile__association"
                  >
                    <span className="support-profile__association-label">
                      {channel.label}
                    </span>
                    <div className="support-profile__association-state">
                      <StateChip
                        tone={channel.registered ? "success" : "neutral"}
                        label={channel.statusLabel}
                      />
                      {channel.reason !== undefined && (
                        <span className="support-profile__reason">
                          {channel.reason}
                        </span>
                      )}
                    </div>
                    <p className="support-profile__association-detail">
                      {channel.detail}
                    </p>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      </SurfacePanel>

      <SurfacePanel
        as="section"
        elevation={1}
        className="support-profile__section"
      >
        <h2 className="support-profile__section-title">{SECTION_SOURCES}</h2>
        <div
          className="support-profile__section-body"
          aria-busy={policyRead.kind === "loading" || undefined}
        >
          {policyRead.kind === "unavailable" && (
            <SectionUnavailable copy={SOURCES_UNAVAILABLE} />
          )}
          {policyRead.kind === "loaded" && (
            <ul className="support-profile__sources">
              {policyRead.data.sources.map((source) => (
                <li key={source.kind} className="support-profile__source">
                  <span className="support-profile__source-label">
                    {source.label}
                  </span>
                  {source.activation === "enabled" ? (
                    <span className="support-profile__source-marker">
                      {source.activationMarker}
                    </span>
                  ) : (
                    <span className="support-profile__reason">
                      {source.reason}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      </SurfacePanel>

      <SurfacePanel
        as="section"
        elevation={1}
        className="support-profile__section"
      >
        <h2 className="support-profile__section-title">{SECTION_POSTURE}</h2>
        <p className="support-profile__posture">{DISTRIBUTION_POSTURE}</p>
      </SurfacePanel>
    </div>
  );
}
