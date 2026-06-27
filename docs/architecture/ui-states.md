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
| Fixable blocking issue exists | `à corriger` | A detected issue blocks the send but the user can repair it (e.g. an invalid title) | A hard unrecoverable block, or a mere warning |
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
| `to_fix` | `à corriger` | A repairable block (e.g. an invalid title); show the cause + the fixing action |
| `presumed_transferable` | `présumée transférable` | Valid for decision surfaces before send |
| `preparing` | `en préparation` | May coexist with preserved local work |
| `transferring` | `en transfert` | Must stay visibly in-context; honest progress (real `%` only during the content copy, never a fake value nor 100 % before the terminal); a non-destructive `Consulter le détail` discloses phase/progress in-context |
| `verified` | `transférée et vérifiée` | Only after explicit confirmation |
| `partial` | `état partiel` | Never collapse into success wording (a verification verdict) |
| `retryable` (`échoué`) | `échec récupérable` | Device left UNTOUCHED; keep enough context for `Relancer` / `Abandonner` |
| `incomplete` | `transfert incomplet` | Write STARTED then interrupted (device mutated); the device may hold a partial copy, a relaunch restores a safe state; distinct from `état partiel`; `Relancer` / `Abandonner` |

## Post-MVP Import State Contract

These states are for post-MVP local structured import flows and must not be mistaken for MVP transfer states. The first flow that realizes them is the
`Local Artifact Import Contract` below (`.rustory` file import); see it for the full two-phase machine, recognition model and error taxonomy.

| Internal Contract State | UI Label | Scope |
| --- | --- | --- |
| `recognized` | `reconnu` | Import analysis found usable material |
| `partial` | `partiel` | Some content is usable, some is not |
| `needs_review` | `à revoir` | The user must inspect before accepting |
| `blocked` | `bloqué` | Import cannot continue safely |
| `resolved` | `résolu` | The import issue has been handled (declared; not emitted in the first iteration) |

## State Transition Rules

- `brouillon local` may lead to `en vérification` or remain local.
- `en vérification` may lead to `bloquée`, `à corriger`, or `présumée transférable`.
- `à corriger` may lead back to `en vérification` after the user repairs the flagged issue (e.g. renames the story).
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
- carry a node media block only in a `toast` — it lives inline at its slot
- paint a node `Enregistré` while one of its fields or media is still blocked
- show an editable node field or media action that cannot be saved (an imported story's node is read-only)

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
- `Envoi indisponible: aucune histoire sélectionnée`
- `Envoi indisponible: sélection multiple`
- `Envoi indisponible: aucun appareil connecté`
- `Envoi indisponible: profil non supporté` — covers a V3 (device write is still being reverse-engineered, like import), FLAM, and any unsupported profile
- `Envoi indisponible: profil ambigu`
- `Envoi indisponible: détection en cours`
- `Envoi indisponible: détection en échec`
- `Envoi indisponible: prépare l'histoire d'abord` — no fresh transfer-artifact descriptor yet; run `Préparer` first
- `Envoi indisponible: histoire native non transférable (pas de pack appareil)` — a native story has no device-format pack; enforced by the backend transfer outcome (`notTransferable`) and surfaced in context
- `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` — **legacy fallback only**: it no longer applies to the write-authorized cohorts (V1/V2), where the `Envoyer vers la Lunii` CTA is now activable; it subsists solely where the panel is given no write target at all (tests/storybook)
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
- `Préparation indisponible: aucune histoire sélectionnée`
- `Préparation indisponible: sélection multiple`
- `Préparation indisponible: aucun appareil connecté`
- `Préparation indisponible: profil non supporté`
- `Préparation indisponible: corrige les blocages d'abord`

For an imported story the node editor HIDES its write affordances (it renders
the projection read-only with a named `Histoire importée (lecture seule)` note)
rather than showing a disabled control with a reason; `Aperçu` / `Retirer` only
render when a media is present, so they are never shown-but-disabled either.

Node-media error payloads carry a stable `details.source` so support can triage
without parsing the user-facing message:

- `media_invalid` — the chosen file is refused at attach: `details.stage` is one of `unsupported_format`, `oversize`, `non_filesystem_path`, `unknown_slot`. Surfaces as `MEDIA_INVALID` at the media slot, never a toast.
- `media_processing_failed` — a transport failure of the media store: `details.stage` is one of `staging`, `promote`, `read`, `db`, `invalid_name`, `app_data_unavailable`, `dialog_failed`, `spawn_blocking_join`. Surfaces as `MEDIA_PROCESSING_FAILED`.

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

## Transfer Decision / Comparison Contract

Before any send, the right-column decision panel shows a **read-only**
comparison between the selected local story and the live device inventory,
so the user understands the impact of a transfer *before* launching one.
The comparison is **composed in Rust** (command `read_transfer_preview`,
returning a `TransferPreviewDto`) and only **presented** by the panel — the
frontend never recomputes device truth. It is a snapshot read: the device is
re-scanned authoritatively at the moment of the decision, never mirrored.

The comparison block lives **inside** the decision panel, in a
`<section aria-label="Comparaison avant envoi" aria-live="polite">` between the
`Sélection courante` and `État de l'appareil` regions. The polite live region
lets the async `loading`→`ready` verdict be announced to screen readers (the
`error` case additionally carries `role="alert"`). It never competes with the
center-column collection as the main work surface, and it never hosts the
device library (which stays a distinct center-column section).

Each "no comparison" cause renders a **distinct, actionable** hint — the panel
never collapses them into one generic line. The route owns the cause (the hook
cannot tell a selection gap from a missing device apart):

| Comparison state | When | Panel rendering |
| --- | --- | --- |
| no comparison — no selection | No local story selected | `Sélectionne une histoire locale pour comparer avant l'envoi.` |
| no comparison — multi selection | More than one local story selected (multi-transfer is out of scope) | `Sélectionne une seule histoire locale pour comparer (le transfert multiple n'est pas encore disponible).` |
| no comparison — no readable device | Exactly one story selected, but no read-authorized device is connected | `Branche une Lunii lisible pour comparer l'histoire sélectionnée avant l'envoi.` |
| loading | The preview read is in flight | `ProgressIndicator` (calm) + `Comparaison en cours…` |
| ready — new | The selected story's pack is NOT on the device (no `pack_uuid`, or `pack_uuid` absent from the inventory) | `Nouvelle sur l'appareil` chip + `Cette histoire serait ajoutée à l'appareil.` + the unchanged-count line |
| ready — replace | The selected story's pack IS on the device (`pack_uuid` present in the inventory) | `Déjà présente sur l'appareil` chip + `Déjà présente sur l'appareil — un envoi la remplacerait.` + the unchanged-count line |
| device changed during comparison | A readable device WAS detected, but the authoritative re-read no longer resolves to it (unplugged / swapped, or the `ready` payload's identifiers don't match the request) | Recoverable `role="alert"` — `L'appareil a changé pendant la comparaison.` + next gesture + a `Réessayer` CTA. NOT the "branche une Lunii" hint, which would contradict the just-detected device. |
| error | The preview read failed (FS error, timeout, local store unavailable, selected story vanished) | In-context message (cause + impact + next gesture), `role="alert"` + `Réessayer`, **never a toast**. The local library and the device section stay intact. |

The unchanged-count line states what stays untouched:
`Aucune autre histoire de l'appareil ne sera modifiée.` (0),
`1 autre histoire de l'appareil restera inchangée.` (1), or
`{n} autres histoires de l'appareil resteront inchangées.` (n > 1).

Rules:

- The comparison is keyed on the pack identity (`pack_uuid`, the
  `story_imports` join key), never on the title or the device identifier — the
  same pack seen from another Lunii resolves to the same identity.
- It shows ONLY what changes the decision. **No size / volume metric** is
  shown in this contract (UX-DR37 — there is no decisional volume before the
  media-preparation step). A useful size reappears later, when preparation
  produces one.
- The send CTA (`Envoyer vers la Lunii`) stays **disabled with the standardized
  reason** `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` in
  every state — the comparison informs, it never enables a write. The send
  capability is governed by the `WriteStory` gate (always `false` in MVP).
- The comparison never asserts a validation verdict: a local story remains a
  `brouillon local`. The `présumée transférable` / `bloquée` states belong to
  the compatibility-validation flow, not to this comparison.
- Errors carry the same closed `details.source` taxonomy as the device-library
  read it reuses (`device_changed`, `fs_read`, `pack_index`, `read_timeout`,
  `scan_timeout`, `os_enum`, `mount_unavailable`, `spawn_blocking_join`,
  `other`), plus `transfer_preview` for the local-store / missing-story cases
  (`LIBRARY_INCONSISTENT` when the selected story vanished,
  `LOCAL_STORAGE_UNAVAILABLE` on a local read failure). No new error code is
  introduced.

## Story Validation / Preflight Contract

Before any send, the right-column decision panel can run a **read-only**
`preflight` that composes a per-story validation verdict, so the user learns
whether an issue must be fixed *before* a transfer is attempted. Like the
comparison above, the verdict is **composed in Rust** (command
`read_story_validation`, returning a `StoryValidationDto`) and only
**presented** by the panel — the frontend never decides "valide / prête /
bloquée". It is an authoritative snapshot: the device profile is re-read at the
moment of the decision, never mirrored, and nothing is persisted (no
`validation_status` row — freshness beats caching on a decision surface).

**Two orthogonal axes (AC1).** The verdict distinguishes:

- **Rustory canonical validity** — is the LOCAL data sound? (axes `structure` /
  `media` / `filesystem`)
- **Lunii compatibility** — would the detected device profile accept this
  story? (axis `deviceProfile`)

Each blocker carries its `axis`, so the panel groups blockers under the two
headers (canonical vs Lunii) and the AC1 distinction stays visible.

**Verdict (a successful read, never an error).** The verdict is one of:

| Internal verdict | UI Label | Chip tone | Meaning |
| --- | --- | --- | --- |
| `presumed_transferable` | `présumée transférable` | success | No real block found (canonically valid + a recognized, supported profile). NOT a transfer success. |
| `to_fix` | `à corriger` | warning | A repairable block was found (e.g. an invalid title) — fixable before send. |
| `blocked` | `bloquée` | error | A hard canonical block was found (corrupt structure, checksum mismatch, unsupported schema). |

The verdict is derived in Rust: `bloquée` if any `Blocking` cause exists, else
`à corriger` if any `Fixable` cause exists, else `présumée transférable`. An
empty-`nodes` story is canonically valid (the v1 canonical form) and is never a
block.

**Closed taxonomy `axis × cause` (AC2).** Every blocker is a closed
`(axis, cause)` PAIR — never a free-form string, never an independent axis and
cause, never two wordings for one cause. The runtime guard validates the pair
against this closed set (an impossible couple like `deviceProfile ×
checksumMismatch` is rejected, not grouped under the wrong heading). Severity is
a fixed property of the cause, and the verdict is DERIVED from the blockers
(blocking > fixable > none) — the guard also rejects a verdict that contradicts
its blockers (e.g. `bloquée` with no blocker). Each cause maps to one canonical
`message` (cause + impact) and one `userAction` (the next gesture); Rust owns
both strings, React renders them verbatim.

| Axis | Cause | Severity | Canonical `message` | Canonical `userAction` |
| --- | --- | --- | --- | --- |
| `structure` | `titleInvalid` | fixable | Le titre enregistré de l'histoire n'est pas valide. | Renomme l'histoire avec un titre valide puis relance la vérification. |
| `structure` | `schemaUnsupported` | blocking | Cette histoire utilise un format plus récent que celui pris en charge par cette version de Rustory. | Mets à jour Rustory pour transférer cette histoire. |
| `structure` | `structureCorrupt` | blocking | La structure interne de l'histoire est illisible ou incohérente. | Restaure une version saine de l'histoire puis relance la vérification. |
| `structure` | `checksumMismatch` | blocking | Les données locales de l'histoire ont changé de façon inattendue (corruption détectée). | Restaure une sauvegarde saine de l'histoire avant de la transférer. |
| `media` | `mediaUnsupported` | blocking | Ce média utilise un format non pris en charge. | Choisis une image PNG ou JPEG, ou un son MP3, WAV ou OGG. |
| `media` | `mediaUnreadable` | blocking | Ce média est illisible ou dépasse la taille autorisée. | Choisis un fichier plus léger et lisible puis réessaie. |
| `media` | `mediaSourceMissing` | fixable | Le fichier d'un média associé n'est plus accessible. | Ré-associe le média ou retire-le ; le reste du nœud reste modifiable. |
| `deviceProfile` | `metadataUnsupported` | blocking | Le profil de la Lunii connectée n'est pas pris en charge. | Consulte le profil de support pour voir les Lunii compatibles. |
| `deviceProfile` | `metadataCorrupt` | blocking | Les marqueurs de la Lunii connectée sont incomplets ou illisibles. | Rebranche la Lunii puis relance la vérification. |
| `deviceProfile` | `familyUnknown` | blocking | La famille de l'appareil connecté n'est pas reconnue. | Branche une Lunii prise en charge puis relance la vérification. |
| `deviceProfile` | `multipleCandidates` | blocking | Plusieurs Lunii compatibles sont connectées en même temps. | Ne garde qu'une seule Lunii branchée puis relance la vérification. |
| `deviceProfile` | `firmwareUnsupported` | blocking | Le firmware de la Lunii connectée n'est pas pris en charge. | Consulte le profil de support pour les firmwares compatibles. |
| `deviceProfile` | `operationNotAuthorized` | blocking | Le profil détecté n'autorise pas la lecture de la bibliothèque de l'appareil. | Consulte le profil de support pour comprendre ce qui est permis. |

**The `media` axis now has a LIVE emitter — the `Story Node Editor`** (see
`Story Node Editor Contract`), which is where a parent attaches and validates a
node's source media. That is where the `(axis = media, cause)` blockers above
are produced and surfaced inline, with the `needs attention` vs `blocked`
distinction. The TRANSFER preflight panel itself does not re-emit media blockers
for a native single-node story: such a story is structurally coherent for the
canonical checks, and converting its source media to a device pack is a
preparation/transfer concern, not a canonical-validity one. The `media` causes
are nonetheless part of the closed wire taxonomy so the verdict surface stays
consistent across both contexts.

**`filesystem` AND `deviceProfile` are still declared without a live emitter
here** (the `deviceProfile` causes above are part of the closed wire taxonomy,
ready for a future device-format validation): filesystem failures surface as
transport `AppError`s; and the `deviceProfile` axis only ever applies to a
CONFIRMED readable supported device — which is compatible by construction in MVP
(a supported profile passes by definition, see
[device-support-profile.md](./device-support-profile.md)). A verdict is composed
ONLY when the live re-scan resolves to the requested readable supported device
(its identity matched). If the re-scan finds no device, an unsupported device, or
more than one (ambiguous), the present device's identity cannot be confirmed as
the one the UI asked about, so the read surfaces a recoverable `device_changed`
(see "No new error code") rather than a compatibility verdict on an unconfirmed
device — never false coverage, never a stale verdict.

**Verdict ⟂ send gate.** The verdict answers "did this story pass the canonical
+ profile checks?"; it is INDEPENDENT of whether a transfer is enabled. The
send CTA (`Envoyer vers la Lunii`) stays disabled with its standardized phase
reason `Envoi indisponible: transfert pas encore activé (MVP Phase 1)` in EVERY
verdict — even `présumée transférable` — because the send capability is
governed by the `WriteStory` gate (always `false` in MVP), never by the
verdict. Folding the gate into the verdict would make every story `bloquée` and
`présumée transférable` unreachable. A `bloquée` verdict does NOT change the CTA
reason: the gate reason is about the phase, while the verdict's block is
surfaced in the validation section itself.

**Surface.** The validation block lives **inside** the decision panel, in a
`<section aria-label="Validation avant envoi" aria-live="polite">`, sibling to
`Comparaison avant envoi`. The polite live region announces the async
`loading`→verdict transition; a transport `error` additionally carries
`role="alert"` and a `Réessayer` CTA, never a toast. The panel renders:
`loading` → a calm `ProgressIndicator`; a verdict → the verdict `StateChip`
(glyph + text, never color alone) + the blockers grouped by axis (`Validité
Rustory` / `Compatibilité Lunii`), each showing its cause copy and `userAction`;
`error` → the in-context recoverable message. Transitions follow the
`State Transition Rules`: `brouillon local → en vérification → {bloquée | à
corriger | présumée transférable}`.

**No new error code.** The verdict is a successful read (`Ok`, a tagged union).
Only transport failures become `AppError`: the selected story vanished
(`LIBRARY_INCONSISTENT`, `details.source = "story_validation"`), the local store
read failed (`LOCAL_STORAGE_UNAVAILABLE`, same source), or the device changed /
became unreadable mid-read (`DEVICE_SCAN_FAILED`, `details.source =
"device_changed"` and the rest of the device-library read taxonomy). The
`device_changed` case ALSO covers a re-scan that resolves to an unsupported or
ambiguous device (the requested readable device is no longer confirmable). The
only non-error read outcomes are `noDevice` (no device at all — the hook folds it
to the same recoverable "device changed" surface) and `ready` (the verdict). The
triggering command is `read_story_validation`; it emits no `device_log` event (a
light read-only snapshot).

## Story Preparation Contract

Once a story is `présumée transférable` (or its only blockers were fixable and
are now repaired) and a readable supported device is connected, Rustory can
**prepare** the artifacts a transfer would need. Preparation is a **purely local**
operation that produces **derived** artifacts: it never writes to the device and
does NOT depend on the `WriteStory` gate. Like the comparison and the validation
it sits beside, the preparation state is **composed in Rust** and only
**presented** by the panel — the frontend never recomposes phases or outcome from
raw events.

**Local ⟂ send gate.** Preparation only assembles, locally, *what a transfer
would need*, so it can be genuinely active in MVP Phase 1 even while the send CTA
(`Envoyer vers la Lunii`) stays disabled with its standardized reason
`Envoi indisponible: transfert pas encore activé (MVP Phase 1)`. The `Préparer`
action is gated on the validation verdict (`présumée transférable`), NEVER on
`WriteStory`; reaching the rest state never enables the send. This mirrors the
preview's `transferable = false` and the verdict ⟂ gate rule above. Folding the
send gate into preparation would make `Préparer` unreachable.

**Observable phases.** Preparation exposes the transfer machine's first two
phases; the rest/failure outcomes reuse the existing MVP states:

| Internal phase / state | UI Label | Notes |
| --- | --- | --- |
| `preflight` | `en vérification` | Re-runs the read-only validation + authoritative device re-scan before any assembly. |
| `preparing` | `en préparation` | Local artifact assembly + integrity re-check. May coexist with preserved local work. |
| `prepared` | rest state: keeps `présumée transférable` + a discreet `Préparée` indicator | Artifacts assembled and fresh. NOT a transfer success — it never enables the send. |
| `retryable` | `échec récupérable` | Keep enough context for `Relancer`. |

The `transfer` and `verify` phases of the machine are **never emitted by the
preparation flow** — they belong to the device-write flow (see the `Story
Transfer Contract` and the `Story Verification Contract` below, both implemented).
In MVP there is **no media transcoding** to perform (the available stories are
raw imported packs already in device format, or minimal native stories with an
empty `nodes`): the media transformer is declared but has no live implementation,
exactly like the `media` / `filesystem` validation axes. The real, testable work
is the observable phase progression plus a genuine local assembly — re-`preflight`
then enumerating and re-checksumming the required artifacts (the imported pack's
files, or the canonical structure) into an ephemeral transfer-artifact descriptor
stamped with a pipeline version.

**Long-running, non-blocking (AC2).** While preparation runs, the **library stays
usable** (navigation, search, selection, sort): the preparation lives in the
right-column panel and never becomes the visual center, never a modal, never a
tunnel, never a toast. Progress is **honest**: the current phase is always named;
a percentage appears ONLY when it is reliable, otherwise the named-phase state
stands on its own (no fake percentage). At minimum the `preparing` and `preflight`
states, and the still-blocking or ready elements, are shown.

**Failure is a state, never a faked success (AC3).** If preparation fails or is
interrupted (error, device unplugged during `preflight`, window close, missing /
corrupt artifact), Rustory **preserves the local draft in full** — the canonical
story is **never** mutated by preparation (only derived artifacts are produced;
in MVP the descriptor stays in memory, so there is not even a derived write to
compensate). The outcome is marked `échec récupérable` with the cause and the
next gesture (`Relancer`) **consultable in the current context** (`role="alert"`,
never a toast), never disguised as success and never promising a hidden partial
resume. The closed failure causes are: `preflightNotPassing` (the verdict is not
`présumée transférable` — the offending blockers are reported, reusing the
validation blocker grouping), `artifactMissing`, `artifactCorrupt`,
`deviceChanged` (the live re-scan no longer resolves to the requested device
during `preflight`), and `interrupted` (deadline / close). Each cause carries one
canonical `message` (cause + impact) and one `userAction`; Rust owns both
strings, React renders them verbatim.

**Surface.** The preparation block lives **inside** the decision panel, in a
`<section aria-label="Préparation" aria-live="polite">`, sibling to
`Comparaison avant envoi` and `Validation avant envoi`. The polite live region
announces each phase transition; a transport `error` additionally carries
`role="alert"` and a `Réessayer` CTA, never a toast. The panel renders:
`preflight` → a `StateChip` `en vérification`; `preparing` → `en préparation` +
calm progress (named phase; a `%` bar only when progress is reliable); `prepared`
→ the discreet `Préparée` indicator (the send stays disabled); `retryable` → the
in-context recoverable message (cause + `userAction`) + a `Relancer` button. Each
state uses a non-color signal (glyph + text). The story-card badge in the library
reflects `en préparation` / `échec récupérable` through the existing `StateChip`
(derived from the panel state, never a competing source of truth).

**Authoritative re-read, not event reconstruction.** Preparation is the first
long-running flow: a `start_prepare_story` command returns an acceptance
immediately and the work continues in the background, reporting through typed
`job:progress` / `job:completed` / `job:failed` events correlated by `job_id`,
each carrying a monotonic `sequence` so consumers stay idempotent and tolerate
late / duplicate delivery. On a terminal event the UI performs an **authoritative
re-read** (`read_preparation_state`) rather than rebuilding truth from the events
alone. The `preflight` follows the same I/O-first / scoped-DB-lock pattern as the
comparison and the validation; a device change during `preflight` surfaces the
recoverable `deviceChanged` outcome. The `preparing` phase is local and does not
require the device to stay plugged in.

**No new persistence.** The transfer-artifact descriptor is **ephemeral and
re-derivable**, like the preview and the verdict: no migration, no
`preparation_outputs` row, no job record in MVP (freshness beats caching; the
transfer is not resumable, so each attempt re-prepares). AC3's "consultable
detail" is satisfied by the in-context state (the hook / panel keeps the last
result + error), exactly like the recoverable errors of the comparison and the
validation. Durable derived artifacts + their table are deferred to the real
transfer step.

**Error contract.** The preparation states (`preflight` / `preparing` /
`prepared` / `retryable`) are outcomes of a **successful** read (a tagged union +
event payloads), never an error. Only a **transport** failure that prevents even
producing a terminal job outcome becomes an `AppError` — a new code
`PREPARATION_FAILED`, reserved for transport. A **functional** preparation failure
(missing / corrupt artifact, `preflight` not passing, interruption) is the
terminal `retryable` state of the job, not a raw `AppError`.

## Story Transfer Contract

Once a story is **`Préparée`** (a transfer-artifact descriptor was assembled — so
an **imported** story carrying device-format pack files) and a **write-authorized**
Lunii is connected, Rustory can run the real **transfer** — the FIRST device write.
The transfer state is **composed in Rust** and only **presented** by the panel.

**Gate before mutation, fail-closed (AC2 / FR34).** The `WriteStory` capability is
checked **before any write I/O**: the send is allowed only on a write-authorized
cohort. In MVP Phase 1 the matrix wires writes for **Lunii Origine v1** and
**Mid-Gen v2**; **V3 stays read-only** (active reverse-engineering, same rationale
as import) and FLAM is unsupported. The `Envoyer vers la Lunii` CTA is therefore
**activable** — active ONLY on (write-authorized cohort + `Préparée` story + a
single clear target), disabled everywhere else with a standardized
`Envoi indisponible: …` reason. **No confirmation modal** is shown when the
context and target are unambiguous (single local selection + one writable device).

**Observable phase.** The transfer drives the machine's `transfer` then `verify`
phases on the same job (the `verify` phase is the FINAL phase — see the `Story
Verification Contract` below):

| Internal phase / state | UI Label | Notes |
| --- | --- | --- |
| `transferring` | `en transfert` + honest progress | The write runs in the background; the library stays usable. A `%` bar appears ONLY during the measurable content copy — never a fake value nor 100 % before the terminal; a named phase otherwise. A non-destructive `Consulter le détail` discloses the phase / progress in-context (no cancel — out of scope). |
| `verify` (TRANSIENT, not a resting terminal) | `écriture effectuée — vérification à venir` | The write is done; the read-only verification re-read is running. It settles to one of the verdicts below. |
| `verified` (terminal) | `transférée et vérifiée` | Verification PROVED the write (indexed + content present + byte-faithful). The sober success + the AC2 summary (what changed / stayed unchanged / final state). |
| `partial` (terminal) | `état partiel` | Verification re-read the device but confirmed only an incoherent/incomplete result. Never collapsed into success. Distinct from `transfert incomplet`. `Relancer` / `Abandonner`. |
| `retryable` (`échoué`) | `échec récupérable` | The device was left UNTOUCHED (a write-phase refusal), OR the verification could not confirm the result (device gone / unreadable during `verify`). Keep enough context for `Relancer` / `Abandonner`. The local draft is preserved in full. |
| `incomplete` | `transfert incomplet` | The write STARTED then was interrupted (device mutated): the Lunii may hold a partial copy; a relaunch (full cycle) restores a safe state. Distinct from `état partiel`. `Relancer` / `Abandonner`. |

**No false success (AC3).** No success is communicated until BOTH the write AND the
verification have completed. After a successful write the job enters the TRANSIENT
`verify` phase (`écriture effectuée — vérification à venir`), which settles to
`transférée et vérifiée` (proof passed), `état partiel` (re-read incoherent) or
`échec récupérable` (re-read could not confirm) — `transférée et vérifiée` is
**never** shown without the verification proof. An interruption / failure (device unplugged mid-write,
`.content` not writable, no space, a stale / corrupt descriptor, a native story
with no device-format pack) is the terminal `retryable` state: the canonical story
is **never** mutated (FR18), there is **no partial resume** (a failed transfer
requires a fresh full cycle), and the recoverable detail is shown **in context**
(`role="alert"`, never a toast) with `Relancer`.

**Échoué vs incomplet (AC2).** An interruption resolves to one of two honest
terminals, classified by a property of the DEVICE — whether the write reached the
device mutation — not by the cause: **`échoué`** (`échec récupérable`) when the
device was left untouched (the failure happened before the atomic promotion), and
**`incomplet`** (`transfert incomplet`) when the mutation had started (a promoted
folder may exist before the index update). The writer reports a
`reached_device_mutation` signal; the closed cause taxonomy stays orthogonal.
Neither is a success nor a false failure; a relaunch is always a full cycle (never
a hidden partial resume), and the writer proves-or-refuses an existing target pack
so a relaunch converges safely.

**Context preserved in-session (AC3).** The `échoué` / `incomplet` outcome (cause
+ message + next action) lives in the CURRENT context — the live state of the
transfer hook, kept sticky (a late `job:progress` never regresses it) — until the
user chooses `Relancer` (full cycle) or `Abandonner` (back to a stable library,
draft intact). It is NOT carried by the job-shell store (which holds only
phase / progress).

**Durable cross-session memory (AC2).** The LAST terminal outcome ALSO survives an
app restart and a later return to the library / device panel: it is persisted to a
minimal `transfer_jobs` table (one row per story, PK `story_id`, FK `stories(id) ON
DELETE CASCADE`, UPSERT "latest wins" — the exact shape of `story_drafts`). Only
TERMINALS are written (`verified` / `partial` / `retryable` / `incomplete`), never
an in-flight `transferring` / `verifying` phase (it would be a lie after a restart —
the job died with the app). The full rules — reconciliation, consumption / purge,
boot probe — live in the **Transfer Resume Contract** below.

**Closed write-error taxonomy.** `fs_write` (write / space failure on the device),
`device_changed` (the live re-scan no longer resolves to the requested device),
`checksum_mismatch` (the assembled artifact no longer matches its stored
reference), `timeout` (budget exceeded), `interrupted` (deadline / close /
unplugged). Each maps to one canonical `message` + `userAction` — Rust owns both
strings, React renders them verbatim.

**Safe, atomic, offline write.** The write reuses the safe-write pattern: stage on
the device volume → promote atomically (`rename`) → `fsync` the promoted tree +
parent → update the device pack index atomically (files first, index after — a
pack is never indexed without its content present). Zero network I/O (USB only —
FR19). On failure the staging is swept; the canonical draft is untouched.

**Authoritative re-read, not event reconstruction.** A `start_transfer_story`
command returns an acceptance immediately and the write runs in the background,
reporting through the shared typed `job:progress` / `job:completed` / `job:failed`
events correlated by `job_id` (`jobType = "transfer_story"`, monotonic `sequence`).
On a terminal event the UI performs an **authoritative re-read**
(`read_transfer_state`) rather than rebuilding truth from the events alone.

**Surface.** The transfer block lives **inside** the decision panel, in a
`<section aria-label="Transfert" aria-live="polite">`, sibling to the preparation
region. It renders: `transferring` → a `StateChip` `en transfert` + honest
progress + a non-destructive `Consulter le détail` disclosure; the TRANSIENT
`verify` phase → the factual `écriture effectuée — vérification à venir` line;
`verified` → a `success` `StateChip` `transférée et vérifiée` + the sober summary
(`« <Titre> » est maintenant sur la Lunii` + how many other stories stayed
unchanged), `aria-live="polite"`, never an alert; `partial` → a `warning`
`StateChip` `état partiel` (distinct text from `transfert incomplet`) in
`role="alert"`; `retryable` (`échoué`) → `échec récupérable`; `incomplete` → the
distinct `transfert incomplet` chip (its own glyph) + the partial-copy message;
the non-success failure terminals offer `Relancer` AND `Abandonner`; transport
`error` → the in-context message + `Réessayer`; never a toast. Each state uses a
non-color signal (glyph + text). The story-card badge reflects `en transfert` /
`transférée et vérifiée` / `état partiel` / `échec récupérable` / `transfert
incomplet` through the existing `StateChip`.

**Durable resume memory.** Unlike the preview / verdict / preparation, the LAST
terminal outcome IS persisted (the minimal `transfer_jobs` table, migration
`0005_transfer_jobs.sql`), so the panel can re-offer `Relancer` / `Abandonner`
after an app restart — see the **Transfer Resume Contract**. The live appliance
stays the truth for a `verified` re-derivation (re-scan + re-checksum via
`read_transfer_state`); the durable memory only fills the gap the appliance cannot
reproduce (the non-success terminals), and a `Relancer` is always a full fresh
cycle (never a hidden partial resume).

**Error contract.** The transfer states (`transferring` / `verify` / `verified` /
`partial` / `retryable`) are outcomes of a **successful** read, never an error.
Only a **transport** failure that prevents even producing a terminal job outcome
becomes an `AppError` — `TRANSFER_FAILED`, reserved for transport (the exact
parallel of `PREPARATION_FAILED`). A **functional** transfer/verify failure is a
terminal job state (`retryable` / `partial`), not a raw `AppError`; the verify
verdicts are job states too, never new error codes.

## Story Verification Contract

The **`verify` phase is the FINAL phase of the same `transfer_story` job**, emitted
automatically after a successful write — never a separate command or job. It is the
explicit proof the reliability NFR requires ("no success shown before the required
verification step completes"): it PROVES what the write CLAIMS.

**Read-only re-read, gated `ReadLibrary`.** Verification re-scans the device, then
reads its inventory through the proven `read_library` path. It reuses the
`ReadLibrary` capability (true for every MVP cohort) — there is **no new
`SupportedOperation`**. Because the write itself is gated `WriteStory` (V1/V2 ✅,
V3 ❌), `verify` only ever runs after a write on a write-authorized cohort, so the
success path (`verified`) is demonstrable on V1/V2 or a fake mount; V3 keeps
blocking **before** the write, never reaching `verify`. It writes nothing — the
device and the canonical draft are never mutated (FR18).

**Three honest verdicts.** From the re-read facts:

| Verdict | UI Label | When |
| --- | --- | --- |
| `verified` | `transférée et vérifiée` | The UUID is indexed in `.pi`, the `.content/<SHORT_ID>` folder is present, AND the device bytes re-checksum to the prepared baseline (byte fidelity). |
| `partial` | `état partiel` | The device was mutated and the pack is present but NOT fully coherent (e.g. indexed but the bytes diverge, or content present but not indexed). A non-success, never a silent success. |
| `failed` | `échec récupérable` | The re-read PROVES the write did not land (pack absent) OR cannot confirm it (device gone / unreadable during `verify`). A reconnected relaunch re-verifies. |

**What is really verifiable.** On an opaque imported pack, verification proves —
offline, key-free — that the UUID is indexed, the content folder is present, and
the written bytes re-checksum to the prepared artifact's baseline (the EXACT import
aggregation: `rel_path` + NUL + bytes, in manifest order). It does **not** decrypt,
inspect media, or validate the internal pack structure. `transférée et vérifiée`
means **byte fidelity + indexing confirmed — nothing more**; it never implies a
semantic content validation.

**Continuity, not pre-write identity.** A successful write mutates `.pi`, so the
device identity Rustory derives from it legitimately CHANGES across the write —
the pre-write identifier can no longer be re-pinned. The in-job verify therefore
binds to the device it WROTE TO via a STABLE proof that survives the `.pi`
mutation: the USB **volume serial** (falling back to the written **mount path**
when no serial is available). A Lunii swapped after the write for ANOTHER
supported device — even one that already holds the same pack + bytes — fails this
continuity check and ends `failed`, so `verified` is never attributed to the wrong
device; a vanished / ambiguous device is `failed` too. Content presence is probed
on `.content/<SHORT_ID>` INDEPENDENTLY of the `.pi` index (a promoted-but-unindexed
pack reads as present ⇒ `état partiel`, not a false `failed`); a readable byte
DIVERGENCE is `état partiel`, while an ABSENT pack or an UNCONFIRMABLE re-checksum
is `failed`. The later authoritative `read_transfer_state` is pinned to the
(re-detected) target device.

**Confirmation summary (FR15), composed in Rust, carried on the terminal.** On
`verified` the panel shows a sober confirmation summarizing what changed (`« <Titre>
» est maintenant sur la Lunii`) and what stayed unchanged (the N other device
stories — reusing the comparison's `unchanged_count`). Both lines are **composed in
Rust** and travel READY-MADE on the `job:completed` event, so the UI renders the
success straight from the terminal — never via a re-read with the now-stale
pre-write identifier — and never recomposes the text in React. `aria-live="polite"`,
never a toast / modal. `état partiel` and `échec récupérable` are shown
`role="alert"` in-context with `Relancer` / `Abandonner`.

**Durable resume memory.** The verdict lives in the live transfer-hook state (sticky
via the same settle/teardown discipline); the `verified` success is settled from
the `job:completed` summary, and `read_transfer_state` can also re-derive `verified`
on demand (same re-scan + re-checksum) for a re-mount with a freshly detected id. A
transient `partial` / `failed` verdict is NOT reproduced by the passive re-read — it
belongs to the live session, but it IS now remembered across an app restart by the
durable `transfer_jobs` memory (migration `0005_transfer_jobs.sql`): on re-mount the
panel re-hydrates the last `état partiel` / `échec récupérable` / `transfert
incomplet` terminal so the user can still `Relancer` / `Abandonner`. The
reconciliation rule still forbids promoting an `idle` live read to `verified` from
memory (no false success) — see the **Transfer Resume Contract**.

## Transfer Resume Contract

A transfer's last TERMINAL outcome can survive an app restart (or a later return to
the library / device panel). When the story is selected again, the panel re-hydrates
the surviving outcome so the parent can still `Relancer` (a full fresh cycle) or
`Abandonner` (purge the memory, library intact). The canonical story is never touched
(FR18); a relaunch is never a hidden partial resume.

| Aspect | Value |
| --- | --- |
| Trigger rule | When a story is selected, `useStoryTransfer` calls `read_transfer_outcome({ storyId })` and seeds its sticky state with a remembered NON-success terminal (`partial` / `retryable` / `incomplete`) so the panel re-shows `état partiel` / `échec récupérable` / `transfert incomplet` + `Relancer` / `Abandonner`, exactly as if the `job:failed` had just fired. `settledRef` is preserved, so a remembered terminal is never regressed by a late live re-read. |
| Reconciliation rule | The live `read_transfer_state` ALWAYS wins for `verified`: a connected device that proves the pack present + byte-faithful is `transférée et vérifiée`, whatever the memory says. The memory only supplies what the live read cannot reproduce: the non-success terminals, and — when NO device is connected — the last known result, presented as a RECALL of the last transfer, never as a live device truth. It is FORBIDDEN to promote an `idle` live read to `verified` from memory (that would be a false success). |
| Detection rule | At boot, `lib.rs::run().setup` queries `SELECT story_id FROM transfer_jobs WHERE terminal_kind <> 'verified' ORDER BY recorded_at DESC` (capped) and emits a single `interrupted_transfer_detected` event into `{app_data_dir}/diagnostics/transfer.jsonl`. `verified` rows are EXCLUDED — a quit-after-success has nothing pending to acknowledge (it is never re-surfaced by `hydrate`), so it must not raise a false interruption; the `verified` row is kept only for the "latest wins" overwrite of a failure by a successful relaunch. The boot probe never blocks on a log write failure. The per-story re-hydration is driven by the route-level read, not the probe. |
| Surface | The remembered terminal is rendered in the panel's anchored transfer region (`role="alert"` for a non-success, `aria-live="polite"` for a recalled `verified`), never a toast / modal; the story-card badge reflects the remembered outcome through the existing `StateChip`. No UI jargon (`job` / `write` / `staging` / `.pi` / `checksum` / `verify` / `transfer_jobs`). |
| Badge derivation (scope) | The story-card badge is derived from the SINGLE tracked transfer-hook state and is re-hydrated PER SELECTION (the route effect keyed on the selected story), NOT seeded at boot. So after a restart, a story with a remembered recoverable terminal shows its badge only once it has been selected at least once (re-visiting the story restores it — AC2). The boot probe only emits one `interrupted_transfer_detected` trace; it does not push per-story badges to the front. Seeding every visible story's badge at boot is a deliberate non-goal (it would mean reading the whole table eagerly for a cosmetic anchor). |
| Relaunch rule | `Relancer` re-runs the WHOLE cycle (`preflight → prepare → transfer → verify`) from the preserved local draft via the existing send path, with the CURRENT/fresh `writableDeviceId` — never the stored `device_identifier` (a write mutated `.pi`, so the stored identity is stale by construction). Gated on `writableDeviceId !== null`; otherwise the panel shows `Rebranche la Lunii pour relancer` (the fail-closed mirror of the send gate) while keeping `Abandonner`. Single-flight: `Relancer` is disabled while a job is in flight. |
| Consumption / auto-clear rule | A terminal UPSERTs the row (`BEGIN IMMEDIATE`, the DB lock released BEFORE any diagnostics write — the `recovery.rs` discipline). `Abandonner` purges the row (idempotent DELETE) and returns to a stable library, draft intact. A successful relaunch ending `verified` sets the row to `verified` (the last useful result becomes the success). Quitting without acting keeps the row for the next session. |
| Persisted terminals | `verified` (carries `summary_changed` / `summary_unchanged`), `partial` (`verify_verdict = "partial"`), `retryable` (`cause` + `completeness = "failed"`, plus `verify_verdict = "failed"` when the verify re-read could not confirm), `incomplete` (`cause` + `completeness = "incomplete"`). An in-flight `transferring` / `verifying` phase is NEVER persisted. |
| Stable diagnostic categories | The transfer log adds `interrupted_transfer_detected` to its closed category set (alongside `transfer_started` / `transfer_completed` / `transfer_failed`). The category is the stable identifier — never a localized message, never a free-form string. |
| Degradation rule | Reading the memory on mount is BEST-EFFORT: a read failure is logged and treated as "no memory" (it never breaks the flow or blocks the UI — this is operational observability). A write / purge failure (and the explicit `Abandonner`, where a purge failure must be visible) surfaces the new `TRANSFER_OUTCOME_UNAVAILABLE` error code, RESERVED for the persistence transport (SQLite / diagnostics) — never for a functional transfer failure (which stays a job terminal). |
| Forbidden | No modal. No toast. No automatic relaunch without a user choice. No promoting an `idle` live read to `verified` from memory. No mutation of the canonical story or its draft by the memory or the relaunch. |

### Error payloads — transfer resume flow

- `record_transfer_outcome` UPSERT fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "sqlite_upsert"`. A FK violation (the story vanished) re-maps to `LIBRARY_INCONSISTENT` with `details.source = "story_missing"`.
- `read_transfer_outcome` SELECT fails — best-effort on mount: the command LOGS a `transfer_outcome_unavailable` trace (`source = "sqlite_select"`) and returns `null` (the hook treats it as "no memory"); the read never rejects for a transport failure.
- `discard_transfer_outcome` DELETE fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "sqlite_delete"`.
- A blocking worker that cannot be joined (panic / cancel) — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source = "spawn_blocking_join"` (the read additionally logs the trace and resolves to `null`).
- `transfer_log` write fails — `TRANSFER_OUTCOME_UNAVAILABLE`, `details.source ∈ { diagnostics_dir, diagnostics_open, diagnostics_write, diagnostics_serialize, diagnostics_clock, diagnostics_rotate, diagnostics_path_invalid, diagnostics_app_data_dir }`.

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
| Input | A single `Titre` field. No `description`, `genre` or `cover image` at this stage — richer story inputs are deferred. After a successful creation the router lands on the `Story Editor Shell` (see `Story Editor Shell Contract`). |
| UI validation | Mirrors the Rust domain rules so `aria-disabled` flips at typing speed: the normalized title (NFC + trim) must be non-empty, at most `120` Unicode code points, and contain no C0 / C1 control characters nor any code point from the Unicode denylist below. |
| Unicode denylist | Beyond C0 (`U+0000..U+001F`) and C1 (`U+007F..U+009F`) controls, the following `Cf` and line-separator code points are rejected because they would make a title hidden, bidirectionally ambiguous, or carry embedded line breaks: `U+FEFF` (BOM / ZWNBSP), `U+202A..U+202E` (LRE / RLE / PDF / LRO / RLO bidi overrides), `U+2066..U+2069` (LRI / RLI / FSI / PDI bidi isolates), `U+200E` (LRM), `U+200F` (RLM), `U+061C` (ALM), `U+2028` (LINE SEPARATOR), `U+2029` (PARAGRAPH SEPARATOR). ZWJ (`U+200D`) and ZWNJ (`U+200C`) are deliberately allowed — they are load-bearing for many scripts and emoji sequences. |
| Authoritative validation | Rust re-validates on every `create_story` call; a title that slipped past the UI is refused via `AppError { code: "INVALID_STORY_TITLE" }` and no row is inserted. |
| Canonical model | Persisted with `schema_version = 1` and a minimal `CanonicalStructure` `{ "schemaVersion": 1, "nodes": [] }`. Any future extension of the canonical shape MUST bump `schema_version` and ship an SQL migration. |
| Integrity | `content_checksum` is the SHA-256 hex digest of the exact `structure_json` bytes written to disk. |
| Timestamps | `created_at` equals `updated_at` on first insert; both are ISO-8601 UTC at millisecond precision (`YYYY-MM-DDTHH:MM:SS.sssZ`). |
| Ordering | Library default sort is `ORDER BY created_at ASC, id ASC`. UUIDv7 keeps the ordering stable without an extra secondary key. |
| Post-success flow | The module-local SWR cache for `useLibraryOverview` is invalidated, a fresh fetch is triggered, and the router navigates to `/story/:storyId/edit` with `replace: true` so the history stack stays flat. |
| Failure recovery | On rejection, the dialog stays open, the typed title survives, the focus returns to the field, and the Rust-supplied `message` + `userAction` are rendered inside a `role="alert"` region below the field. |

## Story Editor Shell Contract

The `/story/:storyId/edit` route renders the `Story Editor Shell` — the
dedicated screen, separate from the library, where a parent resumes a story
without losing the global context. It is the same route the library opens on
double-click / `Éditer` (see `Story Card Interaction Contract`) and leaves
through `Retour à la bibliothèque` (see `Library Routing Contract`, which
already preserves selection and filters across the round trip). The shell
opens for a native story and for an imported one — both are canonical
`stories` rows read by `get_story_detail`. A native story's current node is
editable; an imported story's node is projected **read-only** (its declared
edit scope is a later iteration), so the shell never shows an editable control
that cannot be saved.

This contract owns the SHELL: the three-zone frame, its entry/exit, and its
keyboard model. The behaviors hosted inside it keep their own contracts — the
title field (`Story Autosave Contract`), the current-node editor
(`Story Node Editor Contract`), export (`Story Export Contract`), and draft
recovery (`Story Recovery Contract`). Editing the **content** of the current
node (its text, metadata, image and audio) now lives in the shell through the
`Story Node Editor Contract`; reorganizing the structure and option links
across multiple nodes stays out of scope (it ships the single current node,
not a node tree).

| Aspect | Value |
| --- | --- |
| Three coexisting zones | The shell shows, at once: the global structure (`Story Structure Navigator`), the current node (host zone), and the story state + actions. None is hidden behind a tab or a click — all three are visible together so the global context is never lost while editing. |
| Content states (v2) | The v2 canonical model carries exactly one current node. Each zone renders honest, NAMED states (`Structure de l'histoire` shows the story root + the current node; `Nœud courant` shows the editor for that node, with named empty states for an empty field or an absent optional media), never blank, never disguised, never a fake node. A new story (or a migrated one) starts with an empty current node — that is a valid starting state, not an error. |
| Structure projection (read-only, from Rust) | The navigator consumes the current node PROJECTED by Rust (`detail.node`, see `Story Node Editor Contract`): it shows the story as the structure root plus the current node, clearly identified by its stable id. It NEVER re-serializes or reformats `structureJson` (covered byte-for-byte by `content_checksum`); every mutation goes through a Rust command. When Rust cannot project a node (a corrupt / drifted structure — near-impossible since Rust is authoritative and checksum-guarded) the zone falls back to a NAMED degraded state (`Structure illisible`), never a crash, never a fake node. |
| Current-node zone | The `Story Node Editor` (see `Story Node Editor Contract`): the labelled text and metadata fields plus the image and audio media slots of the current node. For a native story the fields and media actions are active; for an imported story the same projection renders read-only. |
| Story state + actions | The persisted title (`<h1>`, mirrors `detail.title`), the editable title field + autosave chip (`Story Autosave Contract`), the draft recovery banner when present (`Story Recovery Contract`, which keeps priority over the field), and the global actions `Retour à la bibliothèque` (+ `Exporter l'histoire`). |
| Global actions scope | `Retour` and `Exporter` only. Sending a story to a device stays ANCHORED IN THE LIBRARY — it is never an editor action. No node-level or preflight validation lives in the shell (preflight is a transfer flow). |
| Keyboard & focus | Stable focus order: structure → current node → global actions. Focus is visible on every zone (the global `:focus-visible` ring). Meaning is never carried by color alone (glyph + text). A problem is never carried in a toast alone. |
| Context separation | Editing is a SEPARATE dominant context (its own route): the shell never renders the library collection / filters. Returning restores the library with its useful card traces intact (preparation / transfer badges, import provenance) — and never silently erases the selection or filters when they stay coherent. |
| Read rule & performance | The shell consumes the single authoritative `get_story_detail` read already owned by `useStoryEditor` (title + `structureJson` + timestamps) — no extra read, no device call, fully offline. Re-open stays well within NFR2 (one small read + a tiny JSON parse). |
| No regression | Title autosave, draft recovery, and export keep their exact behavior; `flushAutoSave` still fires on `Retour` and on unmount so a keystroke typed mid-debounce is never lost. |

## Story Autosave Contract

A persisted story is editable from the `/story/:storyId/edit` route, inside
the `Story Editor Shell` (see `Story Editor Shell Contract`). The **title** is
autosaved by this contract; the **content of the current node** (text,
metadata, image and audio) is autosaved by the `Story Node Editor Contract`,
which reuses this exact engine (the `500 ms` debounce, the `150 ms` recovery
buffer, `flushAutoSave`, the call-correlation guards, and the overview
invalidation). Reorganizing the structure and option links across multiple
nodes remains deferred.

| Aspect | Value |
| --- | --- |
| Read rule | `useStoryEditor` calls `get_story_detail` on mount and on every `storyId` change. The overview cache never substitutes for this authoritative read. A `null` return maps to the `Histoire introuvable` surface; a rejection maps to `Reprise indisponible`. |
| Write rule | Each `setDraftTitle` plans a single autosave `500 ms` after the last keystroke. The debounce cancels on every new keystroke so only the latest value survives. The save fires `update_story({ id, title })` in a `BEGIN IMMEDIATE` SQLite transaction. The **title** path never modifies `structure_json` or `content_checksum`. Node **content** (text / metadata) takes a SEPARATE path — `update_node_content`, owned by the `Story Node Editor Contract` — which DOES re-serialize `structure_json` and recompute `content_checksum` in Rust, behind the `schema_version` v2 model. The two write paths never share a command: the title-only `update_story` is forbidden from touching the canonical body. |
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

## Story Node Editor Contract

The `Story Node Editor` fills the current-node zone of the `Story Editor Shell`
(see `Story Editor Shell Contract`). It edits the **content of the single
current node**: its narrative **text**, its **metadata** (a human-readable
label), and two optional **media** — an **image** and an **audio**. It does NOT
add, move, delete or relink nodes (that is a separate, deferred flow): there is
exactly one current node, projected by Rust, and this contract edits it in
place.

**Projection from Rust (AC3).** The node is PROJECTED by the Rust core inside
`get_story_detail` as `detail.node` (a `NodeContentDto`), never recomposed in
React from `structureJson`. The projection carries the node's stable `id`, its
`text`, its `label`, and a resolved state for each media slot (see below). The
frontend consumes the projection; it never parses or rewrites `structureJson`
(those bytes stay opaque and are covered by `content_checksum`). The stable
`id` is what keeps the current node clearly identified across a long edit
session — no identity drift after N edits. When Rust cannot project a node
(corrupt / drifted structure) `detail.node` is `null` and the zone renders the
named degraded state (`Structure illisible`), never a fabricated node.

**Editability.** A native story's node is editable. An imported story's node is
projected **read-only** with a named reason (its declared edit scope is a later
iteration) — the editor NEVER shows a text field or a media action that cannot
be saved.

| Aspect | Value |
| --- | --- |
| Supported fields | The narrative **text** (a multi-line field) and the node **metadata label** (a single-line field). Both may be empty — an empty node is a valid starting state, not an error. |
| Text / metadata autosave | Reuses the `Story Autosave Contract` engine verbatim: a `500 ms` debounce after the last keystroke, the `150 ms` recovery buffer, `flushAutoSave` on `Retour` / unmount, the call-correlation guards, and the overview invalidation on a terminal save. The save fires `update_node_content({ storyId, nodeId, text, label })`, which re-validates the canonical structure, re-serializes `structure_json` and recomputes `content_checksum` in a single `BEGIN IMMEDIATE` transaction. It NEVER reuses the title-only `update_story`. |
| Media machine | Each media slot (image, audio) is an explicit action set — `Ajouter` / `Aperçu` / `Remplacer` / `Retirer` — persisted IMMEDIATELY on the action (not debounced like text). `Ajouter` / `Remplacer` open a native file picker (non-blocking), validate the chosen file in Rust, store its bytes under Rust infrastructure, and write the node's asset reference. `Retirer` clears the reference. `Aperçu` reads the stored bytes back through a Rust command for presentation only — the frontend never owns the media bytes. |
| Acknowledgement < 1 s (NFR3) | Adding or replacing a media produces a VISIBLE acknowledgement (a preview / a named state) in under one second; any heavier work (hashing a large file) continues in the background without freezing the UI (NFR5). |
| Supported source formats | Images **PNG / JPEG**; audio **MP3 / WAV / OGG**. Recognized by magic bytes (signature), never by file extension. The set is declared in [device-support-profile.md](./device-support-profile.md). No transcoding happens here — converting to a device pack stays a transfer/preparation concern. |
| Validation — attention vs blocking (AC2) | Rust is the authority (the UI validates only for ergonomics). The node editor is the FIRST living emitter of the `media` axis of the validation taxonomy (see `Story Validation / Preflight Contract`). Two distinct levels: **`needs attention`** (`à corriger`, repairable — e.g. a referenced media whose source file is no longer readable; does NOT block autosaving the rest of the node) and **`blocked`** (`bloquée`, a real block — e.g. an unsupported / unreadable / oversize file at attach time; that media slot is NOT saved until corrected). Each level names the cause, the impact, and the next gesture. |
| Localized errors | An error lives INLINE next to the field / media slot it concerns, in a `role="alert"` surface — NEVER a lone toast, NEVER color alone (glyph + text). A media block lives at its slot; a text validation issue lives at the text field. A partial state is never disguised as a success. |
| Error codes | A media attach / read failure rejects with `MEDIA_INVALID` (unsupported format, unreadable, oversize — a real block surfaced at the slot) or `MEDIA_PROCESSING_FAILED` (a transport failure of the media store: staging / promotion / read). Both carry a closed `details.source` and are PII-safe (no absolute path, no raw OS message). |
| State chip mapping | The node editor reuses the autosave chip for text / metadata (`idle → Brouillon local`, `pending`, `saving`, `saved → Enregistré`, `failed`). A media slot carries its own named state: `Aucune image` / `Aucun audio` (empty), a preview/identity when present, `Média à corriger` (attention), `Média bloqué` (blocked). |
| Recovery (NFR8) | The `150 ms` recovery buffer extends to the node's in-progress text so a hard kill (kill -9) mid-edit does not lose the typed value, mirroring the title recovery. The buffered node text is offered back on the next open through the `Story Recovery Contract` surface. |
| Atomicity (NFR9) | The editor never paints `Enregistré` over an unsaved change: the same source-of-truth reconciliation as the title autosave applies. A failed node save leaves the previous canonical body untouched (the `BEGIN IMMEDIATE` transaction commits or rolls back whole). The canonical body of ANY OTHER story is never touched. |
| Keyboard & focus | Stable tab order: structure → node fields / media slots → global actions. A disabled media action carries a standardized reason (see `Disabled Actions and Reasons`). Focus is visible everywhere (the global `:focus-visible` ring). |

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

## Local Artifact Import Contract

Importing a supported local artifact (`Importer une histoire`) is the **inverse
of the export flow**: it brings a `.rustory` file from the computer into the
local library as a canonical, re-openable story. It is **distinct from the
`Device Story Import Contract`** (`Copier dans ma bibliothèque`, device → library)
— that flow either fully succeeds or explicitly fails, while this one produces a
typed recognition verdict that may be `Partiellement exploitable`. The supported
artifact set for this iteration is **the `.rustory` v1 artifact only** (see
[device-support-profile.md#Local Artifact Import Contract](./device-support-profile.md));
structured archives / multi-element folders are out of scope until the
node/media model exists.

The flow is **two-phase, with no mutation before acceptance (AC1)**:

| Phase | Command | Effect |
| --- | --- | --- |
| Analyze | `analyze_artifact_for_import` | Opens a native file picker (`.rustory`), reads the chosen file bounded, parses + validates every aspect, returns a typed recognition verdict DTO. NO row written, no file promoted. A cancelled dialog returns `{ kind: "cancelled" }` (never an error). A read/transport failure rejects with `IMPORT_FAILED`. |
| Accept | `accept_artifact_import` | On the explicit `Importer ce qui est reconnu` action: re-validates the canonical content from zero (never trusts the frontend), then commits one `stories` row + one `story_local_imports` row in a single `BEGIN IMMEDIATE` transaction. Returns the created `StoryCardDto`. |
| Abandon | — (pure frontend) | `Abandonner` drops the verdict. Nothing was mutated, so no command is needed. |

Recognition model (a typed verdict, NEVER an `AppError` — a partially usable or
functionally blocked artifact is a result state, not a transport error):

- **Recognition quality** (global): `Clean` → `Propre`, `Partial` →
  `Partiellement exploitable`, `Unusable` → `Inexploitable`.
- **Recognition aspect** (per finding): `Envelope`, `FormatVersion`,
  `SchemaVersion`, `Structure`, `Integrity`, `Title`, `Timestamps`.
- **Recognition category** (per finding): `Recognized` → `reconnu`, `Ambiguous`
  → `ambiguïté`, `Missing` → `information manquante`, `Blocking` → `blocage
  réel`. `Missing` (and the UX `dupliqué`) belong to the deferred multi-element
  import and are **declared but never emitted** by the `.rustory` flow — a
  negative test locks this (mirrors `Axis::Media` / `Axis::Filesystem` in the
  preflight contract).
- **Import state** (per story, durable, surfaced as a Story Card chip):
  `recognized` / `partial` / `needs_review` / `blocked` / `resolved`. `resolved`
  is **declared but never emitted** in this iteration (guided repair is later).

Recognition truth table for a `.rustory` artifact:

| Case | Quality | Import state | Result |
| --- | --- | --- | --- |
| Envelope valid, `formatVersion == 1`, canonical structure valid, recomputed checksum == declared, title normalizable and non-empty, canonical timestamps | `Propre` | `recognized` | Importable → canonical story, **no marker** (AC3) |
| Same, but the title had to be normalized (`original != normalize_title(original)`) OR a carried timestamp is not the expected ISO-8601 UTC ms shape | `Partiellement exploitable` | `needs_review` | Importable **with** a durable `Import Issue Marker` + on-demand report (AC2) |
| Malformed JSON / unknown field / `formatVersion != 1` / non-canonical structure / **checksum divergent** / title empty after normalization | `Inexploitable` | `blocked` | **Not importable** → clear error + abandon, **no mutation** (AC1) |

The only real `partial` trigger for a Rustory-exported `.rustory` is a
**hand-edited** title (Rustory always exports a normalized title and canonical
timestamps, and the title never enters the `content_checksum` digest — adjusting
it never diverges the checksum). Timestamps are **preserved** on the imported
row (fidelity of the AC3 "re-openable" story) — never silently rewritten to
`now`; a malformed carried timestamp is preserved AND flagged `needs_review`.
`story_local_imports.imported_at = now`.

UI state machine (owned by `useStoryImport`, surfaced in the library):

| State | Rendering | Announcement |
| --- | --- | --- |
| `idle` | no status content (the polite region stays mounted, empty) | none |
| `analyzing` | indeterminate `ProgressIndicator` labelled `Analyse de l'artefact…` (calm, neutral) | deliberately NOT announced |
| `review` | the recognition report in-context: the quality chip (`Propre` / `Partiellement exploitable` / `Inexploitable`), the per-aspect findings, and — when importable — `Importer ce qui est reconnu` THEN `Abandonner` in tab order. A `blocked` verdict is `role="alert"` (no accept button, only `Abandonner`); an importable verdict is `aria-live="polite"` | `role="alert"` if blocked, else `aria-live="polite"` |
| `importing` | indeterminate `ProgressIndicator` labelled `Import en cours…` | deliberately NOT announced |
| `imported` | `Histoire importée dans ta bibliothèque` (success chip) + the created local title + explicit `Fermer` dismiss; no auto-hide | `aria-live="polite"`, mounted, `aria-atomic` |
| `failed` | `Import impossible` block with the canonical `message` + `userAction`, buttons `Réessayer` THEN `Fermer` in tab order | `role="alert"` |

`Import Issue Marker` (durable, AC2): a story imported as `partial` /
`needs_review` carries a discreet chip on its library card (`partiel` / `à
revoir`), derived from `story_local_imports.import_state` exposed by
`read_stories` / `StoryCardDto` — so it **survives an app restart**. It is
distinct from (and must coexist with) the transfer/preparation `StoryPreparationBadge`,
whose `partial` means a verification verdict: this marker uses its own dedicated
labels/tone/glyph and never reuses the transfer `partial` value or sense.

`Import Review Flow` (on-demand, AC2): clicking the marker opens a simple
in-context report — the global outcome (`Ce que Rustory a reconnu`) + the
recognized aspects + the `Points d'attention`. The single source of truth is
`story_local_imports.findings_summary`. Never a toast / modal to carry a problem
alone; a guided repair level is deferred.

Invariants (locked by tests):

- **No mutation before acceptance (AC1)**: until `accept_artifact_import` runs,
  no `stories` / `story_local_imports` row exists. `Abandonner` and the
  `Unusable` / `blocked` case return to an unchanged library.
- **Canonical data never mutated by another import (FR18)**: an import creates a
  NEW story (fresh UUIDv7); it never touches an existing row.
- **No false success / honesty**: `recognized` is shown only when every aspect
  passes; a `partial` is named as such, never disguised as success.
- **Atomicity**: the commit is one transaction; any failure rolls back fully
  (never a half-imported story).
- Offline, zero dependency, zero network; color is never the sole carrier of
  meaning; a problem is never carried in a toast alone.

Error taxonomy — analyze/accept reject only on TRANSPORT failure, as `AppError {
code: "IMPORT_FAILED" }` with a stable `details.source` from this closed set
(the functional verdict is the typed DTO, never an error):

- `file_read` — reading the chosen file failed (unreadable, oversize beyond `MAX_ARTIFACT_BYTES`, a non-regular file, a non-filesystem path). `details.stage` ∈ `metadata`, `open`, `not_regular_file`, `oversize`, `read`, `non_filesystem_path`.
- `db_commit` — the final SQLite transaction failed; nothing is committed (atomic rollback). `details.stage` ∈ `begin_transaction`, `insert_story`, `insert_provenance`, `commit`; `details.kind` ∈ `busy`, `locked`, `constraint_violation`, `other`.
- `spawn_blocking_join` — the worker task could not be joined.
- `app_data_unavailable` — the managed local store has no resolvable home.
- `dialog_failed` — the native file dialog backend could not open.
- `other` — fallback for unmapped causes; `details.cause` names the specific reason (`revalidation`, `invalid_provenance` with `details.field`, `system_clock_invalid`).

Every refusal constructor carries a non-empty `message` (cause + impact) AND a
non-empty `userAction` (next gesture); the frontend renders both verbatim and
branches on `code` + `details.source` only to choose the surface, never to
compose the text.

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
