# Project Context

## Purpose

Ce document décrit les conventions de livraison et d'hygiène git spécifiques à Rustory.

## Commits atomiques et publication Git

- Un commit porte une intention autonome, compréhensible et vérifiable. Séparer les changements indépendants ; une livraison peut donc comprendre plusieurs commits atomiques.
- Exécuter les vérifications locales pertinentes avant de créer les commits.
- Pour le moment, pousser directement sur `main`, sans pull request, conformément à la consigne explicite de l'utilisateur.
- Vérifier ensuite la CI GitHub sur le commit exact à livrer. Une CI rouge doit être corrigée avant de publier la release.
- `git commit --amend` est réservé aux commits locaux qui n'ont pas encore été poussés. Il permet de corriger leur contenu ou leur message sans multiplier les commits préparatoires.
- Après un push, conserver l'historique publié : corriger un échec de CI par un nouveau commit atomique, relancer les vérifications nécessaires et pousser ce correctif. Ne jamais forcer le push de `main`.

## Version et livraison complète

- Chaque livraison incrémente la version selon SemVer : `PATCH` pour une correction compatible, `MINOR` pour une fonctionnalité compatible, `MAJOR` pour une rupture de compatibilité, en tenant compte du stade de développement `0.x`.
- Synchroniser dans un même commit les quatre emplacements de version : `package.json`, `src-tauri/Cargo.toml`, l'entrée du paquet `rustory` dans `src-tauri/Cargo.lock` et `src-tauri/tauri.conf.json`.
- Après les vérifications locales, pousser le commit sur `main`, puis créer et pousser son tag `vMAJOR.MINOR.PATCH`. Les workflows `Verify` et `Build Release` peuvent s'exécuter en parallèle.
- Attendre une CI `Verify` verte et le succès de `Build Release` sur ce même commit, puis contrôler la release draft : notes, artefacts des trois plateformes, signatures et manifeste `latest.json` cohérents avec la version. Suivre le [runbook de livraison](release-runbook.md).
- Lancer ensuite `promote-release.yml` et vérifier que la release et son manifeste de mise à jour sont publics. Une draft seule ou un tag seul ne constituent pas une livraison terminée.
- L'utilisateur a explicitement autorisé le push direct et la publication des releases à chaque livraison : ces contrôles restent obligatoires, mais ne nécessitent pas de lui redemander son accord à chaque fois. Toute consigne ultérieure contraire prévaut.
- Si une version a déjà été publiée, livrer toute correction avec une nouvelle version, un nouveau tag et une nouvelle release ; ne pas remplacer les artefacts ou déplacer le tag de la version publiée.
