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
| Pre-send comparison (panel section) | `Comparaison avant envoi` | Read-only comparison of the selected local story against the live device inventory, shown before any transfer | `diff`, `preview`, `dry-run` |
| Selected story is not yet on the device | `Nouvelle sur l'appareil` | Comparison verdict: a send would ADD this story to the device | `nouvelle` used loosely, `à transférer`, `manquante` |
| Selected story already lives on the device | `Déjà présente sur l'appareil` | Comparison verdict: a send would REPLACE the copy already on the device | `existe`, `doublon`, `déjà transférée` (asserts a past send) |
| Other device stories left untouched by a send | `resteront inchangées` | Reassurance that a transfer touches only the selected story, not the rest of the device library | `non affectées`, `safe`, technical counts without the verb |
| Preparation step | `préparation` | Pre-transfer work needed to make the story sendable | `pipeline`, `build`, `compile assets` |
| Start the preparation (action) | `Préparer` | User-visible action that starts assembling the artifacts a transfer would need, locally | `Lancer`, `Commencer`, `Build`, `Compiler` |
| Preparation in flight | `Préparation en cours…` | Calm feedback while the preparation is running | `job en cours`, `traitement`, `processing` |
| Story is prepared (indicator) | `Préparée` | Discreet marker that the artifacts were assembled and are fresh — never implies the transfer is enabled | `Prête`, `prête à transférer`, `prête à l'envoi` |
| Send operation | `transfert` / `Envoyer vers la Lunii` | User-visible act of sending a story to the device | `deploy`, `sync job`, `push` |
| Post-send confirmation | `vérification` | Explicit check that confirms what really happened on device | `post-check`, `validation finale` when it means something else |
| Write interrupted after the device was touched | `transfert incomplet` | The write STARTED then was interrupted; the device may hold a partial copy and a relaunch (full cycle) restores a safe state. Distinct from `état partiel` (a verification verdict) and from `Contenu incomplet` (a device pack with a missing payload) | `incomplete`, `partial`, `corrupt`, `staging` |
| Inspect the in-flight transfer detail (action) | `Consulter le détail` | Non-destructive disclosure of the running transfer's phase / progress, in-context (never a modal); distinct from any cancel (out of MVP scope) | `détails`, `debug`, `logs` |
| Abandon a failed / incomplete transfer (action) | `Abandonner` | Returns to a stable library after a failed / incomplete transfer; the local draft stays intact | `annuler`, `cancel`, `supprimer` |
| Supported local input/output | `artefact local supporté` | Project, archive, or local file explicitly supported by Rustory | `payload`, `package`, `blob` |
| Availability policy | `profil de support` | Official support statement for devices and local artifacts | `matrix` in user-facing copy |
| Import a local artifact (action) | `Importer une histoire` | User-visible act of bringing a supported local artifact (`.rustory`) from the computer into the local library; opens a native file picker | `Charger`, `Ouvrir`, `Copier` (reserved for the device flow) |
| Local artifact recognition — clean | `Propre` | Analysis verdict: every aspect of the artifact is recognized; it imports as a canonical story with no marker | `valide`, `ok`, `parfait` |
| Local artifact recognition — partially usable | `Partiellement exploitable` | Analysis verdict: the artifact imports, but one or more aspects need attention (a discreet marker + an on-demand report) | `partiel` used loosely, `dégradé`, `warning` |
| Local artifact recognition — unusable | `Inexploitable` | Analysis verdict: a real blocker prevents a safe import; nothing is added to the library | `corrompu`, `cassé`, `erreur` |
| Import report finding — recognized aspect | `reconnu` | Per-aspect report category: this aspect of the artifact is understood and accepted. Distinct from the per-story import-state chip below | `ok`, `valide` |
| Import report finding — ambiguity | `ambiguïté` | Per-aspect report category: the aspect is usable but had to be adjusted or could not be fully trusted (e.g. a normalized title) | `warning`, `bizarre` |
| Import report finding — missing information | `information manquante` | Per-aspect report category: an expected aspect is absent. Emitted by the structured-folder creation flow (a referenced media absent from the folder); never emitted by the `.rustory` flow | `vide`, `null`, `absent` |
| Import report finding — real blocker | `blocage réel` | Per-aspect report category: the aspect makes the artifact unusable as-is | `erreur`, `fatal` |
| Import state chip (Story Card) | `reconnu` / `partiel` / `à revoir` | Durable per-story import state surfaced as a discreet card chip, reserved for the file-provenance flows (`.rustory` import, structured-folder creation) — see [ui-states.md#Post-MVP Import State Contract](./ui-states.md), `#Local Artifact Import Contract` and `#Structured Folder Creation Contract`. Never reuse the transfer/verification `partiel` / `état partiel` sense | reusing the transfer `partial` / `état partiel` chip |
| Accept the recognized result of an import (action) | `Importer ce qui est reconnu` | User-visible act of committing the recognized story (with its points of attention) from an analyzed artifact; pairs with `Abandonner` | `valider`, `confirmer`, `OK` |
| Imported-artifact provenance (Story Card) | `Importée` | Discreet origin marker on a library card whose story came from a local artifact import — distinct from a native story and from a device copy | `import`, `external`, `fichier` |
| Import report — recognized facts header | `Ce que Rustory a reconnu` | On-demand import report header grouping the global outcome + the recognized aspects | `résumé`, `rapport`, `détails` |
| Import report — attention header | `Points d'attention` | On-demand import report header grouping the aspects to review | `warnings`, `problèmes`, `erreurs` |
| Structured input folder | `dossier structuré` | A local folder (`histoire.json` + referenced media) explicitly supported as a story-creation entry point; its format contract lives in [device-support-profile.md#Structured folder v1 format contract](./device-support-profile.md) | `dossier projet`, `archive`, `template` |
| Create from a structured folder (dialog secondary entry) | `Ou démarre depuis un dossier préparé hors de Rustory` | Secondary entry of the creation dialog introducing the structured-folder path; the interactive path (title → `Créer`) stays primary | a third bar CTA, `Importer un dossier` |
| Pick the structured folder (action) | `Choisir un dossier…` | Opens the native folder picker for the structured-folder creation | `Parcourir`, `Browse`, `Ouvrir` |
| Structured-folder creation surface (title) | `Création depuis un dossier` | In-context surface presenting the recognition report of an analyzed structured folder | `import`, `wizard`, `assistant` |
| Structured-folder report — what-will-be-created header | `Ce qui sera créé` | Report group naming exactly what an accepted folder will create: the (normalized) title, the node count, the retained media and the discarded ones by basename | `résumé`, `aperçu`, `preview` |
| Structured-folder report — created title line | `Titre : {titre}` | The normalized title the created story will carry | showing the raw pre-normalization value |
| Structured-folder report — node count line | `{n} nœud` / `{n} nœuds` | The number of nodes the created story will carry | `steps`, `écrans` |
| Structured-folder report — retained media line | `Médias retenus : {basenames}` | The referenced media files that WILL be wired into the story (comma-separated basenames; the line is absent when none) | `assets`, absolute paths |
| Structured-folder report — discarded media line | `Médias écartés : {basenames}` | The referenced media files that will NOT be wired (absent or unusable — the empty slots stay repairable in the editor); absent when none | `rejetés`, `erreurs`, absolute paths |
| Accept the structured-folder creation (action) | `Créer l'histoire` | Unique CTA committing the analyzed folder into a canonical story — the report already says what will be discarded (no second CTA); pairs with `Abandonner` | `Importer` (reserved for `.rustory`), `Valider`, dual-CTA variants |
| Story editing screen | `Éditeur d'histoire` | Dedicated screen, separate from the library, where the user resumes and edits a local story | `workspace`, `projet`, `canvas`, `editor` |
| Editor zone — global structure | `Structure de l'histoire` | Editor zone showing the story's overall layout (ordered node list, start node, option links) and the current node, clearly identified; projected from the core, with explicit per-node actions on a full-scope story (native or `.rustory` import) | `arbre`, `outline`, `tree`, `plan`, `canvas` |
| Editor zone — current node | `Nœud courant` | Editor zone hosting the editor for the node currently in focus (its text, metadata and media) | `current node`, `panneau`, `étape courante` |
| Story node | `nœud` | A single step of an interactive story (a narrative moment and its choices) | `node`, `step`, `écran` |
| Node narrative text (field) | `Texte du nœud` | The narrative text of the current node | `contenu`, `body`, `script` |
| Node metadata label (field) | `Libellé du nœud` | The short human-readable name of the current node | `nom technique`, `id`, `tag` |
| Node media — image | `Image` | The image associated with the current node | `visuel`, `asset`, `media image` |
| Node media — audio | `Audio` | The audio associated with the current node | `son` used loosely, `track`, `media audio` |
| Add a node media (action) | `Ajouter` | Associate an image or audio file with the current node | `Importer` (reserved for `.rustory`), `Charger`, `Upload` |
| Replace a node media (action) | `Remplacer` | Swap the associated image or audio for another file | `Changer`, `Mettre à jour`, `Re-upload` |
| Remove a node media (action) | `Retirer` | Drop the associated image or audio from the current node | `Supprimer`, `Effacer`, `Delete` |
| Preview a node media (action) | `Aperçu` | Show the associated image, or play the associated audio, for review | `voir`, `lire`, `play`, `preview` |
| Node media is absent (optional, expected) | `Aucune image` / `Aucun audio` | Named empty state for an unset, optional media slot — not an error | `vide`, `null`, `pas de média` |
| Node media needs attention (repairable) | `Média à corriger` | The media is associated but its source is no longer accessible; the rest of the node stays editable | `warning`, `cassé`, `erreur média` |
| Node media is blocked (real block) | `Média bloqué` | The chosen file is an unsupported format, unreadable, or too large; it is not saved until corrected | `erreur`, `fatal`, `rejeté` |
| Supported node media formats | `Formats acceptés : images PNG, JPEG ; sons MP3, WAV, OGG` | The closed set of source media formats the editor accepts | listing extensions as the gate, `codecs`, `mime` |
| Node option (choice) | `option` | A choice a node offers to the listener; each option may point at a destination node | `choice`, `branch`, `transition`, `lien` used alone |
| Option destination | `destination` | The node an option points at | `target`, `cible technique`, `pointeur` |
| Start node | `Nœud de départ` | The node where the story begins; marked `Départ` in the structure zone | `racine`, `entry point`, `node 1` |
| Start node mark (structure entry) | `Départ` | Textual mark on the start node in the structure list | color-only or icon-only start marks |
| Add a node (action) | `Ajouter un nœud` | Append a new empty node to the story's structure | `Créer un nœud`, `Nouveau`, `+` alone |
| Move a node up / down (actions) | `Monter` / `Descendre` | Swap a node with its neighbor in the displayed order | `Déplacer` used loosely, drag-and-drop wording |
| Delete a node (action, first gesture) | `Supprimer le nœud` | Start removing a node; always followed by an explicit confirmation | `Effacer`, `Retirer` (reserved for options/media) |
| Confirm a node deletion (second gesture) | `Confirmer la suppression` | Explicit second gesture that actually removes the node | `OK`, `Oui`, single-gesture deletes |
| Add an option (action) | `Ajouter une option` | Add a new choice to the current node, with its label typed at creation | `Nouvelle option`, `+` alone |
| Link an option (action) | `Lier` | Point an option at an existing node, chosen in a flat selector | `connecter`, `pointer`, `attacher` |
| Create and link a new node (action) | `Créer et lier un nouveau nœud` | Create an empty node and link the option to it, in one atomic action | `lier vers nouveau`, two-step wordings |
| Unlink an option (action) | `Délier` | Remove an option's destination; the option becomes `non liée` | `déconnecter`, `détacher` |
| Remove an option (action) | `Retirer l'option` | Drop the option from its node | `Supprimer` (reserved for nodes), `Effacer` |
| Option has no destination yet (state) | `non liée` | Normal authoring state: the option exists but points nowhere yet | `vide`, `incomplète`, `en attente` |
| Option destination is present (state) | `liée` | The option points at an existing node, named next to the state | `ok`, `valide`, `connectée` |
| Option destination must be repaired (state) | `destination à corriger` | The option points at a node that no longer exists; repairable in place | `lien cassé`, `broken`, `invalide`, `erreur` |
| Device-pack content zone (editor, named state) | `Contenu porté par le pack de l'appareil` | Named state of the current-node zone for a device-pack story: the content lives in the binary pack copied from the device; no field, media or option control is rendered (absent, not disabled) | `lecture seule`, `read-only`, `verrouillée`, `Histoire importée (lecture seule)` |
| Device-pack structure zone (editor, named state) | `Structure portée par le pack de l'appareil` | Named state of the structure zone for a device-pack story — shown INSTEAD of the node navigator (the local placeholder graph would be a lying projection of the pack) | `Structure illisible` (reserved for a corrupt canonical), `lecture seule` |
| Editor review chip (imported story, review pending) | `à revoir` / `partiel` | The Story Card import-state labels reused as a STATIC chip in the editor shell banner while the `.rustory` import review is pending; nothing is rendered once settled | new ad-hoc labels, alert announcements |
| Import review settled (durable `resolved` state) | — (no label rendered) | A `resolved` story renders NO chip and NO report anywhere — the marker's disappearance IS the feedback; the provenance marker `Importée` stays | `résolue`, `corrigée`, any success announcement |

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
| A fixable blocking issue was detected (repairable before send) | `à corriger` |
| Validation says the story may be sent | `présumée transférable` |
| Preparation is running | `en préparation` |
| Start the preparation (action) | `Préparer` |
| Preparation is in flight | `Préparation en cours…` |
| Preparation finished and artifacts are fresh | `Préparée` |
| Write/send is running | `en transfert` |
| Write done, the verification re-read is running (TRANSIENT, not a resting terminal) | `écriture effectuée — vérification à venir` |
| Verification confirmed the result — present, indexed and byte-faithful. The sober success terminal, never shown before the verification proves it | `transférée et vérifiée` |
| Failure can be retried safely (device left untouched) | `échec récupérable` |
| Write started then interrupted; the device may hold a partial copy and a relaunch restores a safe state | `transfert incomplet` |
| Verification re-read the device and confirmed only an incoherent/incomplete result — neither a success nor necessarily a failure. Distinct from `transfert incomplet` (a write interruption), `échec récupérable` (device untouched) and `Contenu incomplet` (a device pack with a missing payload) | `état partiel` |
| Inspect the in-flight transfer detail (secondary, non-destructive) | `Consulter le détail` |
| Relaunch a failed / incomplete transfer from the preserved local draft — a full fresh cycle (preflight → prepare → transfer → verify), never a hidden partial resume (action). Avoid `retry`, `recommencer`, `reprendre` (`reprendre` is reserved for editing) | `Relancer` |
| Abandon a failed / incomplete transfer (local draft intact) | `Abandonner` |
| Relaunch is gated — no writable device is connected | `Rebranche la Lunii pour relancer.` |
| No device is connected | `Aucun appareil connecté` |
| Supported device detected (at least one activated capability) | `Appareil prêt — {famille} {cohort}` |
| Supported device recognized with ZERO activated capability (static durable state, never an alert) | `Appareil reconnu — {famille}` |
| Second supported family name (uppercase, invariant) | `FLAM` |
| Recognized-without-capability explanation (TEXT-ONLY support-profile pointer — no navigation, no network; rendered in that idle state only) | `Appareil reconnu, aucune opération activée dans cette version. Consulte le profil de support pour comprendre ce qui est permis.` |
| Non-activated capability line (per-operation rendering) | `— {libellé de l'opération}` |
| Transfer capability line label for a non-Lunii family (Lunii keeps `Transfert vers la Lunii`) | `Transfert vers l'appareil` |
| Detected device but profile not allow-listed | `Profil non supporté` |
| Multiple supported devices detected at once | `Profil ambigu — plusieurs candidats détectés` |
| Device scan transport itself failed | `Détection indisponible` |
| Reading the device-side library | `Lecture de la bibliothèque de l'appareil…` |
| Connected device holds no readable story | `Aucune histoire sur l'appareil` |
| Reading the device-side library failed | `Bibliothèque de l'appareil indisponible` |
| Device story selected for inspection before copy | `Histoire sélectionnée` |
| Pre-send comparison is in flight | `Comparaison en cours…` |
| Selected local story is absent from the device | `Nouvelle sur l'appareil` |
| Selected local story already on the device | `Déjà présente sur l'appareil` |
| A send would replace the on-device copy | `Déjà présente sur l'appareil — un envoi la remplacerait.` |
| A send would add the story to the device | `Cette histoire serait ajoutée à l'appareil.` |
| No comparison — no local story selected | `Sélectionne une histoire locale pour comparer avant l'envoi.` |
| No comparison — more than one story selected | `Sélectionne une seule histoire locale pour comparer (le transfert multiple n'est pas encore disponible).` |
| No comparison — no readable device connected | `Branche une Lunii lisible pour comparer l'histoire sélectionnée avant l'envoi.` |
| Device changed while comparing (recoverable) | `L'appareil a changé pendant la comparaison.` |
| Copy not allowed for this profile | `Copie indisponible: profil non supporté` |
| Copy refused — local copy already exists | `Copie indisponible: déjà dans ta bibliothèque` |
| Copy refused — pack payload missing on device | `Copie indisponible: contenu incomplet sur l'appareil` |
| Device copy in flight | `Copie en cours…` |
| Device copy just succeeded | `Histoire copiée dans ta bibliothèque` |
| Device copy failed and user can retry | `Copie impossible` |
| Device story with a local copy | `Dans ta bibliothèque` |
| Device story whose payload folder is present | `Contenu présent` |
| Device story not covered by any local index | `Histoire non reconnue` |
| Recognized title from the official commercial catalog | `Titre officiel` |
| Recognized title inferred from the local library / community | `Titre non-officiel` |
| Title the user typed for a device story | `Titre saisi` |
| Name an unrecognized device story | `Nommer cette histoire` |
| Edit a name the user typed earlier | `Renommer cette histoire` |
| Official-catalog cache management area | `Catalogue officiel` |
| Fetch / refresh the official catalog (explicit, networked) | `Récupérer / mettre à jour` |
| Import the official catalog from a local file (offline) | `Importer depuis un fichier` |
| Official-catalog action failed (chip header; the actionable text is the alert's `message` + `userAction`) | `Catalogue indisponible` |
| Import a local artifact (action) | `Importer une histoire` |
| Local artifact analysis in flight | `Analyse de l'artefact…` |
| Artifact recognized — clean | `Propre` |
| Artifact partially usable | `Partiellement exploitable` |
| Artifact unusable | `Inexploitable` |
| Accept the recognized import (action) | `Importer ce qui est reconnu` |
| Abandon an analyzed import (no mutation) | `Abandonner` |
| Import commit in flight | `Import en cours…` |
| Import just succeeded | `Histoire importée dans ta bibliothèque` |
| Import failed (transport) and user can retry | `Import impossible` |
| Imported story origin marker (Story Card) | `Importée` |
| Open the durable on-demand import report (Story Card) | `Voir le rapport d'import` |
| Start a story from a structured folder (dialog secondary entry) | `Ou démarre depuis un dossier préparé hors de Rustory` |
| Pick the structured folder (action) | `Choisir un dossier…` |
| Structured-folder creation surface (title) | `Création depuis un dossier` |
| Structured-folder analysis in flight | `Analyse du dossier…` |
| Accept the structured-folder creation (action) | `Créer l'histoire` |
| Abandon an analyzed structured folder (no mutation) | `Abandonner` |
| Structured-folder creation commit in flight | `Création en cours…` |
| Structured-folder creation just succeeded | `Histoire créée dans ta bibliothèque` |
| Structured-folder creation failed (transport; the actionable text is the alert's `message` + `userAction`) | `Création impossible` |
| Structured-folder refused — the folder's NAME cannot be carried as provenance (cause) | `Création impossible: le nom du dossier choisi ne peut pas être utilisé par Rustory.` |
| Structured-folder refused — the folder's NAME cannot be carried as provenance (next gesture) | `Renomme le dossier (nom plus court, sans caractère spécial) puis relance l'analyse.` |
| Story editing screen (separate from the library) | `Éditeur d'histoire` |
| Editor zone showing the global structure | `Structure de l'histoire` |
| Editor zone hosting the current node | `Nœud courant` |
| Structure could not be read (degraded; near-impossible) | `Structure illisible.` |
| Node narrative text field (label) | `Texte du nœud` |
| Node narrative text is empty (placeholder, valid starting state) | `Écris le texte de ce nœud…` |
| Node metadata label field (label) | `Libellé du nœud` |
| Node image slot (label) | `Image` |
| Node audio slot (label) | `Audio` |
| Optional node image is unset (named empty state, not an error) | `Aucune image` |
| Optional node audio is unset (named empty state, not an error) | `Aucun audio` |
| A node media is present and ready (chip, with a humanized size) | `Média ajouté · {taille}` |
| A node media needs attention (repairable; rest of the node still saves) | `Média à corriger` |
| A node media is blocked (unsupported / unreadable / oversize; not saved) | `Média bloqué` |
| Device-pack story — current-node zone named state | `Contenu porté par le pack de l'appareil` |
| Device-pack story — current-node zone explanation | `Le texte, les médias et les options de cette histoire vivent dans le pack copié depuis l'appareil. Tu peux modifier le titre depuis l'éditeur.` |
| Device-pack story — structure zone named state | `Structure portée par le pack de l'appareil` |
| Content write refused on a device-pack story (message) | `Le contenu de cette histoire est porté par le pack copié depuis l'appareil et ne peut pas être modifié ici.` |
| Content write refused on a device-pack story (next gesture) | `Tu peux modifier le titre depuis l'éditeur ; le contenu du pack reste celui de l'appareil.` |
| Imported story with a pending review (editor banner chip) | `à revoir` / `partiel` (the Story Card labels, static) |
| Import review settled (`resolved`) | no label rendered — the chip disappears; `Importée` stays |
| Start node mark in the structure list | `Départ` |
| Append a new empty node to the structure (action) | `Ajouter un nœud` |
| Swap a node with its neighbor (actions) | `Monter` / `Descendre` |
| Start removing a node (action, first gesture) | `Supprimer le nœud` |
| Actually remove the node (action, second gesture) | `Confirmer la suppression` |
| Add a choice to the current node (action) | `Ajouter une option` |
| Point an option at an existing node (action) | `Lier` |
| Create an empty node and link the option to it (action, atomic) | `Créer et lier un nouveau nœud` |
| Remove an option's destination (action) | `Délier` |
| Drop an option from its node (action) | `Retirer l'option` |
| Option without a destination (normal authoring state) | `non liée` |
| Option pointing at an existing node | `liée` |
| Option pointing at a node that no longer exists (repairable) | `destination à corriger` |

Do not alternate freely between synonyms such as `sync`, `envoi`, `upload`, or `job`.
When a different wording is necessary in context, it must still map back to one of the preferred labels above.

### Structured-folder recognition copy (per-pair, frozen)

One canonical FR message per `(aspect, catégorie)` pair of the structured-folder
matrix (see [device-support-profile.md#Structured folder v1 format contract](./device-support-profile.md)).
The UI branches on the discriminants, never on this text; the folder flow owns
the wording of EVERY `Envelope`, `FormatVersion`, `Title` and `Structure` pair
of its matrix (they speak of a manifest and a creation — the `.rustory` copy
keeps speaking of an artifact and an import), except the shared
`Title × reconnu` line whose copy is identical; the `Media` pairs exist only in
this flow. Every `blocage réel` copy names the corrective gesture (fix the
folder/manifest, re-run the analysis). Quality labels are reused verbatim
(`Propre` / `Partiellement exploitable` / `Inexploitable`), as are the report
headers and the per-category chips (`reconnu` / `ambiguïté` /
`information manquante` / `blocage réel`).

| Aspect | Catégorie | Message figé |
| --- | --- | --- |
| Envelope | reconnu | `Le manifest histoire.json est présent et lisible.` |
| Envelope | blocage réel | `Le dossier ne contient pas de manifest histoire.json lisible. Corrige le dossier puis relance l'analyse.` |
| FormatVersion | reconnu | `La version de format du manifest est prise en charge.` |
| FormatVersion | blocage réel | `La version de format de ce manifest n'est pas prise en charge par cette version de Rustory. Corrige le manifest puis relance l'analyse.` |
| Title | reconnu | `Le titre de l'histoire est valide.` |
| Title | ambiguïté | `Le titre a été normalisé à la création (espaces ou caractères ajustés).` |
| Title | blocage réel | `Le titre du manifest est manquant ou n'est pas valide. Corrige le manifest puis relance l'analyse.` |
| Structure | reconnu | `La structure de l'histoire est reconnue.` |
| Structure | ambiguïté | `La structure contient un champ inattendu ou un lien d'option vers un nœud inconnu ; l'histoire sera créée telle quelle et tu pourras corriger dans l'éditeur.` |
| Structure | blocage réel | `La structure du manifest est incomplète ou incohérente. Corrige le manifest puis relance l'analyse.` |
| Media | reconnu | `Tous les fichiers audio et image référencés par le dossier sont présents et reconnus.` |
| Media | ambiguïté | `Certains fichiers audio ou image référencés ne sont pas utilisables (format non reconnu, fichier trop volumineux ou nom invalide). L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.` |
| Media | information manquante | `Certains fichiers audio ou image référencés par le dossier sont introuvables. L'histoire sera créée sans eux ; tu pourras les ajouter dans l'éditeur.` |

No new card provenance label (the existing `Importée` marker is honest — the
content comes from outside Rustory) and no label for a settled review
(`resolved` renders nothing — the marker's disappearance IS the feedback).

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
- `write` / `staging` when `écriture` / `Envoyer vers la Lunii` is meant
- `artifact` when `artefact local supporté` is more precise
- `state machine`

## Technical Mapping Rule

- Code and IPC contracts may use stable internal identifiers such as `jobId`, `transferring`, `write_story`, `verified`, `retryable`, `incomplete`, `completeness`, `progress`, `stage`, or `reached_device_mutation`.
- User-facing UI must map those identifiers to the preferred French labels defined here and in [ui-states.md](./ui-states.md).
- Logs may keep technical codes, but any surfaced message must still respect this glossary.

## Change Control

Before introducing a new user-visible term:

1. Check whether an existing canonical term already covers it.
2. If not, add the term here and the corresponding state behavior in `ui-states.md` if applicable.
3. Update UX, architecture, and affected stories so the same wording is reused everywhere.
