import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { openUrl } from "@tauri-apps/plugin-opener";

import {
  DeviceStoryCollection,
  DeviceStoryInspector,
  useConnectedLunii,
  useDeviceLibrary,
  useDeviceStoryImport,
} from "../../features/device";

/** Public URL of the canonical device-support-profile document. Kept
 *  as a single constant so a future move (rename, monorepo, branch
 *  policy) is a one-line change. */
const SUPPORT_PROFILE_URL =
  "https://github.com/roukmoute/Rustory/blob/main/docs/architecture/device-support-profile.md";

function openSupportProfile(): void {
  // tauri-plugin-opener delegates to the OS default browser. The
  // promise is intentionally not awaited: a failure (no network, no
  // browser configured) does not block the UI — the user can still
  // click again or copy the URL by hand.
  void openUrl(SUPPORT_PROFILE_URL);
}
import { CreateStoryDialog } from "../../features/library/components/CreateStoryDialog";
import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { LibraryFiltersNav } from "../../features/library/components/LibraryFiltersNav";
import {
  LuniiDecisionPanel,
  type LuniiDeviceState,
} from "../../features/library/components/LuniiDecisionPanel";
import { StoryCollection } from "../../features/library/components/StoryCollection";
import {
  invalidateLibraryOverviewCache,
  useLibraryOverview,
} from "../../features/library/hooks/use-library-overview";
import type {
  ConnectedDeviceDto,
  SupportedOperationsDto,
} from "../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../shared/ipc-contracts/device-library";
import type { StoryCardDto } from "../../shared/ipc-contracts/library";
import { LibraryLayout } from "../../shell/layout/LibraryLayout";
import { useLibraryShell } from "../../shell/state/library-shell-store";

export function LibraryRoute(): React.JSX.Element {
  const { state, retry, invalidate } = useLibraryOverview();
  const device = useConnectedLunii();
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

  const { deviceState, deviceLabel, deviceReason, supportedOperations } =
    mapDeviceForPanel(device.state, device.isRefreshing);

  // Derive the device whose library we may read: a supported Lunii that
  // is read-authorized. Fall back to the cached snapshot so the device
  // section survives a background detection refresh (SWR). `null` ⇒ the
  // device-library hook stays idle and issues no IPC.
  const effectiveDevice: ConnectedDeviceDto | null =
    device.state.kind === "ready" ? device.state.device : device.cached;
  const readableDeviceId =
    effectiveDevice &&
    effectiveDevice.kind === "supported" &&
    effectiveDevice.supportedOperations.readLibrary
      ? effectiveDevice.deviceIdentifier
      : null;
  const deviceLibrary = useDeviceLibrary(readableDeviceId);

  // Inspection is offered when the supported profile authorizes it.
  // `inspectStory` is ✅ for every supported cohort (V3 included, unlike
  // import), so in practice this tracks `readableDeviceId`; gating on the
  // capability keeps the authoritative matrix the single source of truth.
  const supportedDeviceOperations: SupportedOperationsDto | undefined =
    effectiveDevice && effectiveDevice.kind === "supported"
      ? effectiveDevice.supportedOperations
      : undefined;
  const canInspect =
    readableDeviceId !== null &&
    supportedDeviceOperations?.inspectStory === true;

  // Device-story selection for inspection. Local UI state, intentionally
  // SEPARATE from the library's `selectedStoryIds`: device truth and local
  // truth never merge. Single selection, never persisted.
  const [selectedDeviceStoryUuid, setSelectedDeviceStoryUuid] = useState<
    string | null
  >(null);

  // Resolve the selection against the CURRENT inventory so a stale id (entry
  // gone after a re-read, device swapped, or no longer inspect-authorized)
  // surfaces no inspector for this render — never a frozen stale target (AC3).
  const selectedDeviceStory =
    canInspect &&
    selectedDeviceStoryUuid &&
    deviceLibrary.state.kind === "ready"
      ? deviceLibrary.state.stories.find(
          (s) => s.uuid === selectedDeviceStoryUuid,
        ) ?? null
      : null;

  // Drop a selection only when it can no longer be inspected (device gone /
  // unsupported / not authorized) OR when a FRESH authoritative inventory
  // genuinely lacks it. A transient loading/error/refresh state keeps the
  // selection so it survives and is restored once the entry is confirmed
  // present again — it is never wiped on a passing state (AC3).
  useEffect(() => {
    if (selectedDeviceStoryUuid === null) return;
    if (!canInspect) {
      setSelectedDeviceStoryUuid(null);
      return;
    }
    if (
      deviceLibrary.state.kind === "ready" &&
      !deviceLibrary.state.stories.some(
        (s) => s.uuid === selectedDeviceStoryUuid,
      )
    ) {
      setSelectedDeviceStoryUuid(null);
    }
  }, [canInspect, deviceLibrary.state, selectedDeviceStoryUuid]);

  const handleSelectDeviceStory = (uuid: string): void => {
    // Toggle: clicking the already-selected entry clears the inspection.
    setSelectedDeviceStoryUuid((prev) => (prev === uuid ? null : uuid));
  };

  // Device-story copy flow. On success, BOTH sides re-read their
  // authoritative truth: the local overview (the new card appears) and
  // the device inventory (the `alreadyImported` stamp flips the chip and
  // the CTA). The device selection is intentionally NOT touched — the
  // story still lives on the device, a copy is not a move; the resilient
  // purge above keeps it across the transient refresh states.
  const deviceImport = useDeviceStoryImport({
    onImported: () => {
      invalidate();
      deviceLibrary.refresh();
    },
  });

  // Mirror of the Rust capability gate: the CTA handler is wired only
  // when the authoritative matrix POSITIVELY allows the copy. Rust stays
  // the authority — this gate only shapes the affordance.
  const canImportDeviceStory =
    canInspect && supportedDeviceOperations?.importStory === true;

  const handleImportDeviceStory = (story: DeviceStoryDto): void => {
    if (!readableDeviceId) return;
    void deviceImport.triggerImport(readableDeviceId, story.uuid);
  };

  // The import status belongs to ONE pack — the one the hook actually
  // started a copy for (`targetPackUuid`, set past its re-entrancy
  // guard). Selecting another card shows THAT card's (idle) status,
  // never the previous card's success; re-selecting the copied card
  // surfaces its status again. A second "Copier" clicked while a copy is
  // in flight is swallowed by the hook AND leaves the target untouched,
  // so the status can never follow the wrong card.
  const selectedDeviceImportState =
    selectedDeviceStoryUuid !== null &&
    selectedDeviceStoryUuid === deviceImport.targetPackUuid
      ? deviceImport.status
      : undefined;

  const center = (
    <>
      {renderCenter(
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
      )}
      <DeviceStoryCollection
        state={deviceLibrary.state}
        isRefreshing={deviceLibrary.isRefreshing}
        deviceLabel={deviceLabel}
        selectedUuid={canInspect ? selectedDeviceStoryUuid : null}
        onSelectStory={canInspect ? handleSelectDeviceStory : undefined}
        onRetry={deviceLibrary.refresh}
      />
    </>
  );

  return (
    <>
      <LibraryLayout
        leftNav={<LibraryFiltersNav />}
        center={center}
        rightPanel={
          <>
            <DeviceStoryInspector
              story={selectedDeviceStory}
              supportedOperations={supportedDeviceOperations}
              importState={selectedDeviceImportState}
              onImport={
                canImportDeviceStory ? handleImportDeviceStory : undefined
              }
              onRetryImport={() => {
                void deviceImport.retryImport();
              }}
              onDismissImportStatus={deviceImport.dismissStatus}
            />
            <LuniiDecisionPanel
              deviceState={deviceState}
              deviceLabel={deviceLabel}
              deviceReason={deviceReason}
              supportedOperations={supportedOperations}
              selectedCount={presentSelectedIds.size}
              onEdit={handleEditSelected}
              onRefreshDevice={device.refresh}
              onConsultSupportProfile={openSupportProfile}
            />
          </>
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

interface DevicePanelMapping {
  deviceState: LuniiDeviceState;
  deviceLabel?: string;
  deviceReason?: string;
  /** Authoritative per-profile operation matrix surfaced to the panel
   *  so the user sees, in the device area, which capabilities the
   *  detected Lunii actually exposes (AC1 — "affiche le profil
   *  détecté et les opérations officiellement supportées"). */
  supportedOperations?: SupportedOperationsDto;
}

/**
 * Pure mapper from the `useConnectedLunii` state to the props
 * `LuniiDecisionPanel` expects. Pure so it stays testable in isolation
 * — the route owns no behavior beyond passing the props through.
 *
 * `isRefreshing` lets the route surface a transient `scanning` state
 * even when a cached snapshot is rendered behind it: the UX rule is
 * "show that detection is in flight even if the previous result is
 * still visible".
 */
export function mapDeviceForPanel(
  state: ReturnType<typeof useConnectedLunii>["state"],
  isRefreshing: boolean,
): DevicePanelMapping {
  if (state.kind === "loading") {
    return { deviceState: "scanning" };
  }
  if (state.kind === "error") {
    return {
      deviceState: "error",
      deviceReason:
        state.error.userAction ??
        "Détection indisponible: vérifie que la Lunii est branchée et réessaie.",
    };
  }
  if (isRefreshing) {
    return { deviceState: "scanning" };
  }
  return mapDeviceDtoForPanel(state.device);
}

function mapDeviceDtoForPanel(dto: ConnectedDeviceDto): DevicePanelMapping {
  switch (dto.kind) {
    case "none":
      return { deviceState: "absent" };
    case "supported":
      return {
        deviceState: "idle",
        deviceLabel: formatSupportedLabel(dto.firmwareCohort),
        supportedOperations: dto.supportedOperations,
      };
    case "unsupported":
      return {
        deviceState: "unsupported",
        deviceReason: formatUnsupportedReason(dto.reason, dto.firmwareHint),
      };
    case "ambiguous":
      return {
        deviceState: "ambiguous",
        deviceReason: `Profil ambigu: ${dto.candidateCount} candidats détectés. Débranche les autres puis réessaie.`,
      };
  }
}

function formatSupportedLabel(
  cohort: "origineV1" | "midGenV2" | "v3",
): string {
  switch (cohort) {
    case "origineV1":
      return "Lunii Origine";
    case "midGenV2":
      return "Lunii";
    case "v3":
      return "Lunii V3";
  }
}

function formatUnsupportedReason(
  reason: string,
  hint: string | null,
): string {
  switch (reason) {
    case "metadataUnsupported":
      return hint
        ? `Profil non supporté: format métadonnées ${hint.replace("metadata_v", "v")} non géré`
        : "Profil non supporté: format métadonnées non géré";
    case "metadataCorrupt":
      return "Profil non supporté: marqueurs appareil incomplets";
    case "firmwareUnsupported":
      return hint
        ? `Profil non supporté: firmware ${hint} non géré`
        : "Profil non supporté: firmware non géré";
    case "familyUnknown":
      return "Profil non supporté: famille d'appareil non reconnue";
    case "operationNotAuthorized":
      return "Lecture appareil indisponible: profil non autorisé";
    case "multipleCandidates":
      return "Profil ambigu: plusieurs appareils candidats détectés. Débranche les autres puis réessaie.";
    default:
      return "Profil non supporté";
  }
}

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
