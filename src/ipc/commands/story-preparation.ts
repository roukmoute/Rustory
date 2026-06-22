import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import {
  isPreparationStateDto,
  isStartPreparationAcceptedDto,
  type PreparationStateDto,
  type StartPreparationAcceptedDto,
} from "../../shared/ipc-contracts/story-preparation";

/** Input accepted by {@link startPrepareStory}. Mirror of
 *  `StartPrepareStoryInputDto` — the two identifiers the UI holds. */
export interface StartPrepareStoryInput {
  storyId: string;
  deviceIdentifier: string;
}

/** Input accepted by {@link readPreparationState}. Mirror of
 *  `ReadPreparationStateInputDto` — only the local story id. */
export interface ReadPreparationStateInput {
  storyId: string;
}

/**
 * Thrown when a preparation command returns a payload that does not match the
 * canonical wire shape. The captured `raw` value is kept for support — never
 * surfaced verbatim to the user.
 */
export class PreparationContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "PreparationContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Start preparing the LOCAL story for the connected device. Resolves with the
 * acceptance (the `jobId` to correlate `job:*` events); the work continues in
 * the background. Rejects with the {@link PreparationContractDriftError} on a
 * drifted shape, or a normalized `AppError` otherwise.
 *
 * Components MUST NOT call `invoke` directly — go through this façade so the wire
 * contract stays owned by `src/ipc/`.
 */
export function startPrepareStory(
  input: StartPrepareStoryInput,
): Promise<StartPreparationAcceptedDto> {
  return invoke<unknown>("start_prepare_story", { input })
    .then((raw) => {
      if (!isStartPreparationAcceptedDto(raw)) {
        throw new PreparationContractDriftError(
          "StartPreparationAcceptedDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      if (err instanceof PreparationContractDriftError) throw err;
      throw toAppError(err);
    });
}

/**
 * Authoritative re-read of the preparation state. The deadline is owned by Rust
 * (the preflight + assembly budgets), so this façade sets no frontend timer; the
 * hook supersedes a stale read through its own active-call guard. Rejects with
 * the {@link PreparationContractDriftError} on a drifted shape, or a normalized
 * `AppError` otherwise.
 */
export function readPreparationState(
  input: ReadPreparationStateInput,
): Promise<PreparationStateDto> {
  return invoke<unknown>("read_preparation_state", { input })
    .then((raw) => {
      if (!isPreparationStateDto(raw)) {
        throw new PreparationContractDriftError(
          "PreparationStateDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      if (err instanceof PreparationContractDriftError) throw err;
      throw toAppError(err);
    });
}
