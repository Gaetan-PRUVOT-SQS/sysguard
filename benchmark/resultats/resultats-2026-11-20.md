# Résultats bruts du benchmark — 2026-11-20, VM Debian 13 (4 vCPU, 6 144 Mo)

Méthode : chaque candidat isolé dans une unité systemd (transiente ou service),
CPU lu par cgroup (cpu.stat usage_usec, delta sur fenêtre), mémoire par
memory.current/memory.peak du cgroup et RSS process ; latence = charge de 4
boucles occupées → première trace de détection (horodatage VM, précision ±1 s
pour le script natif, ±0,5 s d'interrogation pour les deux).

## veille-natif.sh (fenêtre 120 s)

```
cpu_ms=234 sur 120s
cpu_pct=0.195
mem_current_ko=428
mem_anon_ko=244
mem_peak_ko=1844
etat=actif
RSS process : 1760 Ko
log : 155 octets, 5 lignes (cooldown 25s)
latence détection cpu>50% (for 5s, cooldown 20s) : 12 s
```

## vmstat 5 (procps-ng, fenêtre 120 s)

```
cpu_ms=8 sur 120s
cpu_pct=0.007
mem_current_ko=404
mem_anon_ko=292
mem_peak_ko=1848
etat=actif
sortie : 2484 octets
```

## sar 5 30 (sysstat 12.7.5, fenêtre 125 s)

```
cpu_ms=13 sur 125s
cpu_pct=0.011
mem_current_ko=580
mem_anon_ko=384
mem_peak_ko=1820
etat=actif
sortie : 2095 octets
```

## sysguard (service, fenêtre 120 s)

```
cpu_ms=32 sur 120s
cpu_pct=0.027
mem_current_ko=16740
mem_anon_ko=7808 (plus anon_thp ~6144 Ko)
mem_peak_ko=19160
etat=actif
RSS process : 15100 Ko
journal : 1977 octets, 8 lignes sur 122s (vm-mem : log + exec + webhook)
latence détection vm-cpu (cpu>50%, for 5s, cooldown 20s) : 15 s
ligne de déclenchement : valeur 100, seuil 50 (18:31:21.98)
```

## Démarrage

sysguard : 4 ms entre `systemctl restart` et état active (Type=notify, l'état
active attend READY=1, donc collecte et notification incluses). Les candidats
shell sont des exec simples (démarrage sub-milliseconde, non mesuré
séparément).

## Notes de protocole

- Unités transientes du benchmark : la première lecture du cgroup d'une unité
  `systemd-run` toute jeune a échoué de façon intermittente (ENOENT alors que
  l'unité était active) ; bench-mesure.sh attend maintenant la lisibilité de
  cpu.stat (30 s max) — aucune attente observée sur les fenêtres retenues.
- Le candidat natif inclut un spawn awk + date par tick (5 s) ; SysGuard ne
  spawn rien par tick hors actions.
- vmstat et sar ne font que collecter : pas de règles, pas d'actions, pas de
  latence à comparer.
- Deux incidents de protocole corrigés en cours de campagne (ligne
  `journalctl --since` en epoch nu refusée — préfixe `@` ; candidat natif
  installé avec extension .sh non appelable). La fenêtre native finale et la
  fenêtre SysGuard sont comparables telles quelles.
