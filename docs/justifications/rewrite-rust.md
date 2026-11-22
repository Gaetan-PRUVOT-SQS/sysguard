# Portage de SysGuard en Rust (v1.1)

## Métier

SysGuard supervise les ressources d'un serveur Linux (CPU, mémoire, swap,
load, disques) et déclenche des actions configurables (log, webhook, exec)
quand des règles à durée et cooldown sont violées. Le projet a été livré en
Go (v0.1, validé en VM le 2026-11-20) puis engagé en C (T1-T6, arrêté en
cours de tests le 2026-11-21). Demande du 2026-11-21 : « on le passe
intégralement en rust ». Un seul langage livré, les sources Go et C restent
dans le dépôt comme oraux et héritage (Go : différentiel de comportement ;
C : référence des sémantiques).

## Changement

La ligne principale devient une crate Rust unique à la racine (édition 2024,
`Cargo.toml`, `src/`), qui remplace les deux lignes. Même CLI, mêmes
messages d'erreur (hérités du C, sans accents), mêmes sémantiques : for en
durée réelle monotone, cooldown compté depuis le dernier tir avec re-tir à
chaque expiration, tick immédiat au démarrage, montages en échec omis du
snapshot, toute erreur /proc abandonne le tick sans évaluation, actions log
exécutées même en dry-run, exécution sans shell avec seules variables SG_*.

Corrections portées par le portage (absentes ou fausses en C, conformes au
Go et à la spec) :

- le message du webhook est désormais passé au template (le Go l'appliquait,
  le C envoyait `{{.Value}}` littéral) — vu en VM le 2026-11-21 ;
- les options seules (`sysguard --config X` sans commande) impliquent `run`
  comme la spec §5 et l'oracle Go — le C exigeait une commande explicite et
  l'unit systemd échouait ; découvert au premier déploiement VM du portage ;
- la validation croisée des champs d'action (durcissement du C) est
  conservée : le Go acceptait `url` sur exec et `command` sur log en les
  ignorant ; le différentiel `scripts/differentiel.sh` documente et vérifie
  cet écart assumé (2 cas sur 47 alignés) ;
- la garde `dIdle <= 0` du calcul CPU (héritée du C) est retirée au profit
  de la formule du Go : sous saturation pleine, le delta idle est
  exactement 0 et le C rapportait 0 % — une règle `cpu.usage > x` ne se
  déclenchait jamais. Vu au benchmark de latence du portage (21:04:23,
  2026-11-21), régression `stat-sature` en place.

## Choix et alternatives

- Zéro dépendance applicative : parseur YAML en sous-ensemble strict,
  client HTTP/1.1 maison, sd_notify via `UnixDatagram` — mêmes choix que le
  C pour la parité et le plancher mémoire. serde_yaml est abandonné en amont
  (RustSec) et aurait tiré serde+libyaml.
- Deux dépendances seulement : `libc` (statfs(2), absent de std) et
  `signal-hook` (pose de handlers sans réécrire les pièges async-signal-safe).
  Le reste de la crate est `deny(unsafe_code)` ; l'unique `unsafe` est
  l'appel statfs commenté `// SAFETY:` dans `src/sys.rs`.
- Identifiants anglais (convention Rust), textes utilisateurs français,
  messages d'erreur identiques au C pour la continuité d'exploitation.
- Horloge du moteur injectée (`Box<dyn Fn() -> i64>`) au lieu d'un pointeur
  de fonction C : les tests pilotent le temps sans statiques partagées.
- Config passée par paramètre à `Collecteur::tick` plutôt que stockée : le
  SIGHUP remplace la config sans conflit d'emprunt, les états du moteur
  persistent tant que le nombre de règles ne change pas (parité C).
- Attente par tranches de 50 ms (boucle et timeout d'exec) : `thread::sleep`
  n'est pas interruptible par signal, les tranches donnent la réactivité du
  `nanosleep` interrompu du C pour ~20 réveils/s négligeables.
- Caps C (message 512 o, corps webhook 2048 o) levés : en Rust les
  allocations sont sûres et bornées par la config validée ; les messages
  longs ne sont plus tronqués silencieusement.
- `%g` du C remplacé par l'écriture à plus courte représentation de Rust,
  comme le Go (les gros nombres ne passent plus en exponentielle C).
- `_exit(127)` et `exec impossible` du C remplacés par `Command::spawn` :
  l'échec d'exec remonte en erreur explicite avec le même texte.
- Pas de Miri : l'unique bloc unsafe appelle statfs(2), non modélisé par
  Miri ; la propriété mémoire est portée par les types (pas d'arène à
  réécrire), les échecs d'allocation restent abort (comportement std).

## Tests

Stratégie fondée sur le risque (iso-29119, conformité adaptée) : les
risques portent sur la validation stricte de la config (frontière
utilisateur), les sémantiques du moteur (for, cooldown, fenêtre), la
lecture /proc aux contenus dégradés et les actions aux échecs réseau/exec.
Techniques : partitions (36 fixtures de refus du C, portées avec leur
fragment de message), valeurs limites (durées i64, tailles de fichiers,
ports, échappements), transitions d'états du moteur (fenêtre, cooldown,
re-tir, survie de la fenêtre aux ticks en erreur), différentiel contre
l'oracle Go (47 cas), serveur webhook réel sur loopback, exécutions
réelles (`/bin/true`, `/bin/false`, `/bin/sleep`, scripts SG_*).

Résultats du 2026-11-21 (sorties fraîches de la session) :

- `make test` : 136 cas verts (113 unitaires, 11 actions, 6 CLI, 6
  daemon/notify en plus des internes), 0 KO.
- Couverture `make couverture` : lignes 96,97 %, régions 95,18 %, seuils
  90 % fixés avant campagne en `--fail-under-*`.
- `make lint` (clippy deny) et `cargo fmt --check` propres ;
  `cargo audit` : 0 advisory sur les 7 crates du lock.
- Différentiel oracle : `scripts/differentiel.sh`, 47 cas alignés
  (codes de sortie check sur toutes les fixtures, run --once), 2 écarts
  assumés du durcissement C vérifiés à part.
- Validation VM Debian 13 (protocole du 20/11) : service actif, RSS
  2,1 Mo contre 13,8 Mo en Go, webhook JSON templated reçu sur l'hôte,
  SIGHUP « configuration rechargée » + nouvelle règle active au tick
  suivant, uptime > 60 s sans kill watchdog, exposition systemd-analyze
  1,9/10, démarrage notify < 2 s.
- Benchmark VM du 2026-11-21 (config tick 2 s, fenêtre 127 s ramenée à
  120 s) : CPU cgroup 26 ms (0,022 %), RSS 2 288 Ko, mémoire cgroup
  480 Ko, latence de détection à 4 vCPU saturés 7,0 s (détection
  21:04:23.795, charge lancée 21:04:16.811). Détail dans docs/etat.md.

## Traçabilité

- Demande : 2026-11-21, « Reprend le projet Sysguard, on le passe
  intégralement en rust ».
- Baseline : docs/etat.md (v0.1 Go + réécriture C), benchmark du 20/11.
- Sémantiques : sources C de `heritage/c` (T1-T6) et Go de `internal/`,
  différentiel machine contre le binaire Go.
- Critères de fin : ceux du C (T8, cibles -30 % vs natifs) restent la
  référence ; les mesures Rust du benchmark sont consignées dans
  docs/etat.md, toute cible inatteignable se renégocie avec les chiffres.
