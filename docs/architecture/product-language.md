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
| Validation/preflight is running | `en vérification` |
| Action is prevented by a real blocking issue | `bloquée` |
| Validation says the story may be sent | `présumée transférable` |
| Preparation is running | `en préparation` |
| Write/send is running | `en transfert` |
| End result was explicitly confirmed | `transférée et vérifiée` |
| Failure can be retried safely | `échec récupérable` |
| Result is incomplete and not a success | `état partiel` |

Do not alternate freely between synonyms such as `sync`, `envoi`, `upload`, or `job`.
When a different wording is necessary in context, it must still map back to one of the preferred labels above.

## Copy Rules

- Prefer `Créer une histoire` over `Nouveau projet`.
- Prefer `Envoyer vers la Lunii` over `Synchroniser`, unless the product is explicitly comparing and reconciling states.
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
