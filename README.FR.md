# find_dups : Détecteur de doublons multi-langages

Un détecteur de doublons complet implémenté en Go, Python et Rust avec des algorithmes identiques pour comparaison des performances et utilisation en production.

## Aperçu

`find_dups` analyse récursivement un ou plusieurs répertoires, identifie les fichiers en double via le hachage SHA-256 et génère des rapports et des scripts de suppression. Il utilise le traitement parallèle pour gérer efficacement les grandes collections de fichiers.

### Fonctionnalités clés

- **Implémentation multi-langage** : versions Go, Python et Rust pour comparaison des performances
- **Hachage parallèle** : utilise tous les cœurs CPU pour une détection rapide des doublons
- **Sécurité** : génère un script de suppression au lieu de supprimer directement les fichiers
- **Rapports détaillés** : exportations CSV avec métadonnées et horodatages
- **Support multi-disques** : peut analyser plusieurs répertoires sur différents points de montage

## Algorithme

Les trois implémentations suivent le même algorithme :

1. **Collecte des fichiers** — Parcours récursif de tous les répertoires spécifiés, enregistrement du chemin, taille, heure de création et de modification
2. **Groupement par taille** — Seuls les fichiers partageant une taille avec au moins un autre fichier passent au hachage (optimisation)
3. **Hachage parallèle SHA-256** — Calcul de hach cryptographiques en parallèle :
   - Go : goroutines avec pool de workers basé sur canaux
   - Python : `multiprocessing.Pool`
   - Rust : itérateur parallèle `rayon`
4. **Identification des doublons** — Groupement des fichiers par hach dans les groupes de taille ; tous les fichiers dans un groupe de hach avec >1 élément sont des doublons
5. **Génération des sorties** :
   - `duplicates_<lang>.csv` — Tous les groupes de doublons avec métadonnées complètes
   - `sort_dup_<lang>.csv` — Tous les fichiers triés par taille (décroissant)
   - `duprm_<lang>.sh` — Script bash qui supprime tous les doublons sauf le premier (par ID) dans chaque groupe

## Fichiers de sortie

### duplicates_<lang>.csv
Fichier CSV contenant tous les doublons groupés par contenu. Colonnes :
- `FileID` : Identifiant séquentiel du fichier
- `Path` : Chemin complet du fichier
- `Size` : Taille du fichier en octets
- `Hash` : Hach SHA-256 (hexadécimal)
- `CreationTime` : Horodatage de création du fichier (ISO 8601)
- `ModificationTime` : Horodatage de modification du fichier (ISO 8601)

### sort_dup_<lang>.csv
Fichier CSV contenant tous les fichiers analysés triés par taille (décroissant). Mêmes colonnes que `duplicates_<lang>.csv`.

### duprm_<lang>.sh
Script bash exécutable qui supprime les fichiers en double, en conservant le premier fichier (FileID le plus bas) dans chaque groupe de doublons. **Vérifiez ce script avant l'exécution** pour vous assurer de ne pas supprimer des fichiers importants.

## Installation & Utilisation

### Implémentation Go

**Prérequis** : Go 1.16+

**Build** :
```bash
cd find_dups_go
go build -o find_dups_go find_dups_go.go
```

**Exécution** :
```bash
./find_dups_go /chemin/vers/scan1 /chemin/vers/scan2 ...
```

**Dépendances** : Bibliothèque standard uniquement

### Implémentation Python

**Prérequis** : Python 3.8+

**Exécution** :
```bash
cd find_dups_pthon
python3 find_dups_python.py /chemin/vers/scan1 /chemin/vers/scan2 ...
```

**Dépendances** : Bibliothèque standard uniquement

### Implémentation Rust

**Prérequis** : Rust 1.70+, Cargo

**Build** :
```bash
cd find_dups_rust
cargo build --release
```

**Exécution** :
```bash
./target/release/find_dups_rust /chemin/vers/scan1 /chemin/vers/scan2 ...
```

**Dépendances** (voir `Cargo.toml`) :
- `walkdir` 2.5 — Traversée de répertoires
- `sha2` 0.10 — Hachage SHA-256
- `csv` 1.4 — Écriture CSV
- `chrono` 0.4 — Formatage de l'heure
- `rayon` 1.12 — Traitement parallèle

## Résultats de benchmark

Testé sur environ 149 000 fichiers dans deux répertoires (SSD local + disque USB externe) :

| Métrique                | Python     | Rust       | Go         |
|-------------------------|------------|------------|------------|
| Fichiers scannés        | 149 044    | 148 819    | 148 819    |
| Fichiers hachés         | 128 964    | 128 738    | 128 738    |
| Temps de hachage        | 1:55.751   | 2:07.520   | 1:31.992   |
| Temps total             | 5:26.243   | 3:55.664   | 3:06.040   |
| Doublons trouvés        | 0          | 696        | 696        |
| Workers/threads         | 12         | 12         | 12         |

**Notes** :
- Temps au format `minutes:secondes.millisecondes`
- L'implémentation Python a trouvé 0 doublons tandis que Go et Rust en ont trouvé 696, indiquant un possible bug dans la logique de détection des doublons de la version Python
- Go montre les meilleures performances globales malgré une collecte de fichiers plus lente que Rust

## Évaluation & Recommandations

### Points forts

- **Valeur pratique** : Élevée — résout un vrai problème de recherche de doublons sur plusieurs répertoires et disques
- **Sécurité** : Bonne — génère un script de suppression pour révision au lieu de supprimer directement
- **Performance** : Les trois implémentations utilisent efficacement le traitement parallèle
- **Transparence** : Les rapports CSV permettent une analyse détaillée avant suppression

### Problèmes connus

1. **Discrepancy Python** : La version Python a trouvé 0 doublons tandis que Go/Rust en ont trouvé 696. Cela nécessite une investigation — probablement lié aux différents nombres de fichiers (149 044 vs 148 819) ou à un bug dans la logique de détection des doublons.

2. **Limitations de plateforme** :
   - Go utilise `syscall.Stat_t` (spécifique macOS) pour l'heure de création
   - Rust utilise `std::os::darwin::fs::MetadataExt` pour l'heure de création
   - Les deux nécessitent une compilation conditionnelle pour le support Linux/Windows

### Quelle implémentation utiliser ?

- **Pour une utilisation en production sur macOS** : Go — plus rapide globalement, binaire unique sans dépendances
- **Pour le développement multi-plateforme** : Rust — plus facile à adapter avec les attributs `#[cfg(target_os)]`
- **Pour le scriptage/prototypage rapide** : Python — plus facile à modifier, mais investiguer d'abord le bug de détection des doublons

## Perspectives d'avenir

1. **Corriger la détection des doublons Python** — investiguer la discrepancy entre implémentations
2. **Barre de progression** — Ajouter une indication de progression en temps réel pendant le hachage
3. **Heure de création multi-plateforme** — Utiliser la compilation conditionnelle pour Linux/Windows
4. **Optimisation du hachage partiel** — Hacher les premiers/derniers N Ko + taille avant le hachage complet du fichier
5. **Sortie configurable** — Permettre de spécifier le répertoire de sortie et les préfixes de fichiers
6. **Mode interactif** — Interface TUI simple pour réviser les doublons avant suppression
7. **Mode dry-run** — Montrer ce qui serait supprimé sans générer de script
8. **Déplacer au lieu de supprimer** — Option de déplacer les doublons vers un répertoire de staging
9. **Filtre de taille minimum** — Ignorer les fichiers sous un seuil configurable (ex: <1Ko)
10. **Déduplication par liens symboliques/durs** — Remplacer les doublons par des liens durs pour économiser l'espace sans supprimer

## Licence

Ce projet est fourni tel quel pour un usage éducatif et pratique.

## Contributions

Les contributions sont les bienvenues, notamment pour :
- Corriger le problème de détection des doublons Python
- Ajouter la compatibilité Windows/Linux
- Implémenter l'une des perspectives d'avenir listées ci-dessus
