# Optimisation continue du système (PSI, zram, tuning mémoire) — v1.2

## Métier

SysGuard supervise (CPU, mémoire, disques, PSI) et réagit par règles. Demande
du 2026-11-21 : « le daemon doit optimiser en continue le système, cpu, ram
(zram et compression intelligente intégré au daemon) », meilleures pratiques
recherchées sur internet. Le daemon passe d'un rôle d'observation à un rôle
d'ajustement, sur des mécanismes noyau documentés.

## Recherche (sources)

- Doc noyau : sysctl/vm.rst (swappiness 0-200 depuis 5.8 ; 133 = swap deux
  fois plus rapide que le système de fichiers, cas zram ; page-cluster),
  accounting/psi.html (some/full, avg10/60/300, triggers poll),
  admin-guide/blockdev/zram.html (comp_algorithm, disksize, mm_stat,
  recompress + recomp_algorithm avec CONFIG_ZRAM_MULTI_COMP depuis 6.2,
  max_comp_streams obsolète depuis 4.7), multigen_lru.rst.
- zram-generator : taille par défaut min(ram/2, 4 G), prio 100, zstd par
  défaut sur noyaux récents ; disksize est une capacité non compressée, la
  RAM tenue vaut ~1/3 à 1/5 (ratios zstd 3:1 à 5:1, lz4 ~2.6:1).
- Consensus 2025 : zstd par défaut sur matériel moderne (40 % mieux que
  lz4), lz4 si CPU contraint ; disksize 50-100 % de la RAM ; couple
  page-cluster 0 + swappiness 100-180 avec zram.
- Démonstrations de référence : systemd-oomd (PSI + cgroups v2, action à
  60 % de full avg10 tenus 30 s), earlyoom (poll /proc simple, tue le plus
  gros), nohang (PSI + corrections graduées avant tuer), psi-notify et
  bustd (trigger PSI + epoll : coût CPU quasi nul quand le système est
  sain), sd_event_add_memory_pressure côté systemd.

## Décisions (choix et alternatives)

1. Signaux : PSI mémoire/CPU lus dans /proc/pressure (lecture non
   privilégiée, une lecture rafraîchit les moyennes) + MemAvailable déjà
   collecté + /proc/swaps et mm_stat pour la santé zram (ratio réel =
   orig/compr). Alternative polling haut rythme (earlyoom, 10 Hz) :
   écartée, le daemon tick déjà ; trigger poll() PSI réservé à une tranche
   suivante (nécessite epoll + fd dédié).
2. Socle statique d'abord, dynamique ensuite : au démarrage, valeurs
   justifiées fixes (swappiness, page-cluster 0, algorithme zram si root),
   idempotent et journalisé. Pas de poursuite continue de swappiness par la
   pression : la doc du noyau ne l'appuie pas et le skill poste l'interdit
   sans mesure avant/après ; l'« intelligence » va dans les seuils PSI à
   hystérésis et dans la recompression zram, pas dans du folklore sysctl.
3. Pression mémoire graduée (style nohang, jamais de kill automatique v1) :
   - soutenue (some avg10 ≥ seuil modéré tenu) → monotone : log
     recommandation ;
   - critique (full avg10 ≥ 60 % tenu 30 s, défaut systemd-oomd) →
     alerte + actions configurées (log/webhook/exec réutilisés) ;
   - hystérésis de sortie décalée (40 %) contre l'oscillation due à la
     décroissance lente des moyennes PSI (~25 s).
   Tuer un processus reste une action exec explicite de l'admin.
4. zram « compression intelligente » : santé continue = ratio orig/compr,
   usage vs disksize ; recompression des pages idle/huge vers un second
   algorithme si le noyau expose recomp_algorithm (6.2+, CONFIG_ZRAM_
   MULTI_COMP), déclenchée à pression soutenue, bornée par une période
   minimale. Redimensionnement à chaud interdit : reset + disksize détruit
   le swap actif ; le daemon vérifie la taille au démarrage (cible
   min(ram/2, 4 G) comme zram-generator) et recommande sinon.
5. Privilèges : /proc/sys s'écrit avec euid 0 (pas besoin de
   CAP_SYS_ADMIN) ; sysfs zram exige root. Le superviseur principal reste
   durci (1,9/10, ProtectKernelTunables rend /proc/sys read-only) : la
   seconde unité `sysguard-opt.service` porte l'optimiseur, root, sans
   réseau (AF_UNIX seulement), sans ProtectKernelTunables, ReadWritePaths
   bornés. Sans privilèges, l'optimiseur dégrade en recommandations
   journalisées, jamais d'échec.
6. CPU : PSI cpu some avg10 tenu → alerte configurée. Pas de changement de
   gouverneur automatique (workload-dépendant, hors périmètre serveur) :
   reste possible via action exec de l'admin.
7. Repli : PSI absent (CONFIG_PSI off, psi= boot) → repli MemAvailable +
   débit swap, journaillé une fois. zram absent → voile zram inactif.
   zswap résiduel actif → avertissement (piège documenté ArchWiki).

## Changement

- `src/pression.rs` : parse /proc/pressure/{memory,cpu,io} (fixtures),
  /proc/swaps, /sys/block/zram0/mm_stat + comp_algorithm (fixtures).
- `src/optimisation.rs` : état (hystérésis, périodes minimales), socle
  statique idempotent, graduations, écritures via trait `CibleReglage`
  (fs réelle + faux pour tests).
- Config : section `optimisation:` (strict, refus inconnus), actions
  réutilisées.
- `packaging/sysguard-opt.service` : unité durcie dédiée.
- VM : validation sur Debian 13 (noyau 6.12), scénario charge mémoire →
  pression → actions → hystérésis, avant/après mesuré.

## Tests

Stratégie par le risque : parsing /proc aux contenus dégradés (absent,
tronqué, PSI désactivé), machine à états d'hystérésis (entrée/sortie,
rebonds), idempotence du socle statique, replis sans privilèges ni zram.
Seuils 90 % lignes/régions maintenus. Acceptation VM : fenêtre avant/après
de /proc/pressure et /proc/swaps, scénario de charge mémoire reproduit.

## Ce que la VM a corrigé (2026-11-21)

- `actions_critique` (liste YAML) était refusée à tort : le contrôle
  « valeur scalaire attendue » s'appliquait avant le dispatch. Corrigé,
  test de liste acceptée et de scalaire refusé en place.
- L'image Debian cloud boot sans `psi=` : les fichiers /proc/pressure
  existent mais le compteur `total` reste immobile sous charge réelle
  (stress-ng 9 G) — PSI figé détecté par le daemon (deux lectures du
  total) → avertissement + repli, test en place. Validation complète
  faite après activation `psi=1` dans grub.
- `comp_algorithm` à chaud sur un device actif : EBUSY noyau (6.12) —
  l'algorithme se règle à la mise en service ; message dédié pointant
  zram-generator au lieu du générique « privilèges ».
- stress-ng 0.19 : `--vm-method write-zero` n'existe pas (zero-one) —
  deux essais de validation perdus à cause d'une sortie jetée ; les
  suivants montrent la charge réellement appliquée (MemAvailable
  5,4 → 1,9 Go, PSI some 47,6 %).

## Mesure d'acceptation (cycle complet en VM, psi=1)

stress-ng 9 G sur 6 G avec swapfile 2 G : PSI some 47,6 % → critique
détectée à « 32.2% tenu 10000ms » (seuil 5 %, durée 10 s de la config de
preuve) → action templated exécutée → décroissance PSI tenue en hystérésis
(aucune retombée prématurée) → « retombee (1.7%) » sous la bande de
sortie (2 %). Socle : swappiness 60 → 150 (config explicite) puis 133 +
page-cluster 0 (auto + zram), relu dans /proc/sys.

## Traçabilité

Demande 2026-11-21 (même tour que le benchmark). Sources citées plus haut ;
le skill poste reglage-swap-linux fait foi pour les valeurs et pièges
(133 pour zram, page-cluster 0 = pratique distro, hystérésis, PEP 475
équivalent Rust : sommeil par tranches déjà en place).
