/**
 * Wire contract for the official-catalog commands (story 2-6, Phase C).
 * Mirror of `src-tauri/src/ipc/dto/device_catalog.rs`.
 *
 * `CatalogStatusDto` is returned by `get_official_catalog_status` and
 * `refresh_official_catalog`. `ImportOfficialCatalogOutcome` is returned by
 * `import_official_catalog` — a tagged enum, because a cancelled file dialog
 * is a normal (non-error) outcome, exactly like the export flow.
 */

export interface CatalogStatusDto {
  /** Number of official titles currently cached locally. */
  count: number;
}

export type ImportOfficialCatalogOutcome =
  | { kind: "cancelled" }
  | { kind: "imported"; count: number };

/** One cached cover served as a self-contained `data:` URL (read from the
 *  local cache, no network). Mirror of `PackCoverDto`. */
export interface PackCoverDto {
  dataUrl: string;
}

function hasOnlyAllowedKeys(
  value: Record<string, unknown>,
  allowed: ReadonlySet<string> | undefined,
): boolean {
  if (!allowed) return false;
  for (const k of Object.keys(value)) {
    if (!allowed.has(k)) return false;
  }
  return true;
}

const STATUS_KEYS: ReadonlySet<string> = new Set(["count"]);

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

/** Runtime guard for a `CatalogStatusDto`. */
export function isCatalogStatusDto(value: unknown): value is CatalogStatusDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, STATUS_KEYS)) return false;
  return isCount(c.count);
}

const IMPORT_KEYS: Record<string, ReadonlySet<string>> = {
  cancelled: new Set(["kind"]),
  imported: new Set(["kind", "count"]),
};

/** Runtime guard for an `ImportOfficialCatalogOutcome`. */
export function isImportOfficialCatalogOutcome(
  value: unknown,
): value is ImportOfficialCatalogOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.kind !== "string") return false;
  if (!hasOnlyAllowedKeys(c, IMPORT_KEYS[c.kind])) return false;
  switch (c.kind) {
    case "cancelled":
      return true;
    case "imported":
      return isCount(c.count);
    default:
      return false;
  }
}

const COVER_KEYS: ReadonlySet<string> = new Set(["dataUrl"]);

/** Runtime guard for a `PackCoverDto` — a non-empty `data:` URL. */
export function isPackCoverDto(value: unknown): value is PackCoverDto {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!hasOnlyAllowedKeys(c, COVER_KEYS)) return false;
  return typeof c.dataUrl === "string" && c.dataUrl.startsWith("data:");
}
