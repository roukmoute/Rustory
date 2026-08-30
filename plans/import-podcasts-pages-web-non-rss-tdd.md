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
