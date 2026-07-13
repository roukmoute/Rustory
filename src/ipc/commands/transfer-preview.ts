import { invoke } from "@tauri-apps/api/core";

import type { SupportedFamilyDto } from "../../shared/ipc-contracts/device";

import { toAppError } from "../../shared/errors/app-error";
import {
  isTransferPreviewDto,
  type TransferPreviewDto,
} from "../../shared/ipc-contracts/transfer-preview";

/** Input accepted by {@link readTransferPreview}. Mirror of
 *  `ReadTransferPreviewInputDto` — exactly the two identifiers the UI holds. */
export interface ReadTransferPreviewInput {
  storyId: string;
  deviceIdentifier: string;
}

/**
 * Upper bound for a transfer-preview read. Sized to the Rust budget (5 s,
 * which itself covers the authoritative re-scan plus the inventory read) plus
 * a 500 ms safety margin so the timer never fires before Rust has had a
 * chance to return.
 */
export const READ_TRANSFER_PREVIEW_TIMEOUT_MS = 5500;

/** Discriminant emitted when {@link readTransferPreview} trips its timeout.
 *  Family-correct: a Lunii keeps the historical next gesture VERBATIM,
 *  any other family reads the device-generic one (product-language.md). */
export function readTransferPreviewTimeoutError(
  family?: SupportedFamilyDto,
): {
  code: "UNKNOWN";
  message: string;
  userAction: string;
  details: null;
} {
  return {
    code: "UNKNOWN",
    message: "Rustory a mis trop de temps à comparer l'histoire avec l'appareil.",
    userAction:
      family === undefined || family === "lunii"
        ? "Réessaie la comparaison. Si le problème persiste, débranche la Lunii puis rebranche-la."
        : "Réessaie la comparaison. Si le problème persiste, débranche l'appareil puis rebranche-le.",
    details: null,
  };
}

/**
 * Cancelable handle returned by {@link readTransferPreview}. Callers that
 * unmount before the IPC settles MUST call `cancel()` so the timer guard is
 * cleared.
 */
export interface ReadTransferPreviewCall {
  promise: Promise<TransferPreviewDto>;
  cancel: () => void;
}

/**
 * Thrown when `read_transfer_preview` returns a payload that does not match
 * the canonical wire shape. The captured `raw` value is kept on the error for
 * support — never surfaced verbatim to the user.
 */
export class ReadTransferPreviewContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ReadTransferPreviewContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Compose the read-only pre-transfer comparison for the selected local story
 * against the connected supported Lunii. Resolves with a `TransferPreviewDto`
 * (tagged enum on `kind`); rejects with a normalized `AppError` on a read-side
 * failure; rejects with the {@link ReadTransferPreviewContractDriftError} on a
 * drifted wire shape; rejects with a synthetic `UNKNOWN`-coded error on timeout.
 *
 * Components MUST NOT call `invoke` directly — go through this façade so the
 * wire contract stays owned by `src/ipc/`.
 */
export function readTransferPreview(
  input: ReadTransferPreviewInput,
  timeoutMs: number = READ_TRANSFER_PREVIEW_TIMEOUT_MS,
  deviceFamily?: SupportedFamilyDto,
): ReadTransferPreviewCall {
  const call = invoke<unknown>("read_transfer_preview", { input })
    .then((raw) => {
      if (!isTransferPreviewDto(raw)) {
        throw new ReadTransferPreviewContractDriftError(
          "TransferPreviewDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      // A drift is its own typed error; everything else is normalized to the
      // stable AppError shape so callers observe one shape.
      if (err instanceof ReadTransferPreviewContractDriftError) throw err;
      throw toAppError(err);
    });

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;

  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timer = undefined;
      if (cancelled) return;
      reject(readTransferPreviewTimeoutError(deviceFamily));
    }, timeoutMs);
  });

  const promise = Promise.race([call, guard]).finally(() => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  });

  const cancel = (): void => {
    cancelled = true;
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  };

  return { promise, cancel };
}
