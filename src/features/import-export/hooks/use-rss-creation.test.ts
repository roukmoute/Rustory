import { act, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  fetchRssSourcePreview: vi.fn(),
  acceptRssStoryCreation: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptRssStoryCreation,
  fetchRssSourcePreview,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useRssCreation } from "./use-rss-creation";

const FEED_URL = "https://exemple.fr/flux.xml";

const RSS_PREVIEW = {
  sourceHost: "exemple.fr",
  items: [
    {
      title: "Episode 1",
      summary: "Premier texte.",
      hasEnclosure: false,
      itemRef: {
        kind: "guid" as const,
        guid: "g-1",
        fingerprint: "a".repeat(64),
      },
    },
    {
      title: "Episode 2",
      summary: "Deuxième texte.",
      hasEnclosure: true,
      itemRef: {
        kind: "guid" as const,
        guid: "g-2",
        fingerprint: "b".repeat(64),
      },
    },
  ],
  findings: [
    {
      aspect: "source" as const,
      category: "ambiguous" as const,
      message:
        "Contenu ingéré depuis une source externe (RSS). Relis le texte et complète l'histoire avant de l'utiliser.",
    },
  ],
  state: "needsReview" as const,
  blocked: false,
};

const RSS_PREVIEW_BLOCKED = {
  sourceHost: "exemple.fr",
  items: [],
  findings: [
    {
      aspect: "envelope" as const,
      category: "blocking" as const,
      message:
        "Ce contenu n'est pas un flux RSS lisible. Relance la récupération du flux.",
    },
  ],
  state: "blocked" as const,
  blocked: true,
};

const CREATED_STORY = {
  id: "0197a5d0-0000-7000-8000-000000000000",
  title: "Episode 1",
  importState: "needsReview" as const,
};

const APP_ERROR = {
  code: "RSS_SOURCE_UNREACHABLE",
  message: "Récupération du flux impossible: la source est injoignable.",
  userAction: "Vérifie l'adresse du flux et ta connexion, puis réessaie.",
  details: { source: "network", stage: "request" },
};

beforeEach(() => {
  vi.mocked(fetchRssSourcePreview).mockReset();
  vi.mocked(acceptRssStoryCreation).mockReset();
  vi.mocked(invalidateLibraryOverviewCache).mockReset();
});

describe("useRssCreation", () => {
  it("starts idle", () => {
    const { result } = renderHook(() => useRssCreation());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("fetchPreview lands on review with the preview and no selection", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(fetchRssSourcePreview).toHaveBeenCalledWith(FEED_URL);
    expect(result.current.status).toEqual({
      kind: "review",
      feedUrl: FEED_URL,
      preview: RSS_PREVIEW,
      selectedItemRef: null,
      sourceChanged: false,
    });
  });

  it("fetchPreview lands on review for a blocked verdict (typed, never failed)", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(
      RSS_PREVIEW_BLOCKED,
    );
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    const status = result.current.status;
    expect(status.kind).toBe("review");
    if (status.kind === "review") {
      expect(status.preview.blocked).toBe(true);
    }
  });

  it("fetchPreview lands on failed for a transport rejection", async () => {
    vi.mocked(fetchRssSourcePreview).mockRejectedValueOnce(APP_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    const status = result.current.status;
    expect(status.kind).toBe("failed");
    if (status.kind === "failed") {
      expect(status.error.code).toBe("RSS_SOURCE_UNREACHABLE");
    }
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("a re-fetch from review replaces the preview", async () => {
    vi.mocked(fetchRssSourcePreview)
      .mockResolvedValueOnce(RSS_PREVIEW)
      .mockResolvedValueOnce(RSS_PREVIEW_BLOCKED);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    const status = result.current.status;
    expect(status.kind).toBe("review");
    if (status.kind === "review") {
      expect(status.preview.blocked).toBe(true);
    }
  });

  it("selectItem stores the reference on an exploitable review only", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-2",
        fingerprint: "b".repeat(64),
      });
    });
    const status = result.current.status;
    expect(status.kind).toBe("review");
    if (status.kind === "review") {
      expect(status.selectedItemRef).toEqual({
        kind: "guid",
        guid: "g-2",
        fingerprint: "b".repeat(64),
      });
    }
  });

  it("selectItem is a no-op on a blocked review and outside review", async () => {
    const { result } = renderHook(() => useRssCreation());
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(
      RSS_PREVIEW_BLOCKED,
    );
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    const status = result.current.status;
    if (status.kind === "review") {
      expect(status.selectedItemRef).toBeNull();
    }
  });

  it("acceptCreation commits the selected item, invalidates the cache and lands on created", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    vi.mocked(acceptRssStoryCreation).mockResolvedValueOnce({
      kind: "created",
      story: CREATED_STORY,
      report: [],
    });
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptRssStoryCreation).toHaveBeenCalledWith(FEED_URL, {
      kind: "guid",
      guid: "g-1",
      fingerprint: "a".repeat(64),
    });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_STORY,
    });
  });

  it("acceptCreation is a no-op without a selection, on a blocked review and outside review", async () => {
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptRssStoryCreation).not.toHaveBeenCalled();

    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptRssStoryCreation).not.toHaveBeenCalled();
  });

  it("a sourceChanged refusal returns to review with dead items and no selection", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    vi.mocked(acceptRssStoryCreation).mockResolvedValueOnce({
      kind: "sourceChanged",
    });
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status).toEqual({
      kind: "review",
      feedUrl: FEED_URL,
      preview: RSS_PREVIEW,
      selectedItemRef: null,
      sourceChanged: true,
    });
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();

    // The diverged review refuses a new accept until a re-fetch.
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptRssStoryCreation).toHaveBeenCalledTimes(1);
  });

  it("acceptCreation lands on failed for a transport rejection", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    vi.mocked(acceptRssStoryCreation).mockRejectedValueOnce(APP_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status.kind).toBe("failed");
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("abandon resets a review (pure frontend) and dismiss resets terminals", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    vi.mocked(fetchRssSourcePreview).mockRejectedValueOnce(APP_ERROR);
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    // `abandon` ignores a terminal status; `dismiss` settles it.
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status.kind).toBe("failed");
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("abandon works from a long fetch and the late result never resurrects the flow", async () => {
    let settlePreview: (value: typeof RSS_PREVIEW) => void = () => {};
    vi.mocked(fetchRssSourcePreview).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settlePreview = resolve;
        }),
    );
    const { result } = renderHook(() => useRssCreation());
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status).toEqual({ kind: "fetching" });

    // Abandon MID-FLIGHT: back to idle immediately.
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // The late settlement is IGNORED — the surface never resurrects.
    await act(async () => {
      settlePreview(RSS_PREVIEW);
      await pending;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("abandon works from a long creation and the late outcome never resurrects the flow", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    let settleAccept: (value: {
      kind: "created";
      story: typeof CREATED_STORY;
      report: never[];
    }) => void = () => {};
    vi.mocked(acceptRssStoryCreation).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settleAccept = resolve;
        }),
    );
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem({
        kind: "guid",
        guid: "g-1",
        fingerprint: "a".repeat(64),
      });
    });
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = result.current.acceptCreation();
    });
    expect(result.current.status).toEqual({ kind: "creating" });

    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // Rust settles its atomic work; the UI stopped listening — but the
    // library cache is still dropped so the next authoritative read shows
    // the created card.
    await act(async () => {
      settleAccept({ kind: "created", story: CREATED_STORY, report: [] });
      await pending;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
  });

  it("a NEW fetch starts immediately after an abandon even while the old call is still in flight", async () => {
    // The re-entrancy gate is generation-aware: an ABANDONED in-flight
    // call must not dead-lock the reopened surface for the rest of its
    // network budget — the new explicit click starts a fresh fetch at
    // once, and the OLD settlement (arriving later) is ignored.
    let settleOld: (value: typeof RSS_PREVIEW_BLOCKED) => void = () => {};
    vi.mocked(fetchRssSourcePreview)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            settleOld = resolve;
          }),
      )
      .mockResolvedValueOnce(RSS_PREVIEW);
    const { result } = renderHook(() => useRssCreation());
    let oldCall: Promise<void> = Promise.resolve();
    act(() => {
      oldCall = result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status).toEqual({ kind: "fetching" });

    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // The user reopens and fetches again WHILE the old call still hangs:
    // the new call must start immediately (no dead click window).
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(fetchRssSourcePreview).toHaveBeenCalledTimes(2);
    expect(result.current.status).toEqual({
      kind: "review",
      feedUrl: FEED_URL,
      preview: RSS_PREVIEW,
      selectedItemRef: null,
      sourceChanged: false,
    });

    // The OLD call settles after the new one: its (blocked) preview must
    // never overwrite the fresh review.
    await act(async () => {
      settleOld(RSS_PREVIEW_BLOCKED);
      await oldCall;
    });
    const status = result.current.status;
    expect(status.kind).toBe("review");
    if (status.kind === "review") {
      expect(status.preview.blocked).toBe(false);
    }
  });

  // ===== The content-source policy refusal (`unavailable`) =====

  const POLICY_ERROR = {
    code: "CONTENT_SOURCE_UNAVAILABLE",
    message:
      "Cette source de contenu n'est pas activée dans la distribution officielle.",
    userAction:
      "Utilise une source activée ou consulte le profil de support de ta version.",
    details: { source: "content_source_policy", kind: "rss" },
  };

  it("fetchPreview lands on the calm unavailable state for a policy refusal (never failed)", async () => {
    vi.mocked(fetchRssSourcePreview).mockRejectedValueOnce(POLICY_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status).toEqual({
      kind: "unavailable",
      error: POLICY_ERROR,
    });
  });

  it("acceptCreation lands on unavailable for a policy refusal (defence in depth)", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    vi.mocked(acceptRssStoryCreation).mockRejectedValueOnce(POLICY_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.selectItem(RSS_PREVIEW.items[0].itemRef);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status.kind).toBe("unavailable");
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
  });

  it("retry-shaped actions are no-ops in unavailable (a retry cannot change a policy)", async () => {
    vi.mocked(fetchRssSourcePreview).mockRejectedValueOnce(POLICY_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status.kind).toBe("unavailable");
    vi.mocked(fetchRssSourcePreview).mockClear();
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
      await result.current.acceptCreation();
    });
    expect(fetchRssSourcePreview).not.toHaveBeenCalled();
    expect(acceptRssStoryCreation).not.toHaveBeenCalled();
    expect(result.current.status.kind).toBe("unavailable");
  });

  it("abandon exits unavailable back to idle (the refusal's only gesture)", async () => {
    vi.mocked(fetchRssSourcePreview).mockRejectedValueOnce(POLICY_ERROR);
    const { result } = renderHook(() => useRssCreation());
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    // The flow restarts cleanly after the abandon.
    vi.mocked(fetchRssSourcePreview).mockResolvedValueOnce(RSS_PREVIEW);
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status.kind).toBe("review");
  });

  it("survives StrictMode double-invocation", async () => {
    vi.mocked(fetchRssSourcePreview).mockResolvedValue(RSS_PREVIEW);
    const { result } = renderHook(() => useRssCreation(), {
      wrapper: StrictMode,
    });
    await act(async () => {
      await result.current.fetchPreview(FEED_URL);
    });
    expect(result.current.status.kind).toBe("review");
  });
});
