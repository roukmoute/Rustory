import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  ReorderDeviceStoriesInput,
  ReorderDeviceStoriesOutcome,
} from "../../shared/ipc-contracts/device-reorder";
import {
  isReorderDeviceStoriesInput,
  isReorderDeviceStoriesOutcome,
} from "../../shared/ipc-contracts/device-reorder";

/** Thrown when `reorder_device_stories` answers off-contract. */
export class ReorderDeviceStoriesContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "reorder_device_stories returned a payload that does not match the contract",
    );
    this.name = "ReorderDeviceStoriesContractDriftError";
    this.raw = raw;
  }
}

/**
 * Rewrite the connected device's story order (its wheel order) to
 * `orderedPackUuids` — the COMPLETE list of its visible packs. Rust owns
 * the boundary: authoritative re-scan, `reorder_stories` capability gate,
 * strict permutation check, atomic `.pi` rewrite. A stale list (the device
 * changed since it was read) is refused with a re-read hint.
 */
export async function reorderDeviceStories(
  input: ReorderDeviceStoriesInput,
): Promise<ReorderDeviceStoriesOutcome> {
  if (!isReorderDeviceStoriesInput(input)) {
    throw new TypeError(
      "reorder_device_stories input rejected client-side: deviceIdentifier must be 32 lowercase hex chars and orderedPackUuids distinct canonical lowercase UUIDs",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("reorder_device_stories", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isReorderDeviceStoriesOutcome(raw)) {
    throw new ReorderDeviceStoriesContractDriftError(raw);
  }
  return raw;
}
