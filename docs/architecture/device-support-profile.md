# Device Support Profile

## Purpose

Authoritative public matrix of the device families, firmware cohorts
and operations Rustory is officially allowed to perform. Every line
here MUST have a corresponding test in
`src-tauri/src/application/device/mod.rs::tests::check_operation_*`
and a domain entry in `src-tauri/src/domain/device/profile.rs`.

The device matrix and the local-artifact registry below are ENUMERABLE
in the pure domain (`src-tauri/src/domain/device/support_matrix.rs`,
`src-tauri/src/domain/import/local_artifact.rs`) and served in-app by
the pure `read_support_profile` command: the classifiers and the
`Profil de support` screen (route `/settings`, see
[ui-states.md#Support Profile Screen Contract](./ui-states.md)) consult
the SAME single truth. The `Consulter le profil de support` gesture
targets that internal screen — this document stays the detailed
developer reference.

This document is the source of truth that the
`application::device::check_operation_allowed` capability gate
implements. Any divergence between the matrix below and the gate
behavior is a bug.

## MVP Phase 1 Matrix

| Famille | Cohort firmware | Format métadonnées | Lecture biblio | Inspection histoire | Import histoire | Écriture (transfert) |
| --- | --- | --- | --- | --- | --- | --- |
| Lunii | Origine v1 (fw 1.x / 2.x) | v3 | ✅ | ✅ | ✅ | ✅ (round-trip d'une histoire importée) |
| Lunii | Mid-Gen v2 (fw 3.0 – 3.1) | v6 | ✅ | ✅ | ✅ | ✅ (round-trip d'une histoire importée) |
| Lunii | V3 (fw 3.2.x +) | v7 | ✅ | ✅ | ❌ (RE actif — corruption risk) | ❌ (RE actif — même rationale que l'import) |
| FLAM | Gen1 (flam_gen1) | — | ✅ | ✅ | ✅ | ❌ (écriture non prouvée — décisions de format sur matériel réel requises) |

FLAM Gen1 is a **recognized** profile whose READ-side capabilities are
activated line by line: library inventory, story inspection and story
import are ✅ (their contract is the "FLAM library inventory & story
import" section below), while every device WRITE stays ❌. The update
flow (write semantics on an already-present pack, below) now EXISTS
without activating FLAM writes: the write column only flips once the
FLAM on-device format decisions are proven on real hardware (see the
deferred-work ledger) — a flow existing family-generically never
weakens the gate. Its metadata format column stays `—`:
the internal structure of `.mdf` is not publicly documented and Rustory
refuses to invent a version byte (see the FLAM recognition markers
below). The general rule stays in force for any FUTURE zero-capability
profile: recognition proves the device is officially known (the panel
renders `Appareil reconnu — {famille}`, never a lying `Profil non
supporté`) while every operation stays ❌ until support activates them
line by line — recognition and capability are separate facts.

The write column is wired by the transfer flow: `WriteStory` is `true` for
**Origine v1** and **Mid-Gen v2**, and stays `false` for **V3** (device-write
reverse-engineering is still active — same rationale as import) and **FLAM**. The
realistic MVP write is the **round-trip of an imported story** (re-writing the
opaque pack bytes back to the device, zero decryption); native stories have no
device-format pack and are not transferable until a media transformer exists.

The writer reports **progress** (bytes / files copied) during the measurable
content-copy step so the UI can show an honest fraction — a named phase otherwise,
never a fabricated value, never 100 % before the terminal. It also signals whether
the **device mutation has started** (`reached_device_mutation`): the staging copy
on the device volume is pre-mutation (the device is untouched), while the atomic
`rename` promotion to `.content/<SHORT_ID>`, the `fsync` of the promoted tree, and
the `.pi` index update are post-mutation. From this the flow derives the two honest
interruption terminals — **`échoué`** (untouched → recoverable) vs **`incomplet`**
(mutation started → the device may hold a partial copy). The invariant stays
**files first, index last** (a pack is never indexed without its content present);
there is **no resume** (a relaunch is a full cycle, and the writer PROVES the
state of an existing target pack — reusing it, replacing it, or refusing an
unprovable one, see "Write on an already-present pack" below — so it converges
safely); orphan staging directories (`.rustory-staging-*`) and set-aside
replaced packs (`.rustory-replaced-*`) are swept best-effort.

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

### FLAM recognition markers

FLAM volumes are recognized during the same stage-2 filesystem scan,
with their own marker set at the volume root. **Lunii precedence is
fixed**: a volume carrying a regular `.md` file is probed as a Lunii
candidate even when `.mdf` coexists — the Lunii probe path is never
altered by FLAM detection. Only a volume WITHOUT a `.md` entry and
WITH `.mdf` enters the FLAM probe: a `.md` entry of any OTHER shape
(directory, broken symlink, special file) keeps the volume out of
BOTH probes — ignored, exactly the pre-FLAM behavior.

| Marker | Required | Rule |
| --- | --- | --- |
| `.mdf` | ✅ | Primary FLAM identifier. Must be a REGULAR file, read no-follow (`symlink_metadata` refusal of symlinks/irregular files, open with `O_NOFOLLOW \| O_NONBLOCK` on Unix, then a `(dev, ino)` re-check of the opened handle against the lstat), within `MAX_METADATA_FILE_BYTES` (4 KiB). An EMPTY `.mdf` still surfaces the candidate (classified `metadataCorrupt` so a broken FLAM is SEEN and explained, never silently skipped); an OVERSIZE `.mdf` means "not a plausible FLAM" and the volume is ignored; a per-volume I/O error (open/read failure) IGNORES the volume and the scan continues — it never escalates to a scan-level error, so a failing FLAM volume cannot mask a healthy candidate on another mount (only the shared scan deadline escalates). |
| `str/` | ✅ | Story content directory. Must be a REAL directory (`symlink_metadata(...).is_dir()`, no-follow — a symlink does not count). Missing ⇒ `metadataUnsupported`. |
| `etc/` | ✅ | Device configuration directory. Same real-directory rule. Missing ⇒ `metadataUnsupported`. |

Knowledge source: public FLAM observations from the `o-daneel/Lunii.QT`
project (the same OSS reference already used for the Lunii marker set).
The internal structure of `.mdf` is NOT publicly documented: Rustory
reads its bytes only to hash the opaque `device_identifier` (same
SHA-256 + volume-serial recipe as `.pi` — each family's PRIMARY marker
is the hashed payload) and deliberately does NOT parse a version byte.
Inventing one would fake a firmware cohort. Real FLAM cohorts are
deferred until the format is confirmed on physical hardware; the single
conservative `Gen1` cohort (`flam_gen1`) covers every recognized FLAM
until then.

Auto-mount note: the udisks2 auto-mount filter (stage 1 above) stays
Lunii-only (the "STM" drive signature). A FLAM volume relies on the
desktop session's own auto-mount, a manual mount, or
`RUSTORY_DEVICE_MOUNT_ROOTS` — an assumed, documented limit until the
FLAM USB bridge signature is confirmed on real hardware.

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
below is the LUNII pack format (a KNOWN format, so its entry names are
validated); the FLAM pack is an UNKNOWN format and follows its own
opaque contract (see "FLAM library inventory & story import"). Closed
set of entry names at the Lunii pack root:

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
  story_id`, source device identifier, SOURCE FAMILY — the closed
  `source_family` set, `'lunii'` here; every row that predates the
  column is backfilled `'lunii'`, the only family the import flow ever
  acquired before it existed — timestamp, file count, total bytes,
  aggregate SHA-256 checksum). The link is UNIQUE on
  `pack_uuid`: re-importing the same pack is refused
  (`already_imported`) while the link exists — even across devices,
  the pack UUID is the content identity.
- **A pack is a device-format pack ONLY for its source family**
  (fail-closed): the preparation and transfer flows read
  `source_family` and treat an imported pack whose family does not
  match the TARGET device's family exactly like a native story — no
  device-format artifact exists for that target, so it is never marked
  transferable and the write plan refuses it (`notTransferable`)
  before a single byte reaches the device.
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

## FLAM library inventory & story import

Sibling of the Lunii "Library Inventory" and "Story Import Contract"
sections above: the FLAM read-side capabilities go through the SAME
shared pipelines (`read_device_library`, `import_device_story` — one
bridge, no parallel flow); only the family-dispatched adapter behind the
`DeviceLibraryReader` / `DevicePackReader` traits changes how the
inventory and the pack bytes are located on the volume. The family is
taken from the re-scanned `DeviceProfile` (Rust authority), never
re-sniffed from the mount.

Knowledge source & certainty: the layout below comes from public FLAM
observations in the `o-daneel/Lunii.QT` project (`pkg/api/device_flam.py`:
`str/` and `str.hidden/` story base dirs, `etc/library/` holding the text
index files `list` and `list.hidden`, story folders named by full
lowercase UUID, ciphered content) — the same OSS reference as the FLAM
recognition markers, and like them NOT yet confirmed on physical
hardware. Every behavior below is therefore fail-closed: if real
hardware diverges, the worst outcome is an EMPTY inventory or an
explicit typed refusal (`pack_missing` / `pack_invalid`) — never a
corruption, never a mount write, never a lying success.

### Inventory (index-founded, like Lunii)

The FLAM inventory is INDEX-FOUNDED: `etc/library/list` plays the role
the `.pi` index plays on a Lunii — the index is authoritative for what
the device library contains.

| Property | Rule |
| --- | --- |
| Visible index | `etc/library/list` — an UTF-8 TEXT file, one canonical lowercase hyphenated pack UUID per line |
| Hidden index | `etc/library/list.hidden` — same shape, read best-effort (like `.pi.hidden`) |
| Read bound | `MAX_PACK_INDEX_BYTES` (64 KiB), read NO-FOLLOW end to end (lstat → `O_NOFOLLOW \| O_NONBLOCK` open on Unix → `(dev, ino)` handle re-check — the hardened reader shared with the `.mdf` probe) |
| Line tolerance | a trailing `\r` is tolerated (CRLF-edited index); empty lines are ignored |
| Malformed line | ignored AND counted into the existing "trailing bytes" diagnostic flag (never a hard failure — the healthy lines still list) |
| Duplicates | deduplicated FIRST-OCCURRENCE (within an index, and across the two indexes — a visible entry wins over a hidden duplicate). The FLAM contract is born hardened here; the Lunii `.pi` duplicate behavior is deliberately NOT changed (family isolation) |
| `list` absent, device still present | the inventory is legitimately EMPTY (a fresh FLAM may not have written it yet — the index is NOT a recognition marker, and the hidden index is not consulted either: the primary index owns the inventory). "Still present" is PROVEN, not assumed: the required recognition markers (`.mdf` regular, real `str/` and `etc/`) are re-probed no-follow when the index reads NotFound |
| `list` absent, markers gone | the same NotFound is also the signature of a mount that vanished mid-read (unplug between the authoritative re-scan and the read): when the marker re-probe fails, the read surfaces a RECOVERABLE failure (`fs_read` / not_found — the honest Lunii behavior), never a lying empty inventory |
| `list` unreadable / not a regular file / oversize | a RECOVERABLE read failure (`DEVICE_SCAN_FAILED`, same error contract as the Lunii inventory read) — never silently folded into an empty inventory |
| Story content | a story's payload is the REAL directory `str/<uuid>/` (visible) or `str.hidden/<uuid>/` (hidden), probed no-follow |
| Index entry without its folder | surfaced as `Contenu incomplet` (visible, not importable) — the index stays authoritative |
| Folder without an index entry | INVISIBLE (never invented into the inventory — symmetric with the Lunii orphan rule) |
| Order | the index order is preserved, visible entries first |
| Budget | the shared 5 s read budget, checked between entries |

### Story import (raw & OPAQUE — structural validation only)

The public documentation of the FLAM story format says it plainly: the
internal format is UNKNOWN. Any per-entry-name whitelist would therefore
be an invention. The honest contract is a RAW, OPAQUE, all-or-nothing
acquisition of the ONE story directory the selected index entry owns —
`str/<uuid>/` for a visible entry, `str.hidden/<uuid>/` for a hidden one
(the index is authoritative: the other root is NEVER consulted, so a
same-UUID folder sitting on the wrong root can never be acquired in its
place) — with STRUCTURAL validation only:

| Property | Rule |
| --- | --- |
| Entry kinds | regular files and real directories ONLY — any symlink or special file refuses the whole pack (`pack_invalid`, no-follow walk end to end; born stricter than the historical Lunii walker). The walk re-probes each directory IMMEDIATELY before recursing into it (shared with the Lunii walk): a directory swapped for a symlink between its lstat and the recursive listing refuses (`details.cause = "dir_swapped_between_stat_and_recursion"`) instead of being followed |
| Entry names | NO whitelist, NO ignore-list — the format is unknown, every regular file is copied verbatim (the pack is opaque) |
| Bounds | reused from the Lunii import: `MAX_IMPORT_PACK_BYTES` (2 GiB), `MAX_IMPORT_PACK_FILES` (4096), `MAX_PACK_ASSET_DEPTH` (2) |
| Empty pack | a story folder holding zero files refuses (`pack_invalid`, `details.cause = "empty_pack"`) |
| Empty directory | a directory with NO file anywhere below it refuses (`pack_invalid`, `details.cause = "empty_directory"`): the manifest/checksum represent files only, so an empty directory cannot round-trip — refused rather than silently importing an altered tree |
| Staging collision | staging writes are EXCLUSIVE, for files (`create_new`) AND directories (`create_dir` tracked per acquisition): two distinct source paths — file or directory — colliding onto one staging path (case-insensitive / Unicode-normalizing local filesystem) refuse (`pack_invalid`, `details.cause = "staging_path_collision"`) — never a silent truncation or a silently merged tree behind an intact-looking manifest |
| Decryption | NONE — the ciphered bytes are copied as-is, never interpreted (no embedded title, no cover, no structure is ever derived) |
| Atomicity & provenance | the SHARED import pipeline unchanged: staging → atomic `rename` → fsync → single SQLite commit. `story_imports.pack_uuid` carries the FLAM story UUID verbatim (the UNIQUE content identity — `already_imported` dedup inherited as-is, across devices too); `source_device_identifier` is the hashed FLAM identifier; `source_family` records `'flam'` durably — the fail-closed fact the transfer/preparation flows consult so an opaque FLAM pack can NEVER be treated as a Lunii device-format pack (it stays `notTransferable` toward a Lunii, exactly like a native story) |
| Default title | `Histoire de mon FLAM ({SHORT_ID})` — the family-correct sibling of the Lunii copy, revalidated by the same title rules; `SHORT_ID` stays the uppercase last 8 hex chars of the UUID |
| Mount | READ-ONLY end to end, never written |
| Refusals | the CLOSED `IMPORT_FAILED` taxonomy is reused verbatim (`already_imported`, `pack_missing`, `pack_invalid`, `pack_oversize`, `device_changed`, `fs_read`, `read_timeout`, …) — no new reason, no new code |
| Bound divergence | if real hardware exceeds a bound (deeper trees…), the refusal is EXPLICIT and actionable; the bound will be adjusted on evidence, never silently |

## Refusal Reasons (closed set)

When classification refuses a candidate, the wire DTO carries a
`reason` value from this fixed set. Each value maps to one canonical
panel copy in `docs/architecture/ui-states.md#Disabled Actions and
Reasons`.

| Wire `reason` | Domain `UnsupportedReason` | Trigger |
| --- | --- | --- |
| `firmwareUnsupported` | `FirmwareUnsupported` | Reserved for future per-firmware blocklists |
| `metadataUnsupported` | `MetadataUnsupported` | `.md` first byte is not in `{3, 6, 7}`; FLAM volume missing the required `str/` or `etc/` directory |
| `metadataCorrupt` | `MetadataCorrupt` | `.pi` missing or empty, `.md` empty / oversize, FLAM `.mdf` empty, FS read failed (Lunii probe only — a FLAM `.mdf` I/O failure IGNORES the volume instead, see the FLAM recognition markers; `.bt` is informational only and never gates this reason) |
| `familyUnknown` | `FamilyUnknown` | Reserved for genuinely unknown families. A RECOGNIZED family (Lunii, FLAM) NEVER maps here — FLAM classification failures reuse the same `metadataCorrupt` / `metadataUnsupported` reasons as Lunii |
| `operationNotAuthorized` | `OperationNotAuthorized` | Capability gate refusal at Epic 3 wiring time |
| `multipleCandidates` | `MultipleCandidates` | More than one supported device detected at once — any families: two Lunii, but also a Lunii + a recognized FLAM |

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
(write-authorized for V1/V2, `false` for V3/FLAM in MVP Phase 1) and is orthogonal
to the verdict: a `présumée transférable` verdict never enables the send by itself
(preparation + the write gate do).

Read vs. write coherence (story preparation): the preparation step
(`start_prepare_story` / `read_preparation_state`, see
[ui-states.md#Story Preparation Contract](./ui-states.md)) is a **local**
operation that produces **derived** artifacts. It does NOT require the
`WriteStory` capability and never attempts a device write. It depends on the
device only for its `preflight` phase, which reuses the `ReadLibrary` gate (a
re-scan + identity guard + the read-only validation) to confirm the requested
device before assembly; the assembly phase itself is local and does not need the
device to stay plugged in. Transfer activation stays governed by `WriteStory`
(write-authorized for V1/V2 in MVP Phase 1) and is orthogonal to preparation — a
story can be fully prepared while the send stays gated on the write capability. No
new `SupportedOperation` is introduced: preparation is not a device capability, it
is local derived work. The media transformer it would host is declared but has no
live implementation in MVP (no story type requires transcoding yet).

Read vs. write coherence (story transfer): the transfer step
(`start_transfer_story` / `read_transfer_state`, see
[ui-states.md#Story Transfer Contract](./ui-states.md)) is the FIRST real device
**write**, the pendant of preparation: preparation assembles, locally, what a
transfer would need; the transfer writes it to the device. The `WriteStory`
capability is checked **before any write I/O** (fail-closed): a re-scan + identity
guard confirm the requested device, then `check_operation_allowed(profile,
WriteStory)` must pass — `true` only for Origine v1 / Mid-Gen v2, never V3 / FLAM.

The writer reuses the safe-write pattern from import: stage on the device volume
(`tempdir_in` at the mount root, never `app_data_dir`) → copy the opaque pack bytes
(read-only from `imports/<story_id>/`, re-checksummed, TOCTOU `lstat`→`fstat`) →
promote atomically (`rename`) to `.content/<SHORT_ID>` → `fsync` the promoted tree
+ parent → update the device index (`.pi`) atomically (append the 16-byte pack
UUID, write a temp + `rename`). Files first, index after: a pack UUID is never
added to `.pi` until its content is safely present. The write is idempotent (a UUID
already present with content is not duplicated), offline (USB only — no network),
and never decrypts.

**Write on an already-present pack (update flow, FR23).** When something sits at
`.content/<SHORT_ID>`, the writer PROVES its state and resolves to exactly one of
THREE provable outcomes — the comparison that matters runs INSIDE the write job
(fresh preflight + state proof), never from a cache or the read-only preview. The
proof is exhaustive and no-follow at every level: the target ROOT itself is
probed `lstat`-first (a symlinked root — dangling or not —, a regular file or a
special entry where the pack folder should be is unprovable; `exists()` would
follow or hide those), then every entry is enumerated AND read in full —
readability is part of the proof, because a replacement verdict leads to deleting
the old content. A non-empty unplanned directory is a CONTAINER, not an entry of
its own: it is traversed and its files decide (only an EMPTY directory — nothing
a files-only pack can explain — is unprovable; refusing every out-of-plan
directory would kill the nominal "another version re-installed" update). And
because the initial proof ages across the staging phase (copy + fsync can be
long), the state is RE-PROVEN ADJACENT to the set-aside mutation — a residual
window of the order of the `rename` itself remains (not eliminable with stdlib
primitives), the same honesty class as the FAT index-without-content window
below.

1. **Identical** (`reused_identical`): every planned file present as a regular
   file with the exact size + SHA-256, and NOT A SINGLE extra entry → the pack is
   already the plan's bytes. Idempotent re-index only; zero content byte written.
2. **Divergent-but-sound** (`replaced_divergent`): any content drift (missing,
   differing or extra files) where EVERY entry encountered is a regular file
   PROVEN readable (opened and read in full during the proof), AND the folder is
   ATTRIBUTABLE to the target UUID: the device index must reference that UUID,
   and NO OTHER indexed UUID may share the target SHORT_ID (the folder name is
   only the last 8 hex — an unindexed divergent folder is an unknown residue or
   a collision with an unindexed UUID, and a BI-INDEXED SHORT_ID collision means
   the folder may hold the other story's only content; both are REFUSED, never
   replaced). When authorized → controlled atomic replacement. The F2
   spirit is preserved — "the old content is never lost before the replacement
   is complete": the new pack is staged IN FULL and fsynced BEFORE any mutation
   (the budget is re-checked after that durability sync AND after the re-proof —
   a mutation never starts over budget); then the state is re-proven, the old
   `.content/<SHORT_ID>` is set aside by a same-volume `rename` to a sweepable
   `.rustory-replaced-*` name (the device mutation starts HERE —
   `reached_device_mutation = true`), the staging is promoted to
   `.content/<SHORT_ID>`, the tree is fsynced, the `.pi` index is converged
   idempotently (the UUID is already there), and the set-aside folder is removed
   best-effort AFTER the fsyncs. An interruption between set-aside and promotion
   is an HONEST `transfert incomplet` (never a false success); a relaunch (always
   a full cycle) converges. Orphan `.rustory-replaced-*` residues are swept with
   the staging residues before any write.
3. **Unprovable** (`device_pack_unprovable`): a non-directory target root, an
   irregular entry (symlink, unplanned EMPTY directory, special file), an entry
   whose bytes could not be read, an unreadable I/O during the proof, or a
   divergent folder that cannot be attributed to the target UUID (not referenced
   by the index, or another indexed UUID shares the SHORT_ID) → REFUSAL with the
   dedicated cause, ZERO device byte modified. Rustory never deletes what it
   cannot understand or attribute (fail-closed); the copy explicitly says
   Rustory is protecting the present content — never that the device refused.

The outcome the writer CONSTATED (`created_new` / `reused_identical` /
`replaced_divergent`) travels to the job's `verified` terminal, where the summary
names it (first send / update / already up to date — see
[ui-states.md#Story Verification Contract](./ui-states.md)). Every outcome —
including `reused_identical` — passes through the SAME full `verify` phase (no
success without verification). A FAT volume has no atomic directory swap: the
index-without-content window between set-aside and promotion is assumed and
classified honestly, exactly like the existing post-promotion interruption.

`SHORT_ID` is the **last 8 hex characters, UPPERCASED**, of the canonical pack
UUID — the same `.content/<SHORT_ID>` folder the library reader enumerates. Cohort
coherence is enforced: the descriptor's target cohort must match the connected
device's cohort. No new `SupportedOperation` is added — `WriteStory` already
exists; the transfer only flips it to `true` for the write-authorized cohorts.

Verification (story transfer, final phase): after a successful write the same job
runs a read-only **`verify`** phase. It re-scans the device and re-reads its
inventory through the `ReadLibrary` gate (true for every supported cohort) — **no
new `SupportedOperation`**: verification is a *re-read*, not a new capability. For
an opaque imported pack it proves, offline and key-free: the UUID is indexed in
`.pi`, the `.content/<SHORT_ID>` folder is present, and the written bytes
re-checksum to the prepared artifact's baseline (the exact import aggregation). It
**cannot** decrypt, parse `ni/li/ri/si`, or inspect media — `transférée et
vérifiée` means byte fidelity + indexing confirmed, never a semantic content
validation. Because `verify` only runs after a write (gated `WriteStory`), the
success path is demonstrable on **Origine v1 / Mid-Gen v2 or a fake mount only**;
**V3 stays write-blocked**, so it never reaches `verify` (it keeps refusing before
the write). The verify verdicts (`verified` / `partial` / `failed`) are job states,
never new `SupportedOperation`s or error codes.

Resume / relaunch (story transfer): the durable `transfer_jobs` memory that lets the
panel re-offer `Relancer` after an app restart (see
[ui-states.md#Transfer Resume Contract](./ui-states.md)) introduces **no new
`SupportedOperation`**. A `Relancer` re-runs the WHOLE transfer cycle through the
same `start_transfer_story` path, so it reuses the `WriteStory` gate unchanged
(write-authorized for Origine v1 / Mid-Gen v2, refused for V3 / FLAM) with a FRESH
device identity re-validated before any write — never the stored, now-stale
`device_identifier`. Reading / writing / purging the memory is a local SQLite
operation, gated by nothing device-side.

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

Adding a new family:

1. Add a `DeviceFamily` variant, a per-family firmware-cohort enum
   wired into the `FirmwareCohort` sum (with its diagnostic tags), and
   a per-family `CandidateFacts` variant (a candidate is mono-family by
   construction — a bi-family candidate must stay unrepresentable).
2. Add the family's marker probe to the scanner (documented in a
   "recognition markers" table above) and the pure classification
   function (`classify_<family>`) producing a `DeviceProfile`.
3. Update the matrix above with one line per cohort. A recognized
   family with zero activated capability keeps every operation ❌ —
   recognition and capability are separate facts.
4. Extend the gate tests in `tests::check_operation_*` (one matrix
   line = one test, all four operations covered), the wire DTO
   variants (+ contract tests), the TS guard family⇔cohort⇔version
   combinations, and the UI labels (`product-language.md` Change
   Control first).
5. When a READ capability activates, the adapter contract covers the
   DOWNSTREAM too: implement the family's inventory reader and pack
   acquirer as internal dispatch branches behind the SHARED
   `DeviceLibraryReader` / `DevicePackReader` traits (one implementation
   per trait, family passed as a parameter from the re-scanned profile —
   never re-sniffed from the mount). The shared pipelines
   (`read_device_library`, `import_device_story`) stay family-agnostic:
   only the source location/walk is family-dispatched, and the historical
   family paths stay verbatim (isolation between families is the adapter
   contract). Document the family's inventory & import section in this
   file FIRST (sibling of "FLAM library inventory & story import").

Both or none, never one without the other (the `family.rs` invariant):
a family variant without its matrix/registry entries — or the reverse —
is a bug.

A line in the matrix that has no test is a bug — the test enforces
that the gate behavior matches the published policy.

## Node Media Source Formats

The support profile also covers the **source media** a parent may associate with
a node while editing a native story (see
[ui-states.md#Story Node Editor Contract](./ui-states.md)). These are the user's
own local files; the editor stores them as-is and **never transcodes** them.

| Media | Accepted source formats | Recognized by |
| --- | --- | --- |
| Image | PNG, JPEG | magic bytes (signature), never the file extension |
| Audio | MP3, WAV, OGG | magic bytes (signature), never the file extension |

- The set is **closed**: anything not listed is refused at attach time as a real
  block (`MEDIA_INVALID`, surfaced inline at the media slot), never written.
- Each file is read **bounded** by a byte ceiling; an oversize or unreadable file
  is refused the same way.
- **No transcoding happens here.** Converting a source media to a device-format
  pack (`rf/` images, `sf/` sounds) remains a transfer/preparation concern — the
  media transformer stays declared but not implemented (no story type requires
  transcoding yet). Associating a source media to a node is editing, not a device
  capability: it introduces no `SupportedOperation`.
- **The native canonical model is a node graph** — one or more ordered nodes, a
  designated start node, and per-node option links toward other nodes. Editing
  that graph (adding, moving, deleting nodes; linking options) is pure local
  editing and introduces no device capability either. Converting the canonical
  node graph to a device pack layout (stage/action nodes, transitions) remains
  EXPLICITLY out of scope: the story transcoder stays declared but not
  implemented, and a native story — single-node or multi-node — stays
  non-transferable at the write-plan gate until it exists. Editing an imported
  story within its declared edit scope — and resolving its import review by
  doing so — changes NOTHING at that gate: a corrected `.rustory` import stays
  non-transferable (no pack files), a locally renamed device pack stays
  transferable.

## Local Artifact Import Contract

The support profile covers **local artifacts** as well as devices. A local
artifact is imported through the file flow (`Importer une histoire`, see
[ui-states.md#Local Artifact Import Contract](./ui-states.md)) — the inverse of
the `.rustory` export — never through the device flow. Each supported artifact
type is documented here with its format contract: what is recognized, what is
ambiguous, and what blocks the import. Anything not explicitly listed is refused
(no implicit format).

The **OS open channel** (a file opened through the operating system, see
[ui-states.md#OS Open Contract](./ui-states.md)) routes into this SAME import
pipeline: the `.rustory` artifact is the only double-clickable file of the
registry (a structured folder is not a file; the structured archive is
deferred), gated by the same `is_supported_artifact_source_name` authority and
producing the same verdicts and copies. The declarative REGISTRATION of Rustory
as an OS handler for its file types (`bundle.fileAssociations`, macOS
`exportedType`, Linux MIME) stays OUTSIDE this channel — it belongs to the
file-association contract of its own dedicated story.

### Supported local artifact types

| Type | Extension | Format version | Status |
| --- | --- | --- | --- |
| Rustory story artifact | `.rustory` | `formatVersion == 1` | ✅ supported (import + export) |
| Structured folder (`histoire.json` + referenced media) | — (a local folder) | `formatVersion == 1` | ✅ supported (creation) |
| Structured archive (zip…) | — | — | ❌ deferred (no archive reader; zero-dependency rule) |

### `.rustory` v1 format contract

A `.rustory` v1 artifact is a single UTF-8 JSON file with a fixed envelope:

```
{ "rustoryArtifact": { "formatVersion": 1, "exportedAt", "exportedBy" },
  "story": { "schemaVersion", "title", "structureJson", "contentChecksum", "createdAt", "updatedAt" } }
```

`deny_unknown_fields` applies to every object — an unknown field fails the parse.
The importer analyzes the following aspects and classifies each as recognized,
ambiguous, or blocking:

| Aspect | Recognized | Blocking |
| --- | --- | --- |
| Envelope | JSON parses, all required fields present, no unknown field | malformed JSON, missing field, unknown field |
| Format version | `formatVersion == 1` | `formatVersion != 1` (a newer/older artifact this build does not understand) |
| Schema version | `schemaVersion` is the supported canonical version | a `structureJson` that fails canonical validation (`validate_canonical`) — unsupported / incoherent schema |
| Structure | `structureJson` is canonically valid per `validate_canonical` (the current canonical schema — an ordered node graph with a start node and option links) | non-canonical / corrupt structure |
| Integrity | `SHA-256(structureJson)` equals the declared `contentChecksum` | checksum divergent (silent corruption) — never recomputed/overwritten, only verified |
| Title | normalizable to a non-empty valid title | empty after normalization / invalid characters |
| Timestamps | `createdAt` / `updatedAt` are ISO-8601 UTC ms | — (a malformed timestamp is **ambiguous**, preserved and flagged, never blocking) |

Ambiguous (importable with a durable marker): a title that had to be **normalized**
(the stored value differs from `normalize_title(value)`), or a carried timestamp
not in the expected ISO-8601 UTC ms shape. Because Rustory's own export always
writes a normalized title and canonical timestamps, an ambiguous verdict is only
reachable from a hand-edited artifact.

Provenance: a successful import records a `story_local_imports` row (source
format `rustory`, format version, source file basename only — never an absolute
path, artifact SHA-256, import state, optional findings summary, import
timestamp) linked to the new `stories` row by `ON DELETE CASCADE`. It is distinct
from `story_imports` (the device-pack provenance): a file artifact has neither a
pack UUID nor a source device. The canonical row **preserves** the artifact's
`createdAt` / `updatedAt` (a re-openable story keeps its history);
`imported_at = now`.

Bounds & safety: the chosen file is read bounded (`MAX_ARTIFACT_BYTES`); the
import is offline, adds zero dependency, never writes a device, and is atomic
(one SQLite transaction — a failure leaves the previous library state intact).

### Structured folder v1 format contract

The structured folder is an **author format**, not a machine artifact: it is the
entry point for content prepared OUTSIDE Rustory (FR30) and it CREATES a brand
new canonical story — it does not round-trip an exported one. It converges to
the exact same canonical v3 model as an interactive creation. Only the folder
shape below is recognized; anything else is a named blocking verdict, never a
half-support (no implicit format).

A structured folder v1 is a **local folder** containing:

- **`histoire.json`** (required, UTF-8 JSON) — the author manifest. One exact
  name, no alias.
- **optional media files** (image/audio), flat in the folder, referenced by the
  manifest by **simple basename** (never a path, never a subfolder in v1).

Manifest v1 schema:

```json
{
  "formatVersion": 1,
  "title": "Le voyage de Nour",
  "startNodeId": "debut",
  "nodes": [
    {
      "id": "debut",
      "text": "Il était une fois…",
      "label": "Départ",
      "image": "couverture.png",
      "audio": "intro.mp3",
      "options": [
        { "label": "Aller à la mer", "target": "mer" },
        { "label": "Aller à la montagne", "target": "montagne" }
      ]
    },
    { "id": "mer", "text": "…", "options": [] },
    { "id": "montagne", "text": "…", "options": [] }
  ]
}
```

Rules: `formatVersion` required and `== 1` (forward guard: anything else blocks,
like the `.rustory` envelope); `title` required; `startNodeId` optional
(default: the first node's `id`); `nodes` required, non-empty; per node: `id`
required, `text` / `label` optional (default `""`), `image` / `audio` optional
(sober basenames), `options` optional (default `[]`) with `label` required and
`target` optional/nullable. **An unknown field does not reject: it produces an
`Ambiguous` finding** — a DELIBERATE difference with the `.rustory` machine
artifact (`deny_unknown_fields`): an author format tolerates a typo but FLAGS
it. The manifest is transcoded to the canonical v3 structure (ids preserved,
`image` / `audio` resolved to asset ids at acceptance); `validate_canonical`
stays the final oracle.

The folder flow analyzes its OWN aspect set (documented separately from the
`.rustory` set — each flow owns its contract). Exactly one finding per aspect;
one matrix cell = one test:

| Aspect | Recognized | Ambiguous (`ambiguïté`) | Missing (`information manquante`) | Blocking (`blocage réel`) |
| --- | --- | --- | --- | --- |
| `Envelope` | `histoire.json` present, readable as a regular file, valid JSON | — | — | manifest absent, unreadable, or malformed JSON |
| `FormatVersion` | `formatVersion == 1` | — | — | absent or `!= 1` |
| `Title` | valid as-is | normalizable (`value != normalize_title(value)`) | — | absent, empty after normalization, or invalid |
| `Structure` | transcodable and canonically valid | unknown manifest field; an option `target` pointing at an unknown node (preserved broken — `BrokenOptionLink` is `Fixable`, repairable in the editor) | — | `nodes` absent/empty, duplicate `id`, `startNodeId` given but unknown, anti-DoS bounds exceeded, untranscodable structure |
| `Media` (new `RecognitionAspect::Media`) | every referenced media present, regular, sniffed inside the closed set, within bounds | a media present but unusable (magic bytes outside the set, wrong slot, oversize, symlink/irregular, non-sober basename) — the media is discarded | a referenced media ABSENT from the folder — the media is discarded | — |

No `SchemaVersion` / `Integrity` / `Timestamps` aspects: an author manifest has
no declared canonical schema, no checksum and no timestamps (the story is BORN
at acceptance — see provenance below). The `Media` aspect is analyzed ONLY when
the declared `formatVersion` is the listed one: an unlisted format never
triggers a single media read and its verdict carries no `Media` finding (no
implicit / partial support). A discarded media (`Ambiguous` or
`Missing`) never prevents the creation: the node is born with the empty slot,
repairable in the editor.

State derivation (folder flow — extends the shared derivation without changing
the `.rustory` one): any `Blocking` → `Unusable` → nothing is created (the
`blocked` verdict is never persisted); else any `Missing` → quality `Partial` →
durable state `partial` (the first real emitter of `ImportState::Partial`);
else any `Ambiguous` → quality `Partial` → `needs_review`; else `Clean` →
`recognized` (no report, no marker).

Named bounds (anti-DoS, tested): `MAX_MANIFEST_BYTES` = 1 MiB;
`MAX_FOLDER_MEDIA_FILES` = 64 referenced media; per-media bound = the node-media
store ceiling (`MAX_MEDIA_BYTES`, 32 MiB); `MAX_FOLDER_TOTAL_MEDIA_BYTES` =
256 MiB. Only `histoire.json` and the files it references are ever read — the
folder is NEVER listed (no recursive walk); unreferenced files are ignored by
construction (never opened). Referenced basenames are validated for sobriety
BEFORE any path join; a symlink or non-regular file is refused at probe time.

Provenance: acceptance records a `story_local_imports` row with
`source_format = 'structured-folder'`, `source_format_version = 1`,
`source_name` = the folder's basename (validated for sobriety, no extension
requirement — never an absolute path), `artifact_checksum` = SHA-256 of the
manifest bytes, the derived import state and its findings summary. A folder
whose NAME cannot be carried as a sober provenance source (no real UTF-8
basename, a name over the length bound or containing `/` `\` `:` / control
characters / only blanks) is refused as an honest TRANSPORT error
(`file_read` / `folder_name`) BEFORE any read — never disguised as a manifest
problem: the `Envelope × Blocking` cell of the matrix keeps meaning exactly
"manifest absent, unreadable, or malformed JSON". The
canonical row is a BIRTH: `created_at = updated_at = now`, exactly like
`create_story` — a DELIBERATE difference with the `.rustory` import (which
PRESERVES the timestamps of an exported story). The card renders the existing
`Importée` provenance marker (the content does come from outside Rustory);
no new provenance label. The edit scope is `Full` BY CONSTRUCTION (the scope
derivation only consults `story_imports` — device packs — never
`story_local_imports`), so the created story opens in the editor with every
control, exactly like a native one, and inherits the import-review resolution
cycle (a real write that leaves the canonical fully sound settles a pending
`partial` / `needs_review` review; media are never part of that oracle).

Acceptance is **files first, DB second** (same discipline as the device
import): the retained media are validated + promoted into the content-addressed
node-media store OUTSIDE the DB lock, then ONE `BEGIN IMMEDIATE` transaction
inserts the `stories` row (fresh UUIDv7, transcoded v3 structure with the asset
ids wired, recomputed checksum), the provenance row and the `assets` rows. A
transaction failure compensates the promoted files (refcounted GC; the boot
sweep stays the net). Acceptance RE-ANALYZES the folder from zero (the disk may
have changed since the analysis) — the re-analysis is authoritative; a verdict
that turned blocking refuses and creates nothing.

Bounds & safety: offline, zero dependency, never writes a device, analysis is
strictly read-only (no row, no promoted file), and the commit is atomic (a
failure leaves the previous library state intact, media files compensated).
