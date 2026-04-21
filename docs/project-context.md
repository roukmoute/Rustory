# Project Context

## Purpose

Ce document décrit les conventions de livraison et d'hygiène git spécifiques à Rustory.

## Commit Delivery Rules

- Une unité de travail implémentée doit toujours correspondre à un seul commit Git.
- Une fois l'implémentation terminée et les tests locaux au vert, créer le commit avant toute vérification distante.
- Vérifier ensuite le statut de la CI GitHub pour le commit ou la PR qui porte ce travail.
- Si la CI GitHub échoue, corriger localement, relancer les tests nécessaires, puis mettre à jour le même commit avec `git commit --amend` au lieu de créer un commit supplémentaire.
- Répéter cette boucle jusqu'à obtenir une CI GitHub au vert tout en conservant un commit unique.
- Ne considérer le travail prêt à revue que lorsque les tests locaux pertinents et la CI GitHub sont tous les deux au vert.

## Commit Hygiene

- Utiliser `git commit --amend --no-edit` pour les corrections de CI qui ne changent pas l'intention du commit.
- Si le message de commit doit être ajusté pour mieux refléter le travail final, utiliser `git commit --amend` et conserver un message unique.
- Éviter les commits de fixup ou de suivi sur la même unité de travail sauf demande explicite contraire.
