import { act, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../ipc/commands/import-export", () => ({
  fetchWebPodcastPreview: vi.fn(),
  acceptWebPodcastCreation: vi.fn(),
}));

vi.mock("../../library/hooks/use-library-overview", () => ({
  invalidateLibraryOverviewCache: vi.fn(),
}));

import {
  acceptWebPodcastCreation,
  fetchWebPodcastPreview,
} from "../../../ipc/commands/import-export";
import { invalidateLibraryOverviewCache } from "../../library/hooks/use-library-overview";
import { useWebCreation } from "./use-web-creation";

const WEB_URL =
  "https://www.radiofrance.fr/franceinter/podcasts/serie-tina-et-le-serpent-a-plumes";

const PAGE_CHECKSUM = "c".repeat(64);

const WEB_PREVIEW = {
  sourceHost: "www.radiofrance.fr",
  pageChecksum: PAGE_CHECKSUM,
  items: [
    {
      title: "Épisode 1",
      summary: "Premier texte.",
      audioUrl: "https://media.exemple.fr/e1.mp3",
      imageUrl: null,
    },
    {
      title: "Épisode 2",
      summary: "Deuxième texte.",
      audioUrl: "https://media.exemple.fr/e2.mp3",
      imageUrl: "https://media.exemple.fr/e2.jpg",
    },
  ],
};

const CREATED_STORY = {
  id: "0197a5d0-0000-7000-8000-000000000000",
  title: "Tina et le Serpent à plumes",
  importState: "needsReview" as const,
};

const S6_ERROR = {
  code: "IMPORT_FAILED",
  message: "Aucun média audio n'a été trouvé.",
  userAction: "Vérifie que la page contient des épisodes audio puis réessaie.",
  details: { source: "parsing", stage: "no_audio_media" },
};

const POLICY_ERROR = {
  code: "CONTENT_SOURCE_UNAVAILABLE",
  message: "Source indisponible: non activée dans la distribution officielle",
  userAction: "Contacte le support.",
  details: { kind: "web" },
};

beforeEach(() => {
  vi.mocked(fetchWebPodcastPreview).mockReset();
  vi.mocked(acceptWebPodcastCreation).mockReset();
  vi.mocked(invalidateLibraryOverviewCache).mockReset();
});

describe("useWebCreation", () => {
  it("starts idle", () => {
    const { result } = renderHook(() => useWebCreation());
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("fetchPreview lands on the review with the preview", async () => {
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    expect(fetchWebPodcastPreview).toHaveBeenCalledWith(WEB_URL);
    expect(result.current.status).toEqual({
      kind: "review",
      webUrl: WEB_URL,
      preview: WEB_PREVIEW,
      sourceChanged: false,
    });
  });

  it("a motivated failure (S4/S5/S6) lands on failed with the AppError", async () => {
    vi.mocked(fetchWebPodcastPreview).mockRejectedValueOnce(S6_ERROR);
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    expect(result.current.status).toEqual({ kind: "failed", error: S6_ERROR });
  });

  it("the policy refusal lands on the calm unavailable state, and the fetch is a no-op there", async () => {
    vi.mocked(fetchWebPodcastPreview).mockRejectedValueOnce(POLICY_ERROR);
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    expect(result.current.status).toEqual({
      kind: "unavailable",
      error: POLICY_ERROR,
    });
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    // The second fetch never reached the IPC (a policy refusal is a
    // dead end for THIS surface — only `abandon` exists).
    expect(fetchWebPodcastPreview).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "unavailable",
      error: POLICY_ERROR,
    });
  });

  it("acceptCreation commits the FULL page: the address + the page checksum", async () => {
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    vi.mocked(acceptWebPodcastCreation).mockResolvedValueOnce({
      kind: "created",
      story: CREATED_STORY,
      report: [],
    });
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptWebPodcastCreation).toHaveBeenCalledWith(
      WEB_URL,
      PAGE_CHECKSUM,
    );
    // The canonical store changed — the library cache was invalidated.
    expect(invalidateLibraryOverviewCache).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual({
      kind: "created",
      story: CREATED_STORY,
    });
  });

  it("a diverged page resolves the typed sourceChanged refusal (back to the review)", async () => {
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    vi.mocked(acceptWebPodcastCreation).mockResolvedValueOnce({
      kind: "sourceChanged",
    });
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(invalidateLibraryOverviewCache).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({
      kind: "review",
      webUrl: WEB_URL,
      preview: WEB_PREVIEW,
      sourceChanged: true,
    });
  });

  it("acceptCreation is a no-op outside a live review", async () => {
    vi.mocked(acceptWebPodcastCreation).mockResolvedValueOnce({
      kind: "created",
      story: CREATED_STORY,
      report: [],
    });
    const { result } = renderHook(() => useWebCreation());
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(acceptWebPodcastCreation).not.toHaveBeenCalled();
    expect(result.current.status).toEqual({ kind: "idle" });

    // A source-changed review has nothing to create either. The no-op
    // above never consumed a mock, so re-arm a FRESH sourceChanged
    // resolution for the live review below.
    vi.mocked(acceptWebPodcastCreation).mockReset();
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    vi.mocked(acceptWebPodcastCreation).mockResolvedValueOnce({
      kind: "sourceChanged",
    });
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    await act(async () => {
      await result.current.acceptCreation();
    });
    expect(result.current.status.kind).toBe("review");
    const review = result.current.status;
    if (review.kind === "review") {
      expect(review.sourceChanged).toBe(true);
    }
    await act(async () => {
      await result.current.acceptCreation();
    });
    // Still refused, still one accept call in total.
    expect(acceptWebPodcastCreation).toHaveBeenCalledTimes(1);
  });

  it("abandon resets to idle from any non-terminal state; dismiss only from the terminals", async () => {
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    const { result } = renderHook(() => useWebCreation());

    // From the review: pure frontend reset.
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });

    // Dismiss is a no-op on a non-terminal state.
    vi.mocked(fetchWebPodcastPreview).mockResolvedValueOnce(WEB_PREVIEW);
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status.kind).toBe("review");

    // From a terminal failed state: only dismiss (or nothing) exists.
    vi.mocked(fetchWebPodcastPreview).mockRejectedValueOnce(S6_ERROR);
    await act(async () => {
      await result.current.fetchPreview(WEB_URL);
    });
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "failed", error: S6_ERROR });
    act(() => {
      result.current.dismiss();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });

  it("an abandon mid-fetch ignores the late settlement (StrictMode-safe)", async () => {
    let settle!: (value: typeof WEB_PREVIEW) => void;
    vi.mocked(fetchWebPodcastPreview).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          settle = resolve;
        }),
    );
    const { result } = renderHook(() => useWebCreation(), {
      wrapper: StrictMode,
    });
    let pending: Promise<void> | undefined;
    act(() => {
      pending = result.current.fetchPreview(WEB_URL);
    });
    // Abandon while the fetch is still in flight…
    act(() => {
      result.current.abandon();
    });
    expect(result.current.status).toEqual({ kind: "idle" });
    // …the late settlement must NOT resurrect the closed surface.
    await act(async () => {
      settle(WEB_PREVIEW);
      await pending;
    });
    expect(result.current.status).toEqual({ kind: "idle" });
  });
});
