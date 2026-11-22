# Campagne de tests ISTQB CTFL 4.0.1 / ISO-IEC-IEEE 29119 — SysGuard v1.4

Campagne du 2026-11-22, conformité adaptée (29119-2 clause 4) : plan et
résultats dans ce document, cas et procédures dans le code de test, sortie
fraîche de la session.

## 1. Objet et base de test

- Éléments : daemon superviseur (`run`), optimiseur (`optimise`), parseur
  YAML strict, collecte /proc et sysfs, moteur de règles, actions
  log/webhook/exec, provisionnement zram, unités systemd.
- Base de test : sysguard-specs.md, configs/sysguard.yaml.example,
  docs du noyau (PSI, zram, sysctl vm), man swapon(2), configs validées
  des fixtures, décisions de docs/justifications/*.md.

## 2. Stratégie par les risques (29119-2)

| Risque produit | I | P | Réponse |
|---|---|---|---|
| R1 parseur YAML sur entrée hostile (config fournie par l'admin) | 3 | 2 | Élevé : partitions, limites, aléatoire à graine, métamorphique, refus explicites |
| R2 moteur de règles faux (critique/retombée, cooldown) | 3 | 2 | Élevé : transitions d'états, table, métamorphique, mutation |
| R3 provisionnement/écritures système (zram, swappiness, irq) | 3 | 2 | Élevé : arbitrage en table, échecs partiels, dégradation sans privilèges, validation VM |
| R4 actions exec/webhook sur processus tierces | 2 | 2 | Moyen : intégration serveur HTTP réel, env SG bornée, timeout |
| R5 collecte /proc dégradée (fichiers tronqués, absents, non numériques) | 2 | 2 | Moyen : fixtures dégradées, replis |
| R6 CLI/usage (codes de sortie, options) | 1 | 2 | Faible : nominaux + une erreur par partition |
| R7 caractéristiques 25010 : fiabilité (watchdog, repli PSI), sécurité (pas d'injection, privilèges bornés), performance (empreinte) | 2 | 2 | Moyen : tests type dédiés + benchmark du 21/11 |

## 3. Modèles de test et éléments de couverture (TD1-TD2)

| Modèle | Technique (29119-4) | Éléments couverts | Cas |
|---|---|---|---|
| M1 domaine YAML | Partitions + syntaxique + aléatoire (graine 20_261_122, 500 entrées) | scalaires/cartes/listes valides ; tabs, ancres, alias, guillemets, blocs, doublons ; 500 entrées aléatoires sans panique | 17 + 2 |
| M2 durées | Limites + supposition d'erreur + métamorphique aller-retour | bornes i64, formes h/m/s/ms, 8 fautes de frappe, 12 aller-retours | 4 |
| M3 config | Fondé sur exigences + partitions | 36 fixtures de refus documentées, défauts, clés inconnues, listes vs scalaires | 32 |
| M4 moteur PSI | Transitions d'états (0-switch et 1-switch) | entrée critique, hystérésis de sortie, figé sous charge, gel de fin de charge, repos | 6 |
| M5 arbitrage compression | Table de décision (4 règles) | swap disque / zram présent / generator binaire+conf / rien | 6 |
| M6 actions | Intégration | webhook 200/201/204/500, refus DNS/port/schéma, exec true/false/timeout/env | 11 |
| M7 collecte | Partitions + limites | /proc tronqué, absents, non numériques, zéros statfs, dédup montage | 20 |
| M8 CPU | Partitions + transitions | gouverneur appliqué/refusé, EPP supporté/absent, démotion, exceptions, restauration | 10 |
| M9 CLI | Partitions argv | codes 0/1/2, formes --config, --once, optimise | 8 |
| M10 réel (CibleReelle) | Intégration environnement | /proc réel, renice soi-même, échec swapon | 7 |

## 4. Niveaux exécutés (TD3-TD4)

| Niveau | Cas | Résultat |
|---|---|---|
| Composant (unitaires) | 177 | 177 ok |
| Intégration (actions HTTP/exec, CLI binaire, cible réelle) | 26 | 26 ok |
| Système (VM Debian 13, cycle pression mémoire complet) | 1 scénario, 2 vérifications | critique ✓, retombée ✓ (après I-1) |
| Statique (clippy deny, fmt, audit RustSec, gate secrets, revue) | 4 contrôles | OK |
| Mutation (analyse manuelle, duration.rs) | 6 mutants | 6/6 tués |

Total : 203 cas automatisés verts + 1 scénario système + 6 mutants tués.
Couverture finale (llvm-cov, seuils 90 en --fail-under) : lignes 94,49 %,
régions 93,55 %, branches 93,75 %.

## 5. Incidents

| ID | Date | Description | Gravité | Cause | Correction + régression |
|---|---|---|---|---|---|
| I-1 | 2026-11-22 14:51 | Fin de charge : total PSI gelé + load1 encore haut → « PSI figé » à tort, `retombée` jamais journalisée, suivi désactivé | 2 (perte de suivi après incident réel) | La garde load1 ≥ 0,5 seule ne prouve pas la charge ; le total gèle légitimement dès que la charge cesse | Le figé n'est déclaré que si le total **n'a jamais bougé depuis le démarrage** (psi=0 ne bouge jamais, même une fois sous charge) ; un total qui a bougé puis gèle n'est pas figé. Tests : `fin_de_charge_gel_ne_declare_pas_fige`, `psi_fige_averti_et_bloque`, `systeme_au_repos_pas_de_fige`. Revalidation VM : critique ✓ retombée ✓ fige=0 |
| I-2 | 2026-11-22 (revue statique) | Clé `cpu:` refusée par l'ancien binaire déployé en VM — le déploiement trainait une version antérieure | 3 (faux négatif de campagne) | binaire périmé en /usr/local/bin | Chaîne de déploiement : construire → copier → installer, vérifiée par empreinte sha256 côté VM |

## 6. Critères de fin et verdict

| Critère (écrit avant exécution) | État |
|---|---|
| Tous les cas verts, sortie fraîche | ✓ 202 ok, 6 suites |
| ≥ 90 % sur chaque couverture rapportée | ✓ 94,49 / 93,55 / 93,75 |
| Aucun incident grave ouvert | ✓ I-1 (gravité 2) corrigé et revalidé ; I-2 processuel, clos |
| Check sécurité OK | ✓ audit 0 advisory, gate secrets no leaks, pas d'injection (Command sans shell, scalaires validés) |
| Système validé en VM | ✓ cycle complet frais du jour |

## 7. Risque résiduel et limites

- La suite verte ne prouve pas l'absence de défaut : le provisionnement en
  environnement réel non Debian (autres init, noyaux sans zram) reste à
  observer en exploitation.
- La recompression zram (`recompress`) n'est pas déclenchée en VM
  (CONFIG_ZRAM_MULTI_COMP non vérifié sur ce noyau) : chemin couvert par
  tests unitaires, pas par le système.
- irqbalance n'est que recommandé : le daemon ne répartit pas les IRQ.
- La mutanalyse est manuelle et ciblée (6 mutants sur le module durées) ;
  une campagne cargo-mutants complète reste possible mais n'est pas
  exécutée (coût de compilation hors proportion).
- L'aléatoire à graine fixe est reproductible mais limité à un profil
  d'octets ; il ne remplace pas les 36 refus syntaxiques documentés.
