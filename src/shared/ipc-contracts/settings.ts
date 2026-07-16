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

/** The read support profile: every line of the distribution's device
 *  matrix and artifact registry. */
export interface SupportProfile {
  devices: DeviceSupportLine[];
  localArtifacts: LocalArtifactLine[];
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

function isDeviceFirmwareCohort(value: unknown): value is DeviceFirmwareCohort {
  return typeof value === "string" && value in COHORT_LABELS;
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
 * line per known cohort and one artifact line per known kind, each
 * locked on its frozen couples. A partial profile (a known cohort or
 * kind missing) is a drift too: the contract promises that the
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
  return (
    kinds.size === Object.keys(ARTIFACT_LABELS).length &&
    Object.keys(ARTIFACT_LABELS).every((kind) => kinds.has(kind))
  );
}
