import type React from "react";
import { useEffect, useId, useMemo, useState } from "react";

import type { SupportedOperationsDto } from "../../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../../shared/ipc-contracts/device-library";
import { Button, Field, StateChip, SurfacePanel } from "../../../shared/ui";
import {
  MAX_STORY_TITLE_CHARS,
  normalizeStoryTitle,
  reasonFor,
  validateStoryTitle,
  type StoryTitleIssue,
} from "../../library/validation/story-title";

import { DeviceImportStatusSurface } from "./DeviceImportStatusSurface";
import type { DeviceStoryImportStatus } from "../hooks/use-device-story-import";
import type { SetDeviceStoryTitleStatus } from "../hooks/use-device-story-title";
import { usePackCover } from "../hooks/use-pack-cover";
import { titleProvenanceChip } from "../title-provenance";

import "./DeviceStoryInspector.css";

const IDLE_IMPORT_STATUS: DeviceStoryImportStatus = { kind: "idle" };
const IDLE_TITLE_STATUS: SetDeviceStoryTitleStatus = { kind: "idle" };

export interface DeviceStoryInspectorProps {
  /** The device story currently selected for inspection, or null when none
   *  is. When null the inspector renders nothing. */
  story: DeviceStoryDto | null;
  /** Authoritative per-profile operation matrix of the connected device,
   *  used to gate and phrase the copy affordance honestly. */
  supportedOperations?: SupportedOperationsDto;
  /** Current state of the copy flow (owned by `useDeviceStoryImport` at
   *  the route level). Defaults to idle when the route does not wire the
   *  import (listing/inspection-only contexts). */
  importState?: DeviceStoryImportStatus;
  /** Start the copy of the inspected story. Wired by the route ONLY when
   *  the capability gate allows it (`importStory === true`); when absent
   *  the CTA stays soft-disabled with a standardized reason. */
  onImport?: (story: DeviceStoryDto) => void;
  /** Re-fire the copy from a failed state (the alert's `Réessayer`). */
  onRetryImport?: () => void;
  /** Dismiss the import status surface (success `Fermer`). */
  onDismissImportStatus?: () => void;
  /** Open the official device-support profile. Wired by the route to the
   *  same action as `LuniiDecisionPanel`; when absent (listing /
   *  inspection-only contexts) the support affordance is hidden. */
  onConsultSupportProfile?: () => void;
  /** Persist a user-typed title for the inspected pack. Wired by the route
   *  only where naming is offered; when absent, the naming affordance is
   *  hidden (listing/inspection-only contexts). Returns `true` when the
   *  write committed so the editor can close. */
  onSetTitle?: (packUuid: string, title: string) => Promise<boolean>;
  /** Status of the naming write for the inspected pack (saving/failed).
   *  Defaults to idle. */
  titleState?: SetDeviceStoryTitleStatus;
  /** Clear a previous naming failure (route-owned). Called when the user
   *  edits, cancels or reopens the editor so a stale error never lingers. */
  onDismissTitleError?: () => void;
}

/**
 * Right-column contextual inspector for the selected device story. Shows
 * only the verified facts already carried by the inventory snapshot (no
 * title, no cover, no asserted content quality — the device stores none
 * and the offline MVP consults no catalog). It makes the provenance
 * explicit ("lives on the device, not yet local") and surfaces
 * ambiguities before any copy.
 *
 * The `Copier dans ma bibliothèque` affordance (device → local library)
 * is ACTIVE when the authoritative matrix allows the copy, the payload is
 * present on the device and no local copy exists yet; otherwise it stays
 * soft-disabled with a standardized, fail-closed reason. The verb is
 * `Copier`, not `Importer`: Importer/Exporter are reserved for local file
 * artifacts (see product-language.md). The internal capability flag stays
 * `importStory`.
 *
 * All copy feedback renders in-context below the CTA (polite success,
 * alert failure with retry) — never a toast, never a modal.
 */
export function DeviceStoryInspector({
  story,
  supportedOperations,
  importState,
  onImport,
  onRetryImport,
  onDismissImportStatus,
  onConsultSupportProfile,
  onSetTitle,
  titleState,
  onDismissTitleError,
}: DeviceStoryInspectorProps): React.JSX.Element | null {
  const titleId = useId();
  const copyReasonId = useId();
  const nameFieldId = useId();
  const nameReasonId = useId();
  const nameErrorId = useId();

  // Naming editor state (hooks must precede the early return). The draft is
  // reset whenever the inspected pack changes so one story's draft never
  // bleeds into another's.
  const storyUuid = story?.uuid ?? null;
  const [isEditingName, setIsEditingName] = useState(false);
  const [nameDraft, setNameDraft] = useState("");
  useEffect(() => {
    setIsEditingName(false);
    setNameDraft("");
  }, [storyUuid]);

  const normalizedDraft = useMemo(
    () => normalizeStoryTitle(nameDraft),
    [nameDraft],
  );
  const nameIssue = useMemo<StoryTitleIssue | null>(
    () => validateStoryTitle(normalizedDraft),
    [normalizedDraft],
  );
  const nameCharCount = useMemo(
    () => Array.from(normalizedDraft).length,
    [normalizedDraft],
  );

  // Cover from the LOCAL cache (no network); decorative, so aria-hidden.
  const coverUrl = usePackCover(storyUuid ?? "", Boolean(story?.thumbnail));

  if (!story) {
    return null;
  }

  const status = importState ?? IDLE_IMPORT_STATUS;
  const isImporting = status.kind === "importing";
  // A just-succeeded copy keeps the CTA soft-disabled until the device
  // re-read lands `alreadyImported=true`: in that window the snapshot
  // still says `alreadyImported=false` and `inFlightRef` is already
  // cleared, so a re-click would relaunch the copy and Rust would turn
  // the success surface into an `already_imported` alert.
  const isImported = status.kind === "imported";
  const canImport =
    supportedOperations?.importStory === true &&
    story.contentPresent &&
    !story.alreadyImported &&
    onImport !== undefined;
  const isSoftDisabled = !canImport || isImporting || isImported;
  const refusalKind = canImport
    ? null
    : copyRefusalKind(supportedOperations, story);
  const copyReason = refusalKind ? formatCopyReason(refusalKind) : null;
  // The support-profile consultation only helps a PROFILE refusal (the V3
  // case: inspectable but not importable, or matrix absent → fail-closed).
  // `déjà dans ta bibliothèque` needs no copy, and `contenu incomplet`
  // already carries its own honest note. Hidden in listing /
  // inspection-only contexts where the route wires no handler.
  //
  // Suppressed once an import status is active (`!== "idle"`): in those
  // states the status surface owns the single next gesture. Without this,
  // a runtime `DEVICE_UNSUPPORTED` failure that coincides with the device
  // reclassifying to V3 (`importStory=false`) would render BOTH this
  // pre-click affordance AND the surface's one — two buttons with the same
  // accessible name in the same region.
  const showSupportProfile =
    refusalKind === "unsupportedProfile" &&
    onConsultSupportProfile !== undefined &&
    status.kind === "idle";

  // Honest triage of the verified snapshot facts (no device re-read, no
  // catalog) — surfaced BEFORE any copy so nothing is imported blindly.
  // Fail-closed: a fact that is absent/unknown is never asserted positively.
  const hasBlockingFacts = !story.contentPresent || story.alreadyImported;

  const handleImportClick = (): void => {
    if (isSoftDisabled) return;
    onImport?.(story);
  };

  // Recognition: show the real title + its provenance when an index covers
  // the pack; otherwise keep "Histoire non reconnue" (AC1).
  const recognized = story.title !== null;
  const provenance = story.titleSource
    ? titleProvenanceChip(story.titleSource)
    : null;

  // Naming affordance (AC2): offered for a genuinely unrecognized pack
  // ("Nommer cette histoire") and to edit a name the user typed earlier
  // ("Renommer"). Not offered for official/community titles — those are not
  // the user's to overwrite from here. Hidden entirely when the route wires
  // no handler (listing/inspection-only contexts).
  const nameStatus = titleState ?? IDLE_TITLE_STATUS;
  const isSavingName = nameStatus.kind === "saving";
  const nameError = nameStatus.kind === "failed" ? nameStatus.error : null;
  const namingOffered =
    onSetTitle !== undefined && (!recognized || story.titleSource === "user");
  const canSaveName = nameIssue === null && !isSavingName;

  const handleStartNaming = (): void => {
    // Clear any stale failure from a previous attempt on (re)open.
    if (nameError) onDismissTitleError?.();
    setNameDraft(story.title ?? "");
    setIsEditingName(true);
  };

  const handleCancelNaming = (): void => {
    if (isSavingName) return;
    if (nameError) onDismissTitleError?.();
    setIsEditingName(false);
    setNameDraft("");
  };

  const handleNameChange = (next: string): void => {
    setNameDraft(next);
    // Editing after a rejection means the user is correcting it — drop the
    // stale error so the alert doesn't keep shouting about an old value.
    if (nameError) onDismissTitleError?.();
  };

  const handleSaveName = async (): Promise<void> => {
    if (!onSetTitle || isSavingName) return;
    // Don't submit a locally-invalid title: the inline reason is already
    // shown, and the client wrapper would otherwise reject a blank/oversize
    // value as an opaque UNKNOWN instead of the canonical reason.
    if (nameIssue !== null) return;
    const committed = await onSetTitle(story.uuid, normalizeStoryTitle(nameDraft));
    if (committed) {
      setIsEditingName(false);
      setNameDraft("");
    }
  };

  const handleNameKeyDown = (
    event: React.KeyboardEvent<HTMLInputElement>,
  ): void => {
    if (event.key === "Enter" && !isSavingName) {
      event.preventDefault();
      void handleSaveName();
    }
  };

  return (
    <SurfacePanel
      elevation={1}
      as="section"
      ariaLabelledBy={titleId}
      className="device-inspector"
    >
      <h2 id={titleId} className="device-inspector__title">
        Histoire sélectionnée
      </h2>

      <div className="device-inspector__provenance">
        <StateChip tone="info" label="Sur l'appareil" />
        <p className="device-inspector__provenance-note">
          {story.alreadyImported
            ? "Cette histoire vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale."
            : "Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque locale."}
        </p>
      </div>

      {coverUrl ? (
        <img
          className="device-inspector__cover"
          src={coverUrl}
          alt=""
          aria-hidden="true"
        />
      ) : null}
      <h3 className="device-inspector__name">
        {recognized ? story.title : "Histoire non reconnue"}
      </h3>
      {provenance ? (
        <div className="device-inspector__provenance-chip">
          <StateChip tone={provenance.tone} label={provenance.label} />
        </div>
      ) : null}

      {namingOffered ? (
        <div className="device-inspector__naming">
          {isEditingName ? (
            <>
              <Field
                id={nameFieldId}
                label="Titre de l'histoire"
                value={nameDraft}
                onChange={handleNameChange}
                placeholder="Le soleil couchant…"
                autoFocus
                disabled={isSavingName}
                onKeyDown={handleNameKeyDown}
                aria-describedby={
                  [
                    nameIssue !== null ? nameReasonId : null,
                    nameError !== null ? nameErrorId : null,
                  ]
                    .filter(Boolean)
                    .join(" ") || undefined
                }
              />
              <p className="device-inspector__name-counter" aria-live="polite">
                {nameCharCount} / {MAX_STORY_TITLE_CHARS} caractères
              </p>
              {nameIssue !== null && !isSavingName ? (
                <p id={nameReasonId} className="device-inspector__reason">
                  {reasonFor(nameIssue, { charCount: nameCharCount })}
                </p>
              ) : null}
              {nameError !== null ? (
                <p
                  id={nameErrorId}
                  className="device-inspector__reason"
                  role="alert"
                >
                  {nameError.message}
                  {nameError.userAction ? ` ${nameError.userAction}` : ""}
                </p>
              ) : null}
              <div className="device-inspector__naming-actions">
                <Button
                  variant="secondary"
                  onClick={handleCancelNaming}
                  aria-disabled={isSavingName || undefined}
                >
                  Annuler
                </Button>
                <Button
                  variant="primary"
                  aria-disabled={!canSaveName || undefined}
                  aria-busy={isSavingName || undefined}
                  aria-describedby={
                    nameIssue !== null ? nameReasonId : undefined
                  }
                  onClick={() => void handleSaveName()}
                >
                  Enregistrer
                </Button>
              </div>
            </>
          ) : (
            <Button variant="secondary" onClick={handleStartNaming}>
              {recognized ? "Renommer cette histoire" : "Nommer cette histoire"}
            </Button>
          )}
        </div>
      ) : null}

      {/* Honest triage of the verified facts, BEFORE any copy. Each group
          renders only when it carries a fact (anti-catalog: only the
          inventory snapshot, no title, no asserted content quality). */}
      <div className="device-inspector__group">
        <h3 className="device-inspector__group-title">
          Ce que Rustory reconnaît
        </h3>
        <dl className="device-inspector__facts">
          <div className="device-inspector__fact">
            <dt className="device-inspector__fact-label">Identifiant</dt>
            <dd className="device-inspector__fact-value">
              <code>{story.shortId}</code>
            </dd>
          </div>
          <div className="device-inspector__fact">
            <dt className="device-inspector__fact-label">UUID</dt>
            <dd className="device-inspector__fact-value">
              <code>{story.uuid}</code>
            </dd>
          </div>
        </dl>
        {story.contentPresent ? (
          <div className="device-inspector__flags">
            {/* Neutral tone on purpose: this is a verified fact about the
                payload FOLDER being present, never a claim about content
                quality (anti-catalog) — a success/green chip would
                over-assert (product-language.md → Contenu présent). */}
            <StateChip tone="neutral" label="Contenu présent" />
          </div>
        ) : null}
      </div>

      {hasBlockingFacts ? (
        <div className="device-inspector__group">
          <h3 className="device-inspector__group-title">
            Ce qui bloque la copie
          </h3>
          <div className="device-inspector__flags">
            {story.alreadyImported ? (
              // Neutral, like the sibling `Contenu présent`: under "Ce qui
              // bloque la copie" a green/success chip would read as a
              // positive state contradicting the blocking header. It is a
              // verified fact (a local copy exists), not a quality claim.
              <StateChip tone="neutral" label="Dans ta bibliothèque" />
            ) : null}
            {!story.contentPresent ? (
              <StateChip tone="warning" label="Contenu incomplet" />
            ) : null}
          </div>
          {!story.contentPresent ? (
            <p className="device-inspector__note">
              Le dossier de contenu de cette histoire est introuvable sur
              l'appareil. Vérifie l'appareil avant de la copier.
            </p>
          ) : null}
          {story.alreadyImported ? (
            <p className="device-inspector__note">
              Une copie de cette histoire existe déjà dans ta bibliothèque
              locale ; aucune nouvelle copie n'est nécessaire.
            </p>
          ) : null}
        </div>
      ) : null}

      {story.hidden ? (
        <div className="device-inspector__group">
          <h3 className="device-inspector__group-title">
            À revoir avant de copier
          </h3>
          <div className="device-inspector__flags">
            <StateChip tone="neutral" label="Masquée" />
          </div>
          <p className="device-inspector__note">
            Cette histoire est marquée comme masquée sur l'appareil.
          </p>
        </div>
      ) : null}

      <Button
        aria-disabled={isSoftDisabled || undefined}
        aria-busy={isImporting || undefined}
        aria-describedby={copyReason ? copyReasonId : undefined}
        onClick={handleImportClick}
      >
        Copier dans ma bibliothèque
      </Button>
      {copyReason ? (
        <p id={copyReasonId} className="device-inspector__reason">
          {copyReason}
        </p>
      ) : null}
      {showSupportProfile ? (
        <Button
          variant="quiet"
          onClick={onConsultSupportProfile}
          aria-label="Consulter le profil de support officiel"
        >
          Consulter le profil de support
        </Button>
      ) : null}

      <DeviceImportStatusSurface
        status={status}
        onRetry={() => onRetryImport?.()}
        onDismiss={() => onDismissImportStatus?.()}
        onConsultSupportProfile={onConsultSupportProfile}
      />
    </SurfacePanel>
  );
}

/**
 * Discriminant of WHY a copy is refused, picked fail-closed in the
 * priority order locked by ui-states.md#Device Story Inspection Contract:
 * 1. a local copy already exists — the most useful fact: no copy needed;
 * 2. the profile does not POSITIVELY allow the copy (ops absent or
 *    `importStory !== true`, V3 included) — the GENUINE profile refusal;
 * 3. the payload folder is missing on the device;
 * 4. the profile DOES allow the copy but the route wired no `onImport`
 *    (listing / inspection-only context) — fail-closed, surfaced with the
 *    same `profil non supporté` copy, but NOT a real profile refusal.
 *
 * Kept separate from the user-facing label so the support-profile
 * affordance branches on the GENUINE refusal (`unsupportedProfile`)
 * only — never on `handlerUnavailable`, where the profile is fine and a
 * support consultation would be misleading.
 */
type CopyRefusalKind =
  | "alreadyImported"
  | "unsupportedProfile"
  | "incompleteContent"
  | "handlerUnavailable";

function copyRefusalKind(
  ops: SupportedOperationsDto | undefined,
  story: DeviceStoryDto,
): CopyRefusalKind {
  if (story.alreadyImported) {
    return "alreadyImported";
  }
  if (ops?.importStory !== true) {
    return "unsupportedProfile";
  }
  if (!story.contentPresent) {
    return "incompleteContent";
  }
  return "handlerUnavailable";
}

/** Canonical, closed-set disabled reason copy (never invented at the call
 *  site — see ui-states.md#Disabled Actions and Reasons). The
 *  `handlerUnavailable` fallback keeps the fail-closed `profil non
 *  supporté` wording (the route's gate is simply not engaged). */
function formatCopyReason(kind: CopyRefusalKind): string {
  switch (kind) {
    case "alreadyImported":
      return "Copie indisponible: déjà dans ta bibliothèque";
    case "incompleteContent":
      return "Copie indisponible: contenu incomplet sur l'appareil";
    case "unsupportedProfile":
    case "handlerUnavailable":
      return "Copie indisponible: profil non supporté";
  }
}
