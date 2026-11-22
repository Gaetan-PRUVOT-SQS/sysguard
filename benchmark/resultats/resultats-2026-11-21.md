# Résultats bruts du benchmark — 2026-11-21, VM Debian 13 (4 vCPU, 6 144 Mo)

Protocole identique au 20/11 : chaque candidat isolé dans une unité systemd
(transiente ou service), CPU lu par cgroup (cpu.stat usage_usec, delta sur
la fenêtre), mémoire par memory.current/memory.peak du cgroup et RSS
process ; latence = charge de 4 boucles occupées (unité `bench-charge`,
`benchmark/charge.sh`) → première trace de chaque candidat, deux essais.
SysGuard tourne avec `vm/config-vm.yaml` (tick 5 s, règles vm-cpu/vm-mem/
vm-disque, actions log + exec + webhook), le script natif avec les mêmes
règles en shell POSIX (`benchmark/veille-natif.sh`, tick 5 s).

## veille-natif.sh (fenêtre 120 s)

```
cpu_ms=194 sur 120s
cpu_pct=0.162
mem_current_ko=436
mem_anon_ko=244
mem_peak_ko=1796
etat=actif
RSS process : 1852 Ko
log : 186 octets, 6 lignes (cooldown 25s)
latence (2 essais) : 8524 ms, 8903 ms
```

## vmstat 5 (procps-ng, fenêtre 120 s)

```
cpu_ms=8 sur 120s
cpu_pct=0.007
mem_current_ko=448
mem_anon_ko=292
mem_peak_ko=1840
etat=actif
sortie : 2461 octets
```

## sar 5 30 (sysstat 12.7.5, fenêtre 125 s)

```
cpu_ms=11 sur 125s
cpu_pct=0.009
mem_current_ko=924
mem_anon_ko=384
mem_peak_ko=1840
etat=actif
sortie : 2156 octets
```

## sysguard (Rust, service, fenêtre 120 s)

```
cpu_ms=21 sur 120s
cpu_pct=0.018
mem_current_ko=496
mem_anon_ko=164
mem_peak_ko=2384
etat=actif
RSS process : 2356 Ko
journal : 915 octets, 5 déclenchements vm-mem sur 122 s
latence (2 essais) : 12531 ms, 9050 ms (message « CPU au-dessus de 50%
pendant 5s : 100% » puis 99.95%)
```

## Lecture

- Contre le script natif de même rôle : SysGuard Rust coûte 9 fois moins
  de CPU au repos (21 ms contre 194 ms par fenêtre de 120 s — le script
  paie un fork/exec de awk et date par tick), mémoire cgroup comparable
  (496 Ko contre 436 Ko), RSS supérieure (2356 Ko contre 1852 Ko, le
  code exécutable est plus grand), latence du même ordre (9 à 12,5 s
  contre 8,5 à 8,9 s, l'écart est dans la phase des ticks).
- vmstat et sar (collecte seule, sans règles ni actions) restent les plus
  légers en CPU : 8 et 11 ms. SysGuard en fait 2 à 3 fois plus mais
  couvre règles, cooldown, actions avec timeout et intégration systemd.
- Contre SysGuard Go du 20/11 (32 ms, 16,7 Mo cgroup, RSS 15,1 Mo,
  latence 15 s) : CPU -34 %, mémoire cgroup -97 %, RSS -84 %, latence
  -15 à -40 %.
- Anecdote de mise au point : le premier essai de latence était nul —
  la charge `bench-charge` échouait à l'ouverture (« Syntax error » :
  `(while :; do :)` sans `done` dans le sous-shell, quoting cassé à
  travers le heredoc ssh). Charge corrigée dans `benchmark/charge.sh`,
  sonde dédiée dans `benchmark/latence.sh`, deux essais propre mesurés.
