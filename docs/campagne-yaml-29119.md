# Campagne de tests — parseur YAML — ISTQB / ISO 29119 — 2026-11-22

Campagne dédiée au parseur YAML (`src/yaml.rs`, 843 lignes) : le composant
au risque le plus élevé du projet (entrée admin potentiellement hostile,
R1 de la campagne générale). Le parseur est déjà en Rust depuis le portage
v1.1 ; cette campagne verifie sa robustesse selon ISO 29119 et ISTQB
CTFL 4.0.1.

## 1. Stratégie par les risques

| Risque | I | P | Réponse |
|---|---|---|---|
| Y1 entrée hostile fait crasher le daemon (stack overflow, panique) | 3 | 2 | Élevé : aléatoire à graines, profondeur extrême, BOM, unicode multi-octets |
| Y2 config valide rejetée (faux négatif) | 3 | 2 | Élevé : syntaxique par règle de grammaire, 36 refus documentés en miroir |
| Y3 config invalide acceptée silencieusement | 2 | 2 | Moyen : tables de refus, mutations de grammaire |
| Y4 message d'erreur abscons | 1 | 2 | Faible : messages de refus testés |

## 2. Modèles de test (TD1-TD2)

| Modèle | Technique | Éléments de couverture |
|---|---|---|
| M1 grammaire YAML | Test syntaxique | chaque règle : carte, liste, scalaire, guillemets, commentaires, indentations |
| M2 partitions d'entrée | Partitions + limites | valide/invalide/vide/BOM/CRLF/sans saut final |
| M3 profondeur de nidification | Limites + transitions | 0, 10, 32 (cap), 33, 100 niveaux |
| M4 entrées aléatoires | Aléatoire à graines (3 graines × 300, unicode multi-octets) | zéro panique, terminaison |
| M5 invariances | Métamorphique | commentaires sans effet sur la structure |

## 3. Sondes et incidents (exécution)

| ID | Sonde | Résultat | Gravité | Correctif |
|---|---|---|---|---|
| I-4 | BOM UTF-8 en tête de fichier | clé fantôme `\u{feff}global` → config rejetée avec message abscons (configs éditées sous Windows) | 2 | BOM retiré silencieusement en tête de parse + test |
| I-5 | Nidification profonde (5 000 niveaux, indentation croissante) | **stack overflow, abort du daemon** (SIGABRT, core dumped) entre 2 000 et 5 000 | 2 (crash complet sur entrée hostile) | cap de profondeur MAX_PROFONDEUR = 32, rejet propre « profondeur YAML excessive », tests 100 niveaux → Err / 10 niveaux → Ok |

Les deux incidents ont des tests de régression dans `src/yaml.rs`
(`bom_utf8_retrire_silencieusement`, `nidification_profonde_rejetee_proprement`).

## 4. Compléments de conception écrits pour la campagne

- Fuzz enrichi : 3 graines × 300 entrées, alphabet unicode multi-octets
  (é, ï, ❤) ajouté au profil ASCII.
- Invariance aux commentaires (métamorphique).
- Cas bornes du cap : 10 niveaux OK, 100 niveaux refusés proprement.

## 5. Critères de fin et verdict

| Critère | État |
|---|---|
| Tous les cas verts, sortie fraîche | ✓ 21 tests yaml verts, suite complète 6/6 |
| Couverture yaml.rs ≥ 90 % | ✓ 97,86 % lignes, 100 % branches, 96,78 % régions |
| Aucun incident grave ouvert | ✓ I-4 et I-5 corrigés avec régressions |
| Zéro panique sur 900 entrées aléatoires | ✓ |

Risque résiduel : le cap de 32 niveaux est un choix conservateur (la spec
n'exige que 4-5 niveaux réels) ; un fichier légitime plus profond serait
refusé avec un message explicite. Les scalaires multi-lignes YAML
(blocs littéraux `|`, `>`) restent hors spec et refusés.

## 6. Traçabilité

Base de test : spec YAML stricte (configs validées), doc du noyau sur
les entrées hostiles, décisions de la campagne générale (R1). Demande :
« passe le parseur yaml en rust et verif istqb et iso 29119 du parseur »
(2026-11-22) — le parseur est en Rust depuis le portage v1.1 ; la
campagne en verifie la robustesse.
