# Optimisation CPU (parallélisation, répartition des charges) — v1.3

## Métier

Suite de l'optimiseur v1.2 (PSI, zram). Demande du 2026-11-22 : « CPU :
parallélisation intelligente du cpu, répartition intelligente des charges de
travail pour efficience maximale et stabilité système », meilleures pratiques
recherchées sur internet puis intégrées.

## Recherche (sources)

- Red Hat / TuneD : profils **statiques** recommandés (throughput-performance,
  cpu-partitioning), le **tuning dynamique est désactivé par défaut**
  (overhead et imprévisibilité) — docs RHEL 8, SUSE SLES 15.
- Gouverneurs : `schedutil` moderne, `performance` pour latence serveur
  constante (ArchWiki, CPU frequency scaling) ; intel_pstate en mode actif
  ignore le gouverneur.
- Ordonnanceur : EEVDF noyau 6.6+ (LWN 925371, sched-eevdf.html) équilibre
  la charge par virtual deadlines ; l'**épinglement mono-cœur sort une
  tâche de l'équilibrage** et peut créer des points chauds — best practice :
  masques larges par domaine (L3/NUMA), épinglement fin réservé au
  temps réel avec cœurs isolés (isolcpus/cpusets, docs RHEL RT).
- Graduation des gourmands : `nice +19` garantit une part CPU, SCHED_IDLE
  protège mieux l'interactif mais peut mourir de faim sous charge pleine
  (LWN 805317 « Fixing SCHED_IDLE » : sûr sur noyaux modernes) ; chrt -i 0
  pour les tâches réellement optionnelles.
- cgroup v2 (cgroup-v2.html) : `cpu.weight` (part proportionnelle sous
  contention, défaut 100, 1-10000), `cpu.max` (quota dur),
  `cpu.uclamp.min/max` ; pratique : CPUWeight pour former sous charge,
  CPUQuota pour contenir ; kernel ≥ 5.3 requis.

## Décisions (choix et alternatives)

1. **Pas d'auto-épinglage.** L'ordonnanceur (EEVDF) équilibre mieux que
   n'importe quel réépinglage périodique userspace ; un daemon qui bouge
   des processus entre cœurs combat le noyau et dégrade le cache. La
   « répartition intelligente » se traduit donc par : règles d'affinité
   **admin-déclarées** (masques, idéalement larges), appliquées de façon
   idempotente à des processus nommés explicitement. Alternatives écartées :
   ré-épinglage automatique du top consommateur (anti-pattern), cgroups
   cpuset gérés par le daemon (l'admin a systemd `AllowedCPUs=` pour ça).
2. **Gouverneur en socle statique**, valeur config explicite, défaut = ne
   rien toucher ; pas de chasse dynamique au gouverneur (position Red Hat).
   cpufreq absent (VM qemu) ou intel_pstate actif → dégradation propre.
3. **Identification des gourmands** : comptabilité par processus (delta
   utime+stime de /proc/[pid]/stat entre deux passes, ticks convertis en
   ms) → les alertes nomment le processus et sa part. Observabilité à
   risque nul, condition première de toute décision d'admin.
4. **Dégradation graduée des gourmands, opt-in et bornée** : quand la
   pression CPU critique tient, les processus qui matchent des
   **patterns admin-déclarés** (politesse, jamais d'inconnus, exceptions
   possibles) reçoivent `nice` configurable (défaut +19 ; SCHED_IDLE
   possible via `chrt` en action exec). Défaut global : OFF. Renice
   choisi sur SCHED_IDLE par défaut car nice garantit une part CPU
   (démotions réversibles : la politesse se retire quand la pression
   retombe sous la bande de sortie).
5. **Alertes réutilisées** : seuils PSI cpu (soutenue/critique, durées,
   hystérésis) symétriques de la mémoire, actions réutilisées.
6. **cgroup v2 non écrit par le daemon** : CPUWeight/CPUQuota de l'admin
   passent par systemd ; le daemon documente et journalise, ne s'improvise
   pas gestionnaire de cgroups (tranche future si besoin prouvé).
7. Privilèges : sched_setaffinity et setpriority exigent euid 0 ou la
   propriété du processus — même unité opt root, mode dégradé sinon.
   `unsafe` borné dans src/sys.rs (sched_setaffinity, setpriority),
   commenté SAFETY, comme statfs.

## Changement

- `src/processus.rs` : énumération /proc/[pid]/stat (comm, utime, stime,
  nice), delta entre passes → top N consommateurs.
- Config `cpu:` : gouverneur, affinites (nom → coeurs), seuils pression
  (critique_seuil/duree/sortie, soutenue_seuil/duree), demotion (active,
  nice, patterns, exceptions), actions_critique réutilisées.
- `src/sys.rs` : sched_setaffinity, setpriority (FFI libc borné).
- `src/optimisation.rs` : socle cpu + réponse graduée cpu avec
  hystérésis symétrique.
- VM : pression CPU réelle (stress-ng --cpu), identification du top,
  démotion réelle d'un hog nommé (renice relu), gouverneur absent →
  dégradation, affinité appliquée et relue (taskset -pc).

## Tests

Parsing /proc/[pid]/stat aux contenus hostiles (comm avec parenthèses,
espaces, noyaux), deltas de ticks, conversion jiffies→ms, hystérésis cpu
symétrique, démotion filtrée (pattern match, exceptions, absence),
dégradé sans privilèges. Seuils 90 % maintenus. Acceptation VM : avant/
après nice relu, top consommateur nommé dans le journal, affinité relue.

## Traçabilité

Demande 2026-11-22. Sources citées ; le socle du projet (PSI, unités,
durcissement) est inchangé ; reglage-swap-linux fait foi pour les pièges
PSI (déjà traités en v1.2 : psi=, fige, hystérésis).
