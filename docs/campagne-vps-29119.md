# Campagne système VPS — ISTQB / ISO 29119 — 2026-11-22

Suite système de la campagne du même jour, exécutée sur le **VPS perso
réel** (Ubuntu 26.04.1 LTS, noyau 7.0.0-43-generic, 8 vCPU, 16,7 Gio RAM,
swapfile 8 Gio priorité -1, systemd 259, glibc 2.43, PSI compteur figé
— psi absent du cmdline) : environnement radicalement différent de la VM
Debian (16 G vs 6 G RAM, swapfile vs zram, cpufreq virtuel en lecture
seule, pas de /dev/kvm).

## Déroulé et résultats (sortie fraîche de session)

| Cas | Vérifié | Résultat |
|---|---|---|
| V1 reconnaissance | OS, kernel, RAM, swap, PSI, cpufreq, gestionnaires, generator, glibc, systemd | fait (détails ci-dessous) |
| V2 déploiement | binaire + `check` config | « configuration valide : 0 regles » |
| V3 dégradé sans privilèges (runuser nobody) | recommandations journalisées, code 0, **aucune écriture** | ✓ |
| V4 arbitrage root, swap disque présent | « swap deja present (/swapfile (disque)), provisionnement zram non applique (swap disque + zram = inversion LRU) », swappiness 60 / zswap N / zram 0 intacts | ✓ branche de refus en conditions réelles |
| V6 service superviseur | utilisateur sysguard, unité Type=notify, WatchdogSec=60, actif, SIGHUP | ✓ actif |
| V7 arbitrage CPU | gouverneur refusé par les 8 cœurs (cpufreq virtuel en lecture seule sur le VPS), irqbalance absent → recommandation | ✓ dégradation exacte |
| V8 PSI figé détecté sous charge | stress-ng 20 G sur 16 G → « PSI memoire fige sous charge » journalisé une fois | ✓ après I-2 |
| V9 service restauré | sysguard actif après les tests | ✓ |

## Incidents

| ID | Description | Cause | Correction |
|---|---|---|---|
| I-2 | Le suivi PSI n'a jamais déclaré figé sur le VPS (compteur gelé depuis le boot) : la branche « total égal ou charge faible » posait `psi_a_bouge = true` au premier pas au calme, tuant la détection définitivement | sémantique du drapeau fausse : « a bougé » doit signifier total **différent**, pas « pas comparable » | Sémantique corrigée dans pas() et pas_cpu(), tests unitaires maintenus verts, démonstration sur VPS réel sous charge (fige = 1) |
| I-3 | Déploiements trainant des binaires périmés (VM : clé `cpu:` refusée ; VPS : ETXTBSY sur binaire en course) | chaîne construire→copier→installer interrompue | chaîne réexécutée dans l'ordre, vérifiée par empreinte |

## Prérequis de déploiement VPS identifiés

- `psi=1` au boot : les fichiers /proc/pressure existent mais les compteurs
  ne bougent jamais (psi absent du cmdline, Ubuntu serveur). L'optimiseur
  détecte ce cas sous charge et bascule en repli journalisé — le
  superviseur, lui, fonctionne sans PSI.
- Gouverneur : le VPS expose cpufreq en lecture seule (virtualisé) — les
  écritures sont refusées par les 8 cœurs, dégradation journalisée.
- zswap inactif + swapfile : configuration saine selon la référence
  Chris Down (swap disque), aucun doublon à arbitrer.

## Verdict

Go pour le déploiement superviseur sur le VPS (actif, durci, surveillé).
L'optimiseur reste en repli jusqu'à `psi=1` + reboot (prérequis
documenté). Risque résiduel : la branche hot_add du provisionnement n'est
pas exercée sur ce VPS (swap disque présent → arbitrage refuse, attendu).
