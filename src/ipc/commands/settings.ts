import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  StartUpdateApplyOutcome,
  SupportProfile,
  UpdateApplyPlan,
  UpdateApplyState,
  UpdateAvailability,
} from "../../shared/ipc-contracts/settings";
import {
  isStartUpdateApplyOutcome,
  isSupportProfile,
  isUpdateApplyPlan,
  isUpdateApplyState,
  isUpdateAvailability,
} from "../../shared/ipc-contracts/settings";

/**
 * Error thrown when `read_support_profile` returns a payload that does
 * not match the wire contract. A payload outside the contract NEVER
 * renders a screen — the raw response is attached for debugging.
 */
export class SupportProfileContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "read_support_profile returned a payload that does not match the contract",
    );
    this.name = "SupportProfileContractDriftError";
    this.raw = raw;
  }
}

/**
 * Read the official support profile: the device support matrix and the
 * local-artifact registry of this distribution, with their frozen
 * labels and per-limit reasons (`Support Profile Screen Contract`). A
 * PURE read on the Rust side — zero network, zero DB, zero lock.
 *
 * The caller treats ANY failure (rejection, drifted payload) as a
 * failed profile read and renders the affected sections in the calm
 * `unavailable` state — fail-closed per section, never invented
 * content, never blocking the sections whose read succeeded.
 *
 * Components MUST NOT call `invoke` directly — go through this facade
 * so the wire contract stays owned by `src/ipc/`.
 */
export async function readSupportProfile(): Promise<SupportProfile> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("read_support_profile");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isSupportProfile(raw)) {
    throw new SupportProfileContractDriftError(raw);
  }
  return raw;
}

/**
 * Error thrown when `read_update_availability` returns a payload that
 * does not match the wire contract. A payload outside the contract
 * NEVER renders a surface — the raw response is attached for debugging.
 */
export class UpdateAvailabilityContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "read_update_availability returned a payload that does not match the contract",
    );
    this.name = "UpdateAvailabilityContractDriftError";
    this.raw = raw;
  }
}

/**
 * Read THE launch's update-availability verdict (`Update Availability
 * Contract`). The command is INFAILLIBLE by contract: a transport
 * failure arrives as the calm `checkUnavailable` STATE of the payload,
 * never a rejection — the only rejections here are an IPC failure
 * (normalized) and a contract drift. The caller treats ANY rejection as
 * "no verdict exists": total silence, the app lives without it.
 *
 * Components MUST NOT call `invoke` directly — go through this facade
 * so the wire contract stays owned by `src/ipc/`.
 */
export async function readUpdateAvailability(): Promise<UpdateAvailability> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("read_update_availability");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isUpdateAvailability(raw)) {
    throw new UpdateAvailabilityContractDriftError(raw);
  }
  return raw;
}

/**
 * Error thrown when an update-apply command returns a payload that does
 * not match the wire contract. A payload outside the contract NEVER
 * renders the gesture zone — the raw response is attached for
 * debugging.
 */
export class UpdateApplyContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "UpdateApplyContractDriftError";
    this.raw = raw;
  }
}

/**
 * Read THIS copy's update-apply plan (`Update Apply Contract`): whether
 * the integrated gesture exists here, or the manual guidance couple.
 * INFALLIBLE Rust-side (a manual plan is a state, never an error) — the
 * only rejections here are an IPC failure (normalized) and a contract
 * drift; the zone treats ANY rejection as "no plan" and renders
 * nothing.
 *
 * Components MUST NOT call `invoke` directly — go through this facade
 * so the wire contract stays owned by `src/ipc/`.
 */
export async function readUpdateApplyPlan(): Promise<UpdateApplyPlan> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("read_update_apply_plan");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isUpdateApplyPlan(raw)) {
    throw new UpdateApplyContractDriftError("read_update_apply_plan", raw);
  }
  return raw;
}

/**
 * Read the gesture's SESSION state (`Update Apply Contract`) — the
 * authoritative re-read the zone always trusts over events (events are
 * a comfort, never the truth).
 */
export async function readUpdateApplyState(): Promise<UpdateApplyState> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("read_update_apply_state");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isUpdateApplyState(raw)) {
    throw new UpdateApplyContractDriftError("read_update_apply_state", raw);
  }
  return raw;
}

/**
 * Start ONE update-apply gesture. The refusal outcomes
 * (`alreadyRunning`, `notEligible`) are STATES of the payload — Rust
 * re-decides the plan fail-closed whatever this frontend believed.
 */
export async function startUpdateApply(): Promise<StartUpdateApplyOutcome> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("start_update_apply");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isStartUpdateApplyOutcome(raw)) {
    throw new UpdateApplyContractDriftError("start_update_apply", raw);
  }
  return raw;
}

/**
 * Restart Rustory to finish an applied update. Guarded RUST-SIDE: a
 * silent no-op unless the session state is ready-to-restart — on the
 * ready state the process restarts and this promise never settles.
 */
export async function restartForUpdate(): Promise<void> {
  try {
    await invoke<unknown>("restart_for_update");
  } catch (err) {
    throw toAppError(err);
  }
}
