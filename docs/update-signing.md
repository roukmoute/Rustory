# Update Signing

## Purpose

Ce document décrit le cycle de vie de la paire de clés qui signe les artefacts de mise à jour de Rustory : génération, stockage, chaîne de responsabilité, rotation, et modèle assumé en cas de perte. Il complète le [release-runbook.md](release-runbook.md) (chaîne de livraison) et le contrat d'application de mise à jour ([architecture/device-support-profile.md](architecture/device-support-profile.md)).

Le mécanisme : chaque artefact updater produit par le workflow de release est signé (minisign Ed25519) ; la clé PUBLIQUE est embarquée dans les binaires distribués à la compilation ; toute copie distribuée VÉRIFIE la signature avant d'appliquer quoi que ce soit — la vérification est intrinsèque au mécanisme et non désactivable.

## Génération initiale de la paire

Une seule fois, sur un poste de confiance, HORS du repo :

```bash
pnpm tauri signer generate
```

La commande produit :

- la **clé privée** (protégée par la passphrase saisie à la génération) ;
- la **clé publique** correspondante.

Ne JAMAIS écrire la clé privée ni la passphrase dans un fichier du repo, un fichier `.env`, un gestionnaire de notes ou un historique shell. La paire officielle n'existe qu'à deux endroits : le gestionnaire de mots de passe de l'opérateur (source de vérité) et les secrets GitHub Actions (copie d'exploitation).

## Stockage

| Élément | Emplacement | Nature |
| --- | --- | --- |
| Clé privée | Secret GitHub Actions `TAURI_SIGNING_PRIVATE_KEY` | SECRET — ne quitte jamais ce périmètre |
| Passphrase | Secret GitHub Actions `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | SECRET |
| Clé publique | Variable GitHub Actions `RUSTORY_UPDATER_PUBKEY` | Publique par nature (embarquée dans chaque binaire distribué) |

Le workflow `build-release.yml` consomme les deux secrets pour signer, et embarque la variable publique dans les binaires via l'environnement de compilation. Un build local n'a besoin d'AUCUN de ces éléments : sans clé publique embarquée, la copie produite retombe simplement sur la guidance manuelle de mise à jour (fail-closed).

## Chaîne de responsabilité

- **Opérateur unique : Roukmoute.** Génération, provisionnement des secrets, promotion des releases et rotation relèvent de ce seul rôle.
- Aucun automatisme ne lit la clé privée en dehors du workflow de release ; aucune promotion n'est automatique.

## Rotation en cas de compromission

Si la clé privée est suspectée compromise :

1. Générer une NOUVELLE paire (procédure ci-dessus).
2. Remplacer les deux secrets et la variable publique dans GitHub Actions.
3. Construire et promouvoir une nouvelle release : les builds suivants embarquent la nouvelle clé publique.

Conséquence assumée : les copies ANTÉRIEURES (porteuses de l'ancienne clé publique) ne peuvent plus vérifier les feeds signés par la nouvelle clé. Elles échouent PROPREMENT — étape de vérification refusée, installation courante intacte, guidance manuelle — jamais une installation non sûre. Le chemin de rattrapage de ces copies est le remplacement manuel : télécharger la nouvelle version depuis la page officielle des versions et réinstaller. C'est le modèle de confiance choisi : une rupture de chaîne dégrade vers le manuel, jamais vers le permissif.

## Perte de la clé privée

AUCUNE récupération n'est possible : il n'existe ni séquestre ni copie de secours en dehors du gestionnaire de mots de passe de l'opérateur. La perte de la clé privée est un **incident opérationnel critique** :

- plus aucune mise à jour signée ne peut être publiée pour les copies distribuées existantes ;
- ces copies basculent définitivement en guidance manuelle (leurs vérifications échoueront sur tout nouveau feed) jusqu'à réinstallation manuelle d'un build porteur d'une nouvelle clé publique ;
- la remédiation est la même que la rotation (nouvelle paire, nouveaux builds, réinstallation manuelle des copies existantes), sans chemin plus court.

Ce modèle est assumé et documenté : sa simplicité (un opérateur, deux secrets, une variable) est préférée à un séquestre multi-dépositaires que ce produit n'a pas les moyens d'opérer proprement.
