# QA Procedure: Importer des épisodes depuis une page web non-RSS

## Story

- Story ID: `import-podcasts-pages-web-non-rss`
- Story artifact: `stories/import-podcasts-pages-web-non-rss.md`
- Feature artifact: `features/import-podcasts-pages-web-non-rss.feature`
- Contract version: 1

## Scope

Examen black-box, effectué uniquement via l'interface visible par l'utilisateur (parcours de création depuis une source externe : adresse URL, aperçu, confirmation ; puis bibliothèque d'histoires), des comportements de la feature « Importer des épisodes depuis une page web non-RSS » :

- S1 — Scénario « Importer une page web publique non-RSS », lignes d'exemples E1 (`https://www.radiofrance.fr/radiofrance/podcasts/selection-pour-partir-a-l-aventure`) et E2 (`https://www.radiofrance.fr/franceinter/podcasts/serie-tina-et-le-serpent-a-plumes`), qui font foi ;
- S2 — Scénario « Créer une histoire complète avec tous les épisodes de la page » (N = nombre d'épisodes valides de la page) ;
- S3 — Scénario « Importer un épisode sans image » ;
- S4 — Scénario « Refuser une URL mal formée » ;
- S5 — Scénario « Signaler une page inaccessible » ;
- S6 — Scénario « Signaler une page sans épisode audio » ;
- S7 — Scénario « Continuer d'importer un flux RSS ».

Interprétations en vigueur : « prête à l'emploi » = l'audio de chaque épisode est réellement téléchargé et associé pendant l'import, à l'identique du comportement existant pour l'import RSS ; une URL = une page, sans suivi automatique de pagination ni de liens « pages suivantes ».

Hors périmètre explicite (aucun comportement de contrat) : pagination multi-pages, blocage SSRF, rate limiting 429, budgets de timeout, refus par content-type, données structurées JSON-LD, rendu JavaScript, chargement progressif, réimport idempotent de la même page.

## Preconditions

- L'application Rustory est installée et démarre sur la machine de l'examinateur, et le parcours de création depuis une source externe avec champ d'adresse URL y est accessible.
- La machine a accès au réseau pour les URLs publiques (S1, S7) et au serveur local de pages contrôlées (S3, S5, S6).
- L'examinateur peut servir des pages locales sous HTTP (serveur statique local) : une page contenant exactement un épisode (titre + média audio, sans image) pour S3, une page sans aucun média audio pour S6, et une page dont la réponse est l'erreur HTTP 500 pour S5.
- L'examinateur peut relever fidèlement le contenu de la bibliothèque d'histoires avant et après chaque tentative d'import.

## Procedure

1. (S1/E1) Ouvrir le parcours de création depuis une source externe, saisir l'URL `https://www.radiofrance.fr/radiofrance/podcasts/selection-pour-partir-a-l-aventure` et lancer la récupération depuis l'interface.
2. (S1/E1) Dans l'aperçu, relever la source reconnue, les épisodes identifiés avec leurs titres, leurs médias audio et leurs images ; puis confirmer la création de l'histoire.
3. (S1/E1) Dans la bibliothèque, relever l'histoire créée : épisodes dans leur ordre, titre de chaque épisode, média audio associé (réellement présent sur la machine, sans nouvelle récupération requise), image associée lorsque la page en fournit une.
4. (S1/E2) Répéter les étapes 1 à 3 avec l'URL `https://www.radiofrance.fr/franceinter/podcasts/serie-tina-et-le-serpent-a-plumes`.
5. (S2) À partir de la page elle-même (pas de l'aperçu), compter N = nombre d'épisodes ayant un titre et un média audio, et comparer ce compte et l'ordre d'apparition sur la page au contenu de l'histoire relevé à l'étape 3.
6. (S3) Servir la page locale à un épisode (titre + média audio, sans image), importer son URL via le même parcours et relever l'aperçu puis l'histoire créée, en particulier le champ image de l'épisode.
7. (S4) Dans un nouveau parcours, saisir une adresse qui n'est pas une URL http(s) valide (ex. `pas-une-url`) et relever la réaction : échec affiché sans récupération en cours, raison indiquée, aucune requête réseau sortante pour cette adresse, bibliothèque inchangée.
8. (S5) Dans un nouveau parcours, saisir une URL valide mais injoignable (ex. `https://import-test-non-rss.exemple.invalid/`, résolution d'hôte impossible) et relever la réaction ; puis, dans un nouveau parcours, saisir l'URL de la page contrôlée dont la réponse est l'erreur HTTP 500 et relever la réaction.
9. (S6) Dans un nouveau parcours, saisir l'URL de la page locale contrôlée sans aucun média audio et relever la réaction : signal affiché et contenu de la bibliothèque.
10. (S7) Dans un nouveau parcours, saisir l'URL d'un flux RSS public valide (par défaut `https://feeds.simplecast.com/Sl5CSM3S`) et relever la source reconnue, l'aperçu, puis l'histoire créée par confirmation via le parcours existant.

## Pass Criteria

- P1 (S1/E1) : sur l'URL E1, la source est reconnue comme une page web non-RSS, au moins un épisode est identifié, chaque épisode identifié a un titre non vide et un média audio, l'absence d'image n'empêche pas l'import, et une histoire complète prête à l'emploi est créée.
- P2 (S1/E2) : sur l'URL E2, mêmes résultats que P1.
- P3 (S2) : l'histoire contient exactement N épisodes dans l'ordre d'apparition sur la page, chaque épisode porte son propre titre sans que le titre de la collection ne le remplace, le média audio de chaque épisode est téléchargé et associé à son épisode, et l'image d'un épisode est associée à cet épisode lorsque la page la fournit.
- P4 (S3) : l'épisode sans image est importé sans erreur et son champ image reste vide.
- P5 (S4) : aucune requête réseau n'est effectuée pour l'URL mal formée, le système indique la raison de l'échec, et aucune histoire n'est créée.
- P6 (S5) : pour l'URL injoignable et pour l'URL renvoyant l'erreur HTTP 500, le système indique la raison de l'échec et aucune histoire n'est créée.
- P7 (S6) : le système signale qu'aucun média audio n'a été trouvé et aucune histoire n'est créée.
- P8 (S7) : la source est reconnue comme un flux RSS et les épisodes sont importés par le comportement existant, sans changement.

## Fail Criteria

- F1 (S1) : sur au moins une des URLs E1 ou E2, la source n'est pas reconnue comme une page web non-RSS, ou un épisode identifié n'a pas de titre non vide ou de média audio, ou l'absence d'image empêche l'import, ou aucune histoire n'est créée.
- F2 (S2) : l'histoire ne contient pas exactement N épisodes, ou l'ordre des épisodes diffère de l'ordre d'apparition sur la page, ou un titre d'épisode est remplacé par le titre de la collection, ou le média audio d'un épisode n'est pas téléchargé et associé, ou une image fournie par la page n'est pas associée à son épisode.
- F3 (S3) : l'import de l'épisode sans image échoue, ou son champ image n'est pas vide.
- F4 (S4) : une requête réseau est effectuée pour l'URL mal formée, ou la raison de l'échec n'est pas indiquée, ou une histoire est créée.
- F5 (S5) : pour l'URL injoignable ou l'URL en erreur HTTP 500, la raison de l'échec n'est pas indiquée, ou une histoire est créée.
- F6 (S6) : le signal « aucun média audio trouvé » n'est pas affiché, ou une histoire est créée.
- F7 (S7) : le flux RSS n'est plus reconnu comme un flux RSS, ou le comportement d'import du flux a changé par rapport au comportement existant.
