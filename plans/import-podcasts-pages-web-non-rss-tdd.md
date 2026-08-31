# Plan TDD — import-podcasts-pages-web-non-rss

- État: `PRÊT`
- Contrat BDD: `708fc15c2c4aaf6edd56c82ae70e224d56619ad998d14c374768932b7825043c`
- Commit sélectionné: `b868a51704ae1db424f3a7ae5891db181fce1285`
- Feature: `features/import-podcasts-pages-web-non-rss.feature`
- Procédure QA: `stories/import-podcasts-pages-web-non-rss.md`

## Carte de couverture

- [x] Scénario: Importer une page web publique non-RSS → TDD-3, TDD-4, TDD-6, TDD-7
- [x] Exemple E1 de « Importer une page web publique non-RSS »: | https://www.radiofrance.fr/radiofrance/podcasts/selection-pour-partir-a-l-aventure | → TDD-6, TDD-7
- [x] Exemple E2 de « Importer une page web publique non-RSS »: | https://www.radiofrance.fr/franceinter/podcasts/serie-tina-et-le-serpent-a-plumes | → TDD-6, TDD-7
- [x] Scénario: Créer une histoire complète avec tous les épisodes de la page → TDD-6, TDD-7
- [x] Scénario: Importer un épisode sans image → TDD-4, TDD-6, TDD-7
- [x] Scénario: Refuser une URL mal formée → TDD-1, TDD-7
- [x] Scénario: Signaler une page inaccessible → TDD-2, TDD-7
- [x] Scénario: Signaler une page sans épisode audio → TDD-5, TDD-7
- [x] Scénario: Continuer d'importer un flux RSS → TDD-3

## Tranches TDD

### TDD-1 — Refuser une URL mal formée avant toute requête réseau
- Couvre: S4 « Refuser une URL mal formée » — adresse non http(s) valide → aucune requête réseau, raison de l'échec indiquée, aucune histoire créée.
- RED: Les deux premiers placeholders vides de `fetch_web_podcast_preview` (`src-tauri/src/commands/import_export.rs`, corps `assert!(true)`) deviennent des tests réels sans réseau : URL vide et schéma non http(s) (ex. `pas-une-url`) → la commande renvoie l'échec motivé existant `invalid_web_url_error()` ; une URL http(s) mal formée (ex. `http://`) échoue aussi à la validation, là où le check par préfixe actuel laisserait passer une sortie réseau.
- GREEN: Remplacer le check par préfixe `starts_with("http://")` de `preview_web_podcast` (et de l'accept à venir) par une validation stricte — schéma http/https + URL et hôte parsables, en réutilisant l'analyse déjà faite par `feed_url_host` — avec échec motivé renvoyé avant toute sortie réseau ; aucune histoire n'est créée.
- REFACTOR: Restreindre la validation à la garde d'entrée du parcours web, sans modifier le comportement RSS existant.
- Dépendances: aucune (fondation du parcours web).
- QA: Étape 7 de la procédure (S4) ; critères P5 / F4 : échec sans récupération en cours, raison affichée, aucune requête sortante, bibliothèque inchangée.
- Mutation d’acceptation: S4 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: `src-tauri/src/application/import_export/web_episode_extraction.rs` et son point d'appel IPC.

### TDD-2 — Signaler une page inaccessible avec sa raison
- Couvre: S5 « Signaler une page inaccessible » — URL valide mais page injoignable ou erreur HTTP → raison de l'échec indiquée, aucune histoire créée.
- RED: Le test fragile `test_fetch_html_rejects_non_200_status` (httpstat.us) et le placeholder « propagates_network_error » de la commande deviennent des tests locaux déterministes : domaine réservé RFC 2606 (`.invalid`) → échec « injoignable » motivé ; petit serveur HTTP local répondant 500 → échec « erreur HTTP » motivé ; aucun des deux ne crée d'histoire.
- GREEN: `fetch_html` propage déjà l'échec de transport et le statut d'erreur HTTP vers un échec motivé ; garantir que les deux cas produisent une raison distincte (injoignable / statut HTTP) observable par l'utilisateur, sans créer d'histoire.
- REFACTOR: Uniformiser la représentation des échecs motivés du parcours web (une variante d'erreur par cas), sans changer les cas d'acceptation.
- Dépendances: TDD-1 (garde d'entrée d'abord).
- QA: Étape 8 de la procédure (S5) ; critères P6 / F5 : variante injoignable (domaine réservé `.invalid`) et erreur HTTP 500 (page locale).
- Mutation d’acceptation: S5 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: le fetch de `src-tauri/src/application/import_export/web_episode_extraction.rs` et la suppression du test httpstat.us fragile.

### TDD-3 — Reconnaitre une source web non-RSS et préserver le chemin RSS
- Couvre: S1 (la source est reconnue comme une page web non-RSS) et S7 (un flux RSS valide est toujours reconnu RSS et importé par le comportement existant, sans changement).
- RED: Test échouant : une page HTML publique de fixture est prévisualisée comme source web non-RSS (hôte de la page dans l'aperçu) ; un flux RSS valide de fixture reste reconnu comme flux RSS et importé par le chemin existant inchangé (régression, sans réseau).
- GREEN: Le parcours web prévisualise la page comme source web non-RSS (hôte affiché dans l'aperçu) ; le chemin RSS (`rss_creation.rs`, commande `accept_rss_story_creation`) est laissé tel quel et couvert par la régression S7.
- REFACTOR: Factoriser la décision de routage de la source sans élargir le périmètre du parcours.
- Dépendances: TDD-2 (fetch motivé déjà en place).
- QA: Étape 2 (source reconnue dans l'aperçu E1/E2) et étape 10 (flux RSS public de référence) ; critères P1/P2 (reconnaissance), P8 / F7.
- Mutation d’acceptation: S1 (reconnaissance) et S7 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: le point de routage de la source dans `src-tauri/src/application/import_export/` et `web_episode_extraction.rs`.

### TDD-4 — Extraire les épisodes de la page en ordre (titre, audio, image optionnelle)
- Couvre: S1 (au moins un épisode identifié, titre non vide, média audio, absence d'image tolérée) et S3 (épisode sans image importé sans erreur, champ image vide).
- RED: Tests échouant sur fixtures HTML locales : les épisodes d'une page de fixture (modèle des pages d'exemple, capturé à l'implémentation) sont extraits avec titre non vide + URL de média audio + image optionnelle, dans l'ordre d'apparition ; fixture d'un épisode sans image → `image_url` vide, sans erreur ; plus de titre de repli « Épisode sans titre ».
- GREEN: `parse_web_episodes` n'émet que des épisodes honnêtes : titre réel non vide, `audio_url` issu du média réellement présent dans le bloc d'épisode (éléments et liens audio des pages d'exemple), `image_url` seulement si la page la fournit, ordre du document conservé.
- REFACTOR: Supprimer les branches de selectors inutiles (repli `article`, `unwrap_or_else` sur selectors) sans élargir le périmètre.
- Dépendances: TDD-3 (reconnaissance de la source).
- QA: Étapes 2-3 (épisodes de l'aperçu E1/E2 : titres, médias audio, images) et étape 6 (page locale à un épisode sans image) ; critères P1/P2, P4 / F1, F3.
- Mutation d’acceptation: S1 (étapes d'identification des épisodes) et S3 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: `parse_web_episodes` et `WebEpisode` de `src-tauri/src/application/import_export/web_episode_extraction.rs`.

### TDD-5 — Rejeter les épisodes invalides avec motif et signaler « aucun média audio »
- Couvre: S6 (page accessible sans aucun épisode avec média audio → signalement « aucun média audio n'a été trouvé », aucune histoire créée) et le rejet motivé des éléments invalides (constat d'audit).
- RED: Tests échouant : fixture avec un épisode à titre vide ou sans média audio → élément rejeté avec motif, absent de l'aperçu ; fixture sans aucun média audio → `preview_web_podcast` renvoie le signalement dédié ; le placeholder « propagates_parsing_error » de la commande devient ce test local (serveur HTTP local servant la page).
- GREEN: Filtrage dans `preview_web_podcast` : un épisode est retenu ssi titre non vide (après trim) + `audio_url` présent ; les éléments invalides sont rejetés avec motif (événements de diagnostic existants) ; zéro épisode valide → échec « aucun média audio n'a été trouvé », aucune histoire possible.
- REFACTOR: Isoler le filtrage des épisodes extraits dans une fonction testable, sans toucher au fetch ni au parsing.
- Dépendances: TDD-4 (extraction honnête).
- QA: Étape 9 de la procédure (page locale sans aucun média audio) ; critères P7 / F6 ; motif du rejet visible dans le rapport de l'aperçu.
- Mutation d’acceptation: S6 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: `preview_web_podcast` de `web_episode_extraction.rs` et le signalement d'erreur dédié.

### TDD-6 — Exposer accept/commit en IPC et créer l'histoire complète prête à l'emploi
- Couvre: S2 (histoire complète créée : exactement N épisodes dans l'ordre d'apparition, titre propre à chaque épisode, audio téléchargé et associé, image associée si fournie) et le bout-en-bout S1/E1-E2.
- RED: Test échouant : sur une fixture de page à N épisodes valides, l'accept web (ré-fetch + re-parse) construit une structure canonique à N nœuds ordonnés — label du nœud = titre d'épisode, jamais le titre de la collection —, chaque audio est téléchargé et promu en ligne `assets` associée au nœud (chaîne `promote_enclosure` du RSS), l'image est associée si présente, et le commit (histoire + lignes `assets`) tient dans une seule transaction.
- GREEN: Nouvelle commande IPC `accept_web_podcast_creation` (miroir de `accept_rss_story_creation`) : préparation multi-épisodes, téléchargement des médias en phase réseau avant le verrou DB, création de l'histoire `source_format` web ; un échec de téléchargement d'un audio suit le comportement RSS existant (histoire créée, état partiel, motif honnête) ; l'accept mono-épisode WIP (`prepare_web_story_creation(selected_episode_title)`, `WebEpisodeRef`) est reformaté pour tous les épisodes.
- REFACTOR: Aligner les types du flux web (`PreparedWebCreation`) sur le schéma prepare/commit du RSS sans dupliquer le téléchargement.
- Dépendances: TDD-1 à TDD-5.
- QA: Étapes 1 à 6 de la procédure (E1, E2, compte N relevé sur la page elle-même, épisode sans image) ; critères P1 à P4 / F1, F2, F3.
- Mutation d’acceptation: S2, S1 (et S3) de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: `web_episode_extraction.rs` (prepare/commit multi-épisodes), `commands/import_export.rs` (commande + enregistrement dans `lib.rs`), `ipc/dto/import_export.rs`.

### TDD-7 — Parcours frontend : URL, aperçu, confirmation, échecs motivés
- Couvre: S1, S2, S3, S4, S5, S6 via l'interface visible : champ d'adresse URL, aperçu des N épisodes, confirmation de création, raison d'échec affichée, histoire dans la bibliothèque.
- RED: Test vitest échouant (modèle `CreateFromRssSurface.test.tsx`) : la surface web affiche la raison dans un `role="alert"` sur URL mal formée, page inaccessible et aucun média audio, liste les épisodes de l'aperçu (titre, audio, image) et émet l'accept à la confirmation ; le wrapper IPC web valide le contrat `WebPreviewDto` comme le wrapper RSS.
- GREEN: Ajouter `CreateFromWebSurface` + hook `use-web-creation` (modèle `use-rss-creation`) + wrappers IPC `fetch_web_podcast_preview` / `accept_web_podcast_creation` dans `src/ipc/commands/import-export.ts` + contrats partagés, et brancher la nouvelle entrée du parcours de création de `src/routes/library/LibraryRoute.tsx` : saisie URL → aperçu → « Créer l'histoire » → bibliothèque, sans sélection mono-épisode.
- REFACTOR: Réutiliser les composants partagés (`Button`, `Field`, `StateChip`, `aria-live`/alert) comme la surface RSS, sans dupliquer de styles.
- Dépendances: TDD-6 (accept exposé en IPC).
- QA: Toutes les étapes 1 à 10 de la procédure via l'interface ; critères P1 à P8 / F1 à F7.
- Mutation d’acceptation: S1 à S6 de `features/import-podcasts-pages-web-non-rss.feature`.
- Mutation source: `src/features/import-export/` (surface, hook, tests), `src/ipc/commands/import-export.ts`, `src/shared/ipc-contracts/import-export.ts`, `src/routes/library/LibraryRoute.tsx`.

## Alternatives rejetées

- 1. Conserver l'accept mono-épisode du WIP (`prepare_web_story_creation(selected_episode_title)` + `WebEpisodeRef`, structure minimale) — rejeté : le scénario S2 exige une histoire contenant exactement N épisodes dans l'ordre d'apparition sur la page ; une sélection mono-épisode ne peut le satisfaire. Remplacé par un accept multi-épisodes miroir de `accept_rss_story_creation`.
- 2. Un champ URL unique avec auto-routage RSS/web (sniffing dans une seule commande) — rejeté : le contrat ne définit pas le comportement d'une URL RSS saisie dans le parcours web ; le flux RSS existant reste l'entrée RSS (S7 sans changement) et le parcours web traite les pages HTML. Deux entrées distinctes, comme aujourd'hui, est le design minimal.
- 3. L'extraction via JSON-LD ou autres données structurées — rejeté : explicitement hors périmètre (aucun comportement de contrat) et non garanti sur les pages d'exemple ; les exigences observables (titre, média audio, image optionnelle, ordre) sont satisfaites par le HTML déjà présent dans les pages.
- 4. Conserver le test réseau externe httpstat.us — rejeté : non déterministe et instable ; remplacé par un serveur HTTP local (thread `std::net::TcpListener`) et un domaine réservé RFC 2606, à l'image des choix d'inputs de la story QA.
- 5. Suivre la pagination ou les liens « pages suivantes » — rejeté : explicitement hors périmètre (« une URL = une page »).

## Journal d'exécution

### Audit du point de départ (pages d'exemple, 2026-08-31)

1. Structure réelle des pages (HTML SSR SvelteKit, zéro `<audio>`, zéro URL `.mp3`/`.m4a` dans le HTML brut) :
   - Page liste E1 : bloc `<script type="application/ld+json">` avec `@graph` contenant un `ItemList` de 20 éléments ordonnés (`position` 1..20, `url` absolue de la page épisode) ; le blob SvelteKit embarqué ne porte que ~20 expressions (chargement progressif client, hors périmètre) ; aucune URL audio dans la page liste.
   - Page liste E2 : `ItemList` de 10 éléments.
   - Page épisode (ex. `l-ile-au-tresor-9062972`) : `@graph` avec nœud `RadioEpisode` : `name` (titre), `mainEntity` = `AudioObject` avec `contentUrl` (URL audio m4a sur `media.radiofrance-podcast.net`), `image.url` (ImageObject), `description`.
2. Vrai sol (fetch réel des 30 pages épisodes, HEAD des 27 audio) : E1 = 17/20 entrées résolues en épisode avec audio (les 3 entrées restantes pointent 3 fois la page série Tintin, sans `RadioEpisode`) ; E2 = 10/10. N observable : E1 = 17, E2 = 10.
3. Audio : format m4a (en-tête `ftyp`, marque `M4A `), tailles 12..106 Mo (max E1 = 106 118 738 o, E2 = 25 733 268 o) ; le plafond actuel `MAX_MEDIA_BYTES` = 32 MiB et `sniff_media` (png/jpeg/bmp/webp/mp3/wav/ogg) excluent le m4a → sans extension, tout audio réel dégénérerait en `(Media, Missing)` et violerait P3.
4. Code WIP : `fetch_html` (échecs transport/HTTP motivés déjà présents), `parse_web_episodes` (fallbacks `item`/`article` + titre de repli « Épisode sans titre », audio seulement via `enclosure[url]`/`a[href$='.mp3'…]`), `preview_web_podcast` (garde `starts_with("http://")`), `prepare_web_story_creation(selected_episode_title)` mono-épisode, `commit_web_story_creation` cassé (colonnes `stories` inexistantes), 4 placeholders `assert!(true)` dans `commands/import_export.rs` (~l. 1158-1180), commande `fetch_web_podcast_preview` enregistrée dans `lib.rs` (l. 550), aucun accept web exposé, test httpstat.us fragile.
5. Ancrages réutilisés : `feed_url_host` (domaine/import/rss.rs : http/https strict, sans userinfo, sans IPv6, port 1..=65535, hôte sobre) ; chaîne RSS `prepare_rss_story_creation`/`commit_rss_story_creation` (téléchargement en phase réseau, transaction `BEGIN IMMEDIATE` stories + assets + `story_local_imports`, verify-rows, compensation) ; `store_media` (sniff magic-bytes, store content-addressé, plafond dur 32 MiB) ; `CanonicalStructure` v3 (nœuds plats ordonnés `n1..`, `text`, `label`, `imageAssetId`, `audioAssetId`, `options[{label,target}]`).
6. Migrations : `story_local_imports.source_format` CHECK `IN ('rustory','structured-folder','rss')` (0013, table à reconstruire) ; `assets.media_format` CHECK `IN ('png','jpeg','mp3','wav','ogg')` (0007, à reconstruire avec l'index `idx_assets__content_hash` de 0008).

### Décisions d'implémentation (fixées à l'audit, sans ajout de contrat)

1. Extraction en trois passes dans l'ordre, un épisode n'est émis que s'il a un titre non vide ET une URL audio (rejet de tout fallback de titre) :
   - P1 — nœuds épisode JSON-LD directs (`RadioEpisode`/`PodcastEpisode`/`BroadcastEpisode`/`MusicRecording` avec `contentUrl`) ;
   - P2 — `ItemList` JSON-LD (tri `position`) : chaque URL résolue (dedup exact) est fetchée et extraite en P1/P3 (profondeur 1, pas de ré-ItemList) ; c'est le chemin des pages E1/E2 ;
   - P3 — DOM (scraper, ordre du document) : liens `<a href>` vers un média audio (extension `.mp3/.m4a/.ogg/.wav/.aac/.opus`), balises `<audio src>`/`<source src>` ; titre = texte de l'ancre (ou `aria-label`), image = `<img>` du conteneur (même `<li>`/parent proche).
2. Accept multi-épisodes (rejet du mono-épisode WIP) : re-fetch + re-parse, structure v3 à N nœuds ordonnés (`label` = titre d'épisode, `text` = description d'épisode sinon le titre, option `Continuer` vers le nœud suivant si le format de navigation l'exige), audio ET image (si fournie) téléchargés en phase réseau, commit en une seule transaction (`source_format = 'web'`).
3. m4a : migration 0016 (reconstruction `story_local_imports` + `'web'` et `assets` + `'m4a'`, index 0008 recréé), sniff `ftyp` + marques audio (`M4A ` observée, plus un petit jeu de marques audio), `mime_for_ext("m4a") = audio/mp4`, plafond média dédié au parcours web (128 MiB, paramétrable) sans toucher le plafond 32 MiB des autres flux (S7 inchangé octet à octet).
4. Zéro épisode audio extrait → échec motivé dédié « Aucun média audio n'a été trouvé » (S6) ; échecs de téléchargement individuels → verdict contenu `(Media, Missing)`, état `partial`, création poursuivie (précédent RSS).
5. Titre de l'histoire : titre de la collection/page si présent (JSON-LD `PodcastSeries.name`/`WebPage.name`, sinon `<title>`), normalisé+validé, repli `Histoire de {hôte}` (non observable par la QA, hypothèse 2 du plan).

#### TDD-1 — Refuser une URL mal formée avant toute requête réseau
1. RED — commande : `cd src-tauri && cargo test --lib test_fetch_web_podcast_preview_rejects` → exit 101 : `test_fetch_web_podcast_preview_rejects_invalid_scheme ... FAILED` — `assertion left == right failed, left: String("request"), right: "url_invalid"` (l'adresse `http://` traverse la garde par préfixe jusqu'au dispatch réseau) ; `..._rejects_empty_url ... ok`. Sortie RED : 1 passed, 1 failed, 1443 filtered out.
2. GREEN — garde d'entrée stricte : le helper `validate_web_entry_url` (`feed_url_host(web_url).ok_or_else(invalid_web_url_error)?`, renvoie l'hôte sobre) est consulté dans `preview_web_podcast` et `prepare_web_story_creation` AVANT tout fetch (remplace le check par préfixe `starts_with("http://")`). Commande : `cd src-tauri && cargo test --lib test_fetch_web_podcast_preview_rejects` → 2 passed, 0 failed, exit 0.
3. REFACTOR — helper commun placé après `ensure_web_source_enabled`, partagé par les deux façades web ; `source_host` dérivé de la garde (plus de `unwrap_or("unknown")`). Commande : `cd src-tauri && cargo test --lib web_episode_extraction` → 7 passed, exit 0.
4. Mutation source — commande : `cd src-tauri && cargo mutants --in-place -f 'src/application/import_export/web_episode_extraction.rs' -F 'validate_web_entry_url|invalid_web_url_error|preview_web_podcast|prepare_web_story_creation|ensure_web_source_enabled' -- --lib` → 7 mutants : 3 caught, 3 unviable, 1 missed, exit 2 (log `/tmp/tdd1_cargo_mutants2.out`). Survivant unique : `web_episode_extraction.rs:257:27 replace == with != in prepare_web_story_creation` — DISPOSITION DIFFÉRÉE : code WIP mono-épisode mort (non exposé en IPC, aucun test au TDD-1), réécrit intégralement au TDD-6 (la fonction disparaît ; à re-vérifier au run fichier du TDD-6), consigné et non masqué.
5. Mutation d'acceptation (APS) — scope S4. Chaîne (depuis le worktree root, chemins absolus hors worktree sous `/home/roukmoute/tmp-swarm-coder-aps/tdd1-s4/`) : `python3 src-tauri/tools/acceptance_scope.py features/…feature build-cible/feature.feature "Refuser une URL mal formée"` → `cd <APS-clone> && ~/.local/bin/bb gherkin-parser <feature.travail> ir.json` → `APS_FEATURE_PATH=<feature.travail> python3 src-tauri/tools/acceptance_entrypoint_generator.py ir.json generated/` → copie de `generated/entry_points.rs` dans le slot `src-tauri/tests/acceptance/generated/entry_points.rs` → `~/.local/bin/bb gherkin-mutator --level full --workers 1 --status-interval 0 --feature <feature.travail> --work-dir <tdd1-s4/mutation> --generated-dir <tdd1-s4/generated> --runner-worker "python3 <abs src-tauri/tools/acceptance_runner_worker.py>"` → `total=0 killed=0 survived=0 errors=0`, exit 0 (S4 sans table Examples → 0 mutations candidates ; scope exécuté et validé). `cd src-tauri && cargo test --test acceptance` → 1 passed (S4), exit 0. Feature gelée octet-à-octet identique au commit de base après le run (`cmp` contre `git show HEAD:features/…`).
6. Commit — `feat(web-import): TDD-1 — garde URL stricte avant tout réseau + infrastructure APS (scope, générateur, worker, harness S4)`.
