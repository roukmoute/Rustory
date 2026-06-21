import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import {
  isStoryValidationDto,
  type StoryValidationDto,
} from "../../shared/ipc-contracts/story-validation";

/** Input accepted by {@link readStoryValidation}. Mirror of
 *  `ReadStoryValidationInputDto` — exactly the two identifiers the UI holds. */
export interface ReadStoryValidationInput {
  storyId: string;
  deviceIdentifier: string;
}

/**
 * Upper bound for a story-validation read. Sized to the Rust budget (5 s, which
 * itself covers the authoritative re-scan plus the inventory read) plus a 500 ms
 * safety margin so the timer never fires before Rust has had a chance to return.
 */
export const READ_STORY_VALIDATION_TIMEOUT_MS = 5500;

/** Discriminant emitted when {@link readStoryValidation} trips its timeout. */
export const READ_STORY_VALIDATION_TIMEOUT_ERROR = {
  code: "UNKNOWN",
  message: "Rustory a mis trop de temps à valider l'histoire avec l'appareil.",
  userAction:
    "Réessaie la validation. Si le problème persiste, débranche la Lunii puis rebranche-la.",
  details: null,
} as const;

/**
 * Cancelable handle returned by {@link readStoryValidation}. Callers that
 * unmount before the IPC settles MUST call `cancel()` so the timer guard is
 * cleared.
 */
export interface ReadStoryValidationCall {
  promise: Promise<StoryValidationDto>;
  cancel: () => void;
}

/**
 * Thrown when `read_story_validation` returns a payload that does not match the
 * canonical wire shape. The captured `raw` value is kept on the error for
 * support — never surfaced verbatim to the user.
 */
export class ReadStoryValidationContractDriftError extends Error {
  public readonly raw: unknown;
  constructor(message: string, options: { raw: unknown }) {
    super(message);
    this.name = "ReadStoryValidationContractDriftError";
    this.raw = options.raw;
  }
}

/**
 * Compose the read-only pre-transfer validation verdict for the selected local
 * story against the connected supported Lunii. Resolves with a
 * `StoryValidationDto` (tagged enum on `kind`); rejects with a normalized
 * `AppError` on a read-side failure; rejects with the
 * {@link ReadStoryValidationContractDriftError} on a drifted wire shape; rejects
 * with a synthetic `UNKNOWN`-coded error on timeout.
 *
 * Components MUST NOT call `invoke` directly — go through this façade so the wire
 * contract stays owned by `src/ipc/`.
 */
export function readStoryValidation(
  input: ReadStoryValidationInput,
  timeoutMs: number = READ_STORY_VALIDATION_TIMEOUT_MS,
): ReadStoryValidationCall {
  const call = invoke<unknown>("read_story_validation", { input })
    .then((raw) => {
      if (!isStoryValidationDto(raw)) {
        throw new ReadStoryValidationContractDriftError(
          "StoryValidationDto wire shape drifted from the canonical contract.",
          { raw },
        );
      }
      return raw;
    })
    .catch((err) => {
      // A drift is its own typed error; everything else is normalized to the
      // stable AppError shape so callers observe one shape.
      if (err instanceof ReadStoryValidationContractDriftError) throw err;
      throw toAppError(err);
    });

  let timer: ReturnType<typeof setTimeout> | undefined;
  let cancelled = false;

  const guard = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timer = undefined;
      if (cancelled) return;
      reject(READ_STORY_VALIDATION_TIMEOUT_ERROR);
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
