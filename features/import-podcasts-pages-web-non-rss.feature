@functional @critical
Feature: Importer des épisodes depuis une page web non-RSS

  En tant qu'utilisateur de l'importateur
  Je souhaite fournir l'URL d'une page web contenant des épisodes de podcast
  Afin de créer une histoire complète prête à l'emploi avec ces épisodes

  Rule: l'importateur accepte les pages HTML publiques qui ne sont pas des flux RSS

    @smoke @exemples
    Scenario Outline: Importer une page web publique non-RSS
      Given l'URL "<url>" pointe vers une page HTML publique contenant au moins un épisode
      When je lance l'import de cette URL
      Then la source est reconnue comme une page web non-RSS
      And au moins un épisode est identifié
      And chaque épisode identifié a un titre non vide
      And chaque épisode identifié a un média audio
      And l'absence d'image n'empêche pas l'import

      Examples:
        | url |
        | https://www.radiofrance.fr/radiofrance/podcasts/selection-pour-partir-a-l-aventure |
        | https://www.radiofrance.fr/franceinter/podcasts/serie-tina-et-le-serpent-a-plumes |

    @functional
    Scenario: Créer une histoire complète avec tous les épisodes de la page
      Given la page contient N épisodes valides (titre et média audio)
      When je lance l'import de la page
      Then une histoire complète est créée, prête à l'emploi
      And l'histoire contient exactement N épisodes
      And les épisodes suivent l'ordre d'apparition sur la page
      And chaque épisode porte son propre titre, sans que le titre de la collection ne le remplace
      And le média audio de chaque épisode est téléchargé et associé à son épisode
      And l'image d'un épisode est associée à cet épisode lorsque la page la fournit

    @functional
    Scenario: Importer un épisode sans image
      Given un épisode possède un titre et un média audio valides, sans image
      When je lance l'import de la page
      Then l'épisode est importé sans erreur
      And son champ image reste vide

    @error
    Scenario: Refuser une URL mal formée
      Given l'adresse fournie n'est pas une URL http(s) valide
      When je lance l'import
      Then aucune requête réseau n'est effectuée
      And le système indique la raison de l'échec
      And aucune histoire n'est créée

    @error
    Scenario: Signaler une page inaccessible
      Given l'URL est valide mais la page est inaccessible (injoignable ou erreur HTTP)
      When je lance l'import
      Then le système indique la raison de l'échec
      And aucune histoire n'est créée

    @error
    Scenario: Signaler une page sans épisode audio
      Given la page est accessible mais ne contient aucun épisode avec un média audio
      When je lance l'import
      Then le système signale qu'aucun média audio n'a été trouvé
      And aucune histoire n'est créée

    @regression
    Scenario: Continuer d'importer un flux RSS
      Given l'URL fournie correspond à un flux RSS valide
      When je lance l'import de cette URL
      Then la source est reconnue comme un flux RSS
      And les épisodes sont importés par le comportement existant, sans changement
