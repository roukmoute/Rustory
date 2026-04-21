/**
 * Wire contract for normalized Rustory errors crossing the IPC boundary.
 *
 * Mirror of `src-tauri/src/domain/shared/error.rs`. The `code` field is the
 * stable discriminant the UI switches on; `message` and `userAction` are
 * already localized strings produced by the Rust side.
 */
export type AppErrorCode = "LOCAL_STORAGE_UNAVAILABLE" | "UNKNOWN";

export interface AppError {
  code: AppErrorCode;
  message: string;
  userAction: string | null;
  /** Always present in the wire shape; `null` when the Rust side had no
   *  structured context to attach. */
  details: unknown | null;
}

export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    (candidate.userAction === null ||
      typeof candidate.userAction === "string") &&
    "details" in candidate
  );
}

/**
 * Wrap any non-AppError IPC rejection into an `UNKNOWN`-coded error. Keeps
 * the UI switch total without masking genuine storage failures under the
 * `LOCAL_STORAGE_UNAVAILABLE` discriminant.
 */
export function toAppError(raw: unknown): AppError {
  if (isAppError(raw)) return raw;
  return {
    code: "UNKNOWN",
    message:
      "Une erreur inattendue est survenue au démarrage de Rustory.",
    userAction:
      "Relance l'application. Si le problème persiste, signale-le avec les traces locales.",
    details: raw ?? null,
  };
}
