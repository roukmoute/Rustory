import type React from "react";
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { Outlet, useLocation, useMatch, useNavigate } from "react-router-dom";

import {
  DeviceBulkImportPanel,
  type DeviceStorySelectionMode,
  DeviceStoryInspector,
  type DevicePackSendStatus,
  useConnectedLunii,
  useDeviceBulkImport,
  useDeviceLibrary,
  useDevicePackSend,
  useDeviceStoryDelete,
  useDeviceStoryImport,
  useDeviceStoryReorder,
  useDeviceStoryTitle,
  useStoryValidation,
  useTransferPreview,
} from "../../features/device";
import { useOnboarding } from "../../features/onboarding/use-onboarding";
import { WelcomeScreen } from "../../features/onboarding/components/WelcomeScreen";
import { DeviceDiscoveryScreen } from "../../features/onboarding/components/DeviceDiscoveryScreen";
import { RecoveryProgress } from "../../features/onboarding/components/RecoveryProgress";
import { DeviceScreen } from "../../features/device/components/DeviceScreen";
import { CreateStoryDialog } from "../../features/library/components/CreateStoryDialog";
import { LibraryErrorBanner } from "../../features/library/components/LibraryErrorBanner";
import { LibraryFiltersNav } from "../../features/library/components/LibraryFiltersNav";
import {
  LuniiDecisionPanel,
  TransferStatus,
  type LuniiDeviceState,
  type StoryValidationView,
  type TransferComparisonView,
  type TransferView,
} from "../../features/library/components/LuniiDecisionPanel";
import { useStoryTransfer } from "../../features/transfer";
import { CreateFromArchiveSurface } from "../../features/import-export/components/CreateFromArchiveSurface";
import { DropAnalyzingOverlay } from "../../features/import-export/components/DropAnalyzingOverlay";
import { CreateFromFolderSurface } from "../../features/import-export/components/CreateFromFolderSurface";
import { CreateFromRssSurface } from "../../features/import-export/components/CreateFromRssSurface";
import { CreateFromWebSurface } from "../../features/import-export/components/CreateFromWebSurface";
import { ImportArtifactSurface } from "../../features/import-export/components/ImportArtifactSurface";
import { StoryArchiveExportSurface } from "../../features/import-export/components/StoryArchiveExportSurface";
import {
  discardDropRequest,
  discardOsOpenRequest,
  readContentSourcePolicy,
} from "../../ipc/commands/import-export";
import { useArchiveCreation } from "../../features/import-export/hooks/use-archive-creation";
import { useRssCreation } from "../../features/import-export/hooks/use-rss-creation";
import { useStoryArchiveExport } from "../../features/import-export/hooks/use-story-archive-export";
import { useWebCreation } from "../../features/import-export/hooks/use-web-creation";
import { useStoryImport } from "../../features/import-export/hooks/use-story-import";
import { useStructuredCreation } from "../../features/import-export/hooks/use-structured-creation";
import type { StoryPreparationBadge } from "../../features/library/components/StoryCard";
import { StoryCollection } from "../../features/library/components/StoryCollection";
import { UpdateAvailabilitySignal } from "../../features/settings/components/UpdateAvailabilitySignal";
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
import { deleteStories } from "../../ipc/commands/story";
import { toAppError } from "../../shared/errors/app-error";
import type { DeviceStoryDto } from "../../shared/ipc-contracts/device-library";
import type { ContentSourcePolicy } from "../../shared/ipc-contracts/import-export";
import type {
  SendBlocker,
  StoryCardDto,
} from "../../shared/ipc-contracts/library";
import {
  Button,
  ContextMenu,
  Dialog,
  Icon,
  SurfacePanel,
} from "../../shared/ui";
import { SelectionActionBar } from "../../features/library/components/SelectionActionBar";
import { DeviceImportStatusSurface } from "../../features/device/components/DeviceImportStatusSurface";
import { getDeviceStoryCopyAvailability } from "../../features/device/device-story-copy";
import { LibraryLayout } from "../../shell/layout/LibraryLayout";
import { useDropShell } from "../../shell/state/drop-shell-store";
import {
  useDraftsShell,
  useLibraryShell,
} from "../../shell/state/library-shell-store";
import { useOsOpenShell } from "../../shell/state/os-open-shell-store";

import "./LibraryRoute.css";

/**
 * Frozen calm-limit copy when an OS-open intent arrives while a library
 * flow (import / creation / transfer) is in flight (`product-language.md`).
 * A frontend-owned SURFACE literal, typed exactly once: the living flow is
 * never interrupted, the intent is discarded, the user reopens the file.
 */
const OS_OPEN_BUSY_NOTICE =
  "Une opération est déjà en cours dans la bibliothèque. Termine-la, puis rouvre le fichier.";

/**
 * Frozen calm-limit copy when a drop intent arrives while a library flow
 * is in flight (`product-language.md`). The OS-open busy copy's SISTER
 * literal — same family, deliberately distinct words (reopening ≠
 * dropping): the living flow is never interrupted, the intent is
 * discarded, the user drops the element again afterwards.
 */
const DROP_BUSY_NOTICE =
  "Une opération est déjà en cours dans la bibliothèque. Termine-la, puis dépose à nouveau ton fichier ou ton dossier.";

/** Stable empty device selection so an "empty" render keeps referential
 *  identity (no new Set() per render feeding the purge effect's deps). */
const EMPTY_DEVICE_SELECTION: ReadonlySet<string> = new Set();
const EMPTY_STORY_ID_SET: ReadonlySet<string> = new Set();

export function LibraryRoute(): React.JSX.Element {
  const { state, retry, invalidate } = useLibraryOverview();
  const device = useConnectedLunii();
  const onboarding = useOnboarding();
  const isWelcome = !!useMatch("/welcome");
  const isDiscovery = !!useMatch("/welcome/device");
  const isOnboardingScreen = isWelcome || isDiscovery;
  const [onboardingRecovery, setOnboardingRecovery] = useState(false);
  const isDraftsScreen = !!useMatch("/drafts");
  const libraryShell = useLibraryShell();
  const draftsShell = useDraftsShell();
  const {
    selectedStoryIds,
    selectStory,
    clearSelection,
    query,
    sort,
    setQuery,
    setSort,
    resetFilters,
  } = isDraftsScreen ? draftsShell : libraryShell;
  const pruneSelection = libraryShell.pruneSelection;
  const pruneDraftSelection = draftsShell.pruneSelection;
  const navigate = useNavigate();
  const location = useLocation();
  const currentPathRef = useRef(location.pathname);
  currentPathRef.current = location.pathname;

  // The `Consulter le profil de support` gesture navigates IN-APP to
  // the support-profile screen (`Support Profile Screen Contract`) —
  // no external browser, no network (NFR14). The three consuming
  // surfaces keep their `onConsultSupportProfile` prop unchanged.
  const openSupportProfile = (): void => {
    navigate("/settings");
  };

  const activeSection = useMatch("/device")
    ? "device"
    : isDraftsScreen
      ? "drafts"
      : "library";
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [operationDetails, setOperationDetails] = useState<
    "transfer" | "copy" | null
  >(null);
  const [deviceQuery, setDeviceQuery] = useState("");
  const [deviceView, setDeviceView] = useState<"grid" | "list">("grid");
  const [collectionViews, setCollectionViews] = useState<{
    library: "grid" | "list";
    drafts: "grid" | "list";
  }>({ library: "grid", drafts: "grid" });
  const collectionKey = isDraftsScreen ? "drafts" : "library";
  const collectionView = collectionViews[collectionKey];
  const setCollectionView = (view: "grid" | "list"): void =>
    setCollectionViews((current) => ({ ...current, [collectionKey]: view }));
  const [createInitialStep, setCreateInitialStep] = useState<
    "choice" | "import"
  >("choice");
  const [isCreateOpen, setIsCreateOpen] = useState<boolean>(false);
  // Mirror of the creation dialog's INTERNAL submission state, reported
  // upward so the primary title creation joins the flows' mutual
  // exclusion (an OS-open intent arriving mid-submission is declined
  // calmly, never interleaved with the create → navigate sequence).
  const [isCreateSubmitting, setIsCreateSubmitting] = useState<boolean>(false);
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
    pruneDraftSelection(presentIds);
  }, [overview, presentIds, pruneSelection, pruneDraftSelection]);

  // Derive a "present selection" for the same render that reads the fresh
  // overview: if a stored id is no longer in the library, we MUST NOT let
  // it activate Éditer before the prune effect flushes on the next tick.
  const presentSelectedIds = useMemo(() => {
    if (!overview) return selectedStoryIds;
    const visibleIds = isDraftsScreen
      ? new Set(
          overview.stories.filter(isCreatedStory).map((story) => story.id),
        )
      : presentIds;
    if ([...selectedStoryIds].every((id) => visibleIds.has(id))) {
      return selectedStoryIds;
    }
    return new Set([...selectedStoryIds].filter((id) => visibleIds.has(id)));
  }, [overview, selectedStoryIds, presentIds, isDraftsScreen]);

  const handleOpenStory = useMemo(
    () => (id: string) => {
      // `replace` keeps one library entry in history instead of stacking a
      // new one at every open — back button returns to the true previous
      // context, not to the library-we-just-left.
      navigate(`/story/${encodeURIComponent(id)}/edit`, {
        replace: true,
        state: { collection: isDraftsScreen ? "/drafts" : "/library" },
      });
    },
    [navigate, isDraftsScreen],
  );

  const handleEditSelected = (): void => {
    if (presentSelectedIds.size !== 1) return;
    const [id] = presentSelectedIds;
    handleOpenStory(id);
  };

  // Deletion of the confirmed selection. All-or-nothing on the Rust side:
  // a rejection means the library was NOT touched, so the selection is
  // kept for a retry; a success clears it and re-reads the overview.
  const [deleteState, setDeleteState] = useState<
    | { kind: "idle" }
    | { kind: "deleting" }
    | { kind: "failed"; message: string }
  >({ kind: "idle" });

  // Core deletion, shared by the panel's Supprimer and the card context
  // menu. All-or-nothing on the Rust side: a rejection leaves the library
  // untouched, so the selection is kept for a retry; success clears it and
  // re-reads the overview.
  const deleteIds = async (ids: string[]): Promise<void> => {
    if (
      ids.length === 0 ||
      deleteState.kind === "deleting" ||
      deviceMutationInFlight
    )
      return;
    setDeleteState({ kind: "deleting" });
    try {
      await deleteStories({ ids });
      clearSelection();
      setDeleteState({ kind: "idle" });
      invalidate();
    } catch (err) {
      setDeleteState({ kind: "failed", message: toAppError(err).message });
    }
  };

  const handleDeleteSelected = (): Promise<void> =>
    deleteIds([...presentSelectedIds]);

  // Right-click context menu on a library card (id + cursor position), and
  // the confirmation for its destructive "Supprimer" action.
  const [storyMenu, setStoryMenu] = useState<{
    id: string;
    x: number;
    y: number;
  } | null>(null);
  const [confirmDeleteIds, setConfirmDeleteIds] = useState<string[] | null>(
    null,
  );
  const confirmDeleteId = useId();
  const confirmDeleteDeviceId = useId();
  const handleStoryContextMenu = (id: string, x: number, y: number): void => {
    setStoryMenu({ id, x, y });
  };

  const handleCreateStoryRequest = (
    step: "choice" | "import" = "choice",
  ): void => {
    setCreateInitialStep(step);
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
  // `isOsOpenSettling` / `isDropSettling` join the busy set: those pulls
  // render no transient state (silent by contract) but ARE in-flight
  // operations — no sibling flow may start under a live channel read.
  const storyImport = useStoryImport();
  const isImportBusy =
    storyImport.status.kind === "analyzing" ||
    storyImport.status.kind === "importing" ||
    storyImport.isOsOpenSettling ||
    storyImport.isDropSettling;
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

  // Structured-ARCHIVE creation flow (community .zip pack → new canonical
  // story). USER-TRIGGERED from the creation dialog's archive entry
  // ("Choisir une archive de pack (.zip)…"). Picker-only channel — the
  // drop/os-open routing of archives is a separate extension.
  const archiveCreation = useArchiveCreation();
  // The shareable-archive export of a library story. Strictly read-only on
  // the library: it never invalidates the overview, whatever its outcome.
  const archiveExport = useStoryArchiveExport();
  const isCreateFromArchiveBusy =
    archiveCreation.status.kind === "analyzing" ||
    archiveCreation.status.kind === "creating";
  const archiveCreationStatusKind = archiveCreation.status.kind;
  useEffect(() => {
    if (archiveCreationStatusKind === "created") {
      invalidate();
    }
  }, [archiveCreationStatusKind, invalidate]);

  const handleArchiveRetry = (): void => {
    if (archiveCreation.failedPhase === "accept") {
      // The commit failed AFTER the (possibly one-shot) verdict was
      // consumed: re-run the accept with the preserved verdict — an
      // external re-pull would answer `none` and retry nothing.
      void archiveCreation.retryAccept();
    } else {
      // An analyze-phase failure only exists on the picker origin (an
      // external settlement arrives already analyzed): re-open the picker.
      void archiveCreation.pickAndAnalyze();
    }
  };

  const handleArchiveAbandon = (): void => {
    if (archiveCreation.origin === "drop") {
      // Terminal channel-wide: drop the pending Rust intent AND
      // invalidate any settlement still in flight — a late verdict must
      // never reopen the review the user just closed.
      storyImport.invalidateDropSettlements();
      void discardDropRequest().catch(() => {});
    } else if (archiveCreation.origin === "osOpen") {
      storyImport.invalidateDropSettlements();
      void discardOsOpenRequest().catch(() => {});
    }
    archiveCreation.abandon();
  };

  const handleArchiveDismiss = (): void => {
    if (archiveCreation.origin === "drop") {
      storyImport.invalidateDropSettlements();
      void discardDropRequest().catch(() => {});
    } else if (archiveCreation.origin === "osOpen") {
      storyImport.invalidateDropSettlements();
      void discardOsOpenRequest().catch(() => {});
    }
    archiveCreation.dismiss();
  };

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

  const webCreation = useWebCreation();
  const [isWebCreationOpen, setIsWebCreationOpen] = useState(false);
  const [webCreationInitialUrl, setWebCreationInitialUrl] = useState("");
  // ACTIVE covers the whole lifetime of the flow (surface open, or any
  // non-idle machine state) — same cross-flow exclusivity as the RSS
  // flow, never a stacked second creation surface.
  const isWebCreationActive =
    isWebCreationOpen || webCreation.status.kind !== "idle";
  const webCreationStatusKind = webCreation.status.kind;
  useEffect(() => {
    if (webCreationStatusKind === "created") {
      invalidate();
    }
  }, [webCreationStatusKind, invalidate]);

  const handleWebAbandon = (): void => {
    webCreation.abandon();
    setIsWebCreationOpen(false);
    setWebCreationInitialUrl("");
  };

  const handleWebDismiss = (): void => {
    webCreation.dismiss();
    setIsWebCreationOpen(false);
    setWebCreationInitialUrl("");
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
  // The device that may RECEIVE a pack archive (.zip): the DEDICATED
  // `sendArchive` capability (Lunii V3), independent of `writeStory` —
  // the authoritative matrix decides, never the cohort name. `null` ⇒
  // the send panel is not rendered.
  const sendableDeviceId =
    effectiveDevice &&
    effectiveDevice.kind === "supported" &&
    effectiveDevice.supportedOperations.sendArchive
      ? effectiveDevice.deviceIdentifier
      : null;
  const deviceLibrary = useDeviceLibrary(readableDeviceId);

  // « Sur la Lunii » stamps: the join of the two authoritative reads — the
  // device inventory says which local story each pack is a copy of
  // (`localStoryId`, composed by Rust); the library cards carry the stamp.
  // No readable device ⇒ no stamp at all (nothing to compare against).
  const onDeviceStoryIds = useMemo<ReadonlySet<string>>(() => {
    if (deviceLibrary.state.kind !== "ready") return EMPTY_STORY_ID_SET;
    return new Set(
      deviceLibrary.state.stories.flatMap((story) =>
        story.localStoryId ? [story.localStoryId] : [],
      ),
    );
  }, [deviceLibrary.state]);
  const deviceStampLabel =
    deviceFamily === "lunii" ? "Sur la Lunii" : "Sur l'appareil";

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

  // Transfer to the device (real WRITE, AC1/AC2/AC3). USER-TRIGGERED via the
  // single `Envoyer` CTA — the hook stays idle until `send()`. Preparation is
  // FOLDED IN: the write job runs the full pipeline (préflight → préparation →
  // écriture → vérification) itself, so there is no separate Préparer step.
  // The send gate is FAIL-CLOSED: enabled only on a writable cohort (V1/V2) + a
  // TRANSFERABLE story (owns a device-format pack) + a single clear target;
  // everything else is a standardized "Envoi indisponible: …" reason. Tracks ONE
  // transfer, independent of the selection (an in-flight write / recoverable
  // failure stays consultable via its card badge).
  const storyTransfer = useStoryTransfer();
  // V3 archive send — the OTHER backend behind the SAME single "Envoyer vers la
  // Lunii" CTA. Sourced from the story's retained `.zip`; on success the device
  // inventory re-reads so the new pack appears. RE-DETECT FIRST: appending to
  // `.pi` changes the content-derived device identifier, and the library hook
  // is keyed on it — the fresh identifier re-reads by itself; the direct
  // refresh below covers the identifier-unchanged case (idempotent re-send).
  // The comparison follows the same two paths: its verdict was composed
  // BEFORE the send, and a send that replaced a pack the library had not
  // tied to the story yet leaves the identifier as it was — without this
  // re-read the panel would keep announcing an addition beside its own
  // "envoyée sur la Lunii".
  const devicePackSend = useDevicePackSend({
    onSent: () => {
      device.refresh();
      deviceLibrary.refresh();
      transferPreview.refreshAfterDeviceWrite();
    },
  });
  // Two durable facts of the selected card, read straight from the overview (no
  // preparation probe): `transferable` = owns a device-format pack (the V1/V2
  // byte-copy round-trip); `sendable` = can become a V3 pack (a retained source
  // `.zip`, or a structure whose episodes all carry an audio — the transcode +
  // re-cipher send), with `sendBlocker` saying why not otherwise.
  const selectedCard =
    singleSelectedStoryId !== null
      ? overview?.stories.find((s) => s.id === singleSelectedStoryId)
      : undefined;
  const selectedStoryTransferable = selectedCard?.transferable ?? false;
  const selectedStorySendable = selectedCard?.sendable ?? false;
  const selectedStorySendBlocker = selectedCard?.sendBlocker;
  // ONE gesture, TWO backends chosen by the connected device's capability: a
  // writable V1/V2 uses the round-trip (`storyTransfer`); a send-capable V3 uses
  // the archive engine (`devicePackSend`). A device has at most one of the two.
  // Scope the V3 send status to the card it belongs to (like the delete/import
  // statuses): selecting another card never shows this one's terminal.
  // A `sent` terminal is true of ONE device: once the connected device's
  // inventory has been read (and is not being re-read), the terminal stays
  // only while that inventory lists the sent pack — another Lunii plugged in
  // after the send never inherits the first one's success. The device is the
  // truth; the post-send re-read of the same device lists the pack and keeps it.
  const sentPackStillOnDevice = (packUuid: string): boolean =>
    deviceLibrary.state.kind !== "ready" ||
    deviceLibrary.isRefreshing ||
    deviceLibrary.state.stories.some((story) => story.uuid === packUuid);
  const scopedSendStatus: DevicePackSendStatus =
    devicePackSend.targetStoryId !== null &&
    devicePackSend.targetStoryId === singleSelectedStoryId &&
    (devicePackSend.status.kind !== "sent" ||
      sentPackStillOnDevice(devicePackSend.status.packUuid))
      ? devicePackSend.status
      : { kind: "idle" };
  const transferView: TransferView =
    scopedSendStatus.kind !== "idle" ||
    (sendableDeviceId !== null &&
      !(
        storyTransfer.state.kind !== "idle" &&
        storyTransfer.state.storyId === singleSelectedStoryId
      ))
      ? mapArchiveSendToTransferView(
          scopedSendStatus,
          singleSelectedStoryId,
          presentSelectedIds.size,
          selectedStorySendable,
          selectedStorySendBlocker,
        )
      : mapTransferView(
          storyTransfer.state,
          singleSelectedStoryId,
          presentSelectedIds.size,
          deviceState,
          writableDeviceId !== null,
          selectedStoryTransferable,
        );
  const handleSendSelected = (): void => {
    if (!singleSelectedStoryId || deviceMutationInFlight) return;
    if (writableDeviceId) {
      storyTransfer.send(singleSelectedStoryId, writableDeviceId);
    } else if (sendableDeviceId) {
      void devicePackSend.triggerSend(sendableDeviceId, singleSelectedStoryId);
    }
  };
  // Dismiss / retry are backend-aware so the single CTA's terminals settle
  // through the right hook.
  const handleDismissTransfer = (): void => {
    if (scopedSendStatus.kind !== "idle") devicePackSend.dismissStatus();
    else storyTransfer.dismiss();
  };
  const canRetrySend = writableDeviceId !== null || sendableDeviceId !== null;

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

  // ===== OS-open intent reaction (`OS Open Contract`) =====
  //
  // The library is the landing context of every OS-open intent: one pull at
  // mount (covers the cold start — the intent was seeded before the
  // frontend existed), plus one pull per `os-open:requested` signal relayed
  // by the bootstrap. The verdict feeds the SAME import machine as the
  // dialog flow; the two calm limits (several files / busy flow) render
  // inline `role="status"` and never touch the machine.
  const osOpenSignal = useOsOpenShell((s) => s.pendingSignal);
  const clearOsOpenSignal = useOsOpenShell((s) => s.clear);
  const [osOpenNotice, setOsOpenNotice] = useState<string | null>(null);

  // StrictMode-safe mount flag for the OS-open settlements (the same
  // re-armed flag the import/creation hooks use): a synthetic
  // unmount+remount re-arms it, so the FIRST mount's settlement (which may
  // carry the one-shot verdict) still applies to the living component,
  // while a REAL unmount drops any late settlement — no ghost notice, no
  // ghost state.
  const osOpenMountedRef = useRef(true);
  useEffect(() => {
    osOpenMountedRef.current = true;
    return () => {
      osOpenMountedRef.current = false;
    };
  }, []);

  const analyzeFromOsOpen = storyImport.analyzeFromOsOpen;
  const injectExternalArchiveVerdict = archiveCreation.injectExternalVerdict;
  const clearExternalArchiveReview = archiveCreation.clearExternalReview;
  const handleOsOpenIntent = useCallback(async (): Promise<void> => {
    const outcome = await analyzeFromOsOpen();
    if (!osOpenMountedRef.current) return;
    if (
      outcome.kind !== "none" &&
      currentPathRef.current.startsWith("/welcome")
    ) {
      navigate("/library", { replace: true });
    }
    if (outcome.kind === "multipleFiles") {
      // Rendered VERBATIM — the copy travels in the Rust DTO.
      setOsOpenNotice(outcome.message);
      return;
    }
    if (outcome.kind === "none") return;
    // A fresh intent replaces a lingering calm limit.
    setOsOpenNotice(null);
    if (outcome.kind === "archive") {
      // An OS-opened `.zip` lands in the archive-creation machine (the
      // import machine already stepped aside inside the hook if it showed
      // an earlier OS-open surface). A commit in flight DECLINES the
      // injection: render the calm busy copy — the one-shot verdict
      // cannot be re-served; the user re-opens the file afterwards.
      if (!injectExternalArchiveVerdict(outcome.verdict, "osOpen")) {
        setOsOpenNotice(OS_OPEN_BUSY_NOTICE);
      }
      return;
    }
    // `review` / `failed` landed in the import machine: an archive
    // surface fed by an earlier external settlement steps aside — the
    // newest gesture is the only one displayed.
    clearExternalArchiveReview();
  }, [
    navigate,
    analyzeFromOsOpen,
    injectExternalArchiveVerdict,
    clearExternalArchiveReview,
  ]);

  // One-shot pull at mount. The Rust-side one-shot take makes the
  // StrictMode double-effect harmless by construction: the second pull
  // answers `none` (total no-op).
  useEffect(() => {
    void handleOsOpenIntent();
  }, [handleOsOpenIntent]);

  // A live flow has priority over an arriving intent: decline calmly,
  // consume the intent, NEVER interrupt (Pattern Priorities). The existing
  // flags are CONSULTED, not modified; the primary creation's submission
  // (reported by the dialog) joins them — a create → navigate sequence
  // must never be interleaved with an arriving intent.
  //
  // The OS-open settlement itself (`isOsOpenSettling`) is DELIBERATELY
  // absent: the channel serializes through its OWN mono-slot queue. A
  // signal landing while a pull settles must QUEUE (Rust-side the newer
  // offer already replaced the slot — the newest gesture wins), never hit
  // the busy refusal: that refusal would DISCARD the newer intent and
  // render a busy copy while no visible operation exists. The sibling-CTA
  // gates (`isImportBusy` & co) DO keep the settling flag — starting a
  // sibling flow under a live OS read stays refused.
  //
  // The DROP channel joins the gate (announced re-scope of the OS-open
  // gate: cases ADDED, none removed): an OS-open intent arriving while
  // the drop channel is ACTIVE — a drop settlement in flight, or a
  // displayed drop surface (review/failed in either machine) — is
  // declined calmly. A drop-fed review is a consumed one-shot verdict (a
  // dropped folder has no reopenable file); letting an OS-open verdict
  // replace it would destroy it silently. The dialog/picker surfaces stay
  // replaceable (the OS Open Contract behavior, unchanged).
  const isDropChannelActive =
    storyImport.isDropSettling ||
    (storyImport.origin === "drop" &&
      (storyImport.status.kind === "review" ||
        storyImport.status.kind === "failed")) ||
    (structuredCreation.origin === "drop" &&
      (structuredCreation.status.kind === "review" ||
        structuredCreation.status.kind === "failed")) ||
    (archiveCreation.origin === "drop" &&
      (archiveCreation.status.kind === "review" ||
        archiveCreation.status.kind === "failed"));
  const isOsOpenBlockedByLiveFlow =
    storyImport.status.kind === "analyzing" ||
    storyImport.status.kind === "importing" ||
    isCreateFromFolderBusy ||
    isCreateFromArchiveBusy ||
    isRssCreationActive ||
    isWebCreationActive ||
    isCreateSubmitting ||
    storyTransfer.state.kind === "transferring" ||
    isDropChannelActive;

  useEffect(() => {
    if (!osOpenSignal) return;
    clearOsOpenSignal();
    if (isOsOpenBlockedByLiveFlow) {
      void discardOsOpenRequest().catch(() => {
        // Best-effort: a failed discard leaves a pending intent the next
        // pull will surface — never a broken living flow.
      });
      setOsOpenNotice(OS_OPEN_BUSY_NOTICE);
      return;
    }
    void handleOsOpenIntent();
  }, [
    osOpenSignal,
    clearOsOpenSignal,
    isOsOpenBlockedByLiveFlow,
    handleOsOpenIntent,
  ]);

  // ===== Drop intent reaction (`Drop Intent Contract`) =====
  //
  // The library is the landing context of every drop intent — the exact
  // sibling of the OS-open block above: one pull at mount (a dormant
  // intent — cold-start regime), plus one pull per `drop:requested`
  // signal relayed by the bootstrap. A dropped FILE feeds the SAME import
  // machine; a dropped FOLDER feeds the SAME folder-creation machine (the
  // drop replaces the picker); the two calm limits (several elements /
  // busy flow) render inline `role="status"` and never touch a machine.
  const dropSignal = useDropShell((s) => s.pendingSignal);
  const clearDropSignal = useDropShell((s) => s.clearSignal);
  const [dropNotice, setDropNotice] = useState<string | null>(null);

  // StrictMode-safe mount flag, DEDICATED to the drop settlements (its own
  // channel, its own token — never shared with the OS-open one).
  const dropMountedRef = useRef(true);
  useEffect(() => {
    dropMountedRef.current = true;
    return () => {
      dropMountedRef.current = false;
    };
  }, []);

  const analyzeFromDrop = storyImport.analyzeFromDrop;
  const injectDropFolderVerdict = structuredCreation.injectDropVerdict;
  const clearDropFolderReview = structuredCreation.clearDropReview;
  const handleDropIntent = useCallback(async (): Promise<void> => {
    const outcome = await analyzeFromDrop();
    if (!dropMountedRef.current) return;
    if (
      outcome.kind !== "none" &&
      currentPathRef.current.startsWith("/welcome")
    ) {
      navigate("/library", { replace: true });
    }
    if (outcome.kind === "none") return;
    if (outcome.kind === "multipleItems") {
      // Rendered VERBATIM — the copy travels in the Rust DTO. NOTHING was
      // processed; an earlier review (if any) stays valid and displayed.
      setDropNotice(outcome.message);
      return;
    }
    // A settled newest gesture replaces a lingering calm limit…
    setDropNotice(null);
    if (outcome.kind === "folder") {
      // …and lands in the folder-creation machine (the import machine
      // already stepped aside inside the hook if it showed an earlier
      // drop surface; an archive surface fed by an earlier external
      // settlement steps aside too). A commit in flight on that machine
      // DECLINES the injection (it may not be rewritten mid-commit):
      // render the frozen busy copy — the calm refusal the signal gate
      // would have rendered had the commit been visible when the signal
      // arrived (the one-shot verdict cannot be re-served; the user
      // re-drops afterwards).
      clearExternalArchiveReview();
      if (!injectDropFolderVerdict(outcome.verdict)) {
        setDropNotice(DROP_BUSY_NOTICE);
      }
      return;
    }
    if (outcome.kind === "archive") {
      // …and lands in the archive-creation machine — the folder-drop
      // doctrine verbatim (siblings step aside, a mid-commit machine
      // declines into the calm busy copy).
      clearDropFolderReview();
      if (!injectExternalArchiveVerdict(outcome.verdict, "drop")) {
        setDropNotice(DROP_BUSY_NOTICE);
      }
      return;
    }
    // `review` / `failed` landed in the import machine. The drop
    // channel's newest settlement is the only one displayed: a folder or
    // archive surface fed by an EARLIER external settlement steps aside
    // (a picker-origin one is never touched).
    clearDropFolderReview();
    clearExternalArchiveReview();
  }, [
    navigate,
    analyzeFromDrop,
    injectDropFolderVerdict,
    clearDropFolderReview,
    injectExternalArchiveVerdict,
    clearExternalArchiveReview,
  ]);

  // One-shot pull at mount. The Rust-side one-shot take makes the
  // StrictMode double-effect harmless by construction: the second pull
  // answers `none` (total no-op).
  useEffect(() => {
    void handleDropIntent();
  }, [handleDropIntent]);

  // A live flow has priority over an arriving drop intent — the exact
  // mirror of the OS-open gate: decline calmly, consume the intent, NEVER
  // interrupt. `isOsOpenSettling` DOES gate here (a live OS read is a real
  // in-flight operation this channel must not interleave with); the drop
  // channel's OWN settling is deliberately absent — a channel never gates
  // on its own settling, it serializes through its own mono-slot queue.
  const isDropBlockedByLiveFlow =
    storyImport.status.kind === "analyzing" ||
    storyImport.status.kind === "importing" ||
    storyImport.isOsOpenSettling ||
    isCreateFromFolderBusy ||
    isCreateFromArchiveBusy ||
    isRssCreationActive ||
    isWebCreationActive ||
    isCreateSubmitting ||
    storyTransfer.state.kind === "transferring";

  useEffect(() => {
    if (!dropSignal) return;
    clearDropSignal();
    if (isDropBlockedByLiveFlow) {
      void discardDropRequest().catch(() => {
        // Best-effort: a failed discard leaves a pending intent the next
        // pull will surface — never a broken living flow.
      });
      setDropNotice(DROP_BUSY_NOTICE);
      return;
    }
    void handleDropIntent();
  }, [dropSignal, clearDropSignal, isDropBlockedByLiveFlow, handleDropIntent]);

  const handleImportRetry = (): void => {
    if (storyImport.origin === "osOpen") {
      if (storyImport.failedPhase === "accept") {
        // The commit failed AFTER the one-shot intent was consumed:
        // `Réessayer` re-runs the accept with the preserved verdict — a
        // re-pull would answer `none` and retry nothing.
        void storyImport.retryAccept();
      } else {
        // The read failure left the intent pending Rust-side: `Réessayer`
        // replays the SAME intent — never the file picker.
        void handleOsOpenIntent();
      }
    } else if (storyImport.origin === "drop") {
      // Same two-phase semantics as the OS-open origin, on the drop
      // channel's own pull.
      if (storyImport.failedPhase === "accept") {
        void storyImport.retryAccept();
      } else {
        void handleDropIntent();
      }
    } else {
      void storyImport.pickAndAnalyze();
    }
  };

  const handleImportAbandon = (): void => {
    if (storyImport.origin === "drop") {
      // A terminal gesture on a drop-fed review also drops any pending
      // Rust intent (idempotent — the reviewed verdict was consumed
      // one-shot; this only clears a newer, not-yet-pulled gesture).
      void discardDropRequest().catch(() => {});
    }
    storyImport.abandon();
  };

  const handleImportDismiss = (): void => {
    if (
      storyImport.origin === "osOpen" &&
      storyImport.status.kind === "failed"
    ) {
      // `Fermer` on an OS-open read failure abandons the still-pending
      // intent (idempotent — a post-accept failure has nothing left).
      void discardOsOpenRequest().catch(() => {});
    }
    if (storyImport.origin === "drop") {
      // `Fermer` on a drop failure abandons the still-pending intent
      // (idempotent on the other terminals).
      void discardDropRequest().catch(() => {});
    }
    storyImport.dismiss();
  };

  // Folder-creation gestures, branched by origin (`Drop Intent Contract`):
  // a picker-origin retry re-opens the picker (the Structured Folder
  // Creation Contract behavior, unchanged); a drop-origin retry
  // re-commits the PRESERVED verdict after a failed accept (a re-pull
  // would answer `none`) or replays the pending intent after a read
  // failure. Terminal gestures on a drop-fed surface also discard the
  // pending Rust intent (idempotent).
  const handleFolderRetry = (): void => {
    if (structuredCreation.origin === "drop") {
      if (structuredCreation.failedPhase === "accept") {
        void structuredCreation.retryAccept();
      } else {
        void handleDropIntent();
      }
    } else {
      void structuredCreation.pickAndAnalyze();
    }
  };

  const handleFolderAbandon = (): void => {
    if (structuredCreation.origin === "drop") {
      // Terminal channel-wide: drop the pending Rust intent AND
      // invalidate any drop settlement still in flight — a late `folder`
      // verdict must never reopen the review the user just closed
      // (exactly what the import machine's own terminal gestures do).
      storyImport.invalidateDropSettlements();
      void discardDropRequest().catch(() => {});
    }
    structuredCreation.abandon();
  };

  const handleFolderDismiss = (): void => {
    if (structuredCreation.origin === "drop") {
      storyImport.invalidateDropSettlements();
      void discardDropRequest().catch(() => {});
    }
    structuredCreation.dismiss();
  };

  // Reflect the in-flight / failed transfer as a discreet card badge (AC2),
  // keyed on the write job's TARGET story (from the hook state) — never the
  // current selection — so it survives the user selecting another story. The
  // panel stays the authoritative surface; this is a derived signal. (Now that
  // preparation is folded into the write job, the badge tracks the transfer
  // alone.)
  const preparationBadges = useMemo(() => {
    const map = new Map<string, StoryPreparationBadge>();
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
  }, [storyTransfer.state]);

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

  // Device-story selection. Local UI state, intentionally SEPARATE from the
  // library's `selectedStoryIds`: device truth and local truth never merge.
  // A SET so several can be picked for a bulk import; never persisted.
  // Exactly one selected → single inspection; several → the bulk surface.
  const [selectedDeviceUuids, setSelectedDeviceUuids] = useState<
    ReadonlySet<string>
  >(EMPTY_DEVICE_SELECTION);

  // The lone selection, or null when zero/several are selected — the inspector
  // and the single-copy/naming status scoping key on this so their carefully
  // scoped behavior is preserved verbatim from the single-selection era.
  const singleDeviceUuid =
    selectedDeviceUuids.size === 1
      ? (selectedDeviceUuids.values().next().value ?? null)
      : null;

  // Resolve the lone selection against the CURRENT inventory so a stale id
  // (entry gone after a re-read, device swapped, or no longer inspect-
  // authorized) surfaces no inspector for this render — never a frozen stale
  // target (AC3).
  const selectedDeviceStory =
    canInspect && singleDeviceUuid && deviceLibrary.state.kind === "ready"
      ? (deviceLibrary.state.stories.find((s) => s.uuid === singleDeviceUuid) ??
        null)
      : null;

  // The multi-selection resolved to live stories, for the bulk surface.
  const selectedDeviceStories =
    canInspect && deviceLibrary.state.kind === "ready"
      ? deviceLibrary.state.stories.filter((s) =>
          selectedDeviceUuids.has(s.uuid),
        )
      : [];

  // Drop selected ids only when they can no longer be inspected (device gone /
  // unsupported / not authorized) OR when a FRESH authoritative inventory
  // genuinely lacks them. A transient loading/error/refresh state keeps the
  // selection so it survives and is restored once the entries are confirmed
  // present again — never wiped on a passing state (AC3).
  useEffect(() => {
    if (selectedDeviceUuids.size === 0) return;
    if (!canInspect) {
      setSelectedDeviceUuids(EMPTY_DEVICE_SELECTION);
      return;
    }
    if (deviceLibrary.state.kind === "ready") {
      const present = new Set(deviceLibrary.state.stories.map((s) => s.uuid));
      const kept = [...selectedDeviceUuids].filter((u) => present.has(u));
      if (kept.length !== selectedDeviceUuids.size) {
        setSelectedDeviceUuids(new Set(kept));
      }
    }
  }, [canInspect, deviceLibrary.state, selectedDeviceUuids]);

  const handleSelectDeviceStory = (
    uuid: string,
    mode: DeviceStorySelectionMode,
  ): void => {
    setSelectedDeviceUuids((prev) => {
      if (mode === "replace") {
        // Clicking the only-selected entry clears it; otherwise collapse to
        // just this one (single inspection).
        if (prev.size === 1 && prev.has(uuid)) return EMPTY_DEVICE_SELECTION;
        return new Set([uuid]);
      }
      // toggle: add/remove from the multi-selection.
      const next = new Set(prev);
      if (next.has(uuid)) next.delete(uuid);
      else next.add(uuid);
      return next;
    });
  };

  const handleClearDeviceSelection = (): void => {
    setSelectedDeviceUuids(EMPTY_DEVICE_SELECTION);
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
    if (!readableDeviceId || deviceMutationInFlight || !canImportDeviceStory)
      return;
    void deviceImport.triggerImport(readableDeviceId, story.uuid);
  };

  // Bulk copy for a MULTI-selection. Same Rust-owned single-copy boundary,
  // driven sequentially; on completion BOTH sides re-read their truth ONCE
  // (like the single flow, batched). The selection is intentionally kept —
  // a copy is not a move, and the copied entries flip to `alreadyImported`
  // on the re-read so they simply leave the "importable" subset.
  const deviceBulkImport = useDeviceBulkImport({
    onCompleted: () => {
      invalidate();
      deviceLibrary.refresh();
    },
  });

  const handleBulkImportDeviceStories = (packUuids: string[]): void => {
    if (!readableDeviceId || deviceMutationInFlight || !canImportDeviceStory)
      return;
    void deviceBulkImport.start(readableDeviceId, packUuids);
  };

  // Delete-from-device flow. On success the device inventory re-reads so the
  // deleted entry disappears; the LOCAL library is untouched (device delete is
  // independent of any local copy). The confirmation dialog is route-owned —
  // a destructive device write is never one-click.
  const deviceDelete = useDeviceStoryDelete({
    onDeleted: () => {
      // Delisting from `.pi` changes the content-derived device identifier —
      // same re-detect-first discipline as the send flow.
      device.refresh();
      deviceLibrary.refresh();
    },
  });
  const confirmDeleteDeviceTarget = useRef<string | null>(null);
  const [confirmDeleteDeviceStory, setConfirmDeleteDeviceStory] =
    useState<DeviceStoryDto | null>(null);

  // Mirror of the Rust capability gate: the delete affordance is wired only
  // when the authoritative matrix POSITIVELY allows `deleteStory` (V3 does —
  // deletion needs no cipher). Rust stays the authority; this only shapes UI.
  const canDeleteDeviceStory =
    canInspect && supportedDeviceOperations?.deleteStory === true;

  // The wheel order: move controls are wired only when the authoritative
  // matrix POSITIVELY allows `reorderStories` (the `.pi` rewrite). The
  // device stays the single truth — a settled move re-reads the inventory.
  const reorderableDeviceId =
    effectiveDevice &&
    effectiveDevice.kind === "supported" &&
    effectiveDevice.supportedOperations.reorderStories
      ? effectiveDevice.deviceIdentifier
      : null;
  const deviceReorder = useDeviceStoryReorder({
    onReordered: () => deviceLibrary.refresh(),
  });
  const visibleDeviceOrder: string[] =
    deviceLibrary.state.kind === "ready"
      ? deviceLibrary.state.stories.filter((s) => !s.hidden).map((s) => s.uuid)
      : [];
  const handleMoveDeviceStory = (uuid: string, direction: -1 | 1): void => {
    if (reorderableDeviceId === null || deviceMutationInFlight) return;
    void deviceReorder.move(
      reorderableDeviceId,
      visibleDeviceOrder,
      uuid,
      direction,
    );
  };

  const handleRequestDeleteDeviceStory = (story: DeviceStoryDto): void => {
    if (!canDeleteDeviceStory || deviceMutationInFlight) return;
    // The confirmation belongs to this device, never a replacement plugged in later.
    confirmDeleteDeviceTarget.current = readableDeviceId;
    setConfirmDeleteDeviceStory(story);
  };

  const handleConfirmDeleteDeviceStory = (): void => {
    const story = confirmDeleteDeviceStory;
    setConfirmDeleteDeviceStory(null);
    if (
      !story ||
      !readableDeviceId ||
      !canDeleteDeviceStory ||
      readableDeviceId !== confirmDeleteDeviceTarget.current ||
      deviceMutationInFlight
    )
      return;
    void deviceDelete.triggerDelete(readableDeviceId, story.uuid);
  };

  // Scope the delete status to the card it belongs to (like the import/title
  // statuses), so selecting another card never shows this one's status.
  const selectedDeviceDeleteState =
    singleDeviceUuid !== null &&
    singleDeviceUuid === deviceDelete.targetPackUuid
      ? deviceDelete.status
      : undefined;

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
    singleDeviceUuid !== null && singleDeviceUuid === deviceTitle.targetPackUuid
      ? deviceTitle.status
      : undefined;

  // The import status belongs to ONE pack — the one the hook actually
  // started a copy for (`targetPackUuid`, set past its re-entrancy
  // guard). Selecting another card shows THAT card's (idle) status,
  // never the previous card's success; re-selecting the copied card
  // surfaces its status again. A second "Copier" clicked while a copy is
  // in flight is swallowed by the hook AND leaves the target untouched,
  // so the status can never follow the wrong card.
  const selectedDeviceImportState =
    singleDeviceUuid !== null &&
    singleDeviceUuid === deviceImport.targetPackUuid
      ? deviceImport.status
      : undefined;

  const transferInFlight =
    storyTransfer.state.kind === "transferring" ||
    devicePackSend.status.kind === "sending";
  const deviceMutationInFlight =
    transferInFlight ||
    deviceImport.status.kind === "importing" ||
    deviceBulkImport.status.kind === "running" ||
    deviceDelete.status.kind === "deleting" ||
    deviceReorder.status.kind === "moving";
  const closeDetails = (): void => setDetailsOpen(false);

  // Operation views follow the hook's original target, independent of selection
  // and of the device currently plugged in. Closing UI never dismisses a job.
  const trackedTransferView =
    storyTransfer.state.kind === "idle"
      ? null
      : mapTransferView(
          storyTransfer.state,
          storyTransfer.state.storyId,
          1,
          deviceState,
          writableDeviceId !== null,
          true,
        );
  const trackedArchiveView =
    devicePackSend.status.kind === "idle"
      ? null
      : mapArchiveSendToTransferView(
          devicePackSend.status,
          devicePackSend.targetStoryId,
          1,
          true,
        );
  const retryTrackedTransfer = (): void => {
    if (
      writableDeviceId &&
      storyTransfer.state.kind !== "idle" &&
      !deviceMutationInFlight
    )
      storyTransfer.send(storyTransfer.state.storyId, writableDeviceId);
  };
  const retryTrackedArchive = (): void => {
    if (
      sendableDeviceId &&
      devicePackSend.targetStoryId &&
      !deviceMutationInFlight
    )
      void devicePackSend.triggerSend(
        sendableDeviceId,
        devicePackSend.targetStoryId,
      );
  };
  const singleCopyAvailability = selectedDeviceStory
    ? getDeviceStoryCopyAvailability({
        story: selectedDeviceStory,
        supportedOperations: supportedDeviceOperations,
        hasImportHandler: canImportDeviceStory,
        status: selectedDeviceImportState,
      })
    : null;
  const copyableDeviceStories = canImportDeviceStory
    ? selectedDeviceStories.filter(
        (story) => story.contentPresent && !story.alreadyImported,
      )
    : [];
  const multipleDeviceSelection = selectedDeviceUuids.size > 1;
  const deviceCopyEnabled = multipleDeviceSelection
    ? copyableDeviceStories.length > 0
    : singleCopyAvailability?.enabled === true;
  const deviceCopyReason = multipleDeviceSelection
    ? !canImportDeviceStory
      ? "L'import n'est pas disponible pour le profil de cet appareil."
      : "Aucune des histoires sélectionnées n'est importable pour le moment."
    : (singleCopyAvailability?.reason ??
      (singleCopyAvailability?.isImported
        ? "Copie indisponible: déjà dans ta bibliothèque"
        : "Sélectionne une histoire disponible sur l'appareil."));
  const copySelectedDeviceStories = (): void => {
    if (deviceMutationInFlight || !deviceCopyEnabled) return;
    if (multipleDeviceSelection)
      handleBulkImportDeviceStories(copyableDeviceStories.map((s) => s.uuid));
    else if (selectedDeviceStory) handleImportDeviceStory(selectedDeviceStory);
  };
  const clearCurrentSelection = (): void => {
    // The bar disappears, so return keyboard focus to the collection itself.
    const main = document.querySelector<HTMLElement>(".library-layout__main");
    main?.focus();
    if (activeSection === "device") handleClearDeviceSelection();
    else clearSelection();
  };
  const connectionLabel =
    deviceState === "idle"
      ? (deviceLabel ?? "Appareil connecté")
      : deviceState === "scanning"
        ? "Recherche de l'appareil…"
        : deviceState === "absent"
          ? "Aucun appareil connecté"
          : "Appareil à vérifier";
  const continueWelcome = async (): Promise<void> => {
    const path = currentPathRef.current;
    if ((await onboarding.complete(false)) && currentPathRef.current === path)
      navigate(readableDeviceId ? "/welcome/device" : "/library");
  };
  const finishDiscovery = async (): Promise<void> => {
    const path = currentPathRef.current;
    if ((await onboarding.complete(true)) && currentPathRef.current === path)
      navigate("/library");
  };
  const recoverStories = async (): Promise<void> => {
    if (
      !readableDeviceId ||
      !canImportDeviceStory ||
      deviceLibrary.state.kind !== "ready" ||
      isImportBusy ||
      deviceImport.status.kind === "importing" ||
      deviceBulkImport.status.kind === "running" ||
      transferInFlight
    )
      return;
    const path = currentPathRef.current;
    const identifier = readableDeviceId;
    const packs = deviceLibrary.state.stories
      .filter((story) => story.contentPresent && !story.alreadyImported)
      .map((story) => story.uuid);
    if (
      packs.length === 0 ||
      !(await onboarding.complete(true)) ||
      currentPathRef.current !== path
    )
      return;
    setOnboardingRecovery(true);
    void deviceBulkImport.start(identifier, packs);
    navigate("/library");
  };
  const welcomeScreen = (
    <WelcomeScreen
      deviceState={deviceState}
      deviceFamily={deviceFamily ?? undefined}
      deviceLabel={deviceLabel}
      deviceReason={deviceReason}
      onContinue={() => {
        void continueWelcome();
      }}
      onRetryDetection={device.refresh}
      onRetryPreferences={onboarding.retry}
      onOpenLibrary={() => navigate("/library")}
      busy={onboarding.loading || onboarding.saving}
      error={onboarding.error}
    />
  );
  const discoveryScreen = readableDeviceId ? (
    <DeviceDiscoveryScreen
      state={deviceLibrary.state}
      deviceFamily={deviceFamily ?? undefined}
      deviceLabel={deviceLabel}
      importUnavailableReason={
        canImportDeviceStory
          ? undefined
          : "La copie des histoires de cet appareil n'est pas disponible dans cette version. Tu peux consulter son contenu dans Sur l'appareil."
      }
      onRetry={deviceLibrary.refresh}
      onRetryPreferences={onboarding.retry}
      onOpenLibrary={() => navigate("/library")}
      onRecover={() => {
        void recoverStories();
      }}
      onContinue={() => {
        void finishDiscovery();
      }}
      busy={
        onboarding.loading ||
        onboarding.saving ||
        transferInFlight ||
        isImportBusy ||
        deviceImport.status.kind === "importing" ||
        deviceBulkImport.status.kind === "running"
      }
      error={onboarding.error}
    />
  ) : (
    welcomeScreen
  );

  const openCatalog = (): void => {
    navigate("/settings");
  };

  const center = (
    <>
      {/* Prominent update banner at the TOP of the library, full width —
          rendered ONLY when a newer official version is available (silence
          otherwise). Moved here from the nav foot to be actually noticed. */}
      <UpdateAvailabilitySignal />
      {onboardingRecovery && (
        <RecoveryProgress
          status={deviceBulkImport.status}
          onDismiss={() => {
            deviceBulkImport.dismiss();
            setOnboardingRecovery(false);
          }}
        />
      )}
      {readableDeviceId &&
        onboarding.preferences?.welcomeCompleted &&
        !onboarding.preferences.deviceRecoveryPromptDismissed &&
        !onboardingRecovery && (
          <SurfacePanel
            elevation={0}
            className="library-route__operation-notice"
          >
            <div>
              <strong>Ton appareil est reconnu</strong>
              <p>
                Découvre ses histoires et celles que tu peux récupérer dans ta
                bibliothèque.
              </p>
            </div>
            <Button
              variant="secondary"
              disabled={
                deviceImport.status.kind === "importing" ||
                deviceBulkImport.status.kind === "running"
              }
              onClick={() => navigate("/welcome/device")}
            >
              Découvrir mes histoires
            </Button>
          </SurfacePanel>
        )}
      {/* Persistent status region the calm-limit copies route through
          (`OS Open Contract`): mounted (empty) as long as the center
          column exists so AT reliably announces the copy when it LANDS —
          a live region inserted into the DOM already filled (like the
          visual block below) is not reliably announced; only CHANGES of
          an existing region are (the surfaces' `__live` pattern). */}
      <div
        className="library-route__os-open-live"
        role="status"
        aria-atomic="true"
      >
        {osOpenNotice ?? ""}
      </div>
      {osOpenNotice ? (
        // The VISUAL calm-limit block (`OS Open Contract`) — inline, calm,
        // never a toast, never a modal, never an alert tone. The
        // announcement rides the persistent region above.
        <SurfacePanel elevation={0}>
          <p>{osOpenNotice}</p>
          <Button variant="quiet" onClick={() => setOsOpenNotice(null)}>
            Fermer
          </Button>
        </SurfacePanel>
      ) : null}
      {/* Persistent status region of the DROP calm-limit copies
          (`Drop Intent Contract`) — its own region, mounted empty for the
          same AT-announcement reason as the OS-open one above. */}
      <div
        className="library-route__drop-live"
        role="status"
        aria-atomic="true"
      >
        {dropNotice ?? ""}
      </div>
      {dropNotice ? (
        <SurfacePanel elevation={0}>
          <p>{dropNotice}</p>
          <Button variant="quiet" onClick={() => setDropNotice(null)}>
            Fermer
          </Button>
        </SurfacePanel>
      ) : null}
      <ImportArtifactSurface
        status={storyImport.status}
        onAccept={storyImport.acceptRecognized}
        onAbandon={handleImportAbandon}
        onRetry={handleImportRetry}
        onDismiss={handleImportDismiss}
      />
      <CreateFromFolderSurface
        status={structuredCreation.status}
        onAccept={structuredCreation.acceptCreation}
        onAbandon={handleFolderAbandon}
        onRetry={handleFolderRetry}
        onDismiss={handleFolderDismiss}
      />
      <CreateFromArchiveSurface
        status={archiveCreation.status}
        onAccept={() => {
          void archiveCreation.acceptCreation();
        }}
        onAbandon={handleArchiveAbandon}
        onRetry={handleArchiveRetry}
        onDismiss={handleArchiveDismiss}
      />
      <CreateFromRssSurface
        open={isRssCreationOpen}
        status={rssCreation.status}
        onFetch={(url) => {
          void rssCreation.fetchPreview(url);
        }}
        onToggleItem={rssCreation.toggleItem}
        onSelectAll={rssCreation.selectAll}
        onSelectNone={rssCreation.selectNone}
        onAccept={() => {
          void rssCreation.acceptCreation();
        }}
        onAbandon={handleRssAbandon}
        onDismiss={handleRssDismiss}
      />
      <CreateFromWebSurface
        initialUrl={webCreationInitialUrl}
        open={isWebCreationOpen}
        status={webCreation.status}
        onFetch={(url) => {
          void webCreation.fetchPreview(url);
        }}
        onAccept={() => {
          void webCreation.acceptCreation();
        }}
        onAbandon={handleWebAbandon}
        onDismiss={handleWebDismiss}
      />
      <StoryArchiveExportSurface
        status={archiveExport.status}
        onRetry={() => {
          void archiveExport.retryExport();
        }}
        onDismiss={archiveExport.dismissStatus}
      />
      {(trackedTransferView || trackedArchiveView) && (
        <SurfacePanel elevation={0} className="library-route__operation-notice">
          <div role="status">
            {transferInFlight ? (
              <TransferStatus
                view={
                  storyTransfer.state.kind === "transferring"
                    ? trackedTransferView!
                    : trackedArchiveView!
                }
              />
            ) : (
              "Résultat du transfert disponible"
            )}
          </div>
          <Button
            variant="quiet"
            onClick={() => setOperationDetails("transfer")}
          >
            Voir le transfert
          </Button>
        </SurfacePanel>
      )}
      {(deviceImport.status.kind !== "idle" ||
        (!onboardingRecovery && deviceBulkImport.status.kind !== "idle")) && (
        <SurfacePanel elevation={0} className="library-route__operation-notice">
          <p role="status">
            {deviceImport.status.kind === "importing" ||
            deviceBulkImport.status.kind === "running"
              ? "Copie depuis l'appareil en cours"
              : "Résultat de la copie depuis l'appareil disponible"}
          </p>
          <Button variant="quiet" onClick={() => setOperationDetails("copy")}>
            Voir la copie
          </Button>
        </SurfacePanel>
      )}
      {deleteState.kind === "failed" && (
        <p role="alert">{deleteState.message}</p>
      )}
      {deviceDelete.status.kind === "deleting" && (
        <p role="status">Suppression de l'appareil en cours…</p>
      )}
      {deviceDelete.status.kind === "failed" && (
        <SurfacePanel elevation={0}>
          <div role="alert">
            <p>{deviceDelete.status.error.message}</p>
            <p>{deviceDelete.status.error.userAction}</p>
          </div>
          <Button variant="quiet" onClick={deviceDelete.dismissStatus}>
            Fermer l'erreur de suppression
          </Button>
        </SurfacePanel>
      )}
      <Outlet
        context={{
          library: renderCenter(
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
            () => handleCreateStoryRequest(),
            preparationBadges,
            onDeviceStoryIds,
            deviceStampLabel,
            () => handleCreateStoryRequest("import"),
            isImportBusy ||
              isCreateFromFolderBusy ||
              isCreateFromArchiveBusy ||
              isRssCreationActive ||
              isWebCreationActive,
            handleStoryContextMenu,
            collectionView,
            setCollectionView,
            "library",
            openCatalog,
            () => handleCreateStoryRequest("import"),
          ),
          drafts: renderCenter(
            state.kind === "ready"
              ? {
                  ...state,
                  overview: {
                    ...state.overview,
                    stories: state.overview.stories.filter(isCreatedStory),
                  },
                }
              : state,
            retry,
            presentSelectedIds,
            selectStory,
            handleOpenStory,
            query,
            sort,
            setQuery,
            setSort,
            resetFilters,
            () => handleCreateStoryRequest(),
            preparationBadges,
            onDeviceStoryIds,
            deviceStampLabel,
            () => handleCreateStoryRequest("import"),
            isImportBusy ||
              isCreateFromFolderBusy ||
              isCreateFromArchiveBusy ||
              isRssCreationActive ||
              isWebCreationActive,
            handleStoryContextMenu,
            collectionView,
            setCollectionView,
            "drafts",
            openCatalog,
            () => handleCreateStoryRequest("import"),
          ),
          device: (
            <DeviceScreen
              deviceState={deviceState}
              deviceLabel={deviceLabel}
              deviceReason={deviceReason}
              onRetryDetection={device.refresh}
              onConsultSupportProfile={openSupportProfile}
              collectionProps={{
                state: deviceLibrary.state,
                query: deviceQuery,
                onQueryChange: setDeviceQuery,
                view: deviceView,
                onViewChange: setDeviceView,
                isRefreshing: deviceLibrary.isRefreshing,
                deviceLabel,
                selectedUuids: canInspect
                  ? selectedDeviceUuids
                  : EMPTY_DEVICE_SELECTION,
                onSelectStory: canInspect ? handleSelectDeviceStory : undefined,
                onRetry: deviceLibrary.refresh,
                onMoveStory:
                  reorderableDeviceId !== null
                    ? handleMoveDeviceStory
                    : undefined,
                movingUuid:
                  deviceReorder.status.kind === "moving"
                    ? deviceReorder.status.packUuid
                    : null,
                moveError:
                  deviceReorder.status.kind === "failed"
                    ? deviceReorder.status.error
                    : null,
                onDismissMoveError: deviceReorder.dismissStatus,
              }}
            />
          ),
        }}
      />
    </>
  );

  return (
    <>
      {/* Fills the post-drop gap: while a big dropped pack is being analyzed
          (recognition review), signal that work is happening (the decorative
          hover overlay is already gone by then). */}
      <DropAnalyzingOverlay active={storyImport.isDropSettling} />
      {isOnboardingScreen ? (
        <Outlet
          context={{ welcome: welcomeScreen, discovery: discoveryScreen }}
        />
      ) : (
        <LibraryLayout
          mainLabel={
            activeSection === "device"
              ? "Histoires sur l'appareil"
              : isDraftsScreen
                ? "Mes brouillons"
                : "Collection d'histoires"
          }
          leftNav={
            <>
              <LibraryFiltersNav />
              <div className="library-route__nav-footer">
                <button
                  className="library-filters-nav__entry"
                  type="button"
                  onClick={openSupportProfile}
                >
                  <Icon name="settings" />
                  Aide et réglages
                </button>
                <button
                  type="button"
                  className="library-route__device-status"
                  aria-label="Détails de l'appareil"
                  aria-haspopup="dialog"
                  onClick={() => setDetailsOpen(true)}
                >
                  <span className="library-route__device-icon">
                    <Icon name="device" size={26} />
                  </span>
                  <span>
                    <strong>
                      {deviceFamily === "flam" ? "Mon FLAM" : "Ma Lunii"}
                    </strong>
                    <span className="library-route__connection">
                      <i data-connected={deviceState === "idle"} />
                      {connectionLabel}
                    </span>
                  </span>
                  <Icon name="chevron-down" size={16} />
                </button>
                <span className="library-route__local-note">
                  <Icon name="leaf" size={14} />
                  Tes histoires restent sur ton ordinateur.
                </span>
              </div>
            </>
          }
          center={center}
          bottomBar={
            (activeSection === "device"
              ? selectedDeviceUuids.size
              : presentSelectedIds.size) > 0 ? (
              <SelectionActionBar
                selectedCount={
                  activeSection === "device"
                    ? selectedDeviceUuids.size
                    : presentSelectedIds.size
                }
                primaryAction={
                  activeSection === "device"
                    ? {
                        label: multipleDeviceSelection
                          ? `Copier ${copyableDeviceStories.length} histoires`
                          : "Copier dans ma bibliothèque",
                        icon: "download",
                        onClick: copySelectedDeviceStories,
                        disabled: deviceMutationInFlight || !deviceCopyEnabled,
                        disabledReason: deviceMutationInFlight
                          ? "Opération en cours…"
                          : deviceCopyReason,
                      }
                    : {
                        label:
                          deviceFamily === "flam"
                            ? "Ajouter à l'appareil"
                            : "Ajouter à la Lunii",
                        onClick: handleSendSelected,
                        disabled:
                          deviceMutationInFlight ||
                          transferView.kind !== "ready",
                        disabledReason: deviceMutationInFlight
                          ? "Opération en cours…"
                          : transferView.kind === "unavailable"
                            ? transferView.reason
                            : "Consulte le résultat du transfert dans Détails.",
                      }
                }
                onDetails={() => setDetailsOpen(true)}
                onDelete={
                  activeSection === "device"
                    ? () => {
                        if (selectedDeviceStory && canDeleteDeviceStory)
                          handleRequestDeleteDeviceStory(selectedDeviceStory);
                      }
                    : () => setConfirmDeleteIds([...presentSelectedIds])
                }
                deleteLabel={
                  activeSection === "device"
                    ? "Supprimer de l'appareil"
                    : "Supprimer la sélection"
                }
                deleteDisabled={
                  deviceMutationInFlight ||
                  deleteState.kind === "deleting" ||
                  (activeSection === "device" &&
                    (!selectedDeviceStory || !canDeleteDeviceStory))
                }
                deleteDisabledReason={
                  deviceMutationInFlight || deleteState.kind === "deleting"
                    ? "Opération en cours…"
                    : multipleDeviceSelection
                      ? "Sélectionne une seule histoire pour la supprimer de l'appareil."
                      : "Suppression indisponible pour le profil de cet appareil."
                }
                onClearSelection={clearCurrentSelection}
                progress={
                  activeSection === "device" &&
                  multipleDeviceSelection &&
                  canImportDeviceStory && (
                    <p>
                      {
                        selectedDeviceStories.filter((s) => s.alreadyImported)
                          .length
                      }{" "}
                      déjà dans ta bibliothèque ·{" "}
                      {
                        selectedDeviceStories.filter((s) => !s.contentPresent)
                          .length
                      }{" "}
                      au contenu incomplet
                    </p>
                  )
                }
              />
            ) : undefined
          }
        />
      )}
      {detailsOpen && (
        <Dialog open title="Détails de la sélection" onClose={closeDetails}>
          <div className="library-route__details-actions">
            <Button variant="quiet" onClick={closeDetails}>
              Fermer les détails
            </Button>
          </div>
          {/* 2+ device stories selected → the bulk-import surface replaces
                the single-story inspector; 0 or 1 → the inspector as before
                (its own placeholder covers the empty case). */}
          {activeSection === "device" ? (
            canInspect && selectedDeviceStories.length >= 2 ? (
              <DeviceBulkImportPanel
                actionsDisabled={deviceMutationInFlight}
                stories={selectedDeviceStories}
                canImport={canImportDeviceStory}
                status={deviceBulkImport.status}
                onImport={handleBulkImportDeviceStories}
                onClearSelection={handleClearDeviceSelection}
                onDismissStatus={deviceBulkImport.dismiss}
                onConsultSupportProfile={openSupportProfile}
              />
            ) : (
              <DeviceStoryInspector
                actionsDisabled={deviceMutationInFlight}
                story={selectedDeviceStory}
                supportedOperations={supportedDeviceOperations}
                importState={selectedDeviceImportState}
                onImport={
                  canImportDeviceStory ? handleImportDeviceStory : undefined
                }
                onRetryImport={() => {
                  if (!deviceMutationInFlight) void deviceImport.retryImport();
                }}
                onDismissImportStatus={deviceImport.dismissStatus}
                onConsultSupportProfile={openSupportProfile}
                onSetTitle={handleSetDeviceStoryTitle}
                titleState={selectedDeviceTitleState}
                onDismissTitleError={deviceTitle.reset}
                onDelete={
                  canDeleteDeviceStory
                    ? handleRequestDeleteDeviceStory
                    : undefined
                }
                deleteState={selectedDeviceDeleteState}
                onDismissDeleteStatus={deviceDelete.dismissStatus}
              />
            )
          ) : null}
          {activeSection !== "device" ? (
            <LuniiDecisionPanel
              actionsDisabled={deviceMutationInFlight}
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
              transfer={transferView}
              onSend={handleSendSelected}
              onRetryTransfer={canRetrySend ? handleSendSelected : undefined}
              onDismissTransfer={handleDismissTransfer}
              onEdit={handleEditSelected}
              onDeleteSelected={() => {
                void handleDeleteSelected();
              }}
              isDeletingSelection={deleteState.kind === "deleting"}
              deleteSelectionError={
                deleteState.kind === "failed" ? deleteState.message : null
              }
              onRefreshDevice={device.refresh}
              onConsultSupportProfile={openSupportProfile}
            />
          ) : null}
        </Dialog>
      )}
      {operationDetails && (
        <Dialog
          open
          title={
            operationDetails === "transfer"
              ? "Détails du transfert"
              : "Détails de la copie"
          }
          onClose={() => setOperationDetails(null)}
        >
          <Button variant="quiet" onClick={() => setOperationDetails(null)}>
            Fermer les détails
          </Button>
          {operationDetails === "transfer" ? (
            <>
              {trackedTransferView && (
                <>
                  <TransferStatus
                    actionsDisabled={deviceMutationInFlight}
                    view={trackedTransferView}
                    deviceFamily={deviceFamily}
                    onRetryTransfer={
                      writableDeviceId ? retryTrackedTransfer : undefined
                    }
                    onDismissTransfer={storyTransfer.dismiss}
                  />
                  {(trackedTransferView.kind === "verified" ||
                    trackedTransferView.kind === "error") && (
                    <Button variant="quiet" onClick={storyTransfer.dismiss}>
                      Fermer le compte rendu de transfert
                    </Button>
                  )}
                </>
              )}
              {trackedArchiveView && (
                <TransferStatus
                  actionsDisabled={deviceMutationInFlight}
                  view={trackedArchiveView}
                  deviceFamily={deviceFamily}
                  onRetryTransfer={
                    sendableDeviceId ? retryTrackedArchive : undefined
                  }
                  onDismissTransfer={devicePackSend.dismissStatus}
                />
              )}
              {!trackedTransferView && !trackedArchiveView && (
                <p>Aucun transfert en cours.</p>
              )}
            </>
          ) : (
            <>
              <DeviceImportStatusSurface
                actionsDisabled={deviceMutationInFlight}
                status={deviceImport.status}
                onRetry={() => {
                  if (!deviceMutationInFlight) void deviceImport.retryImport();
                }}
                onDismiss={deviceImport.dismissStatus}
                onConsultSupportProfile={openSupportProfile}
              />
              <RecoveryProgress
                status={deviceBulkImport.status}
                onDismiss={deviceBulkImport.dismiss}
              />
            </>
          )}
        </Dialog>
      )}
      <CreateStoryDialog
        initialStep={createInitialStep}
        onImportArtifactRequest={storyImport.pickAndAnalyze}
        isImportArtifactUnavailable={
          isImportBusy ||
          isCreateFromFolderBusy ||
          isCreateFromArchiveBusy ||
          isRssCreationActive ||
          isWebCreationActive
        }
        open={isCreateOpen}
        onClose={() => setIsCreateOpen(false)}
        onCreated={handleCreated}
        onCreateFromFolderRequest={() => {
          void structuredCreation.pickAndAnalyze();
        }}
        isCreateFromFolderUnavailable={
          isImportBusy ||
          isCreateFromFolderBusy ||
          isCreateFromArchiveBusy ||
          isRssCreationActive ||
          isWebCreationActive
        }
        onCreateFromArchiveRequest={() => {
          void archiveCreation.pickAndAnalyze();
        }}
        isCreateFromArchiveUnavailable={
          isImportBusy ||
          isCreateFromFolderBusy ||
          isCreateFromArchiveBusy ||
          isRssCreationActive ||
          isWebCreationActive
        }
        onCreateFromRssRequest={() => {
          setIsRssCreationOpen(true);
        }}
        isCreateFromRssUnavailable={
          isImportBusy ||
          isCreateFromFolderBusy ||
          isCreateFromArchiveBusy ||
          isRssCreationActive ||
          isWebCreationActive
        }
        onCreateFromWebRequest={(url) => {
          // The wizard has collected the address without network access. The
          // explicit fetch gesture now hands it to the existing preview flow.
          setWebCreationInitialUrl(url);
          setIsWebCreationOpen(true);
          void webCreation.fetchPreview(url);
        }}
        isCreateFromWebUnavailable={
          isImportBusy ||
          isCreateFromFolderBusy ||
          isCreateFromArchiveBusy ||
          isRssCreationActive ||
          isWebCreationActive
        }
        isSubmitUnavailable={
          storyImport.isOsOpenSettling || storyImport.isDropSettling
        }
        onSubmittingChange={setIsCreateSubmitting}
        contentSourcePolicy={contentSourcePolicy}
      />
      {storyMenu !== null ? (
        <ContextMenu
          x={storyMenu.x}
          y={storyMenu.y}
          ariaLabel={`Actions pour ${
            overview?.stories.find((s) => s.id === storyMenu.id)?.title ??
            "l'histoire"
          }`}
          onClose={() => setStoryMenu(null)}
          items={[
            {
              label: "Éditer",
              onSelect: () => handleOpenStory(storyMenu.id),
            },
            {
              // The story as a ready-to-use archive, to share with another
              // Rustory or another pack tool. Rust decides whether THIS
              // story can become one and says why not; one export at a
              // time, so the row waits while another is in flight.
              label: "Exporter en archive",
              disabled: archiveExport.isExporting,
              onSelect: () => {
                const title =
                  overview?.stories.find((s) => s.id === storyMenu.id)
                    ?.title ?? "";
                void archiveExport.triggerExport(storyMenu.id, title);
              },
            },
            {
              label: "Supprimer",
              danger: true,
              onSelect: () => setConfirmDeleteIds([storyMenu.id]),
            },
          ]}
        />
      ) : null}
      {confirmDeleteIds !== null ? (
        <Dialog
          open
          onClose={() => setConfirmDeleteIds(null)}
          title={
            confirmDeleteIds.length > 1
              ? `Supprimer ${confirmDeleteIds.length} histoires ?`
              : "Supprimer l'histoire ?"
          }
          ariaDescribedBy={confirmDeleteId}
        >
          <p id={confirmDeleteId}>
            {confirmDeleteIds.length === 1
              ? `« ${
                  overview?.stories.find((s) => s.id === confirmDeleteIds[0])
                    ?.title ?? "Cette histoire"
                } » sera définitivement supprimée de la bibliothèque, avec ses brouillons, médias et mémoires de transfert associés.`
              : `Ces ${confirmDeleteIds.length} histoires seront définitivement supprimées de la bibliothèque, avec leurs brouillons, médias et mémoires de transfert associés.`}
          </p>
          <div className="library-route__confirm-actions">
            <Button
              variant="primary"
              onClick={() => {
                const ids = confirmDeleteIds;
                setConfirmDeleteIds(null);
                void deleteIds(ids);
              }}
            >
              Confirmer la suppression
            </Button>
            <Button variant="quiet" onClick={() => setConfirmDeleteIds(null)}>
              Annuler
            </Button>
          </div>
        </Dialog>
      ) : null}
      {confirmDeleteDeviceStory !== null ? (
        <Dialog
          open
          onClose={() => setConfirmDeleteDeviceStory(null)}
          title="Supprimer de l'appareil ?"
          ariaDescribedBy={confirmDeleteDeviceId}
        >
          <p id={confirmDeleteDeviceId}>
            {`« ${
              confirmDeleteDeviceStory.title ?? "Cette histoire"
            } » sera retirée de l'appareil : son entrée est délistée et son contenu supprimé du support. Cette action est définitive et ne touche pas ta bibliothèque locale.`}
          </p>
          <div className="library-route__confirm-actions">
            <Button
              variant="destructive"
              disabled={
                deviceMutationInFlight ||
                !canDeleteDeviceStory ||
                readableDeviceId !== confirmDeleteDeviceTarget.current
              }
              onClick={handleConfirmDeleteDeviceStory}
            >
              Supprimer de l'appareil
            </Button>
            <Button
              variant="quiet"
              onClick={() => setConfirmDeleteDeviceStory(null)}
            >
              Annuler
            </Button>
          </div>
        </Dialog>
      ) : null}
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

/**
 * Pure mapper from the `useStoryTransfer` state (+ the send-gate context) to the
 * `transfer` prop `LuniiDecisionPanel` expects. Pure so it stays testable in
 * isolation. FAIL-CLOSED: the `Envoyer vers la Lunii` CTA is `ready` ONLY for a
 * single selection + a write-authorized device (V1/V2) + a transferable story
 * (owns a device-format pack); every other case is a standardized "Envoi
 * indisponible: …" reason. The write job prepares internally. An active /
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
/**
 * Map the V3 archive-send status (+ selection gating) into the SAME
 * `TransferView` the round-trip uses, so the single "Envoyer vers la Lunii"
 * CTA renders both backends uniformly. The status is assumed already SCOPED to
 * the selected story by the caller. A settled/in-flight send takes precedence
 * over gating (its terminal stays visible for the selected story); otherwise
 * the gate decides `ready` vs a standardized `unavailable` reason. The success
 * is `sent` (NOT `verified`): the V3 write is not re-read.
 */
export function mapArchiveSendToTransferView(
  status: DevicePackSendStatus,
  selectedStoryId: string | null,
  selectionCount: number,
  sendable: boolean,
  sendBlocker?: SendBlocker,
): TransferView {
  switch (status.kind) {
    case "sending":
      // The send streams an integer percent (0..99) over a channel as the pack
      // is normalized then written; convert to the fraction the bar expects.
      // `null` before the first tick keeps the bar honestly indeterminate.
      return {
        kind: "transferring",
        progress: status.progress == null ? null : status.progress / 100,
        phase: "transfer",
      };
    case "sent":
      return {
        kind: "sent",
        imageCount: status.imageCount,
        audioCount: status.audioCount,
      };
    case "failed":
      return { kind: "error", error: status.error };
    case "idle":
      break;
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
  if (selectedStoryId === null || !sendable) {
    return { kind: "unavailable", reason: sendBlockerReason(sendBlocker) };
  }
  return { kind: "ready" };
}

/** The pre-click reason for a story a V3 cannot receive, per the card's
 *  closed blocker set (Rust decides; this only words it). */
export function sendBlockerReason(blocker: SendBlocker | undefined): string {
  switch (blocker) {
    case "devicePack":
      return "Envoi indisponible: histoire copiée depuis une Lunii, non convertible pour ce modèle";
    case "missingAudio":
      return "Envoi indisponible: un ou plusieurs épisodes n'ont pas d'audio";
    case "missingAnnouncements":
      return "Envoi indisponible: génère d'abord les annonces du menu depuis l'histoire";
    case "optionWithoutDestination":
      return "Envoi indisponible: une option de l'histoire ne mène à aucun nœud";
    case "empty":
      return "Envoi indisponible: l'histoire n'a aucun épisode";
    case "malformed":
      return "Envoi indisponible: structure de l'histoire illisible";
    case undefined:
      return "Envoi indisponible: cette histoire ne peut pas être préparée pour l'appareil";
  }
}

export function mapTransferView(
  state: ReturnType<typeof useStoryTransfer>["state"],
  selectedStoryId: string | null,
  selectionCount: number,
  deviceState: LuniiDeviceState,
  writable: boolean,
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
          : {
              kind: "transferring",
              progress: state.progress,
              phase: state.phase,
            };
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

function formatUnsupportedReason(reason: string, hint: string | null): string {
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
  onDeviceStoryIds: ReadonlySet<string>,
  deviceStampLabel: string,
  onImportArtifactRequest: () => void,
  isImportBusy: boolean,
  onStoryContextMenu: (id: string, x: number, y: number) => void,
  view: "grid" | "list",
  onViewChange: (view: "grid" | "list") => void,
  variant: "library" | "drafts",
  onCatalogRequest: () => void,
  onImportRequest: () => void,
): React.JSX.Element {
  switch (state.kind) {
    case "error": {
      const title =
        state.error.code === "LIBRARY_INCONSISTENT"
          ? "Bibliothèque incohérente, recharge nécessaire"
          : "Bibliothèque indisponible";
      return (
        <LibraryErrorBanner error={state.error} onRetry={retry} title={title} />
      );
    }
    case "loading":
      return (
        <StoryCollection
          variant={variant}
          onCatalogRequest={onCatalogRequest}
          onImportRequest={onImportRequest}
          view={view}
          onViewChange={onViewChange}
          stories={[]}
          isLoading={true}
          query={query}
          sort={sort}
          onQueryChange={onQueryChange}
          onSortChange={onSortChange}
          onResetFilters={onResetFilters}
          selectedStoryIds={selectedStoryIds}
          preparationBadges={preparationBadges}
          onDeviceStoryIds={onDeviceStoryIds}
          deviceStampLabel={deviceStampLabel}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onStoryContextMenu={onStoryContextMenu}
          onCreateStoryRequest={onCreateStoryRequest}
          onImportArtifactRequest={onImportArtifactRequest}
          isImportBusy={isImportBusy}
        />
      );
    case "ready":
      return (
        <StoryCollection
          variant={variant}
          onCatalogRequest={onCatalogRequest}
          onImportRequest={onImportRequest}
          view={view}
          onViewChange={onViewChange}
          stories={state.overview.stories}
          isLoading={false}
          query={query}
          sort={sort}
          onQueryChange={onQueryChange}
          onSortChange={onSortChange}
          onResetFilters={onResetFilters}
          selectedStoryIds={selectedStoryIds}
          preparationBadges={preparationBadges}
          onDeviceStoryIds={onDeviceStoryIds}
          deviceStampLabel={deviceStampLabel}
          onSelectStory={onSelectStory}
          onOpenStory={onOpenStory}
          onStoryContextMenu={onStoryContextMenu}
          onCreateStoryRequest={onCreateStoryRequest}
          onImportArtifactRequest={onImportArtifactRequest}
          isImportBusy={isImportBusy}
        />
      );
  }
}

function isCreatedStory(story: StoryCardDto): boolean {
  return story.importState === undefined && story.transferable !== true;
}
