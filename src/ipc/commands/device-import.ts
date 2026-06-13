import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  ImportDeviceStoryInput,
  ImportDeviceStoryOutcome,
} from "../../shared/ipc-contracts/device-import";
import {
  isImportDeviceStoryInput,
  isImportDeviceStoryOutcome,
} from "../../shared/ipc-contracts/device-import";

/**
 * Error thrown when `import_device_story` resolves with a payload that
 * does not match the wire contract. The raw response is attached to
 * `raw` so production debugging surfaces the shape that drifted.
 */
export class ImportDeviceStoryContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "import_device_story returned a payload that does not match the contract",
    );
    this.name = "ImportDeviceStoryContractDriftError";
    this.raw = raw;
  }
}

/**
 * Copy the device story identified by `packUuid` from the connected
 * supported Lunii identified by `deviceIdentifier` into the local
 * library. Rust owns the whole boundary: authoritative re-scan,
 * capability gate, bounded copy, atomic promotion, canonical commit —
 * the frontend never sees a path or a partial state.
 *
 * Deliberately NO frontend timeout: Rust owns the wall-clock bound
 * (300 s — a pack can weigh hundreds of MB on a slow USB bus), exactly
 * like the export flow. Failures reject with a normalized `AppError`
 * (`IMPORT_FAILED` + closed `details.source` taxonomy).
 *
 * The input is validated client-side BEFORE the round-trip (both values
 * come from Rust DTOs, so a malformed input is a frontend bug — refused
 * loudly, mirroring the strict Rust boundary). Transport rejections are
 * normalized through `toAppError` so callers always observe the stable
 * `AppError` shape; an already-normalized Rust `AppError` passes through
 * verbatim.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export async function importDeviceStory(
  input: ImportDeviceStoryInput,
): Promise<ImportDeviceStoryOutcome> {
  if (!isImportDeviceStoryInput(input)) {
    throw new TypeError(
      "import_device_story input rejected client-side: deviceIdentifier must be 32 lowercase hex chars and packUuid a canonical lowercase UUID",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("import_device_story", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isImportDeviceStoryOutcome(raw)) {
    throw new ImportDeviceStoryContractDriftError(raw);
  }
  return raw;
}
