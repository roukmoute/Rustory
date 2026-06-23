import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import {
  isStartTransferAcceptedDto,
  isTransferOutcomeDto,
  isTransferStateDto,
  type StartTransferAcceptedDto,
  type TransferOutcomeDto,
  type TransferStateDto,
} from "../../shared/ipc-contracts/story-transfer";

/** Input accepted by {@link startTransferStory}. Mirror of
 *  `StartTransferStoryInputDto` — the two identifiers the UI holds. */
export interface StartTransferStoryInput {
  storyId: string;
  deviceIdentifier: string;
}

/** Input accepted by {@link readTransferState}. Mirror of
 *  `ReadTransferStateInputDto` — the local story id AND the TARGETED device, so
 *  the authoritative re-read is pinned to the Lunii the transfer aimed at: it
 *  must prove the pack is present on THAT device, never on any other writable
 *  device that happens to be connected at the terminal (AC3 — no false success,
 *  no terminal attributed to the wrong device). */
export interface ReadTransferStateInput {
  storyId: string;
  deviceIdentifier: string;
}

/** Input accepted by {@link readTransferOutcome} / {@link discardTransferOutcome}.
 *  Just the story whose durable transfer memory to re-hydrate / purge. */
export interface TransferOutcomeStoryInput {
  storyId: string;
}

/**
 * Thrown when a transfer command returns a payload that does not match the
 * canonical wire shape. The captured `raw` value is kept for support — never
 * surfaced verbatim to the user.
 */
export class TransferContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "TransferContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Start transferring (WRITING) the prepared LOCAL story to the connected device.
 * Resolves with the acceptance (the `jobId` to correlate `job:*` events); the
 * write continues in the background. Rejects with the
 * {@link TransferContractDriftError} on a drifted shape, or a normalized
 * `AppError` otherwise.
 *
 * Components MUST NOT call `invoke` directly — go through this façade so the wire
 * contract stays owned by `src/ipc/`.
 */
export function startTransferStory(
  input: StartTransferStoryInput,
): Promise<StartTransferAcceptedDto> {
  return invoke<unknown>("start_transfer_story", { input })
    .then((raw) => {
      if (!isStartTransferAcceptedDto(raw)) {
        throw new TransferContractDriftError(
          "StartTransferAcceptedDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      if (err instanceof TransferContractDriftError) throw err;
      throw toAppError(err);
    });
}

/**
 * Authoritative re-read of the transfer state. The deadline is owned by Rust, so
 * this façade sets no frontend timer; the hook supersedes a stale read through
 * its own active-call guard. Rejects with the {@link TransferContractDriftError}
 * on a drifted shape, or a normalized `AppError` otherwise.
 */
export function readTransferState(
  input: ReadTransferStateInput,
): Promise<TransferStateDto> {
  return invoke<unknown>("read_transfer_state", { input })
    .then((raw) => {
      if (!isTransferStateDto(raw)) {
        throw new TransferContractDriftError(
          "TransferStateDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      if (err instanceof TransferContractDriftError) throw err;
      throw toAppError(err);
    });
}

/**
 * Re-hydrate the durable transfer outcome remembered for a story (the Transfer
 * Resume Contract). Resolves with the outcome, or `null` when there is no memory.
 * Best-effort by contract: the hook treats a rejection as "no memory" so a
 * persistence-read failure never blocks the panel. Rejects with the
 * {@link TransferContractDriftError} on a drifted shape, or a normalized `AppError`.
 */
export function readTransferOutcome(
  input: TransferOutcomeStoryInput,
): Promise<TransferOutcomeDto | null> {
  return invoke<unknown>("read_transfer_outcome", { input })
    .then((raw) => {
      if (raw === null) return null;
      if (!isTransferOutcomeDto(raw)) {
        throw new TransferContractDriftError(
          "TransferOutcomeDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      if (err instanceof TransferContractDriftError) throw err;
      throw toAppError(err);
    });
}

/**
 * Purge the durable transfer outcome for a story (the `Abandonner` gesture).
 * Idempotent; never touches canonical state. Rejects with a normalized `AppError`
 * (a purge failure IS surfaced, unlike the best-effort read).
 */
export function discardTransferOutcome(
  input: TransferOutcomeStoryInput,
): Promise<void> {
  return invoke<unknown>("discard_transfer_outcome", { input })
    .then(() => undefined)
    .catch((err) => {
      throw toAppError(err);
    });
}
