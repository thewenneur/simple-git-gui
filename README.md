# Simple Git

Une interface graphique pour Git pensée pour les personnes qui ne connaissent pas Git
(équipes hardware/VHDL et design), sans rien cacher aux utilisateurs avertis : chaque
action guidée affiche la commande `git` exacte qu'elle exécute.

Application de bureau portable écrite en Rust avec [iced](https://iced.rs). Elle utilise
le `git` installé sur la machine (pas de libgit2).

## Prérequis

- [Git](https://git-scm.com) installé et accessible dans le `PATH`.
- Pour compiler : Rust stable (`rustup`).

## Lancer

```sh
cargo run --release              # écran d'accueil : ouvrir ou cloner un dépôt
cargo run --release -- ~/projet  # ouvre directement ce dépôt
```

## Ce que fait la version actuelle

- Interface en français et en anglais (langue du système par défaut, bouton FR/EN).
- Ouvrir un dépôt existant ou en cloner un (sous-modules compris).
- Voir la branche courante, son état par rapport au serveur et les fichiers modifiés,
  en clair avec la notation git en mode avancé.
- Historique des commits de toutes les branches.
- **Journal Git** : chaque commande lancée, son résultat et sa durée. En mode avancé :
  sorties complètes et console pour taper ses propres commandes git.
- Git ne peut jamais bloquer l'interface : chaque commande tourne en arrière-plan, sans
  terminal ni éditeur, et est arrêtée après un délai maximum.

## Organisation du code

- `src/git/runner.rs` : seul endroit qui lance `git` (timeout, environnement non interactif).
- `src/git/status.rs` : lecture de `git status --porcelain=v2`.
- `src/git/repo.rs` : requêtes sur un dépôt (ouverture, historique, clone, opération en cours).
- `src/app.rs` : état de l'application et écrans.
- `locales/app.yml` : tous les textes, en anglais et en français côte à côte. Un test vérifie
  que chaque clé existe dans les deux langues et que chaque clé utilisée dans le code existe.

## Tests

```sh
cargo test
```

Certains tests créent de vrais dépôts temporaires et exécutent `git`.
