// ===== Support profile (the `Profil de support` screen) =====
//
// Mirror of `src-tauri/src/ipc/dto/settings.rs`. Rust alone decides the
// support profile (`Support Profile Screen Contract`): the frontend
// renders what it declares and never hardcodes a family, cohort, kind,
// label or reason. These guards refuse a drifted wire shape so the
// screen never renders against an arbitrary object — and they lock the
// FROZEN couples (label per tag, reason per limit) so a drifted copy is
// never rendered as authoritative. A distribution wanting another
// matrix is an explicit re-scope of this guard, not a silent
// acceptance.

/** Closed set of device families this product speaks about. */
export type SupportedDeviceFamily = "lunii" | "flam";

/** Closed set of firmware cohorts of the documented matrix. */
export type DeviceFirmwareCohort = "origineV1" | "midGenV2" | "v3" | "flamGen1";

/** Closed set of the four operations of a device matrix line. */
export type DeviceOperation =
  "readLibrary" | "inspectStory" | "importStory" | "writeStory";

/** Closed set of local-artifact kinds of the documented registry. */
export type LocalArtifactKind =
  "rustoryArtifact" | "structuredFolder" | "structuredArchive";

/** One capability of a device matrix line: the closed tag, the frozen
 *  label, the availability and — on a non-available capability ONLY —
 *  the frozen reason (the guard refuses incoherence). */
export interface DeviceCapability {
  operation: DeviceOperation;
  label: string;
  available: boolean;
  /** Present IFF the capability is not available; an available one
   *  carries the state chip instead. */
  reason?: string;
}

/** One line of the device support matrix: the closed tags, the frozen
 *  labels, the frozen metadata-format line (ABSENT for a family
 *  without a documented version byte) and the four capabilities in the
 *  canonical wire order. */
export interface DeviceSupportLine {
  family: SupportedDeviceFamily;
  familyLabel: string;
  cohort: DeviceFirmwareCohort;
  cohortLabel: string;
  metadataFormatLabel?: string;
  capabilities: DeviceCapability[];
}

/** One line of the local-artifact registry: the closed tag, the frozen
 *  label, the frozen format line (ABSENT when the documented table has
 *  none) and the coherent capabilities/reason pair. */
export interface LocalArtifactLine {
  kind: LocalArtifactKind;
  label: string;
  formatLabel?: string;
  available: boolean;
  /** Present IFF the line is available (the documented capability
   *  wording); a deferred line carries the reason instead. */
  capabilitiesLabel?: string;
  /** Present IFF the line is deferred. */
  reason?: string;
}

/** Closed set of the official distribution channels of the
 *  file-association registry. */
export type FileAssociationChannelTag =
  | "linuxSystemPackage"
  | "linuxAppImage"
  | "windowsInstaller"
  | "macosAppBundle";

/** Closed set of the Linux install kinds the pure probe can decide. */
export type LinuxInstallKind = "appImage" | "systemPackage" | "localBuild";

/** One line of the file-association registry: the closed tag, the
 *  frozen label, the registration flag with its frozen status wording,
 *  the frozen detail and — on a non-registered channel ONLY — the
 *  frozen reason (the guard refuses incoherence). */
export interface FileAssociationChannelLine {
  channel: FileAssociationChannelTag;
  label: string;
  registered: boolean;
  statusLabel: string;
  detail: string;
  /** Present IFF the channel does not register by default. */
  reason?: string;
}

/** The Linux install probe's verdict: the closed kind and its frozen
 *  notice — only ever present when the probe SPOKE. */
export interface CurrentInstall {
  kind: LinuxInstallKind;
  notice: string;
}

/** The file-association block of the support profile: the frozen
 *  extension label, the four channel lines in the canonical wire
 *  order, and the current-install verdict (ABSENT when no probe spoke
 *  — Windows/macOS, an indeterminable executable: never invented). */
export interface FileAssociation {
  extensionLabel: string;
  channels: FileAssociationChannelLine[];
  currentInstall?: CurrentInstall;
}

/** The read support profile: every line of the distribution's device
 *  matrix, artifact registry and file-association registry. */
export interface SupportProfile {
  devices: DeviceSupportLine[];
  localArtifacts: LocalArtifactLine[];
  fileAssociation: FileAssociation;
}

/** The frozen family → label couples, exactly as Rust serializes them.
 *  VALIDATION literals only (the rendering keeps using the
 *  Rust-carried values): the guard's job is to refuse a drifted copy
 *  before it is ever rendered as authoritative. */
const FAMILY_LABELS: Readonly<Record<SupportedDeviceFamily, string>> = {
  lunii: "Lunii",
  flam: "FLAM",
};

/** The frozen cohort → label couples. */
const COHORT_LABELS: Readonly<Record<DeviceFirmwareCohort, string>> = {
  origineV1: "Origine v1",
  midGenV2: "Mid-Gen v2",
  v3: "V3",
  flamGen1: "Gen1",
};

/** The frozen cohort → family couples (a V3 line claiming to be a FLAM
 *  is a drift, never a surface to render). */
const COHORT_FAMILIES: Readonly<
  Record<DeviceFirmwareCohort, SupportedDeviceFamily>
> = {
  origineV1: "lunii",
  midGenV2: "lunii",
  v3: "lunii",
  flamGen1: "flam",
};

/** The frozen cohort → metadata-format-label couples — `undefined`
 *  means the key must be ABSENT (FLAM: no version byte is ever
 *  invented). */
const COHORT_FORMAT_LABELS: Readonly<
  Record<DeviceFirmwareCohort, string | undefined>
> = {
  origineV1: "Format métadonnées v3",
  midGenV2: "Format métadonnées v6",
  v3: "Format métadonnées v7",
  flamGen1: undefined,
};

/** The canonical wire order of the four capabilities of a line. */
const OPERATION_ORDER: readonly DeviceOperation[] = [
  "readLibrary",
  "inspectStory",
  "importStory",
  "writeStory",
];

/** The frozen family-invariant operation → label couples; the write
 *  label bifurcates per family below. */
const OPERATION_LABELS: Readonly<
  Record<Exclude<DeviceOperation, "writeStory">, string>
> = {
  readLibrary: "Lecture bibliothèque appareil",
  inspectStory: "Inspection d'histoire",
  importStory: "Copie dans la bibliothèque locale",
};

/** The frozen family → write-label couples (the family is KNOWN on
 *  every line — the neutralize-vs-bifurcate rule). */
const WRITE_LABELS: Readonly<Record<SupportedDeviceFamily, string>> = {
  lunii: "Transfert vers la Lunii",
  flam: "Transfert vers l'appareil",
};

/** The frozen (cohort, operation) → support couples of the OFFICIAL
 *  matrix: `null` = the cell is available (and carries no reason), a
 *  string = the cell is NOT available and carries EXACTLY this frozen
 *  reason. Locking BOTH branches means a regression of the Rust DTO
 *  (a closed cell arriving available, an open cell arriving closed, a
 *  reason-less limit) fails closed instead of rendering. */
const DEVICE_SUPPORT_COUPLES: Readonly<
  Record<DeviceFirmwareCohort, Readonly<Record<DeviceOperation, string | null>>>
> = {
  origineV1: {
    readLibrary: null,
    inspectStory: null,
    importStory: null,
    writeStory: null,
  },
  midGenV2: {
    readLibrary: null,
    inspectStory: null,
    importStory: null,
    writeStory: null,
  },
  v3: {
    readLibrary: null,
    inspectStory: null,
    importStory: "Rétro-ingénierie du format en cours",
    writeStory: "Rétro-ingénierie du format en cours",
  },
  flamGen1: {
    readLibrary: null,
    inspectStory: null,
    importStory: null,
    writeStory: "Écriture non prouvée sur matériel réel",
  },
};

/** The frozen artifact kind → label couples. */
const ARTIFACT_LABELS: Readonly<Record<LocalArtifactKind, string>> = {
  rustoryArtifact: "Artefact d'histoire Rustory (.rustory)",
  structuredFolder: "Dossier structuré",
  structuredArchive: "Archive structurée",
};

/** The frozen artifact kind → format-label couples — `undefined` means
 *  the key must be ABSENT (the documented table has no version). */
const ARTIFACT_FORMAT_LABELS: Readonly<
  Record<LocalArtifactKind, string | undefined>
> = {
  rustoryArtifact: "Format v1",
  structuredFolder: "Format v1",
  structuredArchive: undefined,
};

/** The frozen kind → capabilities-label couples of the AVAILABLE
 *  lines — `undefined` marks the deferred kind (its entry must carry
 *  the reason instead). */
const ARTIFACT_CAPABILITIES_LABELS: Readonly<
  Record<LocalArtifactKind, string | undefined>
> = {
  rustoryArtifact: "Import et export",
  structuredFolder: "Création d'une histoire",
  structuredArchive: undefined,
};

/** The frozen kind → reason couples of the DEFERRED lines. */
const ARTIFACT_LIMIT_REASONS: Readonly<
  Record<LocalArtifactKind, string | undefined>
> = {
  rustoryArtifact: undefined,
  structuredFolder: undefined,
  structuredArchive: "Lecture d'archives non prise en charge",
};

/** The frozen extension label of the file-association block (the
 *  single associated type). */
const FILE_ASSOCIATION_EXTENSION_LABEL = ".rustory";

/** The canonical wire order of the four file-association channels. */
const FILE_ASSOCIATION_CHANNEL_ORDER: readonly FileAssociationChannelTag[] = [
  "linuxSystemPackage",
  "linuxAppImage",
  "windowsInstaller",
  "macosAppBundle",
];

/** The frozen channel → label couples. */
const FILE_ASSOCIATION_CHANNEL_LABELS: Readonly<
  Record<FileAssociationChannelTag, string>
> = {
  linuxSystemPackage: "Paquet Linux (.deb / .rpm)",
  linuxAppImage: "AppImage (Linux)",
  windowsInstaller: "Installeur Windows (.msi / .exe)",
  macosAppBundle: "Application macOS (.dmg)",
};

/** The frozen per-channel couples of the OFFICIAL registry: BOTH
 *  branches locked (registration, status wording, detail, reason) —
 *  the registration itself is a frozen distribution decision, exactly
 *  like `DEVICE_SUPPORT_COUPLES`. `reason: null` = the channel is
 *  registered (and must carry NO reason key). */
const FILE_ASSOCIATION_COUPLES: Readonly<
  Record<
    FileAssociationChannelTag,
    Readonly<{
      registered: boolean;
      statusLabel: string;
      detail: string;
      reason: string | null;
    }>
  >
> = {
  linuxSystemPackage: {
    registered: true,
    statusLabel: "Enregistrée à l'installation",
    detail:
      "L'association est déclarée par le paquet et active dès l'installation.",
    reason: null,
  },
  linuxAppImage: {
    registered: false,
    statusLabel: "Non enregistrée d'office",
    detail:
      "Une AppImage ne modifie pas ton système : rien n'est enregistré automatiquement.",
    reason:
      "Tu peux ajouter l'association avec un outil d'intégration AppImage ou une entrée d'application manuelle.",
  },
  windowsInstaller: {
    registered: true,
    statusLabel: "Enregistrée à l'installation",
    detail:
      "L'installeur déclare l'association. Windows peut te demander de confirmer et respecte ton choix existant.",
    reason: null,
  },
  macosAppBundle: {
    registered: true,
    statusLabel: "Enregistrée par le système",
    detail:
      "macOS enregistre l'association quand l'application est déposée dans Applications.",
    reason: null,
  },
};

/** The frozen install-kind → notice couples of the Linux probe. */
const CURRENT_INSTALL_NOTICES: Readonly<Record<LinuxInstallKind, string>> = {
  appImage:
    "Ton installation actuelle est une AppImage : l'association n'est pas enregistrée d'office.",
  systemPackage:
    "Ton installation actuelle provient d'un paquet système : l'association est enregistrée.",
  localBuild:
    "Cette version de Rustory n'a pas été installée par un paquet officiel : elle n'enregistre pas d'association d'office.",
};

function isDeviceFirmwareCohort(value: unknown): value is DeviceFirmwareCohort {
  return typeof value === "string" && value in COHORT_LABELS;
}

function isLinuxInstallKind(value: unknown): value is LinuxInstallKind {
  return typeof value === "string" && value in CURRENT_INSTALL_NOTICES;
}

function isFileAssociationChannelLine(
  value: unknown,
  expectedChannel: FileAssociationChannelTag,
): value is FileAssociationChannelLine {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  // The four channels arrive in the canonical wire order — a shuffled
  // or duplicated line is a drift.
  if (c.channel !== expectedChannel) return false;
  if (c.label !== FILE_ASSOCIATION_CHANNEL_LABELS[expectedChannel]) {
    return false;
  }
  // BOTH branches are locked on the official couple: the registration
  // itself is a frozen distribution decision, not just its wording.
  const couple = FILE_ASSOCIATION_COUPLES[expectedChannel];
  if (c.registered !== couple.registered) return false;
  if (c.statusLabel !== couple.statusLabel) return false;
  if (c.detail !== couple.detail) return false;
  if (couple.reason === null) {
    // An officially REGISTERED channel must arrive with NO reason key
    // (the status replaces it) — a justified registration is a drift.
    return c.reason === undefined;
  }
  // An officially NON-registered channel must carry EXACTLY its frozen
  // reason — a bare ✗ (or a drifted copy) never renders.
  return c.reason === couple.reason;
}

function isCurrentInstall(value: unknown): value is CurrentInstall {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!isLinuxInstallKind(c.kind)) return false;
  // The notice is the frozen couple of the kind — a drifted copy is
  // never rendered as authoritative.
  return c.notice === CURRENT_INSTALL_NOTICES[c.kind];
}

export function isFileAssociation(value: unknown): value is FileAssociation {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.extensionLabel !== FILE_ASSOCIATION_EXTENSION_LABEL) return false;
  if (!Array.isArray(c.channels)) return false;
  // Exactly the four official channels, in the canonical wire order —
  // a missing line is a drift too: the contract promises the
  // non-registered lines stay VISIBLE with their reasons.
  if (c.channels.length !== FILE_ASSOCIATION_CHANNEL_ORDER.length) {
    return false;
  }
  if (
    !c.channels.every((channel, index) =>
      isFileAssociationChannelLine(
        channel,
        FILE_ASSOCIATION_CHANNEL_ORDER[index],
      ),
    )
  ) {
    return false;
  }
  // The current-install verdict is optional (ABSENT when no probe
  // spoke); when present it must be a frozen couple.
  return c.currentInstall === undefined || isCurrentInstall(c.currentInstall);
}

function isLocalArtifactKind(value: unknown): value is LocalArtifactKind {
  return typeof value === "string" && value in ARTIFACT_LABELS;
}

function isDeviceCapability(
  value: unknown,
  cohort: DeviceFirmwareCohort,
  family: SupportedDeviceFamily,
  expectedOperation: DeviceOperation,
): value is DeviceCapability {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  // The four capabilities arrive in the canonical wire order — a
  // shuffled or duplicated line is a drift.
  if (c.operation !== expectedOperation) return false;
  const expectedLabel =
    expectedOperation === "writeStory"
      ? WRITE_LABELS[family]
      : OPERATION_LABELS[expectedOperation];
  if (c.label !== expectedLabel) return false;
  // BOTH branches are locked on the official support couple: the
  // availability itself is a frozen decision, not just its wording.
  const expectedReason = DEVICE_SUPPORT_COUPLES[cohort][expectedOperation];
  if (expectedReason === null) {
    // An officially OPEN cell must arrive available, with NO reason
    // (the chip replaces it) — a closed rendering here is a drift.
    return c.available === true && c.reason === undefined;
  }
  // An officially CLOSED cell must arrive non-available with EXACTLY
  // its frozen reason — an open rendering (or a bare ✗) is a drift.
  return c.available === false && c.reason === expectedReason;
}

export function isDeviceSupportLine(
  value: unknown,
): value is DeviceSupportLine {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!isDeviceFirmwareCohort(c.cohort)) return false;
  // Family, labels and format line are the FROZEN couples of the
  // cohort — an arbitrary value is a drift, never a copy to render.
  if (c.family !== COHORT_FAMILIES[c.cohort]) return false;
  if (c.familyLabel !== FAMILY_LABELS[COHORT_FAMILIES[c.cohort]]) return false;
  if (c.cohortLabel !== COHORT_LABELS[c.cohort]) return false;
  if (c.metadataFormatLabel !== COHORT_FORMAT_LABELS[c.cohort]) return false;
  if (!Array.isArray(c.capabilities)) return false;
  if (c.capabilities.length !== OPERATION_ORDER.length) return false;
  const family = COHORT_FAMILIES[c.cohort];
  const cohort = c.cohort;
  return c.capabilities.every((capability, index) =>
    isDeviceCapability(capability, cohort, family, OPERATION_ORDER[index]),
  );
}

export function isLocalArtifactLine(
  value: unknown,
): value is LocalArtifactLine {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!isLocalArtifactKind(c.kind)) return false;
  if (c.label !== ARTIFACT_LABELS[c.kind]) return false;
  if (c.formatLabel !== ARTIFACT_FORMAT_LABELS[c.kind]) return false;
  if (c.available === true) {
    // An available line carries EXACTLY its frozen capability wording
    // and no reason. A kind with no frozen wording (the deferred
    // archive) can never arrive available — its capability copy does
    // not exist, so the guard fails closed.
    if (ARTIFACT_CAPABILITIES_LABELS[c.kind] === undefined) return false;
    return (
      c.capabilitiesLabel === ARTIFACT_CAPABILITIES_LABELS[c.kind] &&
      c.reason === undefined
    );
  }
  if (c.available !== false) return false;
  // A deferred line carries EXACTLY its frozen reason and no
  // capability wording. A kind with no frozen reason (the available
  // ones) can never arrive deferred — the screen would show a bare ✗.
  if (ARTIFACT_LIMIT_REASONS[c.kind] === undefined) return false;
  return (
    c.reason === ARTIFACT_LIMIT_REASONS[c.kind] &&
    c.capabilitiesLabel === undefined
  );
}

/**
 * Runtime guard for a [`SupportProfile`] payload: EXACTLY one device
 * line per known cohort, one artifact line per known kind and one
 * channel line per known file-association channel, each locked on its
 * frozen couples. A partial profile (a known cohort, kind or channel
 * missing) is a drift too: the contract promises that the
 * non-available lines stay VISIBLE with their reasons, so their silent
 * disappearance must never render. A refused payload surfaces as a
 * drift error, which the screen treats as a failed profile read —
 * fail-closed per section, never invented content.
 */
export function isSupportProfile(value: unknown): value is SupportProfile {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (!Array.isArray(c.devices)) return false;
  if (!c.devices.every(isDeviceSupportLine)) return false;
  const cohorts = new Set<string>();
  for (const line of c.devices as DeviceSupportLine[]) {
    // A duplicated cohort is a malformed profile, not a surface to
    // render.
    if (cohorts.has(line.cohort)) return false;
    cohorts.add(line.cohort);
  }
  if (
    cohorts.size !== Object.keys(COHORT_LABELS).length ||
    !Object.keys(COHORT_LABELS).every((cohort) => cohorts.has(cohort))
  ) {
    return false;
  }
  if (!Array.isArray(c.localArtifacts)) return false;
  if (!c.localArtifacts.every(isLocalArtifactLine)) return false;
  const kinds = new Set<string>();
  for (const line of c.localArtifacts as LocalArtifactLine[]) {
    if (kinds.has(line.kind)) return false;
    kinds.add(line.kind);
  }
  // Exactly the current closed set — nothing missing, nothing extra.
  if (
    kinds.size !== Object.keys(ARTIFACT_LABELS).length ||
    !Object.keys(ARTIFACT_LABELS).every((kind) => kinds.has(kind))
  ) {
    return false;
  }
  // The file-association block travels with the profile (an additive
  // extension of the same pure read — same drift discipline).
  return isFileAssociation(c.fileAssociation);
}

// ===== Update availability (the `Update Availability Contract`) =====
//
// Mirror of the update-availability DTO of
// `src-tauri/src/ipc/dto/settings.rs`. Rust alone decides the verdict
// and carries the copies; the guard refuses a drifted wire shape so the
// two calm surfaces never render against an arbitrary object. The
// command is INFALLIBLE by contract: a transport failure arrives as the
// `checkUnavailable` STATE, never a rejection — the only facade-side
// rejection is a contract drift.

/** Closed set of the four sealed verdict states. */
export type UpdateAvailabilityStatus =
  | "updateAvailable"
  | "upToDate"
  | "checkUnavailable"
  | "checkNotRun";

/** The read update-availability verdict: the closed status, the frozen
 *  Rust-carried copies (rendered VERBATIM) and the versions in play.
 *  `latestVersion` is present IFF a newer version was found. */
export interface UpdateAvailability {
  status: UpdateAvailabilityStatus;
  headline: string;
  notice: string;
  currentVersion: string;
  latestVersion?: string;
}

/** The strict `MAJOR.MINOR.PATCH` face of a wire version — the TS
 *  mirror of the Rust domain parser's convention (no `v` prefix, no
 *  leading zero, three components). */
const RELEASE_VERSION_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** The Rust domain's component bound: a version the binary can never
 *  emit (a component beyond `u64`) is a drift, not a rendering. */
const U64_MAX = 18446744073709551615n;

/** Parse a wire version under the FULL Rust convention (shape AND the
 *  `u64` bound on every component) — `null` refuses the drift. */
function parseWireReleaseVersion(
  value: unknown,
): [bigint, bigint, bigint] | null {
  if (typeof value !== "string" || !RELEASE_VERSION_PATTERN.test(value)) {
    return null;
  }
  const [major, minor, patch] = value
    .split(".")
    .map((component) => BigInt(component));
  if (major > U64_MAX || minor > U64_MAX || patch > U64_MAX) {
    return null;
  }
  return [major, minor, patch];
}

/** The Rust domain's STRICT `latest > current` — lexicographic on
 *  (major, minor, patch): equality and downgrades never surface as an
 *  available update, so a payload claiming one is a drift. */
function isStrictlyNewer(
  latest: readonly [bigint, bigint, bigint],
  current: readonly [bigint, bigint, bigint],
): boolean {
  if (latest[0] !== current[0]) return latest[0] > current[0];
  if (latest[1] !== current[1]) return latest[1] > current[1];
  return latest[2] > current[2];
}

/** The frozen status → headline couples of the CONSTANT states —
 *  VALIDATION literals only (the rendering keeps the Rust-carried
 *  values); the composed `updateAvailable` copies are validated
 *  STRUCTURALLY below (their byte-for-byte authority lives in the Rust
 *  contract tests). */
const UPDATE_HEADLINES: Readonly<
  Record<Exclude<UpdateAvailabilityStatus, "updateAvailable">, string>
> = {
  upToDate: "Aucune version plus récente n'est publiée.",
  checkUnavailable: "La vérification de version n'a pas pu être faite.",
  checkNotRun: "La vérification de version n'est pas exécutée pour cette copie.",
};

/** The frozen status → notice couples of the CONSTANT states. */
const UPDATE_NOTICES: Readonly<
  Record<Exclude<UpdateAvailabilityStatus, "updateAvailable">, string>
> = {
  upToDate: "Aucune action n'est nécessaire.",
  checkUnavailable:
    "Rustory reste pleinement utilisable. La vérification réessaiera au prochain lancement.",
  checkNotRun:
    "Cette copie de Rustory ne provient pas d'un canal de distribution officiel : aucune vérification réseau n'est effectuée.",
};

/**
 * Runtime guard for an [`UpdateAvailability`] payload, both ways: a
 * valid payload is accepted, a drift is refused — closed status set,
 * versions under the FULL Rust convention (strict shape AND the `u64`
 * component bound), `latestVersion` present IFF `updateAvailable` and
 * STRICTLY newer than `currentVersion` (the domain never signals an
 * equality or a downgrade — a payload claiming one never renders),
 * constant copies locked on their frozen couples and composed copies
 * locked on their structure (headline/notice recomposed from the
 * payload's own versions must match byte-for-byte).
 */
export function isUpdateAvailability(
  value: unknown,
): value is UpdateAvailability {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  const current = parseWireReleaseVersion(c.currentVersion);
  if (current === null) return false;
  if (c.status === "updateAvailable") {
    // The positive state carries the found version — inside the Rust
    // domain (u64 bound) and STRICTLY newer, exactly like the domain's
    // resolution; its copies compose EXACTLY from the payload's own
    // versions.
    const latest = parseWireReleaseVersion(c.latestVersion);
    if (latest === null || !isStrictlyNewer(latest, current)) {
      return false;
    }
    if (c.headline !== `Nouvelle version disponible : ${c.latestVersion}.`) {
      return false;
    }
    return (
      c.notice ===
      `Ta version actuelle est ${c.currentVersion}. Récupère la nouvelle version depuis la page officielle des versions : github.com/roukmoute/Rustory/releases.`
    );
  }
  if (
    c.status !== "upToDate" &&
    c.status !== "checkUnavailable" &&
    c.status !== "checkNotRun"
  ) {
    return false;
  }
  // `latestVersion` is present IFF `updateAvailable` — a stray key on a
  // constant state is a drift.
  if (c.latestVersion !== undefined) return false;
  return (
    c.headline === UPDATE_HEADLINES[c.status] &&
    c.notice === UPDATE_NOTICES[c.status]
  );
}

// ===== Update apply (the `Update Apply Contract`) =====
//
// Mirror of the update-apply DTOs and `update:*` event payloads of
// `src-tauri/src/ipc/dto/settings.rs` + `src-tauri/src/ipc/events.rs`.
// Rust alone decides the plan, drives the gesture and carries every
// copy; the guards refuse a drifted wire shape so the gesture zone
// never renders against an arbitrary object. The commands are
// INFALLIBLE by contract: refusals are STATES of the payloads — the
// only facade-side rejection is a contract drift.

/** Closed set of the manual-plan reasons of the gesture gate. */
export type UpdateApplyManualReason =
  | "development_build"
  | "unofficial_install"
  | "package_manager_owned"
  | "channel_unproven"
  | "trust_chain_not_configured";

/** The read gesture plan: integrated (this copy may install updates) or
 *  manual with its frozen reason; the Rust-carried couple renders
 *  VERBATIM. `reason` is present IFF manual. */
export interface UpdateApplyPlan {
  mode: "integrated" | "manual";
  reason?: UpdateApplyManualReason;
  headline: string;
  guidance: string;
}

/** Closed set of the phases of a gesture in flight. */
export type UpdateApplyPhaseTag = "checking" | "downloading" | "installing";

/** Closed set of the failure stages of the gesture. */
export type UpdateApplyFailureStageTag =
  | "feed"
  | "not_applicable"
  | "download"
  | "verification"
  | "install";

/** The read SESSION state of the gesture — the authoritative re-read
 *  the zone always trusts over events. Strict omission discipline:
 *  `jobId`/`phase`/`percent` exist IFF running, `stage` IFF failed, the
 *  copies IFF the state carries any. `jobId` makes a live flight
 *  recoverable from the re-read alone (renderer reload, lost local
 *  tracking): the zone re-attaches its event subscription to it. */
export interface UpdateApplyState {
  status: "idle" | "running" | "readyToRestart" | "failed";
  jobId?: string;
  phase?: UpdateApplyPhaseTag;
  percent?: number;
  stage?: UpdateApplyFailureStageTag;
  headline?: string;
  notice?: string;
}

/** The start decision: a refusal is a STATE, never an error. `jobId`
 *  is present IFF started. */
export interface StartUpdateApplyOutcome {
  outcome: "started" | "alreadyRunning" | "notEligible";
  jobId?: string;
}

/** `update:progress` payload — SAMPLED transitions of the gesture in
 *  flight; `percent` present IFF a reliable integer fraction is known. */
export interface UpdateApplyProgressEvent {
  jobId: string;
  phase: UpdateApplyPhaseTag;
  percent?: number;
  sequence: number;
}

/** `update:completed` payload — the successful terminal (applied,
 *  restart pending as a USER gesture). */
export interface UpdateApplyCompletedEvent {
  jobId: string;
  sequence: number;
}

/** `update:failed` payload — the failure terminal with its closed stage
 *  and Rust-carried copies. */
export interface UpdateApplyFailedEvent {
  jobId: string;
  sequence: number;
  stage: UpdateApplyFailureStageTag;
  headline: string;
  notice: string;
}

/** The frozen plan couples (headline, guidance) — VALIDATION literals
 *  only (the rendering keeps the Rust-carried values): `integrated`
 *  plus one couple per manual reason. */
const APPLY_PLAN_COUPLES: Readonly<
  Record<"integrated" | UpdateApplyManualReason, readonly [string, string]>
> = {
  integrated: [
    "Cette copie peut installer les mises à jour de Rustory.",
    "Le téléchargement vérifie l'authenticité de la mise à jour avant de l'installer.",
  ],
  development_build: [
    "La mise à jour intégrée n'est pas disponible pour un build de développement.",
    "Reconstruis Rustory depuis les sources pour obtenir la dernière version.",
  ],
  unofficial_install: [
    "La mise à jour intégrée n'est pas disponible pour cette copie.",
    "Cette copie n'est pas passée par un canal de distribution officiel. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
  package_manager_owned: [
    "La mise à jour de Rustory passe par ton gestionnaire de paquets.",
    "Cette copie a été installée comme paquet système : mets-la à jour avec l'outil de ton système, puis relance Rustory.",
  ],
  channel_unproven: [
    "La mise à jour intégrée n'est pas encore disponible pour cette installation.",
    "Rustory ne peut pas confirmer le canal de cette copie. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
  trust_chain_not_configured: [
    "La mise à jour intégrée n'est pas encore activée pour cette copie.",
    "Cette copie ne peut pas vérifier l'authenticité des mises à jour. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
};

/** The frozen phase → running-headline couples. */
const APPLY_RUNNING_HEADLINES: Readonly<Record<UpdateApplyPhaseTag, string>> = {
  checking: "Vérification de la mise à jour en cours…",
  downloading: "Téléchargement de la mise à jour en cours…",
  installing: "Installation de la mise à jour en cours…",
};

/** The frozen COMMON running notice. */
const APPLY_RUNNING_NOTICE =
  "Tu peux continuer à utiliser Rustory pendant cette opération.";

/** The frozen ready-to-restart couple. */
const APPLY_READY_COUPLE: readonly [string, string] = [
  "La mise à jour de Rustory est prête.",
  "Redémarre Rustory pour terminer l'installation. Ton travail local reste en place.",
];

/** The frozen stage → (headline, notice) couples of a failed gesture. */
const APPLY_FAILED_COUPLES: Readonly<
  Record<UpdateApplyFailureStageTag, readonly [string, string]>
> = {
  feed: [
    "Le canal de mise à jour n'a pas répondu.",
    "Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
  not_applicable: [
    "La mise à jour n'est pas encore proposée pour cette installation.",
    "La nouvelle version n'est pas encore publiée sur le canal de mise à jour de cette copie. La page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
  download: [
    "Le téléchargement de la mise à jour n'a pas abouti.",
    "Rustory reste sur sa version actuelle. Vérifie ta connexion, puis réessaie.",
  ],
  verification: [
    "L'authenticité de la mise à jour n'a pas pu être confirmée.",
    "Rien n'a été installé : Rustory reste sur sa version actuelle. Réessaie plus tard ; la page officielle des versions reste disponible : github.com/roukmoute/Rustory/releases.",
  ],
  install: [
    "L'installation de la mise à jour n'a pas abouti.",
    "Ta version actuelle de Rustory reste en place et utilisable. Réessaie, ou passe par la page officielle des versions : github.com/roukmoute/Rustory/releases.",
  ],
};

/** Closed-table membership WITHOUT consulting the prototype chain: an
 *  inherited key (`constructor`, `toString`, `__proto__`…) must be
 *  refused as an IPC drift, never resolved into a prototype value that
 *  would then crash the couple destructuring. */
function hasOwnKey<T extends object>(
  table: T,
  key: unknown,
): key is keyof T & string {
  return (
    typeof key === "string" &&
    Object.prototype.hasOwnProperty.call(table, key)
  );
}

function isUpdateApplyPhaseTag(value: unknown): value is UpdateApplyPhaseTag {
  return hasOwnKey(APPLY_RUNNING_HEADLINES, value);
}

function isUpdateApplyFailureStageTag(
  value: unknown,
): value is UpdateApplyFailureStageTag {
  return hasOwnKey(APPLY_FAILED_COUPLES, value);
}

/** An integer 0..=100 — the only percent the wire may carry. */
function isWirePercent(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 100
  );
}

/** A strictly-typed wire sequence: a non-negative integer. */
function isWireSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

/**
 * Runtime guard for an [`UpdateApplyPlan`] payload, both ways: closed
 * mode set, `reason` present IFF manual (from the closed reason set),
 * and the (headline, guidance) couple locked byte-for-byte on the
 * frozen couple of the mode/reason — a drifted copy never renders as
 * authoritative.
 */
export function isUpdateApplyPlan(value: unknown): value is UpdateApplyPlan {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.mode === "integrated") {
    if (c.reason !== undefined) return false;
    const [headline, guidance] = APPLY_PLAN_COUPLES.integrated;
    return c.headline === headline && c.guidance === guidance;
  }
  if (c.mode !== "manual") return false;
  if (!hasOwnKey(APPLY_PLAN_COUPLES, c.reason)) {
    return false;
  }
  if (c.reason === "integrated") return false;
  const [headline, guidance] =
    APPLY_PLAN_COUPLES[c.reason as UpdateApplyManualReason];
  return c.headline === headline && c.guidance === guidance;
}

/**
 * Runtime guard for an [`UpdateApplyState`] payload, both ways: closed
 * status set, strict omission discipline per status (`phase`/`percent`
 * IFF running, `stage` IFF failed, copies IFF the state carries any),
 * integer percent, and every copy locked on its frozen couple.
 */
export function isUpdateApplyState(value: unknown): value is UpdateApplyState {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  switch (c.status) {
    case "idle":
      return (
        c.jobId === undefined &&
        c.phase === undefined &&
        c.percent === undefined &&
        c.stage === undefined &&
        c.headline === undefined &&
        c.notice === undefined
      );
    case "running": {
      // A live flight is ALWAYS correlatable: the Rust session mints the
      // id atomically with the single-flight claim — a running state
      // without one is a drift, never a surface to render.
      if (typeof c.jobId !== "string" || c.jobId.length === 0) return false;
      if (!isUpdateApplyPhaseTag(c.phase)) return false;
      if (c.percent !== undefined && !isWirePercent(c.percent)) return false;
      if (c.stage !== undefined) return false;
      return (
        c.headline === APPLY_RUNNING_HEADLINES[c.phase] &&
        c.notice === APPLY_RUNNING_NOTICE
      );
    }
    case "readyToRestart":
      return (
        c.jobId === undefined &&
        c.phase === undefined &&
        c.percent === undefined &&
        c.stage === undefined &&
        c.headline === APPLY_READY_COUPLE[0] &&
        c.notice === APPLY_READY_COUPLE[1]
      );
    case "failed": {
      if (!isUpdateApplyFailureStageTag(c.stage)) return false;
      if (
        c.jobId !== undefined ||
        c.phase !== undefined ||
        c.percent !== undefined
      ) {
        return false;
      }
      const [headline, notice] = APPLY_FAILED_COUPLES[c.stage];
      return c.headline === headline && c.notice === notice;
    }
    default:
      return false;
  }
}

/**
 * Runtime guard for a [`StartUpdateApplyOutcome`] payload: closed
 * outcome set, `jobId` a non-empty string present IFF started.
 */
export function isStartUpdateApplyOutcome(
  value: unknown,
): value is StartUpdateApplyOutcome {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (c.outcome === "started") {
    return typeof c.jobId === "string" && c.jobId.length > 0;
  }
  if (c.outcome !== "alreadyRunning" && c.outcome !== "notEligible") {
    return false;
  }
  return c.jobId === undefined;
}

/** Runtime guard for an `update:progress` payload. */
export function isUpdateApplyProgressEvent(
  value: unknown,
): value is UpdateApplyProgressEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.jobId !== "string" || c.jobId.length === 0) return false;
  if (!isUpdateApplyPhaseTag(c.phase)) return false;
  if (c.percent !== undefined && !isWirePercent(c.percent)) return false;
  return isWireSequence(c.sequence);
}

/** Runtime guard for an `update:completed` payload. */
export function isUpdateApplyCompletedEvent(
  value: unknown,
): value is UpdateApplyCompletedEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.jobId !== "string" || c.jobId.length === 0) return false;
  return isWireSequence(c.sequence);
}

/** Runtime guard for an `update:failed` payload — the (headline,
 *  notice) couple is locked on the frozen couple of its stage. */
export function isUpdateApplyFailedEvent(
  value: unknown,
): value is UpdateApplyFailedEvent {
  if (typeof value !== "object" || value === null) return false;
  const c = value as Record<string, unknown>;
  if (typeof c.jobId !== "string" || c.jobId.length === 0) return false;
  if (!isWireSequence(c.sequence)) return false;
  if (!isUpdateApplyFailureStageTag(c.stage)) return false;
  const [headline, notice] = APPLY_FAILED_COUPLES[c.stage];
  return c.headline === headline && c.notice === notice;
}
