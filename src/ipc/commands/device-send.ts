import { Channel, invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  SendPackToDeviceInput,
  SendPackToDeviceOutcome,
} from "../../shared/ipc-contracts/device-send";
import {
  isSendPackToDeviceInput,
  isSendPackToDeviceOutcome,
} from "../../shared/ipc-contracts/device-send";

/**
 * Error thrown when `send_pack_to_device` resolves with a payload that does
 * not match the wire contract. The raw response is attached for debugging.
 */
export class SendPackToDeviceContractDriftError extends Error {
  readonly raw: unknown;
  constructor(raw: unknown) {
    super(
      "send_pack_to_device returned a payload that does not match the contract",
    );
    this.name = "SendPackToDeviceContractDriftError";
    this.raw = raw;
  }
}

/**
 * Send the SELECTED local story identified by `storyId` to the connected
 * device identified by `deviceIdentifier` — the V3 branch of the single
 * "Envoyer vers la Lunii" gesture. Rust owns the whole boundary: it resolves
 * the story's RETAINED source archive itself (no path crosses IPC, no
 * picker), re-scans, gates on the dedicated `send_archive` capability, then
 * transcodes + ciphers for the target device and writes atomically.
 *
 * The input is validated client-side BEFORE the round-trip (both identifiers
 * come from Rust DTOs, so a malformed input is a frontend bug). Transport
 * rejections are normalized through `toAppError`.
 *
 * `onProgress`, when given, receives the send's integer percent (0..99) as the
 * pack is normalized then written — a SIGNAL only, over a Tauri channel; the
 * settled outcome is the resolved value, never reconstructed from progress.
 * A malformed or out-of-range tick is clamped/ignored, never surfaced.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 */
export async function sendPackToDevice(
  input: SendPackToDeviceInput,
  onProgress?: (percent: number) => void,
): Promise<SendPackToDeviceOutcome> {
  if (!isSendPackToDeviceInput(input)) {
    throw new TypeError(
      "send_pack_to_device input rejected client-side: deviceIdentifier must be 32 lowercase hex chars and storyId a canonical lowercase UUID",
    );
  }
  const channel = new Channel<number>();
  if (onProgress) {
    channel.onmessage = (percent) => {
      if (typeof percent === "number" && Number.isFinite(percent)) {
        onProgress(Math.max(0, Math.min(99, Math.round(percent))));
      }
    };
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("send_pack_to_device", {
      input,
      onProgress: channel,
    });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isSendPackToDeviceOutcome(raw)) {
    throw new SendPackToDeviceContractDriftError(raw);
  }
  return raw;
}
