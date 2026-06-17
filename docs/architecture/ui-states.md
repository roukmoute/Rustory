# UI States

## Purpose

This document defines Rustory's canonical user-visible states and the rules for showing them.

It prevents state drift across:
- library views
- transfer surfaces
- recovery flows
- future editing screens
- tests and acceptance criteria

## Authority Rules

- Rust is the authoritative source for canonical business truth.
- The UI may derive presentation, but it must not invent competing truth.
- A visible success state must never appear before the required verification step completes.
- Local state, device state, and last-known transfer outcome must stay distinguishable.

## MVP State Model

These are the minimum user-visible states for `MVP Phase 1`.

| Internal Intent | Preferred UI Label | When It May Be Shown | Must Never Mean |
| --- | --- | --- | --- |
| Local canonical draft exists | `brouillon local` | Story exists locally and is editable/resumable | Present on device |
| Validation/preflight running | `en vérification` | Compatibility or safety checks are actively running | Transfer already started |
| Real blocking issue exists | `bloquée` | A required condition prevents the next action | Generic inconvenience or unknown state |
| Validation passed for send | `présumée transférable` | Rust has not found a real block yet | Transfer already succeeded |
| Preparation running | `en préparation` | Media/artifact preparation is in progress | Device write in progress |
| Device write running | `en transfert` | Rust has accepted and is executing a write operation | Verified success |
| Verification succeeded | `transférée et vérifiée` | Write outcome was explicitly confirmed | Mere write completion without proof |
| Retry-safe failure | `échec récupérable` | The user can relaunch from preserved local state | Partial success |
| Incomplete outcome | `état partiel` | Verification or import found a mixed/incomplete result | A successful end state |

## Transfer State Contract

The transfer contract is part of the MVP and should map internal state to UI labels as follows:

| Internal Contract State | UI Label | Notes |
| --- | --- | --- |
| `blocked` | `bloquée` | Show the blocking reason and the next useful action |
| `presumed_transferable` | `présumée transférable` | Valid for decision surfaces before send |
| `preparing` | `en préparation` | May coexist with preserved local work |
| `transferring` | `en transfert` | Must stay visibly in-context in the library |
| `verified` | `transférée et vérifiée` | Only after explicit confirmation |
| `partial` | `état partiel` | Never collapse into success wording |
| `retryable` | `échec récupérable` | Keep enough context for `Relancer` |

## Post-MVP Import State Contract

These states are for post-MVP local structured import flows and must not be mistaken for MVP transfer states:

| Internal Contract State | UI Label | Scope |
| --- | --- | --- |
| `recognized` | `reconnu` | Import analysis found usable material |
| `partial` | `partiel` | Some content is usable, some is not |
| `needs_review` | `à revoir` | The user must inspect before accepting |
| `blocked` | `bloqué` | Import cannot continue safely |
| `resolved` | `résolu` | The import issue has been handled |

## State Transition Rules

- `brouillon local` may lead to `en vérification` or remain local.
- `en vérification` may lead to `bloquée` or `présumée transférable`.
- `présumée transférable` may lead to `en préparation` or directly to `en transfert`, depending on the flow.
- `en transfert` may lead to `transférée et vérifiée`, `état partiel`, or `échec récupérable`.
- `échec récupérable` may lead back to `en vérification`, `en préparation`, or `en transfert` through an explicit user action such as `Relancer`.

## Forbidden Ambiguities

The UI must never:

- show `transférée` when verification has not happened
- merge local truth and device truth into one ambiguous badge
- use the same visual label for `bloquée` and `échec récupérable`
- treat `état partiel` as a silent success
- hide a critical issue only in a `toast`

## Disabled Actions and Reasons

When a primary action is visible but disabled:

- the reason must be explicit and short
- the reason must map to a stable domain cause
- the wording must stay consistent with [product-language.md](./product-language.md)

Preferred patterns:

- `Envoi indisponible: histoire bloquée`
- `Envoi indisponible: appareil non supporté`
- `Relance indisponible: aucun brouillon préservé`
- `Création d'histoire indisponible pour l'instant.`
- `Filtres avancés à venir`
- `Reprise indisponible: aucune histoire sélectionnée`
- `Reprise indisponible: sélection multiple`
- `Bibliothèque incohérente, recharge nécessaire`
- `Création impossible: titre requis`
- `Création impossible: titre trop long (120 caractères maximum, N en trop)` — `N` is the exact excess (`chars - 120`, minimum `1`) computed both by the Rust domain and the TS mirror so the user sees how many code points to trim.
- `Création impossible: titre contient des caractères non autorisés`
- `Reprise indisponible: histoire introuvable`
- `Enregistrement en échec: vérifie le disque local et réessaie.`
- `Récupération indisponible: vérifie le disque local et réessaie.`
- `Restauration en cours: patiente quelques instants.`
- `Édition en attente: choisis d'abord comment reprendre cette histoire.`
- `Envoi indisponible: aucun appareil connecté`
- `Envoi indisponible: profil non supporté`
- `Envoi indisponible: profil ambigu`
- `Envoi indisponible: détection en cours`
- `Envoi indisponible: détection en échec`
- `Envoi indisponible: transfert pas encore activé (MVP Phase 1)`
- `Détection indisponible: vérifie que la Lunii est branchée et réessaie.`
- `Profil non supporté: format métadonnées v{n} non géré`
- `Profil non supporté: firmware {hint} non géré`
- `Profil non supporté: marqueurs appareil incomplets`
- `Profil ambigu: {n} candidats détectés. Débranche les autres puis réessaie.`
- `Lecture appareil indisponible: profil non autorisé`
- `Lecture de la bibliothèque appareil indisponible: vérifie que la Lunii est branchée et réessaie.`
- `Lecture de la bibliothèque appareil indisponible: l'index des histoires est illisible.`
- `Lecture de la bibliothèque appareil indisponible: l'appareil met trop de temps à répondre.`
- `Lecture de la bibliothèque appareil indisponible: l'appareil connecté a changé.`
- `Copie indisponible: profil non supporté`
- `Copie indisponible: déjà dans ta bibliothèque`
- `Copie indisponible: contenu incomplet sur l'appareil`

Error payloads that surface in the autosave alert carry a stable
`details.source` discriminator so support can triage the cause without
parsing the user-facing message:

- `sqlite_update` — transport failure on the UPDATE itself. `details.stage` is one of `begin_transaction`, `update`, `commit` and `details.kind` is one of `busy`, `locked`, `constraint_violation`, `other`. A `constraint_violation` is re-mapped to `INVALID_STORY_TITLE` so the same canonical reason text as the creation dialog is shown.
- `story_missing` — the UPDATE matched zero rows (the id is no longer in the table). Surfaces as `LIBRARY_INCONSISTENT` with the canonical "Histoire introuvable" copy.
- `story_duplicate` — the UPDATE matched more than one row (schema corruption: duplicate primary keys). Surfaces as `LIBRARY_INCONSISTENT` and includes `details.rowsAffected`.
- `story_id_invalid` — the UPDATE or read received an empty or oversize id. `details.cause` is `empty` or `too_long`.
- `sqlite_select` — transport failure on the detail read. No `stage` subdivision (the query is a single `SELECT`).
- `system_clock_invalid` — `OffsetDateTime::format` refused to produce an ISO-8601 timestamp (system clock outside representable range).

Avoid:

- `Action impossible`
- `Erreur`
- `Not ready`

## Screen Ownership

- `bibliothèque` is the stable home for selection, transfer visibility, and recovery traces
- `édition` is a separate context and must not silently replace library state vocabulary
- `modals` and reports may clarify a state, but should not become the only place where a critical state exists

## Implementation Rule

When a new feature wants to introduce a new user-visible state:

1. Reuse an existing state if it already describes the situation.
2. If reuse is impossible, update this document first.
3. Add or update tests and acceptance criteria using the same wording.

## Window Sizing Contract

Rustory is desktop-only. No `mobile` or `tablet` UI is supported.

| Key | Value |
| --- | --- |
| `default window size` | `1440 × 900` |
| `minimum window size` | `1024 × 720` |
| nominal work range | `portable 13 to 16 inches` |

Enforced in `src-tauri/tauri.conf.json` via `app.windows[0].{width, height, minWidth, minHeight}`.

Two visual modes only:

- **standard mode**: target configuration, full layout, stable hierarchy
- **reduced-density mode**: same architecture, margins and secondary columns lightly compressed. No structural rework, no hamburger, no bottom navigation, no second interface.

Below the minimum size, the window refuses to shrink further through the Tauri constraint. No responsive fallback is attempted.

## Library Layout Contract

The library context is a stable three-column desktop grid. Future routes that
compose the library must reuse this contract rather than invent a variant.

| Zone | Role | ARIA region | CSS column (standard mode) | CSS column (reduced-density) |
| --- | --- | --- | --- | --- |
| Left column | Global filters / navigation entry points | `<nav aria-label="Filtres bibliothèque">` | `minmax(240px, 280px)` | `200px` |
| Center column | Story collection — the main work surface | `<main aria-label="Collection d'histoires">` | `1fr` | `1fr` |
| Right column | Decision panel (device state + send CTA) | `<aside aria-label="Panneau de décision">` | `minmax(320px, 360px)` | `300px` |

Rules:

- The center column is the only working surface; the two side columns support decision without stealing focus.
- Error, empty, filtered-empty and loading states all live inside the center column — never in the nav or in the panel.
- The right-column panel stays intentionally minimal: device state + one primary CTA + a short, standardized reason when the CTA is disabled.
- No fourth column, no collapsible drawer, no hamburger.

## Device Detection Contract

Drives the right-column `Lunii Decision Panel`. The Rust core scans
mounted USB Mass Storage volumes for the canonical Lunii markers
(`.md` + `.pi` at the volume root — `.bt` is observed on some
generations but is **not** required, see
[device-support-profile.md](./device-support-profile.md)); the IPC
façade exposes a tagged-enum DTO; the panel maps each kind to one of
the labels below.

The frontend hook `useConnectedLunii` polls silently every 3 s so
plug / unplug events surface automatically without the user clicking
`Réessayer la détection`. The manual refresh button stays available
as a fallback.

| Internal State | Wire `kind` | Panel Label | Disabled Actions |
| --- | --- | --- | --- |
| no device | `none` | `Aucun appareil connecté` | Envoi |
| supported | `supported` | `Appareil prêt — {family} {cohort}` | Envoi (Phase 1 — wired Epic 3) |
| unsupported | `unsupported` | `Profil non supporté` + standardized reason | Envoi |
| ambiguous | `ambiguous` | `Profil ambigu — {n} candidats` | Envoi |
| scanning | (transient) | `Détection en cours…` | Envoi, Réessayer |
| error | (`AppError DEVICE_SCAN_FAILED`) | `Détection indisponible` | Envoi |

Error payloads under `DEVICE_SCAN_FAILED` carry a stable
`details.source` discriminator so support can triage without parsing
the user-facing message:

- `fs_read` — per-mount filesystem read failure. `details.kind` is one of `permission_denied`, `not_found`, `timeout`, `interrupted`, `other`.
- `os_enum` — failure to enumerate the host disk list. `details.kind` mirrors the I/O kind label set above.
- `scan_timeout` — wall-clock budget exhausted before a candidate emerged. Carries `details.elapsed_ms`.
- `other` — fallback when the upstream source token does not match a known set.

Diagnostic events for the scan flow live in
`{app_data_dir}/diagnostics/device.jsonl` (rotation cap 10 MB,
identical to recovery.jsonl). Each event carries `elapsed_ms` (the
wall-clock duration of the surrounding scan pipeline) so support can
spot slow scans approaching the NFR4 5 s budget. The closed `category`
set is: `device_absent`, `device_detected_supported`,
`device_detected_unsupported`, `device_scan_failed`,
`device_automounted` (Linux only, when Rustory mounts a plugged
Lunii via udisks2 D-Bus), `device_automount_failed` (Linux only,
when the D-Bus mount call is refused), `device_library_read` (the
inventory was read — carries `story_count` and `hidden_count`, never
the raw pack UUIDs), `device_library_read_failed` (carries a
closed-set `source` and the upstream `kind`),
`device_story_imported` (a device pack was copied into the local
library — carries `short_id`, `story_id`, `elapsed_ms`,
`bytes_copied` and `file_count`; NEVER the full pack UUID nor any
absolute path), and `device_story_import_failed` (carries the
closed-set `source` from the import taxonomy and the upstream
`kind`). Each line is one JSON object — `device_identifier` is
always the SHA-256 hash of `.pi` + volume serial, never the raw
payload.

## Device Library Contract

After a device is detected as `supported`, Rustory may read its
installed-story inventory and show it as a **distinct section in the
center column**, below the local collection (separated by a rule). The
device library is never merged into the local collection and is never
hosted by the right decision panel (which stays "device state + send
CTA"). This keeps local truth and device truth distinguishable at all
times.

The frontend hook `useDeviceLibrary(deviceIdentifier)` reads the
inventory through the `read_device_library` command. It is orthogonal to
`useLibraryOverview`: a device-read failure never alters the LOCAL
library. There is no polling of the inventory — device PRESENCE is polled
by `useConnectedLunii`; the heavier inventory read fires when the
identifier changes (a different Lunii) and on a manual retry.

Scope: listing + recognition. Each entry shows its recognized `title` when a
local index covers the pack (composed Rust-side — see
[device-support-profile.md#Title Recognition & Catalog Policy](./device-support-profile.md)),
otherwise the opaque `shortId` under `Histoire non reconnue`. No asserted
content quality (the device stores none). Device entries are
single-selectable for inspection (see `Device Story Inspection Contract`)
but never editable here; that selection is separate from the local
`selectedStoryIds` and is not persisted.

| Internal State | Center-column rendering | Notes |
| --- | --- | --- |
| idle | nothing | No readable device (none / unsupported / not read-authorized). |
| loading | `ProgressIndicator` + `Lecture de la bibliothèque de l'appareil…` | "État non encore chargé" — must never read as "aucune histoire". |
| ready (n > 0) | list of entries + count | Each entry: the recognized `title` (or `Histoire non reconnue`) + `Identifiant: <shortId>` + a provenance chip when recognized. |
| ready (n = 0) | `Aucune histoire sur l'appareil` | Distinct from the loading state. |
| error | `LibraryErrorBanner` (`role="alert"`) titled `Bibliothèque de l'appareil indisponible` + `Réessayer` | Recoverable, in-context, never a toast. The local library stays intact. |

Provenance is explicit: the section heading is `Histoires sur l'appareil`
(plus the device label when known) with a `Sur l'appareil` chip. Per
entry, a title-provenance chip (`Titre officiel` / `Titre non-officiel` /
`Titre saisi`) when recognized, plus `Masquée` (`.pi.hidden`) and `Contenu
incomplet` (no `.content/<shortId>` folder) chips surface structural facts —
all non-color (ASCII-glyph chips). The recognized title and its provenance
are also folded into each card's accessible name.

Error payloads under the read carry a stable `details.source`:

- `fs_read` — index-file read failure. `details.kind` ∈ `permission_denied`, `not_found`, `timeout`, `interrupted`, `other` (a mid-read unplug typically surfaces as `not_found`).
- `pack_index` — the `.pi` exceeded the 64 KB inventory bound (`details.kind = "oversize"`).
- `read_timeout` — the read budget was exhausted (`details.elapsed_ms`).
- `device_changed` — the live re-scan no longer resolves to the requested device (swapped / unplugged-and-replaced).
- `scan_timeout` / `os_enum` / `mount_unavailable` / `spawn_blocking_join` / `other` — re-scan transport failures, mirroring the detection set.

## Device Story Inspection Contract

A device story listed by the `Device Library Contract` can be SELECTED to
inspect it before any import. Selection drives a contextual inspector in the
right column. Inspection is read-only: it consults the inventory snapshot
already in memory, never re-reads or mutates the device, and never imports
anything (the import flow is a later story).

| Aspect | Value |
| --- | --- |
| Source of truth | A single selected device-story `uuid`, held by `LibraryRoute` as local UI state. Separate from the local `selectedStoryIds`: selecting a device story never touches the local selection, and vice-versa. |
| Cardinality | Single selection — inspection (and the later import) target one device story at a time. |
| Persistence | None. Never survives a restart, and is dropped when the readable device changes or the selected entry is absent from a fresh inventory read — no silent stale target (a `uuid` that resolves to no current entry renders no inspector for that render). |
| Capability gate | Offered only when the detected profile authorizes `inspectStory` (✅ for every supported Lunii cohort, V3 included — distinct from `importStory`, which is ❌ for V3). When `inspectStory` is false, the device cards stay non-interactive. |
| Non-color signal | A selected device card renders a colored border, a visible `✓` prefix in the DOM and `aria-pressed="true"` — the selection survives grayscale / color-blindness, exactly like a local `Story Card`. |
| Interaction | A selectable device card is a `role="button" aria-pressed={selected}` focus stop. Click (or `Space` / `Enter` on the focused card) toggles its selection. There is no double-click / open: a device story is not editable. |

The inspector lives in the right column ABOVE the `Lunii Decision Panel`
and renders only when a device story is selected:

| Inspector element | Content |
| --- | --- |
| Heading | `Histoire sélectionnée`. |
| Provenance | A `Sur l'appareil` chip plus the note `Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque locale.` — or, when `alreadyImported`, `Cette histoire vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale.` (the note must never claim "pas encore" about a story whose local copy exists). Keeps local truth and device truth distinguishable. |
| Identity | The recognized `title` when a local index covers the pack, otherwise `Histoire non reconnue`; always `Identifiant: <shortId>` and `UUID: <uuid>`. No asserted content quality. |
| Title provenance | When recognized, a provenance chip states the source honestly: `Titre officiel` (info tone), `Titre non-officiel` (neutral), `Titre saisi` (neutral). A user-typed or community title is NEVER labelled "officiel". Resolution priority (Rust-owned): `user > official > unofficial > none`. |
| Naming | For a genuinely unrecognized pack, a `Nommer cette histoire` action opens an inline editor (same title rules as a local story: NFC + trim + denylist + ≤120, Rust-authoritative). A name the user typed earlier can be edited via `Renommer cette histoire`. A user title is `source = user`, so it is never silently overwritten by a later official/community match; it survives unplug/replug. Naming is not offered for official/community titles (not the user's to overwrite here). A rejected title surfaces in-context next to the field, never as a toast. |
| Honest triage (before any copy) | The verified facts from the in-memory inventory snapshot are grouped under three honest headers so nothing is imported blindly (no device re-read, no catalog lookup): **`Ce que Rustory reconnaît`** — the identifiers and, when `contentPresent`, a `Contenu présent` chip; **`Ce qui bloque la copie`** — `Contenu incomplet` (`!contentPresent`) and `Dans ta bibliothèque` (`alreadyImported`), each with a short honest note; **`À revoir avant de copier`** — `Masquée` (`hidden`). A group is shown only when it has a fact. Fail-closed: a fact that is absent/unknown is never asserted positively. These headers are distinct from the Post-MVP `reconnu` / `partiel` / `à revoir` import-state labels — a device copy is binary (full success or `IMPORT_FAILED`), so this triage is a pre-decision, never a partial-import report. |
| Ambiguity flags | `Masquée` (`.pi.hidden`) and `Contenu incomplet` (no `.content/<shortId>` folder) chips, plus a short honest note when content is missing — surfaced, never hidden, never called "corrupt". |
| Copy affordance | A `Copier dans ma bibliothèque` button (device → local library), ACTIVE when `importStory === true && contentPresent && !alreadyImported` and the device is readable (see `Device Story Import Contract`). Otherwise it stays soft-disabled with a standardized, capability-aware reason picked fail-closed in this priority order: (1) `story.alreadyImported` ⇒ `Copie indisponible: déjà dans ta bibliothèque` (the most useful fact — no copy is needed); (2) `importStory !== true` or operations matrix absent ⇒ `Copie indisponible: profil non supporté` (fail-closed default, V3 included); (3) `!story.contentPresent` ⇒ `Copie indisponible: contenu incomplet sur l'appareil`. The internal capability flag stays `importStory`; only the user-facing verb is `Copier` (Importer / Exporter are reserved for file artifacts). |
| Next gesture on a profile refusal | When the copy is refused for an unsupported profile (the V3 case: inspectable but not importable), the inspector shows a discreet `Consulter le profil de support` affordance (`Button variant="quiet"`, `aria-label="Consulter le profil de support officiel"`) wired to the same `openSupportProfile` action as the detection panel — so a refusal carries a next gesture, never an opaque grayed-out CTA. The affordance is shown ONLY for the profile refusal: not for `déjà dans ta bibliothèque` (a copy is simply not needed) nor `contenu incomplet` (the honest note already tells the user to check the device), and it is hidden entirely in listing/inspection-only contexts where the route wires no handler. |

The inspector is the only place the right column reflects a device story; it
must never merge with the panel's local-selection layer, and the panel
itself stays "device state + send CTA".

## Device Story Import Contract

Activating the `Copier dans ma bibliothèque` affordance starts a raw,
structurally validated acquisition of the selected device pack into the
local library (see
[device-support-profile.md#Story Import Contract](./device-support-profile.md)).
This contract is **distinct from the `Post-MVP Import State Contract`**
(local structured imports, Epic 4): the `reconnu` / `partiel` / `à
revoir` labels MUST NOT be reused here — a device copy either fully
succeeds or explicitly fails.

UI state machine (owned by `useDeviceStoryImport`, surfaced in the
inspector):

| State | Rendering | Announcement |
| --- | --- | --- |
| `idle` | no status content | none (the polite region stays mounted, empty) |
| `importing` | indeterminate `ProgressIndicator` labelled `Copie en cours…` (calm, neutral), CTA soft-disabled + `aria-busy` | deliberately NOT announced (ephemeral noise) |
| `imported` | `Histoire copiée dans ta bibliothèque` (success chip) + the created local title + explicit `Fermer` dismiss | `aria-live="polite"` region, mounted permanently, `aria-atomic` |
| `failed` | `Copie impossible` block with the canonical `message` + `userAction`, buttons `Réessayer` THEN `Fermer` in tab order. EXCEPTION: a profile refusal (`DEVICE_UNSUPPORTED` — the live device turned out non-importable, e.g. after a stale snapshot) is not retryable, so the alert offers `Consulter le profil de support` in place of `Réessayer` (parity with the pre-click affordance), THEN `Fermer` | `role="alert"` |

Surface rules:

- All import feedback lives in the inspector (right column). Never a
  toast for the failure, never a tunnel modal, never a navigation.
- No pre-copy confirmation dialog: the copy is non-destructive (the
  device is read-only end to end).
- The success does not auto-hide — dismiss is explicit, the user reads
  the outcome at their own pace.
- The `failed` state is never wiped IMPLICITLY (no auto-hide, no
  navigation side effect) — but the alert's own explicit `Fermer`
  button DOES dismiss it back to idle, exactly like `Réessayer` or a
  fresh copy trigger leave it. An inoperative close button is a bug,
  not a preservation feature.
- The status surface is attached to the pack that was copied: selecting
  another device story shows THAT story's (idle) status, never the
  previous card's success or failure; re-selecting the copied story
  surfaces its status again.
- A critical copy failure NEVER lives only in a toast (UX-DR15): the
  `failed` alert is `role="alert"` inside the inspector, and the `Toast`
  primitive excludes the `error` tone at the type level so the compiler
  refuses an error-only toast. The failure stays consultable as long as
  the user is on the library, re-displayed by re-selecting its pack.
- Residual limit (assumed for the MVP): the `failed` alert lives in
  route-local state (the hook + the selected-pack gate). It survives
  selection changes WITHIN the library (re-displayed by re-selecting its
  pack), but NOT a full departure from the library context — leaving the
  route unmounts the hook and the alert is gone on return (the pack shows
  idle again). The canonical state stays coherent regardless (Rust
  finished or refused the copy, the overview cache is reconciled), so a
  fresh copy trigger on return simply re-surfaces the honest refusal
  (e.g. `already_imported`). A global, cross-route notifier that would
  carry the failure across a route departure is a deliberately deferred,
  separate concern.

Post-success traces (AC: trace on BOTH sides):

- Local: `invalidateLibraryOverviewCache()` + authoritative overview
  re-read — the new story card appears, titled `Histoire de ma Lunii
  (XXXXXXXX)` (the provenance is carried by the title).
- Device: `deviceLibrary.refresh()` re-reads the inventory; the device
  card now renders a `Dans ta bibliothèque` chip (tone success, glyph,
  folded into the card's `aria-label`) and the inspector CTA flips back
  to soft-disabled with `Copie indisponible: déjà dans ta bibliothèque`.
- The device selection (and keyboard focus) is PRESERVED across the
  re-read: the story still lives on the device — a copy is not a move.

Actionability rule (never an opaque refusal): EVERY `IMPORT_FAILED`
`details.source` below — AND the `DEVICE_UNSUPPORTED` /
`capability_gate` copy refusal (V3) — carries a non-empty `message`
(cause + impact) AND a non-empty `userAction` (the next gesture). The
Rust side is the authority for both strings; the frontend renders them
verbatim (it branches on `code` + `details.source` only to choose the
surface/affordance, never to compose the text). Backend tests lock this
invariant per refusal constructor; the inspector adds the
`Consulter le profil de support` next gesture for the profile refusal
(see `Device Story Inspection Contract`).

Error taxonomy — rejections cross the boundary as `AppError { code:
"IMPORT_FAILED" }` with a stable `details.source` from this closed set:

- `already_imported` — a `story_imports` row already links this `pack_uuid` to a local story.
- `pack_missing` — the pack UUID is no longer in the device index, or its `.content/<SHORT_ID>` folder is absent.
- `pack_invalid` — the pack content violates the declared supported subset (missing/empty required entry, unknown entry, non-regular file, depth exceeded).
- `pack_oversize` — the pack exceeds `MAX_IMPORT_PACK_BYTES` or `MAX_IMPORT_PACK_FILES`.
- `device_changed` — the live re-scan no longer resolves to the requested device (swapped / unplugged), or no supported device answers.
- `fs_read` — reading the pack from the device failed mid-copy. `details.kind` reuses the export I/O set (`permission_denied`, `no_space`, `read_only_filesystem`, `not_found`, `already_exists`, `io`).
- `staging_write` — writing into the local staging area failed (same `details.kind` set).
- `promote` — the atomic staging → `imports/<story_id>` rename failed.
- `db_commit` — the final SQLite transaction failed; the promoted folder is removed by compensation.
- `read_timeout` — the import budget (300 s) was exhausted. Carries `details.elapsed_ms`.
- `spawn_blocking_join` — the worker task could not be joined.
- `other` — fallback for unmapped causes.

A `DEVICE_UNSUPPORTED` rejection with `details.source =
"capability_gate"` (V3 profile) and the `DEVICE_SCAN_FAILED` re-scan
failures keep their existing taxonomies — the UI branches on `code` +
`details.source`, never on free-form strings.

## Official Catalog Contract

The right column hosts a quiet `Catalogue officiel` panel that manages the
local commercial-catalog cache used for title recognition (see
[device-support-profile.md#Title Recognition & Catalog Policy](./device-support-profile.md)).
It is global, not device-specific — caching the catalog recognizes packs even
before a device is plugged in.

| Panel element | Content / behavior |
| --- | --- |
| Status | `N titre(s) officiel(s) en cache` (or `Aucun titre officiel en cache`). Read on mount via `get_official_catalog_status` — the ONLY automatic call (a bounded count query, no network). |
| Offline-first note | States plainly that Rustory contacts no server without a deliberate action. |
| `Récupérer / mettre à jour` | The ONLY networked action: guest auth → catalog download → EAGER cover download into the local cache, on explicit click. Soft-disabled + `aria-busy` while in flight. |
| `Importer depuis un fichier` | 100%-offline alternative: a native open-file dialog picks a catalog file Rust reads/parses/caches (titles only — no cover download on the offline path). A cancelled dialog is a silent no-op. |
| Failure | An in-context `role="alert"` (never a toast) with the normalized message + next gesture and an explicit `Fermer`. |

Covers are cached LOCALLY during the refresh and rendered from that cache:
a recognized entry's `thumbnail` carries the local cache file name (never a
remote URL), and the UI loads the image through the `read_pack_cover` command
(a local read returning a `data:` URL — no network on display). Covers are
decorative (`aria-hidden`); the title carries the accessible name. The offline
file import caches no cover.

Catalog failures carry `code = OFFICIAL_CATALOG_UNAVAILABLE` with
`details.source` ∈ `network` (stage `auth_request` / `packs_request` /
`auth_token` / …), `parse` (stage `json` / `shape` / `empty` / …), `import`
(stage `metadata` / `oversize` / `read` / …), `cover` (stage `not_an_image` /
`oversize` / …) or `storage`. The UI branches on `code`, never on free-form
strings. A missing cover is NOT an error — `read_pack_cover` resolves `null`
and the title shows alone.

## UI Foundation Components

The MVP design system ships a closed core of foundation primitives in `src/shared/ui/`. Feature code consumes them through the barrel export `src/shared/ui/index.ts` — no direct file-by-file import path needed.

| Primitive | Responsibility | Key contract |
| --- | --- | --- |
| `Button` | Interactive action | `variant: "primary" \| "secondary" \| "quiet" \| "destructive"`; when `aria-disabled="true"`, the button stays focusable and clicks are swallowed (keyboard users must reach the reason) |
| `Field` | Labelled text input | Visible `label`, stable `id`, `onChange` receives the next string (never the raw event) |
| `SurfacePanel` | Neutral container surface | `elevation: 0 \| 1 \| 2`, `as`, `ariaLabelledBy` — no business logic |
| `StateChip` | Status pill | `tone: "neutral" \| "info" \| "success" \| "warning" \| "error"`; every tone ships with an ASCII glyph so the signal survives grayscale / color-blindness |
| `ProgressIndicator` | Long-operation feedback | `mode: "indeterminate" \| "determinate"`, always a visible `label`, respects `prefers-reduced-motion` |
| `Dialog` | Modal surface | Native `<dialog>` (free focus trap + Escape); `open`, `onClose`, `title`, `ariaDescribedBy` |
| `Toast` | Lightweight confirmation | `tone` typed to exclude `"error"` at compile time — critical errors never live in a toast alone |

New shared primitives land here only when a motif has appeared at least three times with stable behavior (see UX spec — Core Set Boundary).

## Library Selection Contract

The library supports single and multi selection from the story collection. The contract below is authoritative — feature code MUST NOT invent alternative selection models.

| Aspect | Value |
| --- | --- |
| Source of truth | `selectedStoryIds: ReadonlySet<string>` on the `libraryShell` store (`src/shell/state/library-shell-store.ts`) |
| Persistence | None — selection never survives a restart (avoids stale truth after the app relaunches). |
| Visibility vs selection | Filtering and sorting NEVER change selection. An off-screen selected id stays selected; the counter makes the distinction explicit (`X sur Y — Z sélectionnée(s)`). |
| Purge rule | On every successful `getLibraryOverview` read, the route calls `pruneSelection(presentIds)` to drop ids absent from the fresh overview. The cause is always explicable ("l'histoire a disparu de la bibliothèque locale"). |
| Non-color signal | Selected cards render a colored border, a visible textual prefix (`✓`) in the DOM and `aria-pressed="true"`. Selection survives grayscale and color-blindness checks. |
| Wording | `Aucune histoire sélectionnée` · `1 histoire sélectionnée` · `N histoires sélectionnées`. Strict singular/plural — never `picked`, `checked`, `active`. |

## Story Card Interaction Contract

A `Story Card` is a `role="button" aria-pressed={selected}` focus stop. Interaction maps to:

| Input | Effect |
| --- | --- |
| Click (no modifier) | `replace` — the card becomes the unique selection. |
| `Ctrl+click` / `Cmd+click` | `toggle` — add or remove the card from the current selection. |
| `Shift+click` | Not implemented in the MVP (deferred). |
| Double-click | Open the edit route (`/story/:storyId/edit`). |
| `Enter` on the focused card | Open the edit route. |
| `Space` on the focused card | `toggle` the selection for that card. |
| `Tab` / `Shift+Tab` | Linear traversal through the collection (no composite listbox keyboard model at this stage). |

A click that lands outside a card (header, controls, empty space, other columns) MUST NOT modify the selection — UX-DR7 forbids silent disappearance of selection.

## Library Routing Contract

`React Router` is the sole source of truth for navigation; the Zustand shell store never mirrors the active route.

| Path | Route | Purpose |
| --- | --- | --- |
| `/` | redirect to `/library` | Default entry point. |
| `/library` | `LibraryRoute` | Three-column library context (see `Library Layout Contract`). |
| `/story/:storyId/edit` | `StoryEditRoute` | Resume a `brouillon local` for a library-owned story. No device call, no mutation at this stage. |
| `*` | redirect to `/library` | Unknown paths bounce back to the library. |

Rules:

- Returning to `/library` from `/story/:storyId/edit` preserves shell continuity (selection, filters) through the Zustand store — the URL does not carry that state.
- New routes land here only when a real dominant-context switch appears. The `settings` route is not wired at this stage; add it when a specific need emerges.

## Story Creation Contract

A new `brouillon local` is created through a single modal dialog in the `library` context. No other surface exposes a creation entry point.

| Aspect | Value |
| --- | --- |
| Entry points | Header CTA `Créer une histoire` inside `Story Collection`, plus the same CTA inside the `loaded-empty` region. Both are active in parallel whenever the route wires a handler; they dispatch the exact same flow. |
| Input | A single `Titre` field. No `description`, `genre` or `cover image` at this stage — the editor surface is deferred. |
| UI validation | Mirrors the Rust domain rules so `aria-disabled` flips at typing speed: the normalized title (NFC + trim) must be non-empty, at most `120` Unicode code points, and contain no C0 / C1 control characters nor any code point from the Unicode denylist below. |
| Unicode denylist | Beyond C0 (`U+0000..U+001F`) and C1 (`U+007F..U+009F`) controls, the following `Cf` and line-separator code points are rejected because they would make a title hidden, bidirectionally ambiguous, or carry embedded line breaks: `U+FEFF` (BOM / ZWNBSP), `U+202A..U+202E` (LRE / RLE / PDF / LRO / RLO bidi overrides), `U+2066..U+2069` (LRI / RLI / FSI / PDI bidi isolates), `U+200E` (LRM), `U+200F` (RLM), `U+061C` (ALM), `U+2028` (LINE SEPARATOR), `U+2029` (PARAGRAPH SEPARATOR). ZWJ (`U+200D`) and ZWNJ (`U+200C`) are deliberately allowed — they are load-bearing for many scripts and emoji sequences. |
| Authoritative validation | Rust re-validates on every `create_story` call; a title that slipped past the UI is refused via `AppError { code: "INVALID_STORY_TITLE" }` and no row is inserted. |
| Canonical model | Persisted with `schema_version = 1` and a minimal `CanonicalStructure` `{ "schemaVersion": 1, "nodes": [] }`. Any future extension of the canonical shape MUST bump `schema_version` and ship an SQL migration. |
| Integrity | `content_checksum` is the SHA-256 hex digest of the exact `structure_json` bytes written to disk. |
| Timestamps | `created_at` equals `updated_at` on first insert; both are ISO-8601 UTC at millisecond precision (`YYYY-MM-DDTHH:MM:SS.sssZ`). |
| Ordering | Library default sort is `ORDER BY created_at ASC, id ASC`. UUIDv7 keeps the ordering stable without an extra secondary key. |
| Post-success flow | The module-local SWR cache for `useLibraryOverview` is invalidated, a fresh fetch is triggered, and the router navigates to `/story/:storyId/edit` with `replace: true` so the history stack stays flat. |
| Failure recovery | On rejection, the dialog stays open, the typed title survives, the focus returns to the field, and the Rust-supplied `message` + `userAction` are rendered inside a `role="alert"` region below the field. |

## Story Autosave Contract

A persisted story is editable from the `/story/:storyId/edit` route. MVP
Phase 1 exposes exactly one editable field — the title. The full Story
Node Editor (nodes, media, option links) remains Post-MVP.

| Aspect | Value |
| --- | --- |
| Read rule | `useStoryEditor` calls `get_story_detail` on mount and on every `storyId` change. The overview cache never substitutes for this authoritative read. A `null` return maps to the `Histoire introuvable` surface; a rejection maps to `Reprise indisponible`. |
| Write rule | Each `setDraftTitle` plans a single autosave `500 ms` after the last keystroke. The debounce cancels on every new keystroke so only the latest value survives. The save fires `update_story({ id, title })` in a `BEGIN IMMEDIATE` SQLite transaction. `structure_json` and `content_checksum` are never modified by an autosave. |
| No-op rule | Typing a value that normalizes (NFC + trim) back to the persisted title cancels any pending save, clears any stale failure alert, and settles the chip back to `Brouillon local`. |
| Authoritative validation | Rust re-validates on every `update_story` call with the same rules as `create_story`; an invalid title is refused via `AppError { code: "INVALID_STORY_TITLE" }` and leaves the row untouched. |
| State chip mapping | `idle → Brouillon local (info)`, `pending → Modifications en attente (neutral)`, `saving → Enregistrement… (neutral)`, `saved → Enregistré (success)`, `failed → Enregistrement en échec (error)`. The `saved` state is announced via an `aria-live="polite"` region; transient states stay silent to avoid AT chatter. |
| Failure recovery (AC3) | On rejection, `detail.title` keeps the previous value — never "Enregistré". The typed draft is preserved in the Field. A `role="alert"` region renders the Rust-supplied `message` + `userAction`, with a `Réessayer l'enregistrement` button that re-fires the save using the attempted title. The library overview cache is NOT invalidated on failure — the UPDATE is atomic (NFR9), so the persisted state is unchanged and the overview already reflects the truth. |
| Library coherence | After every successful save, `invalidateLibraryOverviewCache()` drops the module-local SWR snapshot so the next mount of `LibraryRoute` observes the updated title. The invalidation runs BEFORE the `mountedRef` guard so an ACK that arrives after the route unmounted (navigate-away race) still refreshes the overview. No explicit `retry()` is chained — the fetch happens naturally at next mount. |
| Stale success rule | If the save ACK arrives for a title the user has already typed past (e.g. "A" was in flight while the user typed "B"), the route commits the ACK'd value to `detail` (it IS persisted) but re-plans a debounced save for the newer draft. The chip transitions `saving → pending → saving → saved` without ever falsely painting "Enregistré" over a value the field has moved past. |
| H1 source | The visible `<h1>` mirrors `detail.title` (the last-committed title), not `draftTitle` — the H1 must not re-announce at every keystroke nor misrepresent what is actually saved. The editable Field carries the live draft. |
| Missing id rule | `update_story` on an id no longer in `stories` returns `AppError { code: "LIBRARY_INCONSISTENT" }` with `details.source = "story_missing"`. The UI surfaces it through the same `role="alert"` flow as any other save failure. |
| Flush rule | Clicking `Retour à la bibliothèque` (or any route-level unmount) calls `flushAutoSave()`: a pending debounce is cancelled and the save is fired before the synchronous `navigate(...)`. The IPC call itself still resolves asynchronously — the UI will have unmounted before the Rust response arrives, so the save is "fire and commit in Rust, forget in UI". A success is observed on the next `/library` read; a failure after unmount is not re-surfaced to the user (the route is gone) and the prior persisted state remains the source of truth. Durable recovery of a draft that never flushed (crash, kill -9) is a separate feature and not covered by this contract. |
| Persisted-vs-draft invariant | `detail.title` is the source of truth, refreshed from Rust on mount. `draftTitle` is the live Field value. They diverge only while `saveStatus.kind === "failed"`; any other transition reconciles them. |

## Story Export Contract

A persisted story can be exported to a user-chosen file from the
`/story/:storyId/edit` route. MVP Phase 1 exports exactly one story at a
time; batch export from the library is Post-MVP.

| Aspect | Value |
| --- | --- |
| Trigger rule | A `Exporter l'histoire` button lives in the route's action row next to `Retour à la bibliothèque`. The library context menu / batch action flow is Post-MVP; single-story from the edit surface is the only entry point in Phase 1. |
| Dialog rule | The frontend opens a native save dialog via `@tauri-apps/plugin-dialog::save` with `defaultPath = "{sanitizedTitle}.rustory"` and `filters = [{ name: "Artefact Rustory", extensions: ["rustory"] }]`. Cancel returns `null` and is a silent no-op — never an error surface, never an alert. |
| Atomic write rule | Rust stages the artifact through a `NamedTempFile` co-located with the destination, `write_all` + `flush` + `sync_all` the bytes, then `persist()` performs an atomic `rename(2)`. Any intermediate failure drops the `NamedTempFile`, leaving zero residual `.tmp*` files in the target directory. |
| Invariant canonical rule | `save_story_export` is strictly read-only on the `stories` table — `title`, `structure_json`, `content_checksum`, `created_at`, `updated_at` are byte-for-byte invariant across an export. Integration tests assert this row-equality before and after the call. |
| Cache rule | A successful export does NOT call `invalidateLibraryOverviewCache()`. A failed export does NOT call it either. Export never mutates canonical state, so the overview already reflects the truth. |
| State chip mapping | `idle → no chip`, `exporting → Exportation en cours… (neutral)`, `exported → Exporté (success)` inside an `aria-live="polite"` region that auto-hides after `3 s`, `failed → role="alert"` with the canonical `message` + `userAction` plus a `Choisir un autre emplacement` button (relaunches the dialog) and a `Fermer` button (dismiss to idle). The `exporting` transition is NOT announced to AT to avoid noise. |
| Error taxonomy rule | Rust may reject with `EXPORT_DESTINATION_UNAVAILABLE` (`details.source ∈ { invalid_path, parent_missing, temp_create, write_temp, rename, dialog_failed }` and `details.kind ∈ { permission_denied, no_space, io, read_only_filesystem, not_found, invalid_input, other }`, with `details.cause` naming the specific boundary-level reason when `source = "invalid_path"` — e.g. `not_absolute`, `too_long`, `empty`, `trailing_whitespace`, `empty_file_stem`, `symlink_destination`, `internal_app_directory`, `non_filesystem_path`), `LIBRARY_INCONSISTENT` with `details.source = "story_missing"` when the story was deleted between load and export, or `LOCAL_STORAGE_UNAVAILABLE` with `details.source ∈ { sqlite_select, artifact_serialization }` (the latter guards an unreachable serializer failure; an occurrence is a bug, not a user-recoverable state). User-facing messages are picked from a closed canonical table — the raw OS message is never forwarded (PII). |
| Artifact format rule | UTF-8 JSON, pretty-printed with a trailing `\n`, envelope `{ "rustoryArtifact": { "formatVersion": 1, "exportedAt", "exportedBy" }, "story": { "schemaVersion", "title", "structureJson", "contentChecksum", "createdAt", "updatedAt" } }`. `contentChecksum` is recopied byte-for-byte from the row — never recomputed. Forward-compatibility: a future importer MUST refuse `formatVersion != 1` until a new variant is introduced. |
| Default filename rule | `sanitizeFilename(persistedTitle) + ".rustory"`. Sanitization applies NFC, trims whitespace, replaces filesystem-unsafe characters (`\x00-\x1f`, `\x7f`, `/ \\ : * ? " < > |`) with `_`, collapses runs of whitespace/underscore, truncates at 80 code points, and falls back to `histoire` when the result is empty. |
| H1 rule | The visible `<h1>` continues to mirror `detail.title` — export does NOT change the title, so the heading is unaffected regardless of export outcome. |
| Accessibility | The success region is `aria-live="polite"` with `aria-atomic="true"`. The failure region is `role="alert"`. The "Choisir un autre emplacement" button is reachable via keyboard and appears before `Fermer` in tab order so a keyboard user can retry with one keystroke after reading the alert. |

## Story Recovery Contract

A buffered keystroke value can survive an unexpected app shutdown
(crash, kill -9, power loss). On the next mount of `/story/:storyId/edit`
the route detects the surviving draft and proposes a recovery banner
above the editable Field. The user must commit a decision (Apply or
Discard) before resuming editing.

| Aspect | Value |
| --- | --- |
| Trigger rule | At every mount of `/story/:storyId/edit`, `useStoryRecovery(storyId)` calls `read_recoverable_draft({ storyId })`. The banner mounts only when the response is `{ kind: "recoverable" }`. |
| Detection rule | At boot, `lib.rs::run().setup` queries `SELECT story_id FROM story_drafts ORDER BY draft_at DESC` and emits a single `interrupted_session_detected` event into `{app_data_dir}/diagnostics/recovery.jsonl` with the full list of story_ids. The boot probe never blocks on a log write failure. |
| Banner UX | A `<section role="region" aria-label="Brouillon récupéré">` rendered ABOVE the `<h1>`, containing: heading `Brouillon récupéré`, two-line diff `Tu avais tapé : "X"` / `Dernier état enregistré : "Y"`, relative timestamp, two buttons `Restaurer le brouillon` (primary) and `Conserver l'état enregistré` (secondary), and a conditional `role="alert"` block on apply / discard failure. |
| Field locking | While the banner is on screen (`kind ∈ { recoverable, applying, error }`), the title `Field` is `disabled` and the export button is soft-disabled (`aria-disabled="true"`). The user must commit a decision before resuming any editing. |
| Apply rule | `applyRecovery({ storyId })` runs an atomic transaction in Rust: `UPDATE stories SET title = ? + DELETE FROM story_drafts WHERE story_id = ?`. The frontend receives a `UpdateStoryOutput` and patches its in-memory `useStoryEditor` snapshot via `reloadDetailFromOutput` — no follow-up `get_story_detail` round-trip is required. |
| Discard rule | `discardDraft({ storyId })` removes the row without touching `stories`. Idempotent: a second call on an already-empty row resolves silently. |
| Auto-clear rule | `update_story` (autosave) deletes the `story_drafts` row for the same `story_id` inside its existing `BEGIN IMMEDIATE` transaction. A successful autosave consumes the buffer; a failed autosave preserves it for the next session. |
| Validation rule | At apply time, `validate_title(normalize_title(draft_title))` re-validates authoritatively. A draft with control chars / blank-after-trim / > 120 chars is rejected with `INVALID_STORY_TITLE`. The draft row is NOT consumed on rejection so the UI can offer Discard explicitly. |
| Stable error categories | The recovery log produces only five categories: `interrupted_session_detected`, `recovery_draft_proposed`, `recovery_draft_applied`, `recovery_draft_discarded`, `recovery_draft_unavailable`. The category is the NFR24 stable identifier — never a localized message, never a free-form string. |
| Persistence cadence | `useStoryEditor` schedules a `record_draft` 150 ms after each keystroke. The autosave (`update_story`) runs on a 500 ms debounce in parallel. A keystroke between the two windows is captured by the recovery buffer even though no autosave has fired yet — that is the purpose of the shorter window. |
| Persistence cap | The `draft_title` column carries a SQLite `CHECK (length(draft_title) <= 4096)` clause. The Rust application service mirrors the cap by char count and rejects oversize inputs with `RECOVERY_DRAFT_UNAVAILABLE` (`details.source = "draft_too_long"`) before the SQL ever runs. |
| Forbidden | No modal. No toast. No automatic recovery without user choice. No generic "Erreur de récupération" copy — the closed message table from `product-language.md` is the only allowed phrasing. |

### Error payloads — recovery flow

The recovery commands surface failures with a stable `details.source`
discriminator so support can triage without parsing the user-facing
message:

- `record_draft` UPSERT fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_upsert"`. `details.kind` ∈ `busy`, `locked`, `other`.
- `read_recoverable_draft` SELECT fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_select"`.
- `apply_recovery` UPDATE+DELETE atomic fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_apply"`. `details.stage` ∈ `begin_transaction`, `read_draft`, `update`, `delete`, `commit`. A `constraint_violation` re-maps to `INVALID_STORY_TITLE`.
- `discard_draft` DELETE fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "sqlite_delete"`.
- `recovery_log` write fails — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source ∈ { diagnostics_dir, diagnostics_open, diagnostics_write, diagnostics_flush, diagnostics_serialize, diagnostics_app_data_dir, diagnostics_path_invalid, diagnostics_rotate }`. `details.kind` ∈ `permission_denied`, `storage_full`, `read_only_filesystem`, `not_found`, `other`.
- Draft input exceeds 4096 chars — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "draft_too_long"`.
- Draft row vanished between propose and apply — `RECOVERY_DRAFT_UNAVAILABLE`, `details.source = "draft_missing_in_transaction"`.
- Draft fails authoritative validation at apply time — `INVALID_STORY_TITLE` with `details.source = "recovery_draft_invalid"`. The draft row is preserved so Discard remains available.
- Story disappeared (FK violation or rows_affected == 0) — `LIBRARY_INCONSISTENT` with `details.source = "story_missing"`.
