import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type { SupportProfile } from "../../shared/ipc-contracts/settings";
import { isSupportProfile } from "../../shared/ipc-contracts/settings";

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
