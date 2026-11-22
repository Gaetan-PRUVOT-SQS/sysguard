[English](README.md) | Français

# Cahier de recette — SysGuard v1.4

Recette fonctionnelle du daemon SysGuard (superviseur + optimiseur),
exécutée le 2026-11-22 sur deux environnements :

| Environnement | OS | RAM | Swap | Rôle |
|---|---|---|---|---|
| VM Debian 13 (QEMU/KVM) | 6.12, 4 vCPU | 6 Go | zram (test) | banc de test maîtrisé |
| VPS réel (KVM) | 7.0, 8 vCPU | 16,7 Go | swapfile 8 Go | production |

Chaque cas cite son exigence (spec §), sa procédure, son résultat attendu
et son verdict. Les cas automatisés renvoient à leur test dans le dépôt
(`cargo test` / `make test`). Campagne détaillée :
`docs/campagne-tests-istqb-29119.md`.

## R1 — Supervision et réaction (spec §2)

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-1.1 | Démarrer le service, vérifier son état | `systemctl is-active` = active, Type=notify | ✅ VM + VPS |
| REC-1.2 | Charger la mémoire au-delà du seuil critique tenu 10 s | trace `pression memoire critique` dans le journal | ✅ VM |
| REC-1.3 | Relâcher la charge | trace `retombee`, aucun résidu | ✅ VM |
| REC-1.4 | Consommer le CPU au-delà du seuil | alerte nommant le principal consommateur | ✅ VM |
| REC-1.5 | Vérifier la couverture de tests (lignes/régions) | ≥ 90 % sur chaque axe | ✅ 94,4 / 93,5 |

## R2 — Actions (spec §2.3)

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-2.1 | Action `log` avec variables `{{.Value}}`, `{{.Hostname}}` | message interpolé dans le journal JSON | ✅ |
| REC-2.2 | Action `webhook` vers un récepteur HTTP local | POST JSON avec champs rule/severity/value/timestamp | ✅ VM |
| REC-2.3 | Action `exec` avec variables SG_* | le script reçoit SG_RULE_NAME/SEVERITY/VALUE | ✅ |
| REC-2.4 | `exec` dépassant son timeout | processus tué, erreur journalisée | ✅ |
| REC-2.5 | Config en `dry-run` | évaluée sans exécuter les actions webhook/exec | ✅ |

## R3 — Configuration (spec §4)

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-3.1 | `check` sur les 36 configurations invalides | refus avec message explicite et code de sortie 1 | ✅ |
| REC-3.2 | `check` sur une config valide | « configuration valide », code de sortie 0 | ✅ |
| REC-3.3 | SIGHUP sur le service | « configuration rechargée » | ✅ VM + VPS |
| REC-3.4 | Config YAML hostile (500 entrées aléatoires) | aucune panique | ✅ |

## R4 — Optimiseur mémoire (PSI, zram)

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-4.1 | Pas de swap + `zram_provisionner: oui` (root) | zram créé, mkswap, swapon priorité 100 | ✅ VM |
| REC-4.2 | Swap disque déjà présent | provisionnement refusé (inversion LRU), zones nommées | ✅ VM + VPS |
| REC-4.3 | Après provisionnement, installer zram-generator | config appliquée au boot suivant | ✅ VM |
| REC-4.4 | Sans privilèges | recommandations journalisées, aucune écriture, code de sortie 0 | ✅ VM + VPS |
| REC-4.5 | Swappiness/page-cluster auto avec zram | 133 / 0 appliqués | ✅ VM |
| REC-4.6 | Compteur PSI figé (psi absent), machine sous charge | avertissement + repli, aucune fausse alerte | ✅ VM + VPS |

## R5 — Optimiseur CPU

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-5.1 | Gestionnaire actif (tuned / PPD / auto-cpufreq) | gouverneur et EPP intacts, aucun doublon | ✅ VPS |
| REC-5.2 | Pas de gestionnaire, cpufreq absent | dégradation journalisée | ✅ VM + VPS |
| REC-5.3 | irqbalance absent | recommandation journalisée | ✅ VM + VPS |
| REC-5.4 | Pression CPU critique, gourmand matchant un pattern | relégation nice configurée, restaurée à la retombée | ✅ VM |

## R6 — Intégration systemd

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-6.1 | Démarrage du service Type=notify | READY=1 reçu, service actif < 2 s | ✅ VM + VPS |
| REC-6.2 | Watchdog armé, service actif au-delà de WatchdogSec | aucun kill watchdog | ✅ VM + VPS |
| REC-6.3 | SIGHUP | rechargement ou refus argumenté | ✅ VM + VPS |
| REC-6.4 | `systemd-analyze security` | exposition faible | ✅ 1,9/10 |

## R7 — Empreinte (type performance)

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| REC-7.1 | CPU au repos mesuré par cgroup, fenêtre 120 s | < 1 % | ✅ 0,018 % |
| REC-7.2 | RSS en fonctionnement | < 30 Mo | ✅ ~2,3 Mo |
| REC-7.3 | Latence de détection (charge 4 cœurs) | < 30 s | ✅ 9,1-12,5 s |

## Sécurité

| ID | Procédure | Attendu | Verdict |
|---|---|---|---|
| SEC-1 | Audit des dépendances (RustSec) | 0 alerte | ✅ |
| SEC-2 | Scan de secrets du dépôt | aucune fuite | ✅ |
| SEC-3 | Config hostile refusée sans panique | code de sortie 1, message explicite | ✅ |
| SEC-4 | Exec sans shell, environnement limité aux SG_* | variables invitées invisibles | ✅ |

## Synthèse

- 207 cas automatisés verts + recette système sur 2 environnements.
- 0 incident grave ouvert ; les incidents de campagne sont corrigés avec
  leurs régressions (voir `docs/campagne-tests-istqb-29119.md`).
- Risque résiduel documenté dans le rapport de campagne.
