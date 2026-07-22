import type React from "react";
import { useEffect, useId, useState } from "react";

import type { AppError } from "../../../shared/errors/app-error";
import type {
  SupportedFamilyDto,
  SupportedOperationsDto,
} from "../../../shared/ipc-contracts/device";
import type {
  ValidationBlocker,
  ValidationVerdict,
} from "../../../shared/ipc-contracts/story-validation";
import {
  Button,
  ProgressIndicator,
  StateChip,
  SurfacePanel,
} from "../../../shared/ui";

import "./LuniiDecisionPanel.css";

export type LuniiDeviceState =
  | "absent"
  | "idle"
  | "unsupported"
  | "ambiguous"
  | "scanning"
  | "error";

/**
 * Read-only pre-transfer comparison, composed by Rust and only PRESENTED
 * here. `none` is the sober "nothing to compare yet" state (no single local
 * selection, or no readable device); `ready` carries the device membership
 * (`onDevice` ⇒ a send would replace) and how many other device stories stay
 * untouched. No size metric — there is no decisional volume before media
 * preparation.
 */
/** Why no comparison can be shown — each maps to a distinct, actionable
 *  hint so the user knows exactly what to do next (select a story, narrow to
 *  one, or plug a readable Lunii). */
export type NoComparisonReason = "no-selection" | "multi-selection" | "no-device";

export type TransferComparisonView =
  | { kind: "none"; reason: NoComparisonReason }
  | { kind: "loading" }
  | { kind: "ready"; onDevice: boolean; unchangedCount: number }
  | { kind: "error"; error: AppError };

/**
 * Read-only pre-transfer validation verdict, composed by Rust and only
 * PRESENTED here (AC1/AC2/AC3). `none` is the sober "nothing to validate yet"
 * state; `ready` carries the verdict + the closed `axis × cause` blockers the
 * panel groups by axis (canonical validity vs Lunii compatibility). The verdict
 * is ORTHOGONAL to the send gate — the CTA stays disabled regardless.
 */
export type StoryValidationView =
  | { kind: "none" }
  | { kind: "loading" }
  | { kind: "ready"; verdict: ValidationVerdict; blockers: ValidationBlocker[] }
  | { kind: "error"; error: AppError };

/**
 * Read-only-then-LOCAL preparation state, composed by Rust and only PRESENTED
 * here (AC1/AC2/AC3). `unavailable` shows the disabled `Préparer` CTA + the
 * standardized "Préparation indisponible: …" reason; `ready` shows the active
 * CTA (verdict présumée transférable). The `preflight` / `preparing` phases come
 * from `job:progress`; `prepared` / `retryable` from the authoritative re-read.
 * Preparation is ORTHOGONAL to the send gate — reaching `prepared` never enables
 * the `Envoyer` CTA.
 */
export type PreparationView =
  | { kind: "unavailable"; reason: string }
  | { kind: "ready" }
  | { kind: "preflight" }
  | { kind: "preparing"; progress: number | null }
  | { kind: "prepared" }
  | {
      kind: "retryable";
      message: string;
      userAction: string;
      blockers: ValidationBlocker[];
    }
  | { kind: "error"; error: AppError };

/**
 * Transfer (real device WRITE + VERIFY) state, composed by Rust and only
 * PRESENTED here (AC1/AC2/AC3). `unavailable` shows the disabled `Envoyer vers la
 * Lunii` CTA + the standardized "Envoi indisponible: …" reason (fail-closed: the
 * default until a writable cohort + a `Préparée` story + a clear target are all
 * proven); `ready` shows the active CTA (no confirmation modal — AC1).
 * `transferring` comes from `job:progress`; `verifying` is the TRANSIENT
 * "écriture effectuée — vérification à venir" during the `verify` phase. The
 * resting terminals are `verified` (`transférée et vérifiée` + the AC2 summary),
 * `partial` (`état partiel`) and the verify `failed` verdict (reusing
 * `retryable` / `échec récupérable`).
 */
export type TransferView =
  | { kind: "unavailable"; reason: string }
  | { kind: "ready" }
  | { kind: "transferring"; progress: number | null; phase: string | null }
  | { kind: "verifying" }
  | { kind: "verified"; changed: string; unchanged: string }
  | { kind: "partial"; message: string; userAction: string }
  | { kind: "retryable"; message: string; userAction: string }
  | { kind: "incomplete"; message: string; userAction: string }
  | { kind: "error"; error: AppError };

export interface LuniiDecisionPanelProps {
  /** Authoritative device state derived from `useConnectedLunii`. */
  deviceState?: LuniiDeviceState;
  /** Friendly label shown when `deviceState === "idle"` (e.g.
   *  "Lunii Origine 2.x"). Optional; falls back to "Appareil prêt". */
  deviceLabel?: string;
  /** Standardized reason copy shown when an action is disabled.
   *  Sourced from `docs/architecture/ui-states.md#Disabled Actions and
   *  Reasons` — never invented at the call site. */
  deviceReason?: string;
  /** Authoritative per-profile operation matrix. Rendered as a small
   *  list under the device chip when the device is `idle` so AC1's
   *  "affiche les opérations officiellement supportées" requirement
   *  is satisfied. Omitted on non-idle states. */
  supportedOperations?: SupportedOperationsDto;
  /** Family of the detected supported device. Drives ONLY the
   *  family-correct labels: the transfer capability line and the send
   *  CTA (Lunii keeps `Transfert vers la Lunii` / `Envoyer vers la
   *  Lunii`; any other family reads `Transfert vers l'appareil` /
   *  `Envoyer vers l'appareil`). State rendering NEVER branches on it —
   *  the recognized-vs-ready rule derives from the capabilities. */
  deviceFamily?: SupportedFamilyDto;
  /** Number of selected stories in the library. Drives the Éditer
   *  CTA's enabled state. */
  selectedCount?: number;
  /** Read-only pre-transfer comparison. When omitted, the comparison
   *  section is not rendered at all (used by tests/storybook that do not
   *  exercise it). When provided, it renders between the selection and the
   *  device regions — never as the panel's visual center. */
  comparison?: TransferComparisonView;
  /** Retry trigger for a failed comparison — wired by the route to
   *  `useTransferPreview.refresh`. Makes the "Réessaie la comparaison" copy
   *  actionable. When omitted, the error shows its text without a button. */
  onRetryComparison?: () => void;
  /** Read-only pre-transfer validation verdict. When omitted, the validation
   *  section is not rendered at all (used by tests/storybook that do not
   *  exercise it). When provided, it renders between the comparison and the
   *  device regions — never as the panel's visual center. */
  validation?: StoryValidationView;
  /** Retry trigger for a failed validation — wired by the route to
   *  `useStoryValidation.refresh`. Makes the "Réessaie la validation" copy
   *  actionable. When omitted, the error shows its text without a button. */
  onRetryValidation?: () => void;
  /** Local preparation state. When omitted, the preparation section is not
   *  rendered (tests/storybook that do not exercise it). When provided, it
   *  renders between the validation and the device regions — never the panel's
   *  visual center. */
  preparation?: PreparationView;
  /** Start the preparation — wired by the route to `useStoryPreparation.prepare`.
   *  Drives the active `Préparer` CTA (the `ready` view). */
  onPrepare?: () => void;
  /** Re-run a failed preparation — wired by the route to
   *  `useStoryPreparation.retry`. Drives the `Relancer` / `Réessayer` action in
   *  the `retryable` / `error` views. */
  onRetryPreparation?: () => void;
  /** Transfer (real device write) state. When omitted, the panel keeps the
   *  legacy always-disabled send CTA in the device region (tests/storybook that
   *  do not exercise the write flow). When provided, the route OWNS the send
   *  gate: the `Envoyer vers la Lunii` CTA + the transfer status live in a
   *  dedicated "Transfert" region, and the device region no longer renders the
   *  CTA. */
  transfer?: TransferView;
  /** Start the transfer — wired by the route to `useStoryTransfer.send`. Drives
   *  the active `Envoyer vers la Lunii` CTA (the `ready` view). */
  onSend?: () => void;
  /** Re-run a failed transfer — wired by the route to `useStoryTransfer.retry`.
   *  Drives the `Relancer` / `Réessayer` action in the `retryable` / `incomplete`
   *  / `error` views. */
  onRetryTransfer?: () => void;
  /** Abandon a failed / incomplete transfer — wired by the route to
   *  `useStoryTransfer.dismiss`. Drives the `Abandonner` action; the local draft
   *  is never touched (AC3). */
  onDismissTransfer?: () => void;
  /** Required when the panel may expose an active Éditer CTA. */
  onEdit: () => void;
  /** Delete the CONFIRMED selection — wired by the route to the
   *  `delete_stories` command. Fired only after the explicit in-panel
   *  confirmation gesture (never on the first click). When omitted, the
   *  Supprimer CTA stays disabled with its reason (tests/storybook that do
   *  not exercise deletion). */
  onDeleteSelected?: () => void;
  /** True while the route's delete call is in flight — freezes the
   *  confirmation gesture so a double activation cannot fire twice. */
  isDeletingSelection?: boolean;
  /** Canonical message of a failed deletion, surfaced inline as an alert.
   *  `null`/omitted when the last deletion attempt did not fail. */
  deleteSelectionError?: string | null;
  /** Optional refresh trigger — wired by the route to
   *  `useConnectedLunii.refresh`. When omitted, the refresh button is
   *  hidden (used by tests/storybook that do not need the affordance). */
  onRefreshDevice?: () => void;
  /** Optional fallback for the unsupported / ambiguous / error
   *  states. Wired by the route to open
   *  `docs/architecture/device-support-profile.md`. When omitted, the
   *  link is hidden — used by tests that do not need the affordance. */
  onConsultSupportProfile?: () => void;
}

/**
 * Decision surface shown in the library's right column.
 *
 * Layer 1 (selection feedback): a state chip that summarizes how many
 * stories are selected, and an `Éditer` CTA that activates exactly when
 * the selection is a singleton.
 *
 * Layer 2 (device readiness): a state chip + the canonical send CTA
 * (always visible, always disabled in MVP Phase 1 with a typed reason),
 * plus a `Réessayer la détection` action when the route wires
 * `onRefreshDevice`.
 */
export function LuniiDecisionPanel({
  deviceState = "absent",
  deviceLabel,
  deviceReason,
  supportedOperations,
  deviceFamily,
  selectedCount = 0,
  comparison,
  onRetryComparison,
  validation,
  onRetryValidation,
  preparation,
  onPrepare,
  onRetryPreparation,
  transfer,
  onSend,
  onRetryTransfer,
  onDismissTransfer,
  onEdit,
  onDeleteSelected,
  isDeletingSelection = false,
  deleteSelectionError = null,
  onRefreshDevice,
  onConsultSupportProfile,
}: LuniiDecisionPanelProps): React.JSX.Element {
  // GENERAL product rule "recognized ≠ ready": derived from the
  // authoritative capability matrix, NEVER from the family name. A
  // supported profile with zero activated capability (FLAM Gen1 today,
  // any future such profile) renders the honest recognized state.
  // Undefined operations (non-idle states, legacy tests) keep the
  // historical ready behavior.
  const hasAnyCapability =
    supportedOperations === undefined ||
    supportedOperations.readLibrary ||
    supportedOperations.inspectStory ||
    supportedOperations.importStory ||
    supportedOperations.writeStory;
  const isRecognizedWithoutCapability =
    deviceState === "idle" && !hasAnyCapability;
  // The recognized-without-capability idle state deliberately does NOT
  // join this list: its support-profile pointer is the STATIC text
  // below (zero navigation, zero network — NFR14), never the external
  // link this CTA opens. The CTA keeps its pre-existing scope
  // (unsupported / ambiguous / error).
  const showSupportProfile =
    onConsultSupportProfile !== undefined &&
    (deviceState === "unsupported" ||
      deviceState === "ambiguous" ||
      deviceState === "error");
  const titleId = useId();
  const deviceReasonId = useId();
  const editReasonId = useId();
  const preparationReasonId = useId();
  const transferReasonId = useId();

  const deviceChipLabel = formatDeviceChipLabel(
    deviceState,
    deviceLabel,
    hasAnyCapability,
    deviceFamily,
  );
  const deviceChipTone = formatDeviceChipTone(deviceState);

  const selectionChipLabel = formatSelectionLabel(selectedCount);
  const selectionChipTone = selectedCount > 0 ? "info" : "neutral";

  const editReason = formatEditReason(selectedCount);
  const editIsActive = selectedCount === 1;

  const deleteReasonId = useId();
  const deleteIsActive = selectedCount >= 1 && onDeleteSelected !== undefined;
  // Two-gesture destructive confirmation (the StoryStructureNavigator
  // pattern): the first click only OPENS the confirmation; the removal
  // fires on the explicit second gesture. Any change of the selection
  // withdraws a pending confirmation — a stale confirm can never apply to
  // a different selection than the one it named.
  const [isConfirmingDelete, setIsConfirmingDelete] = useState<boolean>(false);
  useEffect(() => {
    setIsConfirmingDelete(false);
  }, [selectedCount]);

  const sendDisabledReason =
    deviceReason ?? formatSendReason(deviceState, hasAnyCapability, deviceFamily);

  const isScanning = deviceState === "scanning";

  return (
    <SurfacePanel
      elevation={1}
      as="div"
      ariaLabelledBy={titleId}
      className="lunii-panel"
    >
      <h2 id={titleId} className="lunii-panel__title">
        Panneau de décision
      </h2>

      <section className="lunii-panel__selection" aria-label="Sélection courante">
        <StateChip tone={selectionChipTone} label={selectionChipLabel} />
        {editIsActive ? (
          <Button variant="secondary" onClick={onEdit}>
            Éditer
          </Button>
        ) : (
          <>
            <Button
              variant="secondary"
              aria-disabled="true"
              aria-describedby={editReasonId}
            >
              Éditer
            </Button>
            <p id={editReasonId} className="lunii-panel__reason">
              {editReason}
            </p>
          </>
        )}
        {!deleteIsActive ? (
          <>
            <Button
              variant="quiet"
              aria-disabled="true"
              aria-describedby={deleteReasonId}
            >
              Supprimer
            </Button>
            <p id={deleteReasonId} className="lunii-panel__reason">
              {formatDeleteReason()}
            </p>
          </>
        ) : !isConfirmingDelete ? (
          <Button variant="quiet" onClick={() => setIsConfirmingDelete(true)}>
            Supprimer
          </Button>
        ) : (
          <div className="lunii-panel__delete-confirm">
            <p className="lunii-panel__reason">
              {formatDeleteConfirmCopy(selectedCount)}
            </p>
            {isDeletingSelection ? (
              <Button variant="primary" aria-disabled="true">
                Suppression…
              </Button>
            ) : (
              <Button variant="primary" onClick={onDeleteSelected}>
                Confirmer la suppression
              </Button>
            )}
            <Button
              variant="quiet"
              onClick={() => setIsConfirmingDelete(false)}
            >
              Annuler
            </Button>
          </div>
        )}
        {deleteSelectionError !== null && (
          <p role="alert" className="lunii-panel__reason">
            {deleteSelectionError}
          </p>
        )}
      </section>

      {comparison && (
        <section
          className="lunii-panel__comparison"
          aria-label="Comparaison avant envoi"
          aria-live="polite"
        >
          {renderComparison(comparison, deviceFamily, onRetryComparison)}
        </section>
      )}

      {validation && (
        <section
          className="lunii-panel__validation"
          aria-label="Validation avant envoi"
          aria-live="polite"
        >
          {renderValidation(validation, deviceFamily, onRetryValidation)}
        </section>
      )}

      {preparation && (
        <section
          className="lunii-panel__preparation"
          aria-label="Préparation"
          aria-live="polite"
        >
          {renderPreparation(
            preparation,
            preparationReasonId,
            deviceFamily,
            onPrepare,
            onRetryPreparation,
          )}
        </section>
      )}

      {transfer && (
        <section
          className="lunii-panel__transfer"
          aria-label="Transfert"
          aria-live="polite"
        >
          {renderTransfer(
            transfer,
            transferReasonId,
            deviceFamily,
            onSend,
            onRetryTransfer,
            onDismissTransfer,
          )}
        </section>
      )}

      <section className="lunii-panel__device" aria-label="État de l'appareil">
        <StateChip tone={deviceChipTone} label={deviceChipLabel} />
        {deviceState === "idle" && supportedOperations && (
          <ul
            className="lunii-panel__operations"
            aria-label="Opérations supportées par l'appareil détecté"
          >
            {formatSupportedOperationLabels(
              supportedOperations,
              deviceFamily,
            ).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        )}
        {isRecognizedWithoutCapability && (
          // Static, durable explanation (never role="alert"): the
          // device is officially recognized while no operation is
          // activated in this version. The support-profile pointer is
          // TEXT ONLY — no navigation, no network (NFR14): consulting
          // the profiles is a separate surface, so no internal target
          // exists to wire yet. Rendered in this idle state ONLY,
          // never in a capability-bearing idle (ui-states.md
          // "Recognized ≠ ready").
          <p className="lunii-panel__reason">
            Appareil reconnu, aucune opération activée dans cette version.
            Consulte le profil de support pour comprendre ce qui est permis.
          </p>
        )}
        {transfer === undefined ? (
          // Legacy fallback (tests/storybook without the write flow): the send
          // CTA lives here, disabled, with its standardized reason.
          <>
            <Button aria-disabled="true" aria-describedby={deviceReasonId}>
              {formatSendCtaLabel(deviceFamily)}
            </Button>
            <p id={deviceReasonId} className="lunii-panel__reason">
              {sendDisabledReason}
            </p>
          </>
        ) : (
          // The route owns the send CTA (in the Transfert region). The device
          // region keeps surfacing the DEVICE detail reason (unsupported /
          // ambiguous / error) so the user still sees why the Lunii is not ready.
          deviceReason && (
            <p className="lunii-panel__reason">{deviceReason}</p>
          )
        )}
        {onRefreshDevice && !isScanning && (
          <Button
            variant="quiet"
            onClick={onRefreshDevice}
            aria-label="Réessayer la détection de l'appareil"
          >
            Réessayer la détection
          </Button>
        )}
        {showSupportProfile && (
          <Button
            variant="quiet"
            onClick={onConsultSupportProfile}
            aria-label="Consulter le profil de support officiel"
          >
            Consulter le profil de support
          </Button>
        )}
      </section>
    </SurfacePanel>
  );
}

function renderComparison(
  view: TransferComparisonView,
  deviceFamily: SupportedFamilyDto | undefined,
  onRetryComparison?: () => void,
): React.JSX.Element {
  switch (view.kind) {
    case "none":
      // Distinct hint per cause so the next gesture is unambiguous.
      return (
        <p className="lunii-panel__reason">
          {formatNoComparisonHint(view.reason, deviceFamily)}
        </p>
      );
    case "loading":
      return (
        <ProgressIndicator mode="indeterminate" label="Comparaison en cours…" />
      );
    case "ready":
      return (
        <>
          <StateChip
            tone={view.onDevice ? "warning" : "info"}
            label={
              view.onDevice
                ? "Déjà présente sur l'appareil"
                : "Nouvelle sur l'appareil"
            }
          />
          <p className="lunii-panel__comparison-verdict">
            {view.onDevice
              ? "Déjà présente sur l'appareil — un envoi la remplacerait."
              : "Cette histoire serait ajoutée à l'appareil."}
          </p>
          <p className="lunii-panel__reason">
            {formatUnchanged(view.unchangedCount)}
          </p>
        </>
      );
    case "error":
      // Critical feedback IN CONTEXT (role="alert"), never a toast (UX-DR15).
      // The "Réessaie la comparaison" copy is made actionable by a retry CTA.
      return (
        <div role="alert" className="lunii-panel__comparison-error">
          <p>{view.error.message}</p>
          {view.error.userAction && <p>{view.error.userAction}</p>}
          {onRetryComparison && (
            <Button
              variant="quiet"
              onClick={onRetryComparison}
              aria-label="Réessayer la comparaison"
            >
              Réessayer
            </Button>
          )}
        </div>
      );
  }
}

function renderValidation(
  view: StoryValidationView,
  deviceFamily: SupportedFamilyDto | undefined,
  onRetryValidation?: () => void,
): React.JSX.Element {
  switch (view.kind) {
    case "none":
      // Sober "nothing to validate yet" — the comparison section above already
      // tells the user which gesture (select / plug) is missing. Family-correct
      // copy: a Lunii panel keeps the historical wording VERBATIM, any other
      // family reads the device-generic one (product-language.md).
      return (
        <p className="lunii-panel__reason">
          {deviceFamily === undefined || deviceFamily === "lunii"
            ? "Sélectionne une histoire locale et branche une Lunii lisible pour vérifier la compatibilité avant l'envoi."
            : "Sélectionne une histoire locale et branche un appareil lisible pour vérifier la compatibilité avant l'envoi."}
        </p>
      );
    case "loading":
      return (
        <ProgressIndicator mode="indeterminate" label="Validation en cours…" />
      );
    case "ready":
      return renderVerdict(view.verdict, view.blockers, deviceFamily);
    case "error":
      // Critical feedback IN CONTEXT (role="alert"), never a toast (UX-DR15).
      // The "Réessaie la validation" copy is made actionable by a retry CTA.
      return (
        <div role="alert" className="lunii-panel__validation-error">
          <p>{view.error.message}</p>
          {view.error.userAction && <p>{view.error.userAction}</p>}
          {onRetryValidation && (
            <Button
              variant="quiet"
              onClick={onRetryValidation}
              aria-label="Réessayer la validation"
            >
              Réessayer
            </Button>
          )}
        </div>
      );
  }
}

function renderPreparation(
  view: PreparationView,
  reasonId: string,
  deviceFamily: SupportedFamilyDto | undefined,
  onPrepare?: () => void,
  onRetryPreparation?: () => void,
): React.JSX.Element {
  switch (view.kind) {
    case "unavailable":
      // The Préparer CTA is visible but disabled, with the standardized reason
      // (kept focusable so keyboard users reach the reason via aria-describedby).
      return (
        <>
          <Button
            variant="secondary"
            aria-disabled="true"
            aria-describedby={reasonId}
          >
            Préparer
          </Button>
          <p id={reasonId} className="lunii-panel__reason">
            {view.reason}
          </p>
        </>
      );
    case "ready":
      // Verdict présumée transférable: the preparation can run. The send CTA
      // below stays disabled regardless — preparation never enables it.
      return (
        <Button variant="secondary" onClick={onPrepare}>
          Préparer
        </Button>
      );
    case "preflight":
      return <StateChip tone="neutral" label="en vérification" />;
    case "preparing":
      // Honest progress: the phase is always named. A determinate bar is shown
      // ONLY when a reliable fraction is known; otherwise the indicator stays
      // calm and indeterminate rather than a fake percentage. MVP sends no
      // fraction, but a future reliable `progress` is surfaced, not hidden.
      return (
        <>
          <StateChip tone="neutral" label="en préparation" />
          {view.progress != null ? (
            <ProgressIndicator
              mode="determinate"
              label="Préparation en cours…"
              value={Math.round(view.progress * 100)}
            />
          ) : (
            <ProgressIndicator
              mode="indeterminate"
              label="Préparation en cours…"
            />
          )}
        </>
      );
    case "prepared":
      // Discreet "Préparée" indicator — NOT a transfer success, and it never
      // enables the send CTA.
      return <StateChip tone="success" label="Préparée" />;
    case "retryable":
      // Recoverable failure IN CONTEXT (role="alert"), never a toast (UX-DR15).
      // The canonical state label `échec récupérable` is shown (glyph + text),
      // and a non-passing preflight reports its blockers (reused 3.x grouping).
      return (
        <div role="alert" className="lunii-panel__preparation-error">
          <StateChip tone="error" label="échec récupérable" />
          <p>{view.message}</p>
          <p className="lunii-panel__reason">{view.userAction}</p>
          {view.blockers.length > 0 &&
            renderPreparationBlockers(view.blockers, deviceFamily)}
          {onRetryPreparation && (
            <Button
              variant="quiet"
              onClick={onRetryPreparation}
              aria-label="Relancer la préparation"
            >
              Relancer
            </Button>
          )}
        </div>
      );
    case "error":
      // Transport failure: in-context, never a toast.
      return (
        <div role="alert" className="lunii-panel__preparation-error">
          <p>{view.error.message}</p>
          {view.error.userAction && <p>{view.error.userAction}</p>}
          {onRetryPreparation && (
            <Button
              variant="quiet"
              onClick={onRetryPreparation}
              aria-label="Réessayer la préparation"
            >
              Réessayer
            </Button>
          )}
        </div>
      );
  }
}

function renderPreparationBlockers(
  blockers: ValidationBlocker[],
  deviceFamily: SupportedFamilyDto | undefined,
): React.JSX.Element {
  // Same two-axis split + grouping as the validation verdict, so a non-passing
  // preflight reuses the exact blocker presentation (never a second wording).
  const canonical = blockers.filter((b) => b.axis !== "deviceProfile");
  const device = blockers.filter((b) => b.axis === "deviceProfile");
  return (
    <>
      {canonical.length > 0 && renderBlockerGroup("Validité Rustory", canonical)}
      {device.length > 0 &&
        renderBlockerGroup(formatDeviceCompatibilityHeading(deviceFamily), device)}
    </>
  );
}

function renderTransfer(
  view: TransferView,
  reasonId: string,
  deviceFamily: SupportedFamilyDto | undefined,
  onSend?: () => void,
  onRetryTransfer?: () => void,
  onDismissTransfer?: () => void,
): React.JSX.Element {
  switch (view.kind) {
    case "unavailable":
      // The send CTA is visible but disabled, with the standardized reason
      // (kept focusable so keyboard users reach the reason via aria-describedby).
      return (
        <>
          <Button aria-disabled="true" aria-describedby={reasonId}>
            {formatSendCtaLabel(deviceFamily)}
          </Button>
          <p id={reasonId} className="lunii-panel__reason">
            {view.reason}
          </p>
        </>
      );
    case "ready":
      // Writable cohort + a `Préparée` story + a clear target: the write can run.
      // No confirmation modal (AC1) — the context is unambiguous. Only a Lunii
      // can be write-authorized in this phase, so `ready` always renders the
      // Lunii label — the family-correct helper keeps that true by derivation.
      return <Button onClick={onSend}>{formatSendCtaLabel(deviceFamily)}</Button>;
    case "transferring": {
      // Honest progress (AC1): the phase is NAMED (preflight gate vs write); a
      // determinate bar shows ONLY when a reliable fraction is known, never a fake
      // percentage, and is CAPPED at 99 % — 100 % is reserved for the terminal
      // (F6). The secondary action is a NON-destructive "Consulter le détail"
      // disclosure naming the real phase (F5); explicit cancel is out of scope.
      const phaseLabel =
        view.phase === "preflight"
          ? "vérification de l'appareil"
          : view.phase === "transfer"
            ? "envoi en cours"
            : // The `prepare` phase (local re-assembly, before any device write) AND
              // the optimistic window before the 1st job:progress both name a NEUTRAL
              // "preparing" phase — never the wrong "envoi en cours" (C2/AC1).
              "préparation de l'envoi…";
      const percent =
        view.progress != null
          ? Math.min(99, Math.round(view.progress * 100))
          : null;
      return (
        <>
          <StateChip tone="neutral" label="en transfert" />
          {percent != null ? (
            <ProgressIndicator
              mode="determinate"
              label="Transfert en cours…"
              value={percent}
            />
          ) : (
            <ProgressIndicator
              mode="indeterminate"
              label="Transfert en cours…"
            />
          )}
          <details className="lunii-panel__transfer-detail">
            <summary>Consulter le détail</summary>
            <p className="lunii-panel__reason">
              Phase : {phaseLabel}.{" "}
              {percent != null
                ? `Avancement : ${percent} %.`
                : "Avancement : en cours."}
            </p>
          </details>
        </>
      );
    }
    case "verifying":
      // TRANSIENT (AC1): the write is done, the verify re-read is running. The
      // honest "écriture effectuée — vérification à venir" — no invented %, and
      // NOT a resting terminal (it settles to verified / état partiel / échoué).
      return (
        <div className="lunii-panel__transfer-done">
          <StateChip tone="neutral" label="écriture effectuée" />
          <p className="lunii-panel__reason">
            Écriture effectuée — vérification à venir.
          </p>
        </div>
      );
    case "verified":
      // SUCCESS terminal (AC2): the FIRST appearance of `transférée et vérifiée`,
      // shown only after the verify proof. Both summary lines are COMPOSED IN RUST
      // and rendered VERBATIM here (no React reinterpretation). `aria-live="polite"`
      // (the section), never a toast.
      return (
        <div className="lunii-panel__transfer-verified">
          <StateChip tone="success" label="transférée et vérifiée" />
          <p className="lunii-panel__comparison-verdict">{view.changed}</p>
          <p className="lunii-panel__reason">{view.unchanged}</p>
        </div>
      );
    case "partial":
      // `état partiel` (AC3): verify found the device mutated + present but
      // INCOHERENT. A non-success IN CONTEXT (role="alert"), never a toast, never
      // success vocabulary. Warning tone like `incomplete` but a DISTINCT label
      // (`état partiel` ≠ `transfert incomplet`). Same two recovery gestures.
      return (
        <div role="alert" className="lunii-panel__transfer-error">
          <StateChip tone="warning" label="état partiel" />
          <p>{view.message}</p>
          <p className="lunii-panel__reason">{view.userAction}</p>
          {renderTransferRecovery(onRetryTransfer, onDismissTransfer)}
        </div>
      );
    case "retryable":
      // `échoué` (AC2): the device was left UNTOUCHED. Recoverable failure IN
      // CONTEXT (role="alert"), never a toast (UX-DR15). Canonical `échec
      // récupérable` chip (glyph + text). Both recovery gestures: Relancer (full
      // cycle) and Abandonner (back to a stable library, draft intact — AC3).
      return (
        <div role="alert" className="lunii-panel__transfer-error">
          <StateChip tone="error" label="échec récupérable" />
          <p>{view.message}</p>
          <p className="lunii-panel__reason">{view.userAction}</p>
          {renderTransferRecovery(onRetryTransfer, onDismissTransfer)}
        </div>
      );
    case "incomplete":
      // `incomplet` (AC2): the write STARTED then was interrupted (device
      // mutated) — the Lunii may hold a partial copy. DISTINCT label `transfert
      // incomplet` with its own glyph (tone ≠ `échoué`, never color-only) and an
      // honest message; a relance (full cycle) restores a safe state. Same two
      // gestures as `échoué`. NEVER a success ("écriture effectuée" stays for the
      // clean terminal) nor `état partiel` (a later flow).
      return (
        <div role="alert" className="lunii-panel__transfer-error">
          <StateChip tone="warning" label="transfert incomplet" />
          <p>{view.message}</p>
          <p className="lunii-panel__reason">{view.userAction}</p>
          {renderTransferRecovery(onRetryTransfer, onDismissTransfer)}
        </div>
      );
    case "error":
      // Transport failure: in-context, never a toast.
      return (
        <div role="alert" className="lunii-panel__transfer-error">
          <p>{view.error.message}</p>
          {view.error.userAction && <p>{view.error.userAction}</p>}
          {onRetryTransfer && (
            <Button
              variant="quiet"
              onClick={onRetryTransfer}
              aria-label="Réessayer le transfert"
            >
              Réessayer
            </Button>
          )}
        </div>
      );
  }
}

/** Recovery gestures shared by the `échoué` (`retryable`) and `incomplet`
 *  terminals (AC3): Relancer re-runs a FULL cycle (never a hidden partial
 *  resume), Abandonner returns to a stable library with the local draft intact. */
function renderTransferRecovery(
  onRetryTransfer?: () => void,
  onDismissTransfer?: () => void,
): React.JSX.Element {
  return (
    <>
      {onRetryTransfer ? (
        <Button
          variant="quiet"
          onClick={onRetryTransfer}
          aria-label="Relancer le transfert"
        >
          Relancer
        </Button>
      ) : (
        // The relaunch needs a writable Lunii; an incomplet / interrupted terminal
        // typically follows a device removal, so the route withholds onRetry. Give
        // an HONEST next gesture instead of an inert button (C1).
        <p className="lunii-panel__reason">Rebranche la Lunii pour relancer.</p>
      )}
      {onDismissTransfer && (
        <Button
          variant="quiet"
          onClick={onDismissTransfer}
          aria-label="Abandonner le transfert"
        >
          Abandonner
        </Button>
      )}
    </>
  );
}

function renderVerdict(
  verdict: ValidationVerdict,
  blockers: ValidationBlocker[],
  deviceFamily: SupportedFamilyDto | undefined,
): React.JSX.Element {
  // AC1: the two axes are kept visible side by side — canonical validity
  // (structure / media / filesystem) vs device compatibility (deviceProfile).
  const canonical = blockers.filter((b) => b.axis !== "deviceProfile");
  const device = blockers.filter((b) => b.axis === "deviceProfile");
  const { tone, label } = formatVerdictChip(verdict);
  return (
    <>
      <StateChip tone={tone} label={label} />
      {blockers.length === 0 ? (
        <p className="lunii-panel__reason">
          Aucun blocage détecté pour l'instant.
        </p>
      ) : (
        <>
          {canonical.length > 0 &&
            renderBlockerGroup("Validité Rustory", canonical)}
          {device.length > 0 &&
            renderBlockerGroup(formatDeviceCompatibilityHeading(deviceFamily), device)}
        </>
      )}
    </>
  );
}

function renderBlockerGroup(
  heading: string,
  blockers: ValidationBlocker[],
): React.JSX.Element {
  // An `h3` WITHOUT a landmark region: the surrounding `<section>` already
  // carries the accessible name; nested regions would over-fragment the panel.
  return (
    <div className="lunii-panel__validation-group">
      <h3 className="lunii-panel__validation-heading">{heading}</h3>
      <ul className="lunii-panel__blockers">
        {blockers.map((b) => (
          <li key={`${b.axis}:${b.cause}`} className="lunii-panel__blocker">
            <p className="lunii-panel__blocker-message">{b.message}</p>
            <p className="lunii-panel__reason">{b.userAction}</p>
          </li>
        ))}
      </ul>
    </div>
  );
}

function formatVerdictChip(verdict: ValidationVerdict): {
  tone: "success" | "warning" | "error";
  label: string;
} {
  switch (verdict) {
    case "presumedTransferable":
      return { tone: "success", label: "Présumée transférable" };
    case "toFix":
      return { tone: "warning", label: "À corriger" };
    case "blocked":
      return { tone: "error", label: "Bloquée" };
  }
}

function formatNoComparisonHint(
  reason: NoComparisonReason,
  deviceFamily: SupportedFamilyDto | undefined,
): string {
  switch (reason) {
    case "no-selection":
      return "Sélectionne une histoire locale pour comparer avant l'envoi.";
    case "multi-selection":
      return "Sélectionne une seule histoire locale pour comparer (le transfert multiple n'est pas encore disponible).";
    case "no-device":
      // Family-correct: a Lunii panel keeps the historical wording
      // VERBATIM, any other family reads the device-generic one.
      return deviceFamily === undefined || deviceFamily === "lunii"
        ? "Branche une Lunii lisible pour comparer l'histoire sélectionnée avant l'envoi."
        : "Branche un appareil lisible pour comparer l'histoire sélectionnée avant l'envoi.";
  }
}

function formatUnchanged(count: number): string {
  if (count <= 0) return "Aucune autre histoire de l'appareil ne sera modifiée.";
  if (count === 1) return "1 autre histoire de l'appareil restera inchangée.";
  return `${count} autres histoires de l'appareil resteront inchangées.`;
}

function formatSelectionLabel(count: number): string {
  if (count <= 0) return "Aucune histoire sélectionnée";
  if (count === 1) return "1 histoire sélectionnée";
  return `${count} histoires sélectionnées`;
}

function formatEditReason(count: number): string {
  if (count <= 0) return "Reprise indisponible: aucune histoire sélectionnée";
  return "Reprise indisponible: sélection multiple";
}

function formatDeleteReason(): string {
  return "Suppression indisponible: aucune histoire sélectionnée";
}

/** Impact copy of the two-gesture confirmation — names the exact scope of
 *  the removal so the confirmation is informed, never a reflex click. */
function formatDeleteConfirmCopy(count: number): string {
  const scope =
    count === 1
      ? "Supprimer définitivement cette histoire de la bibliothèque ?"
      : `Supprimer définitivement ces ${count} histoires de la bibliothèque ?`;
  return `${scope} Les brouillons, médias et mémoires de transfert associés seront aussi supprimés.`;
}

/** Canonical FAMILY names (product-language.md). Distinct from the
 *  cohort labels (`formatSupportedLabel` in the route): the recognized
 *  chip is contractually `Appareil reconnu — {famille}`, never a
 *  cohort wording. */
function formatFamilyLabel(family: SupportedFamilyDto): string {
  switch (family) {
    case "lunii":
      return "Lunii";
    case "flam":
      return "FLAM";
  }
}

/** Family-correct send CTA label (product-language.md Change Control):
 *  a Lunii panel keeps `Envoyer vers la Lunii` VERBATIM; any other
 *  family reads the generic device wording. `undefined` (legacy callers
 *  without a family) keeps the historical Lunii copy — the same rule as
 *  the transfer capability line. */
function formatSendCtaLabel(family: SupportedFamilyDto | undefined): string {
  return family === undefined || family === "lunii"
    ? "Envoyer vers la Lunii"
    : "Envoyer vers l'appareil";
}

/** Family-correct heading of the `deviceProfile` blocker group: a Lunii
 *  panel keeps `Compatibilité Lunii` VERBATIM; any other family reads the
 *  generic device wording (product-language.md Change Control). */
function formatDeviceCompatibilityHeading(
  family: SupportedFamilyDto | undefined,
): string {
  return family === undefined || family === "lunii"
    ? "Compatibilité Lunii"
    : "Compatibilité appareil";
}

function formatDeviceChipLabel(
  state: LuniiDeviceState,
  deviceLabel: string | undefined,
  hasAnyCapability: boolean,
  deviceFamily: SupportedFamilyDto | undefined,
): string {
  switch (state) {
    case "absent":
      return "Aucun appareil connecté";
    case "idle": {
      // "Recognized ≠ ready" (general product rule, ui-states.md):
      // `Appareil prêt` REQUIRES at least one activated capability —
      // derived from the matrix, never from the family. A recognized
      // zero-capability profile renders the honest static state, and
      // its suffix is the FAMILY name (`Appareil reconnu — {famille}`,
      // product-language.md) — the cohort-flavored `deviceLabel` is
      // only the fallback when no family is supplied.
      if (!hasAnyCapability) {
        const familyLabel =
          deviceFamily !== undefined
            ? formatFamilyLabel(deviceFamily)
            : deviceLabel;
        return familyLabel
          ? `Appareil reconnu — ${familyLabel}`
          : "Appareil reconnu";
      }
      return deviceLabel ? `Appareil prêt — ${deviceLabel}` : "Appareil prêt";
    }
    case "unsupported":
      return "Profil non supporté";
    case "ambiguous":
      return "Profil ambigu";
    case "scanning":
      return "Détection en cours…";
    case "error":
      return "Détection indisponible";
  }
}

function formatDeviceChipTone(
  state: LuniiDeviceState,
): "neutral" | "info" | "warning" | "error" {
  switch (state) {
    case "idle":
      return "info";
    case "unsupported":
    case "ambiguous":
      return "warning";
    case "error":
      return "error";
    case "scanning":
    case "absent":
      return "neutral";
  }
}

function formatSupportedOperationLabels(
  ops: SupportedOperationsDto,
  family?: SupportedFamilyDto,
): string[] {
  // Stable, parent-friendly French copy mirroring the canonical
  // labels in docs/architecture/device-support-profile.md. The
  // transfer line is family-correct: only a Lunii line may read
  // "Transfert vers la Lunii" (product-language.md) — any other
  // family gets the generic device wording. `undefined` (legacy
  // callers without a family) keeps the historical Lunii copy.
  const transferLabel =
    family === undefined || family === "lunii"
      ? "Transfert vers la Lunii"
      : "Transfert vers l'appareil";
  const matrix: Array<[keyof SupportedOperationsDto, string]> = [
    ["readLibrary", "Lecture bibliothèque appareil"],
    ["inspectStory", "Inspection d'histoire"],
    ["importStory", "Copie dans la bibliothèque locale"],
    ["writeStory", transferLabel],
  ];
  return matrix.map(([k, label]) => `${ops[k] ? "✓" : "—"} ${label}`);
}

function formatSendReason(
  state: LuniiDeviceState,
  hasAnyCapability: boolean,
  deviceFamily: SupportedFamilyDto | undefined,
): string {
  switch (state) {
    case "absent":
      // Distinct from "appareil non supporté" so the user knows
      // whether to plug something in (absent) or check the profile
      // (unsupported). The canonical phrasing lives in
      // docs/architecture/ui-states.md.
      return "Envoi indisponible: aucun appareil connecté";
    case "idle":
      // The "MVP Phase 1" copy PROMISES a future transfer and stays
      // exclusive to write-planned Lunii cohorts: a zero-capability
      // profile AND any capability-bearing non-Lunii family (FLAM —
      // read capabilities active, write not planned) both follow the
      // EXISTING capability-closed path (the V3 pattern) instead.
      if (!hasAnyCapability) {
        return "Envoi indisponible: profil non supporté";
      }
      if (deviceFamily !== undefined && deviceFamily !== "lunii") {
        return "Envoi indisponible: profil non supporté";
      }
      // MVP Phase 1: even a supported device cannot accept a transfer
      // yet — Epic 3 wires the gate. Distinct copy from "appareil
      // non supporté" so the user sees a positive "supported device,
      // transfer not wired yet" message instead of a contradiction
      // with the `Appareil prêt — Lunii …` chip just above.
      return "Envoi indisponible: transfert pas encore activé (MVP Phase 1)";
    case "unsupported":
      return "Envoi indisponible: profil non supporté";
    case "ambiguous":
      return "Envoi indisponible: profil ambigu";
    case "scanning":
      return "Envoi indisponible: détection en cours";
    case "error":
      return "Envoi indisponible: détection en échec";
  }
}
