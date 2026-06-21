# Device Support Profile

## Purpose

Authoritative public matrix of the device families, firmware cohorts
and operations Rustory is officially allowed to perform. Every line
here MUST have a corresponding test in
`src-tauri/src/application/device/mod.rs::tests::check_operation_*`
and a domain entry in `src-tauri/src/domain/device/profile.rs`.

This document is the source of truth that the
`application::device::check_operation_allowed` capability gate
implements. Any divergence between the matrix below and the gate
behavior is a bug.

## MVP Phase 1 Matrix

| Famille | Cohort firmware | Format métadonnées | Lecture biblio | Inspection histoire | Import histoire | Écriture (transfert) |
| --- | --- | --- | --- | --- | --- | --- |
| Lunii | Origine v1 (fw 1.x / 2.x) | v3 | ✅ | ✅ | ✅ | ❌ (Phase 1 — Epic 3 wires the gate) |
| Lunii | Mid-Gen v2 (fw 3.0 – 3.1) | v6 | ✅ | ✅ | ✅ | ❌ (Phase 1 — Epic 3 wires the gate) |
| Lunii | V3 (fw 3.2.x +) | v7 | ✅ | ✅ | ❌ (RE actif — corruption risk) | ❌ (Phase 1 — Epic 3 wires the gate) |
| FLAM | — | — | ❌ (Post-MVP) | ❌ | ❌ | ❌ |

## Detection Strategy

Rustory recognizes a Lunii in two stages:

1. **Auto-mount (Linux only)**: before each scan, Rustory asks
   udisks2 (over D-Bus, via the `zbus` blocking client) to mount any
   block device whose `Drive` path contains "STM" (the signature of
   the STM32-based USB bridge every observed Lunii ships with), whose
   `IdType` is `vfat`, whose `IdUsage` is `filesystem`, and whose
   `MountPoints` are empty. This filter intentionally rejects generic
   USB sticks (SanDisk, Kingston, …) so Rustory never mutates
   unrelated media without the user's intent. macOS and Windows mount
   USB Mass Storage volumes automatically — the auto-mount path is a
   no-op on those platforms. Set `RUSTORY_DEVICE_AUTOMOUNT=0` to
   disable it entirely.
2. **Filesystem scan**: Rustory enumerates mounted USB Mass Storage
   volumes (via `sysinfo::Disks` and the optional
   `RUSTORY_DEVICE_MOUNT_ROOTS` env-injected list) and probes each one
   for the canonical marker set at the volume root:

| Marker | Required | Purpose |
| --- | --- | --- |
| `.md` | ✅ | Primary identifier; first byte = metadata format version |
| `.pi` | ✅ | Device-id payload; hashed (SHA-256, truncated to 32 hex chars) into the opaque `device_identifier` |
| `.bt` | informational | Binary token marker; surfaced for diagnostics but does NOT gate classification — a real Lunii V3 fw 3.3.2 was observed without `.bt` |
| `.ri` | informational | ROM info — not required by the MVP scan |
| `.li` | informational | Library info — not required by the MVP scan |

References (cross-checked across public OSS reverse-engineering
projects AND validated against a physical Lunii V3 sample, 2026-04-26):

- `marian-m12l/studio` (Java) — supports metadata v3 / v6 / v7.
- `o-daneel/Lunii.QT` (Python+Qt) — README documents the marker set
  for V1 / V2 / V3 cross-platform.
- `o-daneel/Lunii.RE` — Ghidra reverse engineering of the STM32
  firmwares; binary constants source.
- physical Lunii V3 fw 3.3.2 (2026-04-26): `.md` 128 B (first byte
  `0x07`) + `.pi` 32 B + `.pi.hidden` + `.cfg` + `.content/` + `.logo`
  + `etc/` — NO `.bt` present. This sample is the empirical proof
  that `.bt` cannot be a required marker.

## Library Inventory

Reading the installed-story inventory of a connected Lunii reuses the
same volume that detection already classified — it does not re-open a
second bridge. The `mount_path` discovered during the scan stays
Rust-side and is handed to the inventory reader; it never crosses IPC.

The `.pi` marker carries **two** readings of the same bytes:

| Reading | Used by | Interpretation |
| --- | --- | --- |
| Opaque payload | detection | hashed (with the volume serial when available) into the `device_identifier` |
| Pack index | inventory | an ORDERED list of installed pack UUIDs, **16 bytes each**, read back to back until EOF |

`.pi.hidden` (same 16-byte layout) lists packs the user hid; it is
optional and read best-effort. The two lists are disjoint: `.pi` =
visible, `.pi.hidden` = hidden.

Each pack owns a sub-folder under `.content/`, named with the **uppercase
last 8 hexadecimal characters** of its UUID (the tail of the canonical
`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` form). The inventory reader probes
`.content/<SHORT_ID>` per pack: a missing folder flags an
orphan/ambiguous entry rather than dropping it.

Key properties:

- **No decryption.** Enumerating the inventory touches only the index
  files and folder names. Media ciphering is irrelevant to listing —
  which is why `Lecture biblio` is `✅` for every supported cohort,
  including V3 / metadata v7 (only import/write stay gated for V3).
- **No durable mirror.** The inventory is a transient, instant truth.
  It is held in memory for the current view and re-read on demand; it is
  never persisted into SQLite as a device-content mirror.
- **No on-device title.** The device stores no human-readable title or
  cover for official packs — only the UUID. The inventory reader surfaces
  each entry by its opaque short identifier; the human title is composed
  on top by the title-recognition layer (see below), never asserted by the
  reader itself.

Read bounds (separate from detection):

| Bound | Value | Why |
| --- | --- | --- |
| Detection `.md` / `.pi` cap | `4 KB` (`MAX_METADATA_FILE_BYTES`) | sized for the short marker reads on the scan path |
| Inventory `.pi` cap | `64 KB` (`MAX_PACK_INDEX_BYTES`) | a library over 256 packs has a `.pi` bigger than 4 KB (256 × 16 = 4096); reusing the detection cap would silently truncate the inventory |

> **Known ceiling.** The detection scanner still reads `.pi` under the
> 4 KB cap to compute the identifier, so a device whose `.pi` exceeds
> 4 KB (more than ~256 packs) is not classified as supported and never
> reaches the inventory reader. Raising the detection cap for very large
> libraries is deferred; realistic household libraries stay well under
> this ceiling.

References cross-checked against the same public OSS reverse-engineering
projects as the marker set above (notably the pack-index format: 16-byte
UUIDs in `.pi`, the `.content/<SHORT_ID>` folder convention, and the
"listing needs no key" property).

## Title Recognition & Catalog Policy

The device exposes only UUIDs, so recognizing a story means looking its
UUID up in a LOCAL `UUID → title` index. Resolution is Rust-authoritative
and applies a fixed priority; the wire DTO carries the resolved `title` +
`titleSource`, and the frontend never recomposes the truth.

| Priority | Source (`titleSource`) | Origin | Trust |
| --- | --- | --- | --- |
| 1 | `user` | a name the user typed for the pack | highest — never silently overwritten |
| 2 | `official` | Lunii's commercial catalog, cached locally | verified — the only label shown as "officiel" |
| 3 | `unofficial` | inferred offline from a local story linked to the pack (import provenance) | local-library truth |
| — | (none) | no index covers the pack | shown as "non reconnue" |

The `user > official > unofficial` order is enforced once, in the
application layer; the persistence table (`pack_metadata`) holds one row per
`(pack_uuid, source)` so a user title and an official title can coexist for
the same pack without collision.

**Catalog policy (offline-first / anti-catalog).**

- The official catalog is fetched ONLY on an explicit user action ("Récupérer
  / mettre à jour"). There is no implicit network traffic and never any fetch
  during a device read. A 100%-offline alternative imports the catalog from a
  user-provided file.
- The official cache is **disposable**: a refresh replaces every `official`
  row wholesale and never touches `user` rows.
- A downloaded or imported catalog is **untrusted input**: every title is
  normalized + validated with the local-story title rules (NFC + trim +
  denylist + ≤120), every UUID must be canonical; invalid entries are
  skipped, never executed.
- A refresh that parses to **zero** recognized entries is refused (the
  previous cache is kept) so a server blip / wrong-shaped response can never
  silently wipe good titles. Network reads are byte-bounded; the whole
  auth + packs + covers cycle shares one wall-clock budget.
- Honesty: a user-typed or community title is NEVER presented as "officiel".

**Covers.** The catalog references covers as RELATIVE paths
(`/public/images/packs/…`) under a CDN host. Offline-first forbids fetching
them on display, so covers are downloaded EAGERLY during the explicit network
refresh into a disposable local cache (`{app_data}/catalog-covers/<uuid>.<ext>`):
downloaded bytes are validated by image magic-bytes and bounded, the path is a
fixed `<uuid>.<ext>` (no traversal), and `pack_metadata.thumbnail` stores ONLY
the local file name — never a remote URL. The UI loads a cover via the
`read_pack_cover` command (a local read returning a `data:` URL, no network).
Cover download is best-effort (a failure leaves a pack cover-less, never fails
the catalog); the offline FILE import path caches no cover (that would be
network).

The `unofficial` source is also reserved for a future opt-in community index;
its governance/licensing is out of current scope.

## Story Import Contract

Importing a device story ("Copier dans ma bibliothèque") is a **raw,
structurally validated acquisition without decryption**: Rustory copies
the pack bytes as-is from `.content/<SHORT_ID>` into its managed local
storage and never interprets the ciphered content. This prolongs the
"No decryption" property above — decoding (XXTEA, `ni/li/ri/si` parsing,
media) is the transfer/edition scope of later phases, not the import.

The import is **all-or-nothing per pack**. The declared supported subset
is a closed set of entry names at the pack root:

| Entry | Status | Rule |
| --- | --- | --- |
| `ni`, `li`, `ri`, `si` | required | must exist as non-empty regular files |
| `nm`, `bt` | optional | copied when present (regular files) |
| `rf/`, `sf/` | optional asset trees | regular files only, depth ≤ 2 below the tree root |
| `Thumbs.db`, `.DS_Store`, `._*` | OS cruft ignore-list | silently skipped, never copied |
| anything else | refused | unknown entry ⇒ explicit `pack_invalid` refusal, no blind copy |

Bounds (validated before and during the copy):

| Bound | Value | Why |
| --- | --- | --- |
| `MAX_IMPORT_PACK_BYTES` | 2 GiB | a pack beyond this is outside any observed real library |
| `MAX_IMPORT_PACK_FILES` | 4096 | bounds enumeration and manifest size |
| `MAX_PACK_ASSET_DEPTH` | 2 | `rf/` and `sf/` trees are flat in practice (`rf/000/…`) |
| File kind | regular files only | symlinks and special files are refused (`symlink_metadata`) |

Atomicity & provenance:

- Sequence: staging copy (`{app_data_dir}/imports/.staging/`) →
  structural validation → atomic `rename` promotion to
  `{app_data_dir}/imports/<story_id>/` → SQLite commit (canonical
  `stories` row + `story_imports` provenance row). Files first, DB
  second: a DB row must never reference files that are not known to
  exist and be valid. Any intermediate failure cleans the staging and
  leaves no DB row and no orphan folder.
- Provenance is persisted in `story_imports` (link `pack_uuid ↔
  story_id`, source device identifier, timestamp, file count, total
  bytes, aggregate SHA-256 checksum). The link is UNIQUE on
  `pack_uuid`: re-importing the same pack is refused
  (`already_imported`) while the link exists — even across devices,
  the pack UUID is the content identity.
- The device mount is **never written** (read-only end to end), and
  the import never blocks the UI (async command, bounded budget).
- The wall-clock budget (300 s) is enforced **cooperatively**: the
  deadline is checked between files and between copy chunks. A single
  `read`/`write` syscall blocked by a stalled mount (dying USB bridge,
  hung FUSE) cannot be interrupted mid-call — an accepted MVP residual.
  The copy runs on a background worker, so the UI stays responsive
  regardless; a physically yanked device surfaces as a kernel I/O error
  on the next call. Hard per-syscall bounding is deferred to the
  transfer job contract (post-MVP), which will own cancellable I/O.
- After the atomic promotion, the promoted directory tree AND its
  parent are fsynced BEFORE the SQLite commit, so a post-commit crash
  cannot leave a committed row pointing at directory entries the
  filesystem never persisted (files-first invariant holds across power
  loss).
- A hidden pack (`.pi.hidden`) is importable: hidden is a device-side
  display state, not a content defect.

V3 (metadata v7) stays ❌ for import — the matrix above is unchanged
and the capability gate (`check_operation_allowed(ImportStory)`) is
consulted before any acquisition.

## Refusal Reasons (closed set)

When classification refuses a candidate, the wire DTO carries a
`reason` value from this fixed set. Each value maps to one canonical
panel copy in `docs/architecture/ui-states.md#Disabled Actions and
Reasons`.

| Wire `reason` | Domain `UnsupportedReason` | Trigger |
| --- | --- | --- |
| `firmwareUnsupported` | `FirmwareUnsupported` | Reserved for future per-firmware blocklists |
| `metadataUnsupported` | `MetadataUnsupported` | `.md` first byte is not in `{3, 6, 7}` |
| `metadataCorrupt` | `MetadataCorrupt` | `.pi` missing or empty, `.md` empty / oversize, FS read failed (`.bt` is informational only and never gates this reason) |
| `familyUnknown` | `FamilyUnknown` | Reserved for non-Lunii families discovered later |
| `operationNotAuthorized` | `OperationNotAuthorized` | Capability gate refusal at Epic 3 wiring time |
| `multipleCandidates` | `MultipleCandidates` | More than one supported Lunii detected at once |

## Capability Gate Contract

`application::device::check_operation_allowed(profile, operation)`:

- Returns `Ok(())` only when `profile.supported_operations.{operation}` is `true`.
- Returns `Err(AppError::DeviceUnsupported)` otherwise, with
  `details = { source: "capability_gate", operation: "<tag>",
  family: "<tag>", firmware_cohort: "<tag>" }`.
- MUST be called BEFORE any device write attempt. NFR17 + NFR18
  fail-closed.
- Its refusal is actionable, never opaque: the `message` and
  `userAction` are both non-empty. The `import_story` refusal (the V3
  case: inspectable but not importable) is surfaced in the device-story
  inspector as `Copie indisponible: profil non supporté` plus a
  `Consulter le profil de support` next gesture — parity with the
  detection panel (see
  [ui-states.md#Device Story Inspection Contract](./ui-states.md)).

Read vs. write coherence (transfer preview): the pre-send comparison
(`read_transfer_preview`, see
[ui-states.md#Transfer Decision / Comparison Contract](./ui-states.md)) is a
**read-only** snapshot. It passes the `ReadLibrary` gate (allowed for every
supported MVP cohort) to enumerate the device inventory; it never attempts a
write. The transfer CTA it sits beside is governed by the `WriteStory`
operation, which is hard-coded `false` for every cohort in MVP Phase 1 — so the
preview can show *what would change* while the send stays disabled with its
standardized reason. The preview therefore reports `transferable = false` for
every supported profile, mirroring the gate; Epic 3 wires the real
`WriteStory` gate.

Read vs. write coherence (story validation / preflight): the per-story
validation (`read_story_validation`, see
[ui-states.md#Story Validation / Preflight Contract](./ui-states.md)) is
also a **read-only** snapshot. Its Lunii-compatibility axis reuses
`read_device_library` (the `ReadLibrary` gate + `classify_lunii`): a verdict is
composed ONLY for a CONFIRMED readable supported device (the `Readable`
outcome, whose identity matched the request), which is compatible by
construction — so no `deviceProfile` blocker is ever emitted in MVP. A re-scan
that no longer resolves to that device (none / unsupported / ambiguous) cannot
prove the present device is the requested one, so it surfaces a recoverable
`device_changed` (a `DEVICE_SCAN_FAILED` transport error), never a compatibility
verdict on an unconfirmed device. The `deviceProfile` axis and its
`UnsupportedReason`-derived causes stay DECLARED in the closed wire taxonomy
(ready for a future device-format validation) but have no live emitter in MVP
Phase 1 — exactly like the `media` / `filesystem` axes. The validation verdict
NEVER consults `WriteStory` — transfer activation stays governed by that gate
(`false` for every cohort in MVP Phase 1) and is orthogonal to the verdict, so a
`présumée transférable` story still sits beside a disabled send CTA.

Adding a new operation:

1. Add a boolean field on `SupportedOperations`.
2. Add the matching `SupportedOperation` enum variant + diagnostic tag.
3. Update the matrix above with the per-cohort allow value.
4. Add a per-cohort × per-operation test in `tests::check_operation_*`.

Adding a new cohort:

1. Add a `LuniiFirmwareCohort` variant + diagnostic tag.
2. Add the metadata-version → cohort branch in `classify_lunii`.
3. Update the matrix above with the operation values.
4. Add a per-version classification test.

A line in the matrix that has no test is a bug — the test enforces
that the gate behavior matches the published policy.
