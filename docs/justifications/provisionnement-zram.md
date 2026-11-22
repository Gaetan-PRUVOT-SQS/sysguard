# Provisionnement zram et arbitrage des doublons de compression — v1.4

## Métier

Demande du 2026-11-22 : « la solution doit compléter ce que le noyau ne fait
pas, zram et compression ram si on l'a pas, apporter des solutions qu'on n'a
pas en natif de manière intelligente et conserver le plus performant en cas
de doublon ».

## Recherche (sources)

- zram-generator (systemd) : provisionne zram **au boot** via
  `/etc/systemd/zram-generator.conf` — `zram-size` (ex. `min(ram / 2, 4096)`),
  `compression-algorithm`, `swap-priority` ; persistance automatique, pas de
  swapon manuel (ArchWiki Zram, zram-generator.conf(5)).
- Chris Down (mainteneur noyau zswap, mars 2026, « Debunking zswap and zram
  myths ») : **zswap s'interpose devant un swap disque ; zram EST le swap**.
  Préférer zswap quand un disque peut servir de swap. **zram + swap disque =
  inversion LRU** (pages chaudes en zram, froides expulsées vers disque,
  ordre d'ancienneté inversé) — combinaison à éviter.
- ArchWiki + consensus : zram pour les machines **sans swap disque** (pas
  d'usure SSD, faible RAM) ; dans ce cas **désactiver zswap** (double
  compression coûteuse pour rien) — `zswap.enabled=0` ou
  /sys/module/zswap/parameters/enabled.
- Taille : `min(ram / 2, 4 G)` (défaut zram-generator) ; ratio réel ~3:1 à
  5:1 avec zstd ; algorithme zstd si CPU moderne, lz4 si CPU contraint.

## Décisions (choix et alternatives)

1. **Matrice d'arbitrage** (socle mémoire, avant tout réglage) :
   - swap disque présent → **ne pas provisionner zram** (inversion LRU),
     état journalisé ; le plus performant ici est le setup de la distro
     (swap disque + zswap éventuel) ;
   - zram déjà présent → ne pas dupliquer : vérifier algorithme/taille vs
     cibles, ajuster si configuré (déjà fait en v1.2) ;
   - zram-generator installé **avec** config → il gère au boot, le daemon
     ne provisionne pas au runtime (pas de doublon) ;
   - aucun swap + pas de generator configuré → provisionnement **opt-in**
     (`provisionner: oui`) : modprobe zram, hot_add si besoin, algorithme,
     disksize, mkswap, swapon priorité 100, puis écriture de
     zram-generator.conf pour la persistance au boot suivant (le generator
     reprend la main sans conflit : le device RAM disparaît au reboot).
2. **zswap en présence de zram** : la double compression coûte CPU et
   mémoire sans bénéfice (ArchWiki + Chris Down). Nouvelle clé
   `zswap: desactive` (défaut `garde`) : écrit 0 dans
   /sys/module/zswap/parameters/enabled ; l'inverse (garder zswap) reste
   le bon choix quand le swap disque existe.
3. **Taille et algorithme « intelligents »** : `taille: auto` =
   min(ram/2, 4 G) ; `algorithme: auto` = zstd si ≥ 4 cœurs, lz4 sinon
   (consensus 2025). Explicitables.
4. **Pas de kill, pas de surprise** : le provisionnement change l'état
   système (ajoute du swap) → opt-in explicite en config, chaque étape
   journalisée, échec → dégradation propre (jamais de configuration
   à moitié posée sans trace : mkswap/swapon en échec → avertissement
   et rapport).
5. Repli : module zram absent (conteneur, noyau minimal) → recommandation
   journalisée, pas d'erreur fatale.

## Changement

- `src/compression.rs` : état de compression (zones swap typées, zram
  présent, generator installé/configuré, zswap) + détection via trait.
- Config `optimisation.zram` enrichie : `provisionner`, `taille` (auto/2G),
  `algorithme: auto`.
- `src/optimisation.rs` : matrice d'arbitrage au socle, provisionnement
  (commandes modprobe/mkswap via trait, swapon libc borné, hot_add,
  écriture zram-generator.conf), désactivation zswap optionnelle.
- VM : machine sans swap → provisionnement complet end-to-end ; cas
  swap disque présent → refus argumenté ; cas generator → non-duplication.

## Tests

Parsing mm_stat/swaps déjà en place ; nouveaux : matrice d'arbitrage
(fausses cibles : zram présent, generator configuré, swap disque, vide),
provisionnement simulé (commandes enregistrées dans l'ordre, config
generator écrite, priorité 100), échecs partiels (mkswap en échec →
avertissement sans état faux), dégradation sans module. Seuils 90 %
maintenus.

## Traçabilité

Demande 2026-11-22. Sources citées ; la v1.2 (santé zram, seuils PSI,
hystérésis) est inchangée ; l'avertissement zswap se nuance (il ne vise
que le cas zram actif).
