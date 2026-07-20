import type React from "react";
import { useEffect, useRef } from "react";

import {
  readUpdateApplyPlan,
  readUpdateApplyState,
  restartForUpdate,
  startUpdateApply,
} from "../../../ipc/commands/settings";
import { subscribeUpdateApplyEvents } from "../../../ipc/events/update-apply-events";
import type { UpdateApplyState } from "../../../shared/ipc-contracts/settings";
import { Button, ProgressIndicator } from "../../../shared/ui";
import { useUpdateApplyShell } from "../../../shell/state/update-apply-shell-store";
import { useUpdateShell } from "../../../shell/state/update-shell-store";

import "./UpdateApplyZone.css";

// The ONLY frontend-frozen literals of the gesture zone
// (product-language.md): the four gestures and their accessible names —
// every other copy renders Rust-carried, verbatim.
const START_LABEL = "Mettre à jour Rustory";
const START_ARIA_LABEL = "Télécharger et installer la mise à jour de Rustory";
const RESTART_LABEL = "Redémarrer maintenant";
const RESTART_ARIA_LABEL = "Redémarrer Rustory pour terminer la mise à jour";
const DEFER_LABEL = "Plus tard";
const RETRY_LABEL = "Réessayer la mise à jour";

/**
 * The `/settings` gesture zone of the update-apply contract
 * (`ui-states.md#Update Apply Contract`): exists IFF the session's
 * availability verdict is `updateAvailable`, rendered UNDER the status
 * line. A manual plan is ONE calm `role="status"` block (no button); the
 * integrated plan walks idle → running → readyToRestart / failed on the
 * session state, always re-read authoritatively (events are a comfort).
 * Mounting NEVER starts a download — every start is a user click. The
 * whole app stays usable while the gesture runs: no tunnel, no overlay,
 * no alert, no toast, no modal, no external link.
 */
export function UpdateApplyZone(): React.JSX.Element | null {
  const availability = useUpdateShell((s) => s.availability);
  const plan = useUpdateApplyShell((s) => s.plan);
  const applyState = useUpdateApplyShell((s) => s.state);
  const restartInviteFolded = useUpdateApplyShell((s) => s.restartInviteFolded);
  const setRestartInviteFolded = useUpdateApplyShell(
    (s) => s.setRestartInviteFolded,
  );

  const positive = availability !== null && availability.status === "updateAvailable";

  // Mount token: only the reads issued by the LATEST mount may apply
  // their result (the settings route's `readTokenRef` pattern,
  // StrictMode-safe — the cleanup invalidates the aborted first pass).
  const readTokenRef = useRef(0);
  // Re-read sequence: concurrent catch-ups of the SAME mount are
  // numbered, and only the MOST RECENTLY ISSUED one may apply — a stale
  // `running` answer landing after a terminal answer must never regress
  // the rendered state.
  const readSeqRef = useRef(0);
  const unsubscribeRef = useRef<(() => void) | null>(null);
  // The job the live event subscription is attached to — reconciliation
  // re-attaches when the AUTHORITATIVE state names a different flight.
  const subscribedJobRef = useRef<string | null>(null);

  // Store writes go through getState(): the imperative flows below
  // (start, events, catch-up re-reads) must not close over stale setters.
  const pourState = (state: UpdateApplyState): void => {
    useUpdateApplyShell.getState().setState(state);
  };

  const teardownSubscription = (): void => {
    subscribedJobRef.current = null;
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
  };

  // Reconcile the event subscription with the AUTHORITATIVE state: a
  // live flight names its correlation id on the wire, so a frontend that
  // lost its tracked id (renderer reload, unmounted start resolution)
  // re-attaches from the re-read alone; a non-running state confirms the
  // terminal — only THEN is the tracked id released (never before).
  const reconcileSubscription = (state: UpdateApplyState, token: number): void => {
    const store = useUpdateApplyShell.getState();
    if (state.status === "running" && state.jobId !== undefined) {
      if (store.jobId !== state.jobId) {
        store.setJobId(state.jobId);
      }
      if (subscribedJobRef.current !== state.jobId) {
        subscribeTo(state.jobId, token);
      }
    } else {
      if (store.jobId !== null) {
        store.setJobId(null);
      }
      teardownSubscription();
    }
  };

  const catchUpPlan = (token: number): void => {
    void readUpdateApplyPlan().then(
      (readPlan) => {
        if (readTokenRef.current === token) {
          useUpdateApplyShell.getState().setPlan(readPlan);
        }
      },
      () => {
        // Drift: no plan, no zone — calm silence.
      },
    );
  };

  const catchUpState = (token: number): void => {
    const seq = ++readSeqRef.current;
    void readUpdateApplyState().then(
      (state) => {
        if (readTokenRef.current !== token) {
          return;
        }
        if (readSeqRef.current !== seq) {
          // A newer re-read was issued meanwhile: this answer is stale —
          // applying it could regress a terminal back to `running`.
          return;
        }
        pourState(state);
        reconcileSubscription(state, token);
      },
      () => {
        // Drift: the zone keeps its last coherent state — calm silence.
      },
    );
  };

  const subscribeTo = (jobId: string, token: number): void => {
    unsubscribeRef.current?.();
    subscribedJobRef.current = jobId;
    const handle = subscribeUpdateApplyEvents({
      jobId,
      onProgress: (event) => {
        const current = useUpdateApplyShell.getState().state;
        if (
          current !== null &&
          current.status === "running" &&
          current.phase === event.phase
        ) {
          // Same phase: only the integer percent moved — patch it, the
          // Rust-carried couple of the phase stays valid.
          if (event.percent !== undefined) {
            pourState({ ...current, percent: event.percent });
          }
        } else {
          // Phase transition: re-read the authoritative state (it
          // carries the new phase's copies).
          catchUpState(token);
        }
      },
      onCompleted: () => {
        // The re-read confirms the terminal — and only that
        // confirmation releases the tracked job id (reconciliation).
        catchUpState(token);
      },
      onFailed: (event) => {
        // The failure terminal carries its frozen couple — render it
        // immediately, then reconcile authoritatively.
        pourState({
          status: "failed",
          stage: event.stage,
          headline: event.headline,
          notice: event.notice,
        });
        catchUpState(token);
      },
    });
    unsubscribeRef.current = handle.unsubscribe;
    // The authoritative catch-up runs AFTER the three listeners are
    // guaranteed installed: reading earlier could observe a pre-terminal
    // state whose terminal event then fires unheard and is lost. A
    // failed registration still resolves `ready` — the re-read IS the
    // fallback.
    void handle.ready.then(() => {
      if (readTokenRef.current === token) {
        catchUpState(token);
      }
    });
  };

  useEffect(() => {
    if (!positive) {
      return;
    }
    readTokenRef.current += 1;
    const token = readTokenRef.current;
    catchUpPlan(token);
    // The initial authoritative read reconciles EVERYTHING: a live
    // flight re-attaches through its wire correlation id (renderer
    // reload included), a terminal renders as such — no local memory
    // is needed to recover.
    catchUpState(token);
    return () => {
      readTokenRef.current += 1;
      teardownSubscription();
    };
    // `positive` is the only reactive input: the reads and the
    // re-subscription are imperative one-shots of the mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [positive]);

  if (!positive || plan === null) {
    return null;
  }

  // The manual guidance is the PLAN's face alone (`ui-states.md`: ONE
  // calm status block, unconditional once the zone exists) — it renders
  // even while the STATE read failed or has not landed yet.
  if (plan.mode === "manual") {
    return (
      <div className="update-apply-zone" role="status">
        <p className="update-apply-zone__headline">{plan.headline}</p>
        <p className="update-apply-zone__notice">{plan.guidance}</p>
      </div>
    );
  }

  if (applyState === null) {
    return null;
  }

  const handleStart = (): void => {
    const token = readTokenRef.current;
    void startUpdateApply().then(
      (outcome) => {
        if (readTokenRef.current !== token) {
          return;
        }
        if (outcome.outcome === "started" && outcome.jobId !== undefined) {
          useUpdateApplyShell.getState().setRestartInviteFolded(false);
          useUpdateApplyShell.getState().setJobId(outcome.jobId);
          // The subscription runs its own authoritative catch-up AFTER
          // its listeners are installed — the start/events race is
          // covered there, never by a read racing the registration.
          subscribeTo(outcome.jobId, token);
        } else {
          // Refused (alreadyRunning / notEligible): the authoritative
          // re-reads reconcile — a live flight re-attaches through its
          // wire correlation id, and the RE-READ plan re-renders a copy
          // whose eligibility changed (the stale CTA never survives).
          catchUpPlan(token);
          catchUpState(token);
        }
      },
      () => {
        // Drift: the zone keeps its last coherent state.
      },
    );
  };

  switch (applyState.status) {
    case "idle":
      return (
        <div className="update-apply-zone">
          <Button
            variant="primary"
            aria-label={START_ARIA_LABEL}
            onClick={handleStart}
          >
            {START_LABEL}
          </Button>
        </div>
      );
    case "running":
      return (
        <div className="update-apply-zone">
          <ProgressIndicator
            mode={applyState.percent === undefined ? "indeterminate" : "determinate"}
            label={applyState.headline ?? ""}
            value={applyState.percent}
          />
          <p className="update-apply-zone__notice">{applyState.notice}</p>
        </div>
      );
    case "readyToRestart":
      if (restartInviteFolded) {
        // `Plus tard` folded the invite: the state stays rendered as a
        // sober consultable line, the restart stays reachable — never a
        // re-proposed prompt.
        return (
          <div className="update-apply-zone update-apply-zone--folded" role="status">
            <span className="update-apply-zone__headline">
              {applyState.headline}
            </span>
            <Button
              variant="quiet"
              aria-label={RESTART_ARIA_LABEL}
              onClick={() => {
                void restartForUpdate().then(
                  () => {},
                  () => {},
                );
              }}
            >
              {RESTART_LABEL}
            </Button>
          </div>
        );
      }
      return (
        <div className="update-apply-zone" role="status">
          <p className="update-apply-zone__headline">{applyState.headline}</p>
          <p className="update-apply-zone__notice">{applyState.notice}</p>
          <div className="update-apply-zone__actions">
            <Button
              variant="primary"
              aria-label={RESTART_ARIA_LABEL}
              onClick={() => {
                void restartForUpdate().then(
                  () => {},
                  () => {},
                );
              }}
            >
              {RESTART_LABEL}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setRestartInviteFolded(true);
              }}
            >
              {DEFER_LABEL}
            </Button>
          </div>
        </div>
      );
    case "failed":
      return (
        <div className="update-apply-zone" role="status">
          <p className="update-apply-zone__headline">{applyState.headline}</p>
          <p className="update-apply-zone__notice">{applyState.notice}</p>
          <div className="update-apply-zone__actions">
            <Button variant="secondary" onClick={handleStart}>
              {RETRY_LABEL}
            </Button>
          </div>
        </div>
      );
    default:
      return null;
  }
}
