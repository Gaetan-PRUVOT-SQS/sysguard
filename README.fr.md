[English](README.md) | Français

# SysGuard

Daemon Linux léger de supervision des ressources système avec réaction
automatique et optimiseur intégré (PSI, zram, tuning mémoire et CPU).
Pensé pour les homelabs, petits serveurs et NAS qui n'ont pas besoin d'une
stack Prometheus + Alertmanager.

Écrit en Rust, sans framework : deux dépendances seulement (`libc` et
`signal-hook`), un parseur YAML strict maison, un client HTTP/1.1 minimal.

## Fonctionnalités

- Collecte CPU (`/proc/stat`), mémoire et swap (`/proc/meminfo`), load
  (`/proc/loadavg`), disques (`statfs`), pression PSI (`/proc/pressure/*`)
- Règles à seuil avec durée minimale (`for`) et anti-répétition
  (`cooldown`)
- Actions `log`, `webhook` (POST JSON) et `exec` (variables `SG_*`, sans
  shell, avec timeout)
- Configuration YAML validée strictement au démarrage, rechargée à chaud
  par SIGHUP
- Intégration systemd : `Type=notify`, watchdog sd_notify (ping
  conditionné à la réussite de la collecte)
- CLI : `run`, `optimise` (optimiseur), `check` (validation), `dry-run`,
  `version`
- **Optimiseur mémoire** : santé zram (ratio de compression réel),
  arbitrage des doublons (swap disque présent → pas de zram, risque
  d'inversion LRU), provisionnement complet du swap compressé s'il manque,
  seuils PSI à hystérésis avec actions configurables
- **Optimisation CPU** : détection des gestionnaires de fréquence
  existants (tuned, power-profiles-daemon, auto-cpufreq — jamais de
  doublon), gouverneur et EPP au socle, affinités admin-déclarées,
  identification du principal consommateur, démotion opt-in par patterns

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

Prérequis : systemd ≥ 245 (notify + watchdog), un utilisateur `sysguard`
dédié et un fichier de configuration valide. Binaire x86-64 dynamique
(glibc ≥ 2.32).

L'optimiseur utilise la seconde unité `packaging/sysguard-opt.service`
(`sysguard optimise`) ; sur les images cloud où PSI est désactivé, ajouter
`psi=1` à la ligne de commande du noyau.

## Configuration

Format et défauts : `configs/sysguard.yaml.example`. Champs de `global` :

| Champ | Défaut | Rôle |
|---|---|---|
| `check_interval` | `15s` | fréquence d'évaluation des règles |
| `log_level` | `info` | `debug`, `info`, `warn`, `error` |
| `exec_timeout` | `10s` | timeout par défaut des actions `exec` |
| `webhook_timeout` | `10s` | timeout par défaut des actions `webhook` |

Métriques : `cpu.usage` (0-100), `mem.usage` (0-100), `mem.available`
(Mio), `swap.usage` (0-100), `load.load1`, et par point de montage
`disk.usage` (0-100), `disk.free` (octets), `disk.inodes` (0-100). Les
métriques `disk.*` exigent un champ `mount` ; les autres l'interdisent.

Opérateurs : `<`, `<=`, `>`, `>=`, `==`. Une règle se déclenche quand sa
condition est vraie depuis au moins `for` et ne se déclenche pas plus
souvent que son `cooldown` ; le compteur repart de zéro dès que la
condition redevient fausse.

### Variables dans les actions

`{{.RuleName}}`, `{{.Severity}}`, `{{.Value}}`, `{{.Threshold}}`,
`{{.Hostname}}`, `{{.Timestamp}}` (syntaxe de template). Le webhook envoie
un JSON avec ces champs ; la validation accepte une URL http(s), l'envoi
ne supporte que `http://` (TLS en tranche future). L'action `exec` reçoit
aussi `SG_RULE_NAME`, `SG_SEVERITY`, `SG_VALUE`, `SG_THRESHOLD`,
`SG_HOSTNAME`, `SG_TIMESTAMP`, et la commande est découpée sans shell (pas
de pipe ni de `&&`).

### Optimisation mémoire

```yaml
optimisation:
  intervalle: 10s
  swappiness: auto          # auto = 133 si zram présent, sinon intact
  page_cluster: auto        # auto = 0 si zram présent
  zram_provisionner: oui    # crée le swap compressé s'il n'existe aucun swap
  zram_algorithme: auto     # auto = zstd si ≥ 4 cœurs, sinon lz4
  zswap: desactive          # coupe la double compression devant zram
  critique_seuil: 60        # % de full avg10 (PSI)
  critique_duree: 30s
  critique_sortie: 40       # hystérésis de sortie
  actions_critique:
    - type: log
      message: "pression mémoire critique : {{.Value}}%"
```

Sans privilèges, les écritures `/proc/sys` et sysfs deviennent des
recommandations journalisées. Un swap disque existant bloque le
provisionnement (risque d'inversion LRU) et la présence d'un gestionnaire
de fréquence (tuned, power-profiles-daemon, auto-cpufreq) empêche tout
doublon côté CPU.

## Utilisation

```sh
sysguard --config /etc/sysguard/sysguard.yaml     # daemon (défaut : run)
sysguard check --config /etc/sysguard/sysguard.yaml
sysguard dry-run --config ... [--once]            # évalue sans exécuter
sysguard optimise --config ... [--once]           # lance l'optimiseur
sysguard version
```

`--once` fait une seule évaluation puis sort.

## systemd et durcissement

L'unit `packaging/sysguard.service` tourne en `Type=notify` avec
`WatchdogSec=60` : le daemon envoie `WATCHDOG=1` seulement si la dernière
collecte a réussi — une collecte bloquée mène au redémarrage.
`Restart=on-failure` couvre watchdog, timeout et OOM. L'unit applique un
sandbox strict : système en lecture seule, pas de privilèges, filtre
d'appels système `@system-service`, seules AF_UNIX/AF_INET/AF_INET6
ouvertes. Conséquences pour les actions `exec` : pas d'écriture hors du
répertoire de la règle sans drop-in `ReadWritePaths=`, `/tmp` isolé,
`sudo` interdit dans les scripts.

## Développement

```sh
make test            # suite Rust complète
make lint            # clippy (avertissements en erreur)
make fmt             # rustfmt
make couverture      # cargo-llvm-cov, seuils 90 % lignes et régions
make audit           # cargo audit (base RustSec)
make construire      # binaire release dans build/sysguard
```

Rust édition 2024, `cargo` ≥ 1.97. Tests : 207 cas (composant,
intégration, cible système réelle), couverture ≥ 90 % lignes et régions,
campagne ISTQB / ISO 29119 documentée dans `docs/`.

## Benchmarks (VM Debian 13, 4 vCPU, fenêtres 120 s)

| Candidat | CPU au repos | Mémoire cgroup | Latence détection |
|---|---|---|---|
| script shell natif (même rôle) | 0,162 % | 436 Ko | 8,5-8,9 s |
| vmstat 5 (collecte seule) | 0,007 % | 448 Ko | N/A |
| sar 5 (collecte seule) | 0,009 % | 924 Ko | N/A |
| **SysGuard** | **0,018 %** | **496 Ko** | 9,1-12,5 s |

Détails et protocole : `benchmark/resultats/`.
