import { invoke } from "@tauri-apps/api/core";

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
 * Send a STUdio-format pack archive (`.zip`) to the connected device
 * identified by `deviceIdentifier` ("Envoyer un pack (.zip)"). Rust owns the
 * whole boundary: the NATIVE archive picker (no path ever crosses IPC), the
 * authoritative re-scan, the dedicated `send_archive` capability gate, the
 * transcode + per-device ciphering and the atomic on-volume write. A
 * dismissed picker resolves `{ kind: "cancelled" }` — a non-event.
 *
 * The input is validated client-side BEFORE the round-trip (the identifier
 * comes from a Rust DTO, so a malformed input is a frontend bug). Transport
 * rejections are normalized through `toAppError`.
 *
 * Components MUST NOT call `invoke` directly — go through this facade so the
 * wire contract stays owned by `src/ipc/`.
 */
export async function sendPackToDevice(
  input: SendPackToDeviceInput,
): Promise<SendPackToDeviceOutcome> {
  if (!isSendPackToDeviceInput(input)) {
    throw new TypeError(
      "send_pack_to_device input rejected client-side: deviceIdentifier must be 32 lowercase hex chars",
    );
  }
  let raw: unknown;
  try {
    raw = await invoke<unknown>("send_pack_to_device", { input });
  } catch (err) {
    throw toAppError(err);
  }
  if (!isSendPackToDeviceOutcome(raw)) {
    throw new SendPackToDeviceContractDriftError(raw);
  }
  return raw;
}
