English | [Français](README.fr.md)

# SysGuard

A lightweight Linux daemon for system resource monitoring with automatic
reaction and a built-in optimizer (PSI, zram, memory and CPU tuning).
Designed for homelabs, small servers and NAS boxes that don't need a full
Prometheus + Alertmanager stack.

Written in Rust, no framework: only two dependencies (`libc` and
`signal-hook`), a strict in-house YAML parser, a minimal HTTP/1.1 client.

## Features

- Collects CPU (`/proc/stat`), memory and swap (`/proc/meminfo`), load
  (`/proc/loadavg`), disks (`statfs`), pressure PSI (`/proc/pressure/*`)
- Threshold rules with minimum duration (`for`) and anti-spam
  (`cooldown`)
- Actions: `log`, `webhook` (JSON POST) and `exec` (`SG_*` variables, no
  shell, with timeout)
- YAML configuration strictly validated at startup, hot-reloaded on
  SIGHUP
- systemd integration: `Type=notify`, sd_notify watchdog (ping only when
  the collection succeeded)
- CLI: `run`, `optimise` (optimizer), `check` (validation), `dry-run`,
  `version`
- **Memory optimizer**: zram health (real compression ratio), duplicate
  arbitration (disk swap present → no zram, LRU inversion risk), full
  provisioning of compressed swap when missing, PSI thresholds with
  hysteresis and configurable actions
- **CPU optimizer**: detection of existing frequency managers (tuned,
  power-profiles-daemon, auto-cpufreq — never duplicated), governor and
  EPP at setup, admin-declared affinities, top consumer identification,
  opt-in demotion by patterns

## Installation

```sh
make construire            # binary in build/sysguard
sudo install -m 755 build/sysguard /usr/local/bin/sysguard
sudo useradd -r -s /usr/sbin/nologin sysguard
sudo mkdir -p /etc/sysguard
sudo install -m 640 configs/sysguard.yaml.example /etc/sysguard/sysguard.yaml
sudo install -m 644 packaging/sysguard.service /etc/systemd/system/sysguard.service
sudo systemctl daemon-reload
sudo systemctl enable --now sysguard
```

Requirements: systemd ≥ 245 (notify + watchdog), a dedicated `sysguard`
user and a valid configuration file. Dynamic x86-64 binary
(glibc ≥ 2.32).

The optimizer uses the second unit `packaging/sysguard-opt.service`
(`sysguard optimise`); on cloud images where PSI is disabled, add `psi=1`
to the kernel command line.

## Configuration

Format and defaults: `configs/sysguard.yaml.example`. Fields in `global`:

| Field | Default | Role |
|---|---|---|
| `check_interval` | `15s` | rule evaluation frequency |
| `log_level` | `info` | `debug`, `info`, `warn`, `error` |
| `exec_timeout` | `10s` | default timeout for `exec` actions |
| `webhook_timeout` | `10s` | default timeout for `webhook` actions |

Metrics: `cpu.usage` (0-100), `mem.usage` (0-100), `mem.available`
(MiB), `swap.usage` (0-100), `load.load1`, and per mount point
`disk.usage` (0-100), `disk.free` (bytes), `disk.inodes` (0-100). The
`disk.*` metrics require a `mount` field; the others forbid it.

Operators: `<`, `<=`, `>`, `>=`, `==`. A rule triggers when its condition
has been true for at least `for` and does not fire more often than its
`cooldown`; the counter resets as soon as the condition turns false.

### Variables in actions

`{{.RuleName}}`, `{{.Severity}}`, `{{.Value}}`, `{{.Threshold}}`,
`{{.Hostname}}`, `{{.Timestamp}}` (template syntax). The webhook posts
a JSON payload with those fields; validation accepts an http(s) URL,
sending only supports `http://` (TLS in a future milestone). The `exec`
action also receives `SG_RULE_NAME`, `SG_SEVERITY`, `SG_VALUE`,
`SG_THRESHOLD`, `SG_HOSTNAME`, `SG_TIMESTAMP`, and the command is split
without a shell (no pipes, no `&&`).

### Memory optimization

```yaml
optimisation:
  intervalle: 10s
  swappiness: auto          # auto = 133 when zram present, else untouched
  page_cluster: auto        # auto = 0 when zram present
  zram_provisionner: oui    # creates compressed swap when no swap exists
  zram_algorithme: auto     # auto = zstd on ≥ 4 cores, else lz4
  zswap: desactive          # turns off double compression in front of zram
  critique_seuil: 60        # % of full avg10 (PSI)
  critique_duree: 30s
  critique_sortie: 40       # exit hysteresis
  actions_critique:
    - type: log
      message: "critical memory pressure: {{.Value}}%"
```

Without privileges, `/proc/sys` and sysfs writes become logged
recommendations. An existing disk swap blocks zram provisioning (LRU
inversion risk) and an active frequency manager (tuned,
power-profiles-daemon, auto-cpufreq) prevents any CPU duplication.

## Usage

```sh
sysguard --config /etc/sysguard/sysguard.yaml     # daemon (default: run)
sysguard check --config /etc/sysguard/sysguard.yaml
sysguard dry-run --config ... [--once]            # evaluates without running
sysguard optimise --config ... [--once]           # runs the optimizer
sysguard version
```

`--once` performs a single evaluation then exits.

## systemd and hardening

The `packaging/sysguard.service` unit runs as `Type=notify` with
`WatchdogSec=60`: the daemon sends `WATCHDOG=1` only if the last
collection succeeded — a blocked collection leads to a restart.
`Restart=on-failure` covers watchdog, timeouts and OOM. The unit applies a
strict sandbox: read-only system, no privileges, `@system-service` syscall
filter, only AF_UNIX/AF_INET/AF_INET6 open. Consequences for `exec`
actions: no writes outside the rule directory without a `ReadWritePaths=`
drop-in, isolated `/tmp`, `sudo` forbidden in scripts.

## Development

```sh
make test            # full Rust test suite
make lint            # clippy (warnings as errors)
make fmt             # rustfmt
make couverture      # cargo-llvm-cov, 90 % line and region thresholds
make audit           # cargo audit (RustSec database)
make construire      # release binary in build/sysguard
```

Rust edition 2024, `cargo` ≥ 1.97. Tests: 203 cases (component,
integration, real system target), coverage ≥ 90 % lines and regions,
ISTQB / ISO 29119 campaign documented in `docs/`.

## Benchmarks (Debian 13 VM, 4 vCPU, 120 s windows)

| Candidate | Idle CPU | cgroup memory | Detection latency |
|---|---|---|---|
| native shell script (same role) | 0.162 % | 436 KiB | 8.5-8.9 s |
| vmstat 5 (collection only) | 0.007 % | 448 KiB | N/A |
| sar 5 (collection only) | 0.009 % | 924 KiB | N/A |
| **SysGuard** | **0.018 %** | **496 KiB** | 9.1-12.5 s |

Details and protocol: `benchmark/resultats/`.
