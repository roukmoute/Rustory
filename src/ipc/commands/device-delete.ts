import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  DeleteDeviceStoryInput,
  DeleteDeviceStoryOutcome,
} from "../../shared/ipc-contracts/device-delete";
import {
  isDeleteDeviceStoryInput,
  isDeleteDeviceStoryOutcome,
} from "../../shared/ipc-contracts/device-delete";

/**
 * Error thrown when `delete_device_story` resolves with a payload that does
 * not match the wire contract. The raw response is attached for debugging.
 */
export class DeleteDeviceStoryContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "delete_device_story returned a payload that does not match the contract",
    );
    this.name = "DeleteDeviceStoryContractDriftError";
    this.raw = raw;
  }
}

/**
 * Delete the device story identified by `packUuid` from the connected device
 * identified by `deviceIdentifier` ("Supprimer de l'appareil"). Rust owns the
 * whole boundary: authoritative re-scan, `delete_story` capability gate,
 * atomic `.pi` delist then content removal. The frontend never sees a path.
 *
 * The input is validated client-side BEFORE the round-trip (both values come
 * from Rust DTOs, so a malformed input is a frontend bug). Transport
 * rejections are normalized through `toAppError`.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 */
export async function deleteDeviceStory(
  input: DeleteDeviceStoryInput,
): Promise<DeleteDeviceStoryOutcome> {
  if (!isDeleteDeviceStoryInput(input)) {
    throw new TypeError(
      "delete_device_story input rejected client-side: deviceIdentifier must be 32 lowercase hex chars and packUuid a canonical lowercase UUID",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("delete_device_story", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isDeleteDeviceStoryOutcome(raw)) {
    throw new DeleteDeviceStoryContractDriftError(raw);
  }
  return raw;
}
