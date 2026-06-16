# Product Language

## Purpose

This document is the authoritative glossary for Rustory's user-facing language.

It exists to keep the same product concepts named the same way across:
- UI copy
- UX specifications
- architecture documents
- logs and diagnostics when they surface user-visible messages
- tests and acceptance criteria

## Language Principles

- Prefer parent-friendly language over internal jargon.
- One stable product concept must map to one preferred label.
- Use French product terms in user-facing copy.
- Keep internal code names and wire formats separate from displayed wording.
- When a term becomes visible in more than one screen, it must be defined here first.

## Canonical Terms

| Concept | Preferred Term | Definition | Avoid in UI |
| --- | --- | --- | --- |
| Main working area | `bibliothèque` / `bibliothèque locale` | Durable local home where the user finds and resumes stories | `dashboard`, `workspace`, `home screen` |
| Story unit | `histoire` | Main content unit manipulated by the user | `story project`, `content item` |
| Local editable state | `brouillon local` | Canonical local work state kept on the computer | `workspace local`, `draft file` |
| Connected hardware target | `appareil` | Connected storytelling device in generic copy | `device target`, `mount point` |
| Specific supported family | `Lunii` | Explicit device family name when the distinction matters | generic aliases that hide the family |
| Validated target scope | `profil d’appareil validé` | Supported family + firmware + authorized operations | `compatible target` used loosely |
| Detected device profile | `profil détecté` | Stable description of family + firmware cohort + authorized operations | `device profile`, `compat info` |
| Unsupported reason | `raison de non-support` | Standardized cause closed-set surfaced in the panel | `error`, `failure` (when used loosely) |
| Device candidate | `appareil candidat` | A volume that may be a Lunii but is not yet classified | `mount`, `partition`, `drive` |
| Refresh detection | `Réessayer la détection` | User-triggered re-scan from the library decision panel | `Refresh`, `Reload`, `Sync` |
| Device-side library | `bibliothèque de l'appareil` / `histoires sur l'appareil` | The stories currently installed on the connected device, read live | `device library`, `remote library` |
| Device-resident story without verified title | `histoire non reconnue` | A device story Rustory can list by its opaque identifier but cannot name without a catalog | `unknown story`, `untitled` |
| Provenance marker (device) | `Sur l'appareil` | Marks an item as living on the device, distinct from the local library | `remote`, `external`, `cloud` |
| Hidden device pack | `Masquée` | A device story listed as hidden by the user | `hidden`, `archived` |
| Incomplete device pack | `Contenu incomplet` | A listed device story whose payload folder is missing/ambiguous | `corrupt`, `broken`, `orphan` |
| Device story under inspection | `Histoire sélectionnée` | Heading of the right-column inspector for the device story being consulted before import | `selected item`, `current pack` |
| Device story provenance note | `Cette histoire vit sur l'appareil, pas encore dans ta bibliothèque locale` | Reminds the user that a consulted device story is not yet a local draft | wording implying it is already imported |
| Device story provenance note (copy exists) | `Cette histoire vit sur l'appareil et une copie existe déjà dans ta bibliothèque locale` | Variant of the provenance note once a local copy exists (`alreadyImported`) | `pas encore` wording on an already-copied story |
| Copy a device story into the library (action) | `Copier dans ma bibliothèque` | User-visible act of bringing a device story from the connected device into the local library | `importer` (reserved for file artifacts), `download`, `sync` |
| Device story already copied locally | `Dans ta bibliothèque` | Marker on a device story card: a local copy of this device story exists (provenance link present) | `imported`, `synced`, `duplicate` |
| Default title of a copied device story | `Histoire de ma Lunii (XXXXXXXX)` | Title given to the local draft created by a device copy — `XXXXXXXX` is the opaque short identifier; renamable immediately in the editor | titles asserting unverified content (`Histoire non reconnue` is a device-side state, never a local title) |
| Complete device pack | `Contenu présent` | A listed device story whose payload folder is present on the device — a verified fact about the folder, never a claim about content quality | `valid`, `complete`, `ok`, asserting content quality |
| Recognized device-story facts (inspector group) | `Ce que Rustory reconnaît` | Inspector header grouping the verified facts Rustory can vouch for before a copy (identifiers, content present) | `infos`, `metadata`, anything implying a recognized title |
| Copy-blocking device-story facts (inspector group) | `Ce qui bloque la copie` | Inspector header grouping the verified facts that prevent a copy (incomplete content, copy already exists) | `erreurs`, `problèmes`, `blockers` |
| Device-story facts to review (inspector group) | `À revoir avant de copier` | Inspector header grouping the verified facts to weigh before copying (hidden). Distinct from the Post-MVP `à revoir` import-state chip | `warnings`, `alertes`, reusing the Post-MVP `à revoir` chip |
| Consult the official support profile (action) | `Consulter le profil de support` | Affordance opening the official device-support profile; shown on the detection panel and, on a profile-based copy refusal, in the device-story inspector | `aide`, `docs`, `support`, `FAQ` |
| Preparation step | `préparation` | Pre-transfer work needed to make the story sendable | `pipeline`, `build`, `compile assets` |
| Send operation | `transfert` / `Envoyer vers la Lunii` | User-visible act of sending a story to the device | `deploy`, `sync job`, `push` |
| Post-send confirmation | `vérification` | Explicit check that confirms what really happened on device | `post-check`, `validation finale` when it means something else |
| Supported local input/output | `artefact local supporté` | Project, archive, or local file explicitly supported by Rustory | `payload`, `package`, `blob` |
| Availability policy | `profil de support` | Official support statement for devices and local artifacts | `matrix` in user-facing copy |

## Preferred State Labels

The UI should favor these labels when they are user-visible:

| Meaning | Preferred Label |
| --- | --- |
| User can keep editing locally | `brouillon local` |
| Autosave in flight | `Enregistrement…` |
| Autosave just succeeded | `Enregistré` |
| Autosave failed and user must retry | `Enregistrement en échec` |
| Export in flight | `Exportation en cours…` |
| Export just succeeded | `Exporté` |
| Export failed and user must choose another path | `Exportation échouée` |
| Recovery draft available for the current story | `Brouillon récupéré` |
| Restoring the recovered draft | `Restauration en cours…` |
| Discarding the recovered draft | `Conserver l'état enregistré` |
| Recovery itself failed (read/write/lock) | `Récupération indisponible` |
| Recovery banner — what the user had typed before the interruption | `Tu avais tapé` |
| Recovery banner — last value committed to disk | `Dernier état enregistré` |
| Validation/preflight is running | `en vérification` |
| Action is prevented by a real blocking issue | `bloquée` |
| Validation says the story may be sent | `présumée transférable` |
| Preparation is running | `en préparation` |
| Write/send is running | `en transfert` |
| End result was explicitly confirmed | `transférée et vérifiée` |
| Failure can be retried safely | `échec récupérable` |
| Result is incomplete and not a success | `état partiel` |
| No device is connected | `Aucun appareil connecté` |
| Supported device detected | `Appareil prêt — {famille} {cohort}` |
| Detected device but profile not allow-listed | `Profil non supporté` |
| Multiple supported devices detected at once | `Profil ambigu — plusieurs candidats détectés` |
| Device scan transport itself failed | `Détection indisponible` |
| Reading the device-side library | `Lecture de la bibliothèque de l'appareil…` |
| Connected device holds no readable story | `Aucune histoire sur l'appareil` |
| Reading the device-side library failed | `Bibliothèque de l'appareil indisponible` |
| Device story selected for inspection before copy | `Histoire sélectionnée` |
| Copy not allowed for this profile | `Copie indisponible: profil non supporté` |
| Copy refused — local copy already exists | `Copie indisponible: déjà dans ta bibliothèque` |
| Copy refused — pack payload missing on device | `Copie indisponible: contenu incomplet sur l'appareil` |
| Device copy in flight | `Copie en cours…` |
| Device copy just succeeded | `Histoire copiée dans ta bibliothèque` |
| Device copy failed and user can retry | `Copie impossible` |
| Device story with a local copy | `Dans ta bibliothèque` |
| Device story whose payload folder is present | `Contenu présent` |

Do not alternate freely between synonyms such as `sync`, `envoi`, `upload`, or `job`.
When a different wording is necessary in context, it must still map back to one of the preferred labels above.

## Copy Rules

- Prefer `Créer une histoire` over `Nouveau projet`.
- Prefer `Envoyer vers la Lunii` over `Synchroniser`, unless the product is explicitly comparing and reconciling states.
- Prefer `Copier dans ma bibliothèque` (from a device) over `Importer`; reserve `Importer` / `Exporter` for local file artifacts (`.rustory`, archives). The device pair is `Envoyer vers la Lunii` (library → device) / `Copier dans ma bibliothèque` (device → library).
- Prefer `Reprendre` over `Restaurer la session` when the user continues a local story.
- Prefer `Bloquée` with a short cause over a generic `Erreur`.
- Prefer sober confirmations such as `Transférée et vérifiée` over celebratory success copy.

## Terms to Avoid in User-Facing UI

These terms may exist in code or technical documentation, but should not be primary UI language:

- `workspace`
- `pipeline`
- `build`
- `job`
- `payload`
- `mount`
- `artifact` when `artefact local supporté` is more precise
- `state machine`

## Technical Mapping Rule

- Code and IPC contracts may use stable internal identifiers such as `jobId`, `verified`, or `retryable`.
- User-facing UI must map those identifiers to the preferred French labels defined here and in [ui-states.md](./ui-states.md).
- Logs may keep technical codes, but any surfaced message must still respect this glossary.

## Change Control

Before introducing a new user-visible term:

1. Check whether an existing canonical term already covers it.
2. If not, add the term here and the corresponding state behavior in `ui-states.md` if applicable.
3. Update UX, architecture, and affected stories so the same wording is reused everywhere.
