import type React from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { openUrl } from "@tauri-apps/plugin-opener";

import {
  CatalogPanel,
  DeviceStoryCollection,
  DeviceStoryInspector,
  invalidatePackCoverCache,
  useConnectedLunii,
  useDeviceLibrary,
  useDeviceStoryImport,
  useDeviceStoryTitle,
  useOfficialCatalog,
  useStoryValidation,
  useTransferPreview,
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
  type PreparationView,
  type StoryValidationView,
  type TransferComparisonView,
  type TransferView,
} from "../../features/library/components/LuniiDecisionPanel";
import { useStoryPreparation, useStoryTransfer } from "../../features/transfer";
import { CreateFromFolderSurface } from "../../features/import-export/components/CreateFromFolderSurface";
import { CreateFromRssSurface } from "../../features/import-export/components/CreateFromRssSurface";
import { ImportArtifactSurface } from "../../features/import-export/components/ImportArtifactSurface";
import { readContentSourcePolicy } from "../../ipc/commands/import-export";
import { useRssCreation } from "../../features/import-export/hooks/use-rss-creation";
import { useStoryImport } from "../../features/import-export/hooks/use-story-import";
import { useStructuredCreation } from "../../features/import-export/hooks/use-structured-creation";
import type { StoryPreparationBadge } from "../../features/library/components/StoryCard";
import { StoryCollection } from "../../features/library/components/StoryCollection";
import {
  invalidateLibraryOverviewCache,
  useLibraryOverview,
} from "../../features/library/hooks/use-library-overview";
import type {
  ConnectedDeviceDto,
  FirmwareCohortDto,
  SupportedFamilyDto,
  SupportedOperationsDto,
} from "../../shared/ipc-contracts/device";
import type { DeviceStoryDto } from "../../shared/ipc-contracts/device-library";
import type { ContentSourcePolicy } from "../../shared/ipc-contracts/import-export";
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
  // The distribution's content-source policy, read ANEW at every dialog
  // opening (a point-in-time read — no cache, no authoritative frontend
  // state; Rust alone decides). `null` = not read / read failed: the
  // dialog renders its external-source entries FAIL-CLOSED.
  const [contentSourcePolicy, setContentSourcePolicy] =
    useState<ContentSourcePolicy | null>(null);
  // Opening token: only the read issued by the LATEST opening may apply
  // its result. A slow read from a previous (possibly closed) opening
  // that settles out of order must never overwrite the current opening's
  // state — the point-in-time contract would silently break.
  const policyReadTokenRef = useRef(0);

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
    // Read the policy for THIS opening. The dialog opens immediately and
    // renders fail-closed until the read lands — a policy failure must
    // never block the primary title path. The token pins the result to
    // this opening: a stale resolution (an earlier opening's read landing
    // late) is ignored instead of overwriting the current state.
    setContentSourcePolicy(null);
    policyReadTokenRef.current += 1;
    const readToken = policyReadTokenRef.current;
    void readContentSourcePolicy().then(
      (policy) => {
        if (policyReadTokenRef.current === readToken) {
          setContentSourcePolicy(policy);
        }
      },
      () => {
        if (policyReadTokenRef.current === readToken) {
          setContentSourcePolicy(null);
        }
      },
    );
  };

  // Local-artifact import flow (file → library). USER-TRIGGERED via the
  // "Importer une histoire" CTA — the hook stays idle until `pickAndAnalyze`.
  // AC1: analysis never mutates; the overview is re-read only after a
  // successful commit (the hook already dropped the module cache; the effect
  // below reloads THIS route so the fresh card appears).
  const storyImport = useStoryImport();
  const isImportBusy =
    storyImport.status.kind === "analyzing" ||
    storyImport.status.kind === "importing";
  const importStatusKind = storyImport.status.kind;
  useEffect(() => {
    if (importStatusKind === "imported") {
      invalidate();
    }
  }, [importStatusKind, invalidate]);

  // Structured-folder creation flow (folder → new canonical story).
  // USER-TRIGGERED from the creation dialog's secondary entry ("Choisir un
  // dossier…"). Analysis never mutates (AC4); the overview reloads only
  // after a successful creation — the fresh card (with its possible marker)
  // IS the sober success feedback; the editor is NOT auto-opened.
  const structuredCreation = useStructuredCreation();
  const isCreateFromFolderBusy =
    structuredCreation.status.kind === "analyzing" ||
    structuredCreation.status.kind === "creating";
  const structuredCreationStatusKind = structuredCreation.status.kind;
  useEffect(() => {
    if (structuredCreationStatusKind === "created") {
      invalidate();
    }
  }, [structuredCreationStatusKind, invalidate]);

  // RSS external-source creation flow (feed → new canonical draft).
  // USER-TRIGGERED from the creation dialog's third entry ("Démarrer depuis
  // une source externe (RSS)"). The preview never mutates; the overview
  // reloads only after a successful creation — the fresh card with its
  // `à revoir` / `partiel` chip IS the sober success feedback; the editor
  // is NOT auto-opened. The surface owns the address field, so the route
  // only tracks whether it is open.
  const rssCreation = useRssCreation();
  const [isRssCreationOpen, setIsRssCreationOpen] = useState(false);
  // ACTIVE covers the whole lifetime of the flow (surface open, or any
  // non-idle machine state): the cross-flow busy exclusivity must keep a
  // second creation/import surface from stacking on top of a live RSS
  // review, not only during the two in-flight operations.
  const isRssCreationActive =
    isRssCreationOpen || rssCreation.status.kind !== "idle";
  const rssCreationStatusKind = rssCreation.status.kind;
  useEffect(() => {
    if (rssCreationStatusKind === "created") {
      invalidate();
    }
  }, [rssCreationStatusKind, invalidate]);

  const handleRssAbandon = (): void => {
    // A pure frontend reset: nothing was mutated. Closing the surface and
    // resetting the machine keeps the next opening on a clean slate.
    rssCreation.abandon();
    setIsRssCreationOpen(false);
  };

  const handleRssDismiss = (): void => {
    rssCreation.dismiss();
    setIsRssCreationOpen(false);
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

  const {
    deviceState,
    deviceLabel,
    deviceReason,
    supportedOperations,
    deviceFamily,
  } = mapDeviceForPanel(device.state, device.isRefreshing);

  // Derive the device whose library we may read: a supported device
  // that is read-authorized (the capability matrix decides — a FLAM
  // Gen1 carries readLibrary=true and reads through this same gate; a
  // future zero-capability profile never reads). Fall back to the
  // cached snapshot so the device section survives a background
  // detection refresh (SWR). `null` ⇒ the device-library hook stays
  // idle and issues no IPC.
  const effectiveDevice: ConnectedDeviceDto | null =
    device.state.kind === "ready" ? device.state.device : device.cached;
  const readableDeviceId =
    effectiveDevice &&
    effectiveDevice.kind === "supported" &&
    effectiveDevice.supportedOperations.readLibrary
      ? effectiveDevice.deviceIdentifier
      : null;
  // The device whose WRITE gate is open: a supported device that is
  // write-authorized (Lunii V1/V2 in MVP; V3 and recognized FLAM stay
  // non-writable — the authoritative capability matrix decides, never
  // the cohort or family name). `null` ⇒ no write target.
  const writableDeviceId =
    effectiveDevice &&
    effectiveDevice.kind === "supported" &&
    effectiveDevice.supportedOperations.writeStory
      ? effectiveDevice.deviceIdentifier
      : null;
  const deviceLibrary = useDeviceLibrary(readableDeviceId);

  // Pre-transfer comparison (read-only). Composed in Rust and only presented:
  // trigger it ONLY for a single local selection against a readable device;
  // the hook stays idle (no IPC) otherwise. Keyed on the selected story id and
  // the device identifier, so a selection change or a device swap re-reads.
  const singleSelectedStoryId =
    presentSelectedIds.size === 1 ? [...presentSelectedIds][0] : null;
  const transferPreview = useTransferPreview(
    singleSelectedStoryId,
    readableDeviceId,
    deviceFamily,
  );
  // Distinguish WHY there is no comparison so the hint is actionable: the
  // route knows the cause (no/multi selection, or no readable device) that
  // the hook's `idle` cannot tell apart.
  const transferComparison: TransferComparisonView =
    presentSelectedIds.size === 0
      ? { kind: "none", reason: "no-selection" }
      : presentSelectedIds.size > 1
        ? { kind: "none", reason: "multi-selection" }
        : readableDeviceId === null
          ? { kind: "none", reason: "no-device" }
          : mapTransferPreviewToComparison(transferPreview.state);

  // Pre-transfer validation verdict (read-only). Composed in Rust and only
  // presented. Same gating as the comparison: trigger ONLY for a single local
  // selection against a readable device; the hook stays idle (no IPC) otherwise.
  // The verdict is ORTHOGONAL to the send gate — the CTA stays disabled below.
  const storyValidation = useStoryValidation(
    singleSelectedStoryId,
    readableDeviceId,
    deviceFamily,
  );
  const validationView: StoryValidationView =
    singleSelectedStoryId === null || readableDeviceId === null
      ? { kind: "none" }
      : mapStoryValidationToView(storyValidation.state);

  // Pre-transfer preparation (LOCAL, orthogonal to the send gate). USER-TRIGGERED
  // via the Préparer CTA — the hook stays idle until `prepare()`. Same gating
  // pair (single selection + readable device) as the comparison / validation;
  // the CTA is enabled only when the verdict is `présumée transférable`.
  // Tracks ONE preparation, independent of the selection — an in-flight job or a
  // recoverable failure stays consultable when the user selects another story
  // (AC2). The panel reflects it only while its target story is selected.
  const storyPreparation = useStoryPreparation();
  const deviceAvailability: DeviceAvailability =
    readableDeviceId !== null
      ? "readable"
      : effectiveDevice !== null
        ? "unsupported"
        : "absent";
  const preparationView: PreparationView = mapPreparationView(
    storyPreparation.state,
    singleSelectedStoryId,
    presentSelectedIds.size,
    deviceAvailability,
    validationView,
  );
  const handlePrepareSelected = (): void => {
    if (singleSelectedStoryId && readableDeviceId) {
      storyPreparation.prepare(singleSelectedStoryId, readableDeviceId);
    }
  };

  // Pre-write transfer (real device WRITE, AC1/AC2/AC3). USER-TRIGGERED via the
  // Envoyer CTA — the hook stays idle until `send()`. The send gate is
  // FAIL-CLOSED: enabled only on a writable cohort (V1/V2) + a `Préparée` story
  // + a single clear target; everything else is a standardized "Envoi
  // indisponible: …" reason. Tracks ONE transfer, independent of the selection
  // (an in-flight write / recoverable failure stays consultable via its badge).
  const storyTransfer = useStoryTransfer();
  const preparedForSelected =
    storyPreparation.state.kind === "prepared" &&
    storyPreparation.state.storyId === singleSelectedStoryId
      ? storyPreparation.state
      : null;
  // A `prepared` story is sendable ONLY to the device it was prepared for (F6): a
  // story prepared for one target must be re-prepared before it can be sent to
  // another, so a device swap can never send a stale descriptor to the wrong Lunii.
  const selectedStoryPrepared =
    preparedForSelected !== null &&
    writableDeviceId !== null &&
    preparedForSelected.deviceIdentifier === writableDeviceId;
  // A native story (no device-format pack) is `prepared` but NOT transferable —
  // the send gate disables `Envoyer` with a dedicated reason before any write.
  const selectedStoryTransferable =
    preparedForSelected !== null &&
    writableDeviceId !== null &&
    preparedForSelected.deviceIdentifier === writableDeviceId &&
    preparedForSelected.transferable;
  const transferView: TransferView = mapTransferView(
    storyTransfer.state,
    singleSelectedStoryId,
    presentSelectedIds.size,
    deviceState,
    writableDeviceId !== null,
    selectedStoryPrepared,
    selectedStoryTransferable,
  );
  const handleSendSelected = (): void => {
    if (singleSelectedStoryId && writableDeviceId) {
      storyTransfer.send(singleSelectedStoryId, writableDeviceId);
    }
  };

  // Re-hydrate the durable transfer memory for the selected story (Transfer Resume
  // Contract / AC2): on selecting a story, re-offer any remembered NON-success
  // terminal (`échec récupérable` / `transfert incomplet` / `état partiel`) with
  // `Relancer` / `Abandonner`, exactly as if the `job:failed` had just fired —
  // surviving an app restart and a re-visit. The hook reconciles with the live
  // read (a remembered `verified` is never shown as a live success), never
  // disturbs an in-flight write, and treats a read failure as "no memory".
  const hydrateTransfer = storyTransfer.hydrate;
  useEffect(() => {
    if (singleSelectedStoryId) {
      // Pass the writable device id so the hook reconciles with the LIVE read: a
      // device that proves the pack (live `verified`) always wins over the memory,
      // and a remembered `verified` is never shown as a live success without proof.
      hydrateTransfer(singleSelectedStoryId, writableDeviceId);
    }
  }, [singleSelectedStoryId, writableDeviceId, hydrateTransfer]);

  // Reflect the in-flight / failed preparation as a discreet card badge (AC2),
  // keyed on the job's TARGET story (from the hook state) — never the current
  // selection — so it survives the user selecting another story. The panel stays
  // the authoritative surface; this is a derived signal.
  const preparationBadges = useMemo(() => {
    const map = new Map<string, StoryPreparationBadge>();
    const prep = storyPreparation.state;
    if (prep.kind === "preflight" || prep.kind === "preparing") {
      map.set(prep.storyId, "preparing");
    } else if (prep.kind === "retryable") {
      map.set(prep.storyId, "retryable");
    }
    // A transfer badge takes precedence for its target story — a write in flight
    // (or its terminal verdict) is past preparation. The verdicts are sticky
    // anchors across selection changes (the panel restores the full context on
    // re-select).
    const tx = storyTransfer.state;
    if (tx.kind === "transferring") {
      map.set(tx.storyId, "transferring");
    } else if (tx.kind === "verified") {
      map.set(tx.storyId, "verified");
    } else if (tx.kind === "partial") {
      map.set(tx.storyId, "partial");
    } else if (tx.kind === "retryable") {
      map.set(tx.storyId, "retryable");
    } else if (tx.kind === "incomplete") {
      map.set(tx.storyId, "incomplete");
    }
    return map;
  }, [storyPreparation.state, storyTransfer.state]);

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

  // Device-story naming flow (Phase B). A purely local write keyed by pack
  // UUID; on success the device inventory re-reads so the new title surfaces
  // from the single Rust-owned resolution (a user title outranks any later
  // recognition). No device capability gates it — it is local, not a device
  // operation — but the inspector only renders for a selected device story.
  const deviceTitle = useDeviceStoryTitle({
    onTitled: () => {
      deviceLibrary.refresh();
    },
  });
  const handleSetDeviceStoryTitle = (
    packUuid: string,
    title: string,
  ): Promise<boolean> => deviceTitle.setTitle(packUuid, title);
  // Scope the naming status to the card it actually belongs to, exactly like
  // the import status (`targetPackUuid`).
  const selectedDeviceTitleState =
    selectedDeviceStoryUuid !== null &&
    selectedDeviceStoryUuid === deviceTitle.targetPackUuid
      ? deviceTitle.status
      : undefined;

  // Official-catalog management (Phase C). Global (not device-specific):
  // caching the commercial index recognizes packs even before a device is
  // plugged in. Offline-first — only the on-mount count read runs without a
  // deliberate user action. On a cache change, re-read the displayed device
  // inventory so freshly recognized titles appear immediately.
  const officialCatalog = useOfficialCatalog({
    onChanged: () => {
      // Covers may have changed too — drop the cover cache so the re-read
      // resolves the fresh covers from the local cache.
      invalidatePackCoverCache();
      deviceLibrary.refresh();
    },
  });

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
      <ImportArtifactSurface
        status={storyImport.status}
        onAccept={storyImport.acceptRecognized}
        onAbandon={storyImport.abandon}
        onRetry={storyImport.pickAndAnalyze}
        onDismiss={storyImport.dismiss}
      />
      <CreateFromFolderSurface
        status={structuredCreation.status}
        onAccept={structuredCreation.acceptCreation}
        onAbandon={structuredCreation.abandon}
        onRetry={structuredCreation.pickAndAnalyze}
        onDismiss={structuredCreation.dismiss}
      />
      <CreateFromRssSurface
        open={isRssCreationOpen}
        status={rssCreation.status}
        onFetch={(url) => {
          void rssCreation.fetchPreview(url);
        }}
        onSelectItem={rssCreation.selectItem}
        onAccept={() => {
          void rssCreation.acceptCreation();
        }}
        onAbandon={handleRssAbandon}
        onDismiss={handleRssDismiss}
      />
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
        preparationBadges,
        storyImport.pickAndAnalyze,
        isImportBusy || isCreateFromFolderBusy || isRssCreationActive,
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
              onConsultSupportProfile={openSupportProfile}
              onSetTitle={handleSetDeviceStoryTitle}
              titleState={selectedDeviceTitleState}
              onDismissTitleError={deviceTitle.reset}
            />
            <LuniiDecisionPanel
              deviceState={deviceState}
              deviceLabel={deviceLabel}
              deviceReason={deviceReason}
              supportedOperations={supportedOperations}
              deviceFamily={deviceFamily}
              selectedCount={presentSelectedIds.size}
              comparison={transferComparison}
              onRetryComparison={transferPreview.refresh}
              validation={validationView}
              onRetryValidation={storyValidation.refresh}
              preparation={preparationView}
              onPrepare={handlePrepareSelected}
              onRetryPreparation={storyPreparation.retry}
              transfer={transferView}
              onSend={handleSendSelected}
              onRetryTransfer={
                writableDeviceId !== null ? handleSendSelected : undefined
              }
              onDismissTransfer={storyTransfer.dismiss}
              onEdit={handleEditSelected}
              onRefreshDevice={device.refresh}
              onConsultSupportProfile={openSupportProfile}
            />
            <CatalogPanel catalog={officialCatalog} />
          </>
        }
      />
      <CreateStoryDialog
        open={isCreateOpen}
        onClose={() => setIsCreateOpen(false)}
        onCreated={handleCreated}
        onCreateFromFolderRequest={() => {
          void structuredCreation.pickAndAnalyze();
        }}
        isCreateFromFolderUnavailable={
          isImportBusy || isCreateFromFolderBusy || isRssCreationActive
        }
        onCreateFromRssRequest={() => {
          setIsRssCreationOpen(true);
        }}
        isCreateFromRssUnavailable={
          isImportBusy || isCreateFromFolderBusy || isRssCreationActive
        }
        contentSourcePolicy={contentSourcePolicy}
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
  /** Family of the detected supported device — drives only the
   *  family-correct transfer capability label in the panel. */
  deviceFamily?: SupportedFamilyDto;
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

/**
 * Pure mapper from the `useTransferPreview` state to the `comparison` prop
 * `LuniiDecisionPanel` expects. Pure so it stays testable in isolation. Only
 * reached when a single story is selected against a readable device; `idle`
 * therefore means the live re-read folded away (`noDevice` / `unsupported`)
 * — surfaced as the sober `no-device` hint, never an error.
 */
export function mapTransferPreviewToComparison(
  state: ReturnType<typeof useTransferPreview>["state"],
): TransferComparisonView {
  switch (state.kind) {
    case "idle":
      return { kind: "none", reason: "no-device" };
    case "loading":
      return { kind: "loading" };
    case "ready":
      return {
        kind: "ready",
        onDevice: state.onDevice,
        unchangedCount: state.unchangedCount,
      };
    case "error":
      return { kind: "error", error: state.error };
  }
}

/**
 * Pure mapper from the `useStoryValidation` state to the `validation` prop
 * `LuniiDecisionPanel` expects. Pure so it stays testable in isolation. Only
 * reached when a single story is selected against a readable device; `idle`
 * therefore means the live re-read folded away — surfaced as the sober `none`
 * state, never an error.
 */
export function mapStoryValidationToView(
  state: ReturnType<typeof useStoryValidation>["state"],
): StoryValidationView {
  switch (state.kind) {
    case "idle":
      return { kind: "none" };
    case "loading":
      return { kind: "loading" };
    case "ready":
      return {
        kind: "ready",
        verdict: state.verdict,
        blockers: state.blockers,
      };
    case "error":
      return { kind: "error", error: state.error };
  }
}

/** Disposition of the connected device for the preparation gate: a readable
 *  supported Lunii, a device present but not read-authorized, or nothing. */
type DeviceAvailability = "readable" | "unsupported" | "absent";

/**
 * Pure mapper from the `useStoryPreparation` state (+ the gating context) to the
 * `preparation` prop `LuniiDecisionPanel` expects. Pure so it stays testable in
 * isolation. An active / terminal preparation shows its own state; an idle hook
 * shows the `Préparer` CTA, enabled only for a single selection + a readable
 * device + a `présumée transférable` verdict, else disabled with the
 * standardized "Préparation indisponible: …" reason.
 */
export function mapPreparationView(
  state: ReturnType<typeof useStoryPreparation>["state"],
  selectedStoryId: string | null,
  selectionCount: number,
  deviceAvailability: DeviceAvailability,
  validation: StoryValidationView,
): PreparationView {
  // An active / terminal preparation is shown ONLY for the story it targets, so
  // it stays consultable while that story is selected and the panel reflects the
  // CURRENT selection's gate otherwise (the badge keeps the other story flagged).
  if (state.kind !== "idle" && state.storyId === selectedStoryId) {
    switch (state.kind) {
      case "preflight":
        return { kind: "preflight" };
      case "preparing":
        return { kind: "preparing", progress: state.progress };
      case "prepared":
        return { kind: "prepared" };
      case "retryable":
        return {
          kind: "retryable",
          message: state.message,
          userAction: state.userAction,
          blockers: state.blockers,
        };
      case "error":
        return { kind: "error", error: state.error };
    }
  }
  if (selectionCount === 0) {
    return {
      kind: "unavailable",
      reason: "Préparation indisponible: aucune histoire sélectionnée",
    };
  }
  if (selectionCount > 1) {
    return {
      kind: "unavailable",
      reason: "Préparation indisponible: sélection multiple",
    };
  }
  if (deviceAvailability === "absent") {
    return {
      kind: "unavailable",
      reason: "Préparation indisponible: aucun appareil connecté",
    };
  }
  if (deviceAvailability === "unsupported") {
    return {
      kind: "unavailable",
      reason: "Préparation indisponible: profil non supporté",
    };
  }
  if (
    validation.kind === "ready" &&
    validation.verdict === "presumedTransferable"
  ) {
    return { kind: "ready" };
  }
  // Any non-passing or still-pending verdict: repair the blocks first.
  return {
    kind: "unavailable",
    reason: "Préparation indisponible: corrige les blocages d'abord",
  };
}

/**
 * Pure mapper from the `useStoryTransfer` state (+ the send-gate context) to the
 * `transfer` prop `LuniiDecisionPanel` expects. Pure so it stays testable in
 * isolation. FAIL-CLOSED: the `Envoyer vers la Lunii` CTA is `ready` ONLY for a
 * single selection + a write-authorized device (V1/V2) + a `Préparée` story;
 * every other case is a standardized "Envoi indisponible: …" reason. An active /
 * terminal transfer is shown ONLY for the story it targets (so it stays
 * consultable while that story is selected; the card badge keeps the other story
 * flagged). The success terminal `verified` (`transférée et vérifiée`) appears
 * only after the verify proof; `partial` / the verify `failed` verdict are honest
 * non-successes.
 *
 * A native story (no device-format pack) is detected BEFORE click via the
 * prepared state's `transferable` flag and disables the CTA with its own reason.
 * A stale descriptor (the prepared device has since changed) still cannot be told
 * apart client-side, so it is enforced by the BACKEND as a `retryable` terminal
 * (cause `deviceChanged`), surfaced in-context.
 */
export function mapTransferView(
  state: ReturnType<typeof useStoryTransfer>["state"],
  selectedStoryId: string | null,
  selectionCount: number,
  deviceState: LuniiDeviceState,
  writable: boolean,
  prepared: boolean,
  transferable: boolean,
): TransferView {
  // The active / failure terminal is shown in the panel ONLY for the SELECTED
  // target story. The StoryCard badge is the persistent anchor across selection
  // changes; the full panel context (alert + Relancer/Abandonner) is restored by
  // re-selecting the faulty story (C5/T7).
  if (state.kind !== "idle" && state.storyId === selectedStoryId) {
    switch (state.kind) {
      case "transferring":
        // The final `verify` phase gets its own TRANSIENT "écriture effectuée —
        // vérification à venir" view, distinct from the calm "en transfert".
        return state.phase === "verify"
          ? { kind: "verifying" }
          : { kind: "transferring", progress: state.progress, phase: state.phase };
      case "verified":
        return {
          kind: "verified",
          changed: state.summary.changed,
          unchanged: state.summary.unchanged,
        };
      case "partial":
        return {
          kind: "partial",
          message: state.message,
          userAction: state.userAction,
        };
      case "retryable":
        return {
          kind: "retryable",
          message: state.message,
          userAction: state.userAction,
        };
      case "incomplete":
        return {
          kind: "incomplete",
          message: state.message,
          userAction: state.userAction,
        };
      case "error":
        return { kind: "error", error: state.error };
    }
  }
  // Single-flight (F4): while a transfer is in flight for ANY story, refuse to
  // start another — the hook tracks one job and the device volume must never see
  // two concurrent writes. The selected-and-transferring case already returned in
  // the branch above; every OTHER selection is blocked here.
  if (state.kind === "transferring") {
    return {
      kind: "unavailable",
      reason: "Envoi indisponible: un transfert est déjà en cours",
    };
  }
  if (selectionCount === 0) {
    return {
      kind: "unavailable",
      reason: "Envoi indisponible: aucune histoire sélectionnée",
    };
  }
  if (selectionCount > 1) {
    return {
      kind: "unavailable",
      reason: "Envoi indisponible: sélection multiple",
    };
  }
  if (!writable) {
    return { kind: "unavailable", reason: formatSendDeviceReason(deviceState) };
  }
  if (!prepared) {
    return {
      kind: "unavailable",
      reason: "Envoi indisponible: prépare l'histoire d'abord",
    };
  }
  if (!transferable) {
    return {
      kind: "unavailable",
      reason:
        "Envoi indisponible: histoire native non transférable (pas de pack appareil)",
    };
  }
  return { kind: "ready" };
}

/** Standardized "Envoi indisponible: …" reason for a non-writable device. A
 *  supported-but-not-writable device (`idle`, i.e. V3 in MVP) reports "profil non
 *  supporté" — the write capability, not the cohort name, is authoritative. */
function formatSendDeviceReason(state: LuniiDeviceState): string {
  switch (state) {
    case "absent":
      return "Envoi indisponible: aucun appareil connecté";
    case "idle":
    case "unsupported":
      return "Envoi indisponible: profil non supporté";
    case "ambiguous":
      return "Envoi indisponible: profil ambigu";
    case "scanning":
      return "Envoi indisponible: détection en cours";
    case "error":
      return "Envoi indisponible: détection en échec";
  }
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
        deviceFamily: dto.family,
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

function formatSupportedLabel(cohort: FirmwareCohortDto): string {
  switch (cohort) {
    case "origineV1":
      return "Lunii Origine";
    case "midGenV2":
      return "Lunii";
    case "v3":
      return "Lunii V3";
    case "flamGen1":
      return "FLAM";
  }
}

function formatUnsupportedReason(
  reason: string,
  hint: string | null,
): string {
  switch (reason) {
    case "metadataUnsupported":
      // Only a genuine version hint (`metadata_v{n}`, the Lunii
      // classifier's shape) is rendered as a version. Any other hint —
      // e.g. the FLAM family tag `"flam"` carried by an incomplete
      // FLAM structure (`str/`/`etc/` missing) — would read as a fake
      // version (`format métadonnées flam non géré`), so it folds into
      // the standard copy instead.
      return hint && /^metadata_v\d+$/.test(hint)
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
  preparationBadges: ReadonlyMap<string, StoryPreparationBadge>,
  onImportArtifactRequest: () => void,
  isImportBusy: boolean,
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
          preparationBadges={preparationBadges}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onCreateStoryRequest={onCreateStoryRequest}
          onImportArtifactRequest={onImportArtifactRequest}
          isImportBusy={isImportBusy}
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
          preparationBadges={preparationBadges}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onCreateStoryRequest={onCreateStoryRequest}
          onImportArtifactRequest={onImportArtifactRequest}
          isImportBusy={isImportBusy}
        />
      );
  }
}
