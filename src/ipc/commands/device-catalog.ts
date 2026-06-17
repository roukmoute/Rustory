import { invoke } from "@tauri-apps/api/core";

import { toAppError } from "../../shared/errors/app-error";
import type {
  CatalogStatusDto,
  ImportOfficialCatalogOutcome,
  PackCoverDto,
} from "../../shared/ipc-contracts/device-catalog";
import {
  isCatalogStatusDto,
  isImportOfficialCatalogOutcome,
  isPackCoverDto,
} from "../../shared/ipc-contracts/device-catalog";

/**
 * Error thrown when an official-catalog command resolves with a payload
 * that does not match the wire contract. The raw response is attached for
 * production debugging — never surfaced verbatim to the user.
 */
export class OfficialCatalogContractDriftError extends Error {
  readonly raw: unknown;
  constructor(command: string, raw: unknown) {
    super(`${command} returned a payload that does not match the contract`);
    this.name = "OfficialCatalogContractDriftError";
    this.raw = raw;
  }
}

/**
 * Read how many official titles are cached locally. A bounded count query —
 * NO network. Components MUST NOT call `invoke` directly.
 */
export async function getOfficialCatalogStatus(): Promise<CatalogStatusDto> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("get_official_catalog_status");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isCatalogStatusDto(raw)) {
    throw new OfficialCatalogContractDriftError("get_official_catalog_status", raw);
  }
  return raw;
}

/**
 * EXPLICIT network fetch of the official catalog (guest auth → /v2/packs).
 * This is the ONLY frontend action that triggers a network call, and only
 * on a deliberate user click. Rust owns the wall-clock budget (no frontend
 * timeout). Failures reject with a normalized `OFFICIAL_CATALOG_UNAVAILABLE`
 * `AppError`.
 */
export async function refreshOfficialCatalog(): Promise<CatalogStatusDto> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("refresh_official_catalog");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isCatalogStatusDto(raw)) {
    throw new OfficialCatalogContractDriftError("refresh_official_catalog", raw);
  }
  return raw;
}

/**
 * Import the official catalog from a user-picked file (100%-offline path).
 * Rust opens the native open-file dialog, reads, parses and caches it. A
 * cancelled dialog resolves with `{ kind: "cancelled" }` (not an error).
 */
export async function importOfficialCatalog(): Promise<ImportOfficialCatalogOutcome> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("import_official_catalog");
  } catch (err) {
    throw toAppError(err);
  }
  if (!isImportOfficialCatalogOutcome(raw)) {
    throw new OfficialCatalogContractDriftError("import_official_catalog", raw);
  }
  return raw;
}

/**
 * Read the cached cover for a pack as a `data:` URL — a LOCAL read of the
 * cover cache (no network). Resolves with `null` when the pack has no cached
 * cover (the common case for user/local titles and any pack the catalog
 * didn't cover). A drift in the payload also degrades to `null`: a missing
 * cover is decorative, never worth surfacing an error to the user.
 */
export async function readPackCover(
  packUuid: string,
): Promise<PackCoverDto | null> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>("read_pack_cover", { packUuid });
  } catch (err) {
    throw toAppError(err);
  }
  if (raw === null) return null;
  return isPackCoverDto(raw) ? raw : null;
}
