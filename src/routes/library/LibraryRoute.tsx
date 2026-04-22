import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { CreateStoryDialog } from "../../features/library/components/CreateStoryDialog";
import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { LibraryFiltersNav } from "../../features/library/components/LibraryFiltersNav";
import { LuniiDecisionPanel } from "../../features/library/components/LuniiDecisionPanel";
import { StoryCollection } from "../../features/library/components/StoryCollection";
import {
  invalidateLibraryOverviewCache,
  useLibraryOverview,
} from "../../features/library/hooks/use-library-overview";
import type { StoryCardDto } from "../../shared/ipc-contracts/library";
import { LibraryLayout } from "../../shell/layout/LibraryLayout";
import { useLibraryShell } from "../../shell/state/library-shell-store";

export function LibraryRoute(): React.JSX.Element {
  const { state, retry } = useLibraryOverview();
  const selectedStoryIds = useLibraryShell((s) => s.selectedStoryIds);
  const selectStory = useLibraryShell((s) => s.selectStory);
  const pruneSelection = useLibraryShell((s) => s.pruneSelection);
  const query = useLibraryShell((s) => s.query);
  const sort = useLibraryShell((s) => s.sort);
  const setQuery = useLibraryShell((s) => s.setQuery);
  const setSort = useLibraryShell((s) => s.setSort);
  const resetFilters = useLibraryShell((s) => s.resetFilters);
  const navigate = useNavigate();

  const [isCreateOpen, setIsCreateOpen] = useState<boolean>(false);

  const overview = state.kind === "ready" ? state.overview : null;

  // Snapshot the ids present in the current overview. Used both to derive
  // the render-time "present" selection and to drive pruneSelection: when
  // this ref changes value (not identity), a fresh overview has landed.
  const presentIdsRef = useRef<ReadonlySet<string>>(new Set());
  const presentIds = useMemo(() => {
    if (!overview) return presentIdsRef.current;
    const next = new Set(overview.stories.map((s) => s.id));
    presentIdsRef.current = next;
    return next;
  }, [overview]);

  // Prune the store's selection every time a fresh overview lands. Depending
  // on `selectedStoryIds` here would feedback-loop this effect; reading it
  // via the latest-snapshot helper also avoids racing with concurrent
  // `selectStory` dispatches — we pass the freshest value Zustand has seen
  // at the time the effect runs (not the render-time snapshot).
  useEffect(() => {
    if (!overview) return;
    pruneSelection(presentIds);
  }, [overview, presentIds, pruneSelection]);

  // Derive a "present selection" for the same render that reads the fresh
  // overview: if a stored id is no longer in the library, we MUST NOT let
  // it activate Éditer before the prune effect flushes on the next tick.
  const presentSelectedIds = useMemo(() => {
    if (!overview) return selectedStoryIds;
    if ([...selectedStoryIds].every((id) => presentIds.has(id))) {
      return selectedStoryIds;
    }
    return new Set([...selectedStoryIds].filter((id) => presentIds.has(id)));
  }, [overview, selectedStoryIds, presentIds]);

  const handleOpenStory = useMemo(
    () => (id: string) => {
      // `replace` keeps one library entry in history instead of stacking a
      // new one at every open — back button returns to the true previous
      // context, not to the library-we-just-left.
      navigate(`/story/${encodeURIComponent(id)}/edit`, { replace: true });
    },
    [navigate],
  );

  const handleEditSelected = (): void => {
    if (presentSelectedIds.size !== 1) return;
    const [id] = presentSelectedIds;
    handleOpenStory(id);
  };

  const handleCreateStoryRequest = (): void => {
    setIsCreateOpen(true);
  };

  const handleCreated = (story: StoryCardDto): void => {
    // Drop the module-local SWR snapshot so the next useLibraryOverview
    // consumer (this component after rerender, and StoryEditRoute when
    // navigation lands) refetches the canonical overview that includes the
    // freshly inserted row instead of a stale one.
    //
    // We intentionally do NOT call `retry()` here: the synchronous
    // `handleOpenStory` unmounts this route, which would abort the in-flight
    // fetch through the hook's mounted guard and leave the cache still
    // empty. Invalidation alone is enough — the next mount (library return
    // from the edit route) refetches against a clean cache.
    invalidateLibraryOverviewCache();
    handleOpenStory(story.id);
  };

  const center = renderCenter(
    state,
    retry,
    presentSelectedIds,
    selectStory,
    handleOpenStory,
    query,
    sort,
    setQuery,
    setSort,
    resetFilters,
    handleCreateStoryRequest,
  );

  return (
    <>
      <LibraryLayout
        leftNav={<LibraryFiltersNav />}
        center={center}
        rightPanel={
          <LuniiDecisionPanel
            deviceState="absent"
            selectedCount={presentSelectedIds.size}
            onEdit={handleEditSelected}
          />
        }
      />
      <CreateStoryDialog
        open={isCreateOpen}
        onClose={() => setIsCreateOpen(false)}
        onCreated={handleCreated}
      />
    </>
  );
}

type LibraryState = ReturnType<typeof useLibraryOverview>["state"];

function renderCenter(
  state: LibraryState,
  retry: () => void,
  selectedStoryIds: ReadonlySet<string>,
  onSelectStory: (id: string, mode: "replace" | "toggle") => void,
  onOpenStory: (id: string) => void,
  query: string,
  sort: "titre-asc" | "titre-desc",
  onQueryChange: (q: string) => void,
  onSortChange: (s: "titre-asc" | "titre-desc") => void,
  onResetFilters: () => void,
  onCreateStoryRequest: () => void,
): React.JSX.Element {
  switch (state.kind) {
    case "error": {
      const title =
        state.error.code === "LIBRARY_INCONSISTENT"
          ? "Bibliothèque incohérente, recharge nécessaire"
          : "Bibliothèque indisponible";
      return (
        <LibraryErrorBanner
          error={state.error}
          onRetry={retry}
          title={title}
        />
      );
    }
    case "loading":
      return (
        <StoryCollection
          stories={[]}
          isLoading={true}
          query={query}
          sort={sort}
          onQueryChange={onQueryChange}
          onSortChange={onSortChange}
          onResetFilters={onResetFilters}
          selectedStoryIds={selectedStoryIds}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onCreateStoryRequest={onCreateStoryRequest}
        />
      );
    case "ready":
      return (
        <StoryCollection
          stories={state.overview.stories}
          isLoading={false}
          query={query}
          sort={sort}
          onQueryChange={onQueryChange}
          onSortChange={onSortChange}
          onResetFilters={onResetFilters}
          selectedStoryIds={selectedStoryIds}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onCreateStoryRequest={onCreateStoryRequest}
        />
      );
  }
}
