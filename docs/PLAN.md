# Plan

Public : une équipe hardware/VHDL et une équipe design, qui ne connaissent pas Git, ainsi que
des utilisateurs avertis. Règle générale : les choix guidés ne cachent jamais la commande git
correspondante. L'interface est en anglais et en français.
Visuel cible : les maquettes validées (https://claude.ai/artifact/P4D9EvkYgPTQtZDBw82EjJ).

## 1. Socle
App Rust + iced qui appelle le `git` CLI en arrière-plan avec timeout (aucun gel possible).
Journal Git de toutes les commandes, console en mode avancé. Ouvrir/cloner un dépôt,
état des fichiers, historique.

## 2. Gestes de base
- Diffs lisibles, lignes déplacées affichées comme déplacées (`--color-moved`, histogram).
- Choisir ce qu'on garde, « Enregistrer » (commit), « Envoyer » (fetch → intégration → push).
- **Ctrl+Z / Ctrl+Y pour toute action** : instantané des refs, de l'index et des fichiers
  avant chaque action. Un push ne s'annule que par revert ou force push explicite.
- **Menu « Configuration Git » expliqué** : chaque réglage décrit, clé git visible,
  portée globale ou dépôt (identité, stratégie de pull, `core.autocrlf`, style de conflit,
  `rerere`, identifiants…).

## 3. Branches, conflits et vue en arbre
- Assistants merge/rebase. Jamais « ours/theirs » : chaque côté porte le nom de sa branche,
  dans le bon sens pour un merge comme pour un rebase.
- Moins de faux conflits : histogram, `zdiff3`, `rerere`, option d'ignorer les espaces,
  fins de ligne normalisées. Merge d'abord en `--no-commit`, puis détection des blocs
  dupliqués (ligne déplacée d'un côté) avant d'enregistrer.
- Vue en arbre avec glisser-déposer, confirmation affichant la commande exacte :
  - commit de départ d'une branche déposé sur un autre commit → rebase ;
  - commit déposé sur le dernier commit d'une autre branche → merge ;
  - double-clic sur une branche → checkout avec autostash par branche (`auto/<branche>`) ;
  - étiquette de branche glissée sur un commit → déplacer la branche (sauvegarde avant) ;
  - Ctrl + glisser un commit sur une branche → cherry-pick ;
  - glisser un commit sur son voisin → réordonner / squash (rebase interactif piloté) ;
  - branche locale ↔ distante → push (avec pull avant) / pull ;
  - clic droit : branche ou tag ici, diff, revert, modifier le message, restaurer un fichier ;
  - deux commits sélectionnés → comparaison ;
  - suppression de branche sécurisée ; bandeau HEAD détaché avec « créer une branche ici ».

## 4. Sous-modules et métiers
Sous-modules (ajout, mise à jour, synchro, état), Git LFS et verrouillage pour les binaires
design, modèles `.gitignore` Vivado/Quartus, exécutables portables Windows/macOS/Linux via
GitHub Actions.
