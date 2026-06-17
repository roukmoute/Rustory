import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  DeviceStoryTitleDto,
  SetDeviceStoryTitleInput,
} from "../../shared/ipc-contracts/device-title";
import {
  isDeviceStoryTitleDto,
  isSetDeviceStoryTitleInput,
} from "../../shared/ipc-contracts/device-title";

/**
 * Error thrown when `set_device_story_title` resolves with a payload that
 * does not match the wire contract. The raw response is attached for
 * production debugging — never surfaced verbatim to the user.
 */
export class SetDeviceStoryTitleContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "set_device_story_title returned a payload that does not match the contract",
    );
    this.name = "SetDeviceStoryTitleContractDriftError";
    this.raw = raw;
  }
}

/**
 * Name (or rename) a device story that no catalog recognizes. A single
 * bounded SQLite write, no device I/O — so, like `create_story`, there is
 * NO frontend timeout. The `packUuid` is validated client-side before the
 * round-trip (it is a Rust-issued identifier; a malformed value is a
 * frontend bug). Title validation is Rust-authoritative: a too-long or
 * control-char title rejects with the canonical `INVALID_STORY_TITLE`
 * `AppError`, normalized here through `toAppError`.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so
 * the wire contract stays owned by `src/ipc/`.
 */
export async function setDeviceStoryTitle(
  input: SetDeviceStoryTitleInput,
): Promise<DeviceStoryTitleDto> {
  if (!isSetDeviceStoryTitleInput(input)) {
    throw new TypeError(
      "set_device_story_title input rejected client-side: packUuid must be a canonical lowercase UUID and title a non-blank string",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("set_device_story_title", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isDeviceStoryTitleDto(raw)) {
    throw new SetDeviceStoryTitleContractDriftError(raw);
  }
  return raw;
}
