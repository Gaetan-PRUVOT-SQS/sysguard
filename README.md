# SysGuard

Daemon Linux léger de supervision des ressources système avec réaction automatique. Pensé pour les homelabs, petits serveurs et NAS qui n'ont pas besoin d'une stack Prometheus + Alertmanager.

## Fonctionnalités (v0.1)

- Collecte CPU (`/proc/stat`), mémoire et swap (`/proc/meminfo`), load (`/proc/loadavg`), disques (`statfs`)
- Règles à seuil avec durée minimale (`for`) et anti-répétition (`cooldown`)
- Actions `log`, `webhook` (POST JSON) et `exec` (avec timeout)
- Configuration YAML validée strictement au démarrage, rechargée à chaud par SIGHUP
- Intégration systemd : `Type=notify`, watchdog sd_notify (ping conditionné à la réussite de la collecte)
- CLI : `run`, `optimise` (optimiseur PSI/zram), `check` (validation), `dry-run`, `version`

## Installation

```sh
make construire            # binaire dans build/sysguard
sudo install -m 755 build/sysguard /usr/local/bin/sysguard
sudo useradd -r -s /usr/sbin/nologin sysguard
sudo mkdir -p /etc/sysguard
sudo install -m 640 configs/sysguard.yaml.example /etc/sysguard/sysguard.yaml
sudo install -m 644 packaging/sysguard.service /etc/systemd/system/sysguard.service
sudo systemctl daemon-reload
sudo systemctl enable --now sysguard
```

Prérequis : systemd ≥ 245 (notify + watchdog), un utilisateur `sysguard` dédié, et un fichier de configuration valide.

## Configuration

Format et défauts : voir `configs/sysguard.yaml.example`. Champs de `global` :

| Champ | Défaut | Rôle |
|---|---|---|
| `check_interval` | `15s` | fréquence d'évaluation des règles |
| `log_level` | `info` | `debug`, `info`, `warn`, `error` |
| `exec_timeout` | `10s` | timeout par défaut des actions `exec` |
| `webhook_timeout` | `10s` | timeout par défaut des actions `webhook` |

Métriques disponibles : `cpu.usage` (0-100), `mem.usage` (0-100), `mem.available` (MiB), `swap.usage` (0-100), `load.load1`, et par point de montage `disk.usage` (0-100), `disk.free` (octets), `disk.inodes` (0-100, pourcentage d'inodes utilisés). Les métriques `disk.*` exigent un champ `mount` ; les autres l'interdisent.

Opérateurs : `<`, `<=`, `>`, `>=`, `==`. Comparaison exacte sur `==` (décimaux).

Une règle se déclenche quand sa condition est vraie depuis au moins `for`, et ne se déclenche pas plus souvent que son `cooldown`. Le compteur de durée repart de zéro dès que la condition redevient fausse.

### Variables dans les actions

`{{.RuleName}}`, `{{.Severity}}`, `{{.Value}}`, `{{.Threshold}}`, `{{.Hostname}}`, `{{.Timestamp}}` (syntaxe template Go). Le webhook envoie un JSON avec ces champs ; la validation accepte une URL http(s), l'envoi ne supporte que `http://` (pas de TLS : prévu dans une tranche dédiée). L'action `exec` reçoit aussi `SG_RULE_NAME`, `SG_SEVERITY`, `SG_VALUE`, `SG_THRESHOLD`, `SG_HOSTNAME`, `SG_TIMESTAMP` dans son environnement, et la commande est découpée sans shell (pas de pipe ni de `&&`).

### Durée du premier cycle

Le calcul CPU demande deux lectures de `/proc/stat` : le premier tick rapporte `cpu.usage = 0`. Éviter un seuil `cpu.usage < x` sur ce premier tick ou poser un `for` non nul.

## CLI

```sh
sysguard --config /etc/sysguard/sysguard.yaml   # daemon (defaut : run)
sysguard check --config /etc/sysguard/sysguard.yaml
sysguard dry-run --config ... [--once]          # evalue sans executer les actions
sysguard version
```

`--once` fait une seule évaluation puis sort.

## systemd et durcissement

L'unit `packaging/sysguard.service` tourne en `Type=notify` avec `WatchdogSec=60` : le daemon envoie `WATCHDOG=1` au tiers de l'intervalle, seulement si la dernière collecte a réussi — une collecte bloquée mène au redémarrage. `Restart=on-failure` couvre watchdog, timeout et OOM.

L'unit applique un sandbox strict (voir `systemd-analyze security sysguard`) : système en lecture seule, pas de privilèges, filtre d'appels système `@system-service`, seules AF_UNIX/AF_INET/AF_INET6 ouvertes. Conséquences pour les actions `exec` :

- un script qui écrit hors de son répertoire doit passer par un drop-in avec `ReadWritePaths=` (ex. `systemctl edit sysguard`)
- `PrivateTmp=yes` isole `/tmp` : pas d'échange de fichiers par `/tmp` avec l'extérieur
- `NoNewPrivileges` interdit `sudo` dans les scripts

## Optimiseur (PSI, zram, tuning mémoire)

`sysguard optimise` lance un second daemon (unité `packaging/sysguard-opt.service`)
qui :

- pose le socle statique au démarrage : `vm.swappiness` (défaut `133` avec
  zram, la doc noyau donne 133 pour un swap deux fois plus rapide que le
  système de fichiers), `vm.page-cluster` (`0` avec zram, pratique des
  distributions) et l'algorithme zram demandé (`zram_algorithme: zstd`) ;
- suit la pression mémoire PSI (`/proc/pressure/memory`) : alerte critique
  à `critique_seuil` % de `full` avg10 tenu `critique_duree` (défauts
  systemd-oomd : 60 % / 30 s) avec `actions_critique` (mêmes actions que
  les règles), hystérésis de sortie décalée (`critique_sortie`, défaut 40 %)
  contre l'oscillation des moyennes PSI ;
- à pression soutenue, journalise l'état de santé zram (ratio de compression
  réel lu dans `mm_stat`) et déclenche la recompression des pages idle si le
  noyau expose `recompress` (6.2+, CONFIG_ZRAM_MULTI_COMP).

Sans privilèges (euid != 0), les écritures `/proc/sys` et sysfs deviennent
des recommandations journalisées. Le changement d'algorithme zram à chaud
est refusé par le noyau sur un device actif (EBUSY) : le régler au boot via
zram-generator (`compression-algorithm`). PSI doit être actif (`psi=1` sur
les images Debian cloud qui l'ont désactivé) : l'optimiseur détecte un PSI
figé (compteur `total` immobile) et bascule en repli journalisé.

Exemple de section :

```yaml
optimisation:
  intervalle: 10s
  swappiness: auto        # auto = 133 si zram présent, sinon intact
  page_cluster: auto      # auto = 0 si zram présent
  zram_algorithme: zstd
  critique_seuil: 60
  critique_duree: 30s
  critique_sortie: 40
  actions_critique:
    - type: log
      message: "pression mémoire {{.Value}}%"
```

## Optimisation CPU (unité sysguard-opt)

La section `cpu:` complète l'optimiseur :

- `gouverneur` : appliqué au socle sur tous les cœurs (défaut : intact ;
  cpufreq absent ou intel_pstate actif → dégradation journalisée) ;
- `affinites` : règles admin-déclarées (nom de processus → cœurs, masques
  larges conseillés) appliquées de façon idempotente à chaque pas, y
  compris aux processus nés après le démarrage ; jamais d'auto-épinglage
  (EEVDF équilibre mieux des tâches libres) ;
- pression CPU PSI à hystérésis (`critique_seuil/duree/sortie`,
  `soutenue_seuil/duree`) : l'alerte critique **nomme le principal
  consommateur** (delta /proc/[pid]/stat) et exécute `actions_critique` ;
- `demotion` (opt-in, défaut OFF) : sous pression critique, les processus
  matchant `patterns` (hors `exceptions`) passent à `nice` (défaut 19,
  part CPU garantie plutôt que SCHED_IDLE qui peut mourir de faim) ;
  restaurés à 0 quand la pression retombe sous la bande de sortie ;
- détection de PSI figé **sous charge** (compteur total immobile avec
  load1 ≥ 0,5) → repli journalisé ; au repos un total immobile est normal.

La répartition canonique reste cgroup v2 côté systemd (`CPUWeight=`,
`CPUQuota=`) : le daemon ne gère pas les cgroups.

## Limites v0.1

- Pas d'hystérésis de retour (seuil de retour distinct du seuil d'alerte) : prévue v0.2
- Une seule condition par règle (AND multi-conditions : v0.2)
- Métriques réseau, processus et température : v0.2 et v0.3
- Pas d'export Prometheus

## Développement

```sh
make test            # suite Rust complète
make lint            # clippy (avertissements en erreur)
make fmt             # rustfmt
make couverture      # cargo-llvm-cov, seuils 90 % lignes et régions
make audit           # cargo audit (base RustSec)
make construire      # binaire release dans build/sysguard
```

Rust (édition 2024, `cargo` ≥ 1.97). Dépendances : `libc` et `signal-hook` seulement. Le binaire release se dépose sur n'importe quel Linux amd64 x86-64 (glibc ≥ 2.32).
