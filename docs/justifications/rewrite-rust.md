# Portage de SysGuard en Rust (v1.1)

## Métier

SysGuard supervise les ressources d'un serveur Linux (CPU, mémoire, swap,
load, disques) et déclenche des actions configurables (log, webhook, exec)
quand des règles à durée et cooldown sont violées. Le projet a connu deux
implémentations antérieures (livrées et validées en VM les 2026-11-20 et
2026-11-21), retirées du dépôt après le pivot Rust et sauvegardées hors
dépôt. Demande du 2026-11-21 : « on le passe intégralement en rust ». Un
seul langage livré.

## Changement

La ligne principale devient une crate Rust unique à la racine (édition 2024,
`Cargo.toml`, `src/`). Même CLI, mêmes messages d'erreur (sans accents),
mêmes sémantiques : for en durée réelle monotone, cooldown compté depuis le
dernier tir avec re-tir à chaque expiration, tick immédiat au démarrage,
montages en échec omis du snapshot, toute erreur /proc abandonne le tick
sans évaluation, actions log exécutées même en dry-run, exécution sans
shell avec seules variables SG_*.

Corrections portées par le portage (défauts découverts pendant le chantier,
conformes à la spec) :

- le message du webhook est désormais passé au template (`{{.Value}}`
  arrivait littéral chez le récepteur) — vu en VM le 2026-11-21 ;
- les options seules (`sysguard --config X` sans commande) impliquent
  `run` comme la spec §5 — exiger une commande explicite faisait échouer
  l'unit systemd ; découvert au premier déploiement VM du portage ;
- la validation croisée des champs d'action est conservée : une `url` sur
  exec ou une `command` sur log est refusée avec un message explicite
  (2 refus supplémentaires face à l'implémentation initiale, assumés) ;
- la garde `dIdle <= 0` du calcul CPU est retirée : sous saturation pleine,
  le delta idle est exactement 0 et le calcul rapportait 0 % — une règle
  `cpu.usage > x` ne se déclenchait jamais. Vu au benchmark de latence du
  portage (21:04:23, 2026-11-21), régression `stat-sature` en place.

## Choix et alternatives

- Zéro dépendance applicative : parseur YAML en sous-ensemble strict,
  client HTTP/1.1 maison, sd_notify via `UnixDatagram` — mêmes choix que
  l'implémentation antérieure pour la parité et le plancher mémoire.
  serde_yaml est abandonné en amont (RustSec) et aurait tiré serde+libyaml.
- Deux dépendances seulement : `libc` (statfs(2), absent de std) et
  `signal-hook` (pose de handlers sans réécrire les pièges async-signal-safe).
  Le reste de la crate est `deny(unsafe_code)` ; l'unique `unsafe` est
  l'appel statfs commenté `// SAFETY:` dans `src/sys.rs`.
- Identifiants anglais (convention Rust), textes utilisateurs français,
  messages d'erreur inchangés pour la continuité d'exploitation.
- Horloge du moteur injectée (`Box<dyn Fn() -> i64>`) : les tests pilotent
  le temps sans statiques partagées.
- Config passée par paramètre à `Collecteur::tick` plutôt que stockée : le
  SIGHUP remplace la config sans conflit d'emprunt, les états du moteur
  persistent tant que le nombre de règles ne change pas.
- Attente par tranches de 50 ms (boucle et timeout d'exec) : `thread::sleep`
  n'est pas interruptible par signal, les tranches donnent la réactivité
  d'un `nanosleep` interrompu pour ~20 réveils/s négligeables.
- Limites fixes de la version antérieure levées (message 512 o, corps
  webhook 2048 o) : en Rust les allocations sont sûres et bornées par la
  config validée ; les messages longs ne sont plus tronqués silencieusement.
- `%g` remplacé par l'écriture à plus courte représentation de Rust (les
  gros nombres ne passent plus en notation exponentielle à la printf).
- `_exit(127)` et le message `exec impossible` remplacés par
  `Command::spawn` : l'échec d'exec remonte en erreur explicite avec le
  même texte.
- Pas de Miri : l'unique bloc unsafe appelle statfs(2), non modélisé par
  Miri ; la propriété mémoire est portée par les types (pas d'arène à
  réécrire), les échecs d'allocation restent abort (comportement std).

## Tests

Stratégie fondée sur le risque (iso-29119, conformité adaptée) : les
risques portent sur la validation stricte de la config (frontière
utilisateur), les sémantiques du moteur (for, cooldown, fenêtre), la
lecture /proc aux contenus dégradés et les actions aux échecs réseau/exec.
Techniques : partitions (36 fixtures de refus, portées avec leur fragment
de message), valeurs limites (durées i64, tailles de fichiers, ports,
échappements), transitions d'états du moteur (fenêtre, cooldown, re-tir,
survie de la fenêtre aux ticks en erreur), serveur webhook réel sur
loopback, exécutions réelles (`/bin/true`, `/bin/false`, `/bin/sleep`,
scripts SG_*).

Résultats du 2026-11-21 (sorties fraîches de la session) :

- `make test` : 136 cas verts (113 unitaires, 11 actions, 6 CLI, 6
  daemon/notify en plus des internes), 0 KO.
- Couverture `make couverture` : lignes 96,97 %, régions 95,18 %, seuils
  90 % fixés avant campagne en `--fail-under-*`.
- `make lint` (clippy deny) et `cargo fmt --check` propres ;
  `cargo audit` : 0 advisory sur les 7 crates du lock.
- Validation VM Debian 13 (protocole du 20/11) : service actif, RSS
  2,1 Mo (contre 13,8 Mo avant portage), webhook JSON templated reçu sur
  l'hôte, SIGHUP « configuration rechargée » + nouvelle règle active au
  tick suivant, uptime > 60 s sans kill watchdog, exposition systemd-analyze
  1,9/10, démarrage notify < 2 s.
- Benchmark VM du 2026-11-21 (config tick 2 s, fenêtre 127 s ramenée à
  120 s) : CPU cgroup 26 ms (0,022 %), RSS 2 288 Ko, mémoire cgroup
  480 Ko, latence de détection à 4 vCPU saturés 7,0 s (détection
  21:04:23.795, charge lancée 21:04:16.811). Détail dans docs/etat.md.

## Traçabilité

- Demande : 2026-11-21, « Reprend le projet Sysguard, on le passe
  intégralement en rust ».
- Baseline : docs/etat.md (phases antérieures), benchmark du 20/11.
- Sémantiques : reprises de l'implémentation initiale et de la spec
  (états antérieurs sauvegardés hors dépôt).
- Critères de fin : les cibles initiales (-30 % vs natifs) restent la
  référence ; les mesures Rust du benchmark sont consignées dans
  docs/etat.md, toute cible inatteignable se renégocie avec les chiffres.
