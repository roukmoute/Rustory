import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

import {
  OfficialCatalogContractDriftError,
  getOfficialCatalogStatus,
  importOfficialCatalog,
  readPackCover,
  refreshOfficialCatalog,
} from "./device-catalog";

const PACK_UUID = "abababab-abab-abab-abab-ababfac5562d";

describe("official-catalog commands", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("getOfficialCatalogStatus returns the cached count", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ count: 12 });
    const status = await getOfficialCatalogStatus();
    expect(invoke).toHaveBeenCalledWith("get_official_catalog_status");
    expect(status.count).toBe(12);
  });

  it("refreshOfficialCatalog invokes the network command and returns the count", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ count: 1200 });
    const status = await refreshOfficialCatalog();
    expect(invoke).toHaveBeenCalledWith("refresh_official_catalog");
    expect(status.count).toBe(1200);
  });

  it("importOfficialCatalog returns a cancelled outcome verbatim", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "cancelled" });
    const outcome = await importOfficialCatalog();
    expect(outcome).toEqual({ kind: "cancelled" });
  });

  it("importOfficialCatalog returns an imported outcome with a count", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ kind: "imported", count: 9 });
    const outcome = await importOfficialCatalog();
    expect(outcome).toEqual({ kind: "imported", count: 9 });
  });

  it("rejects with the drift error when a status payload drifts", async () => {
    const drifted = { count: -1 };
    vi.mocked(invoke).mockResolvedValueOnce(drifted);
    const error = await getOfficialCatalogStatus().catch((err: unknown) => err);
    expect(error).toBeInstanceOf(OfficialCatalogContractDriftError);
    expect((error as OfficialCatalogContractDriftError).raw).toBe(drifted);
  });

  it("propagates a Rust AppError rejection untouched (offline refresh)", async () => {
    const appError = {
      code: "OFFICIAL_CATALOG_UNAVAILABLE",
      message: "Récupération du catalogue officiel impossible: le service est injoignable.",
      userAction: "Vérifie ta connexion puis réessaie.",
      details: { source: "network", stage: "auth_request" },
    };
    vi.mocked(invoke).mockRejectedValueOnce(appError);
    await expect(refreshOfficialCatalog()).rejects.toBe(appError);
  });

  it("normalizes a non-AppError transport rejection through toAppError", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("bridge blew up"));
    const error = await refreshOfficialCatalog().catch((err: unknown) => err);
    expect(error).toMatchObject({ code: "UNKNOWN" });
  });

  it("readPackCover passes the packUuid and returns a cover data URL", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ dataUrl: "data:image/png;base64,AAAA" });
    const cover = await readPackCover(PACK_UUID);
    expect(invoke).toHaveBeenCalledWith("read_pack_cover", { packUuid: PACK_UUID });
    expect(cover).toEqual({ dataUrl: "data:image/png;base64,AAAA" });
  });

  it("readPackCover resolves null when there is no cover", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    expect(await readPackCover(PACK_UUID)).toBeNull();
  });

  it("readPackCover degrades a drifted payload to null (covers are decorative)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ dataUrl: "https://example/cover.png" });
    expect(await readPackCover(PACK_UUID)).toBeNull();
  });
});
