English | [Français](README.fr.md)

# Acceptance test file — SysGuard v1.4

Functional acceptance of the SysGuard daemon (supervisor + optimizer),
executed on 2026-11-22 on two environments:

| Environment | OS | RAM | Swap | Role |
|---|---|---|---|---|
| Debian 13 VM (QEMU/KVM) | 6.12, 4 vCPU | 6 GB | zram (test) | controlled test bench |
| Real VPS (KVM) | 7.0, 8 vCPU | 16.7 GB | 8 GB swapfile | production |

Each case cites its requirement (spec §), its procedure, its expected
result and its verdict. Automated cases reference their test in the
repository (`cargo test` / `make test`). Detailed campaign:
`docs/campagne-tests-istqb-29119.md`.

## R1 — Monitoring and reaction (spec §2)

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-1.1 | Start the service, check its state | `systemctl is-active` = active, Type=notify | ✅ VM + VPS |
| REC-1.2 | Load memory past the critical threshold held 10 s | `pression memoire critique` trace in the journal | ✅ VM |
| REC-1.3 | Release the load | `retombee` trace, no residue | ✅ VM |
| REC-1.4 | Consume CPU past the threshold | alert naming the top consumer | ✅ VM |
| REC-1.5 | Check test coverage (lines/regions) | ≥ 90 % each axis | ✅ 94.4 / 93.5 |

## R2 — Actions (spec §2.3)

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-2.1 | `log` action with `{{.Value}}`, `{{.Hostname}}` variables | interpolated message in the JSON journal | ✅ |
| REC-2.2 | `webhook` action to a local HTTP receiver | JSON POST with rule/severity/value/timestamp fields | ✅ VM |
| REC-2.3 | `exec` action with SG_* variables | script receives SG_RULE_NAME/SEVERITY/VALUE | ✅ |
| REC-2.4 | `exec` exceeding its timeout | process killed, error logged | ✅ |
| REC-2.5 | `dry-run` config | evaluated without running webhook/exec actions | ✅ |

## R3 — Configuration (spec §4)

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-3.1 | `check` on the 36 invalid configurations | refusal with explicit message and exit code 1 | ✅ |
| REC-3.2 | `check` on a valid config | "configuration valide", exit code 0 | ✅ |
| REC-3.3 | SIGHUP on the service | "configuration rechargée" | ✅ VM + VPS |
| REC-3.4 | Hostile YAML config (500 random inputs) | never panics | ✅ |

## R4 — Memory optimizer (PSI, zram)

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-4.1 | No swap + `zram_provisionner: oui` (root) | zram created, mkswap, swapon priority 100 | ✅ VM |
| REC-4.2 | Disk swap already present | provisioning refused (LRU inversion), zones named | ✅ VM + VPS |
| REC-4.3 | After provisioning, install zram-generator | config applied on next boot | ✅ VM |
| REC-4.4 | Without privileges | logged recommendations, no write, exit code 0 | ✅ VM + VPS |
| REC-4.5 | Auto swappiness/page-cluster with zram | 133 / 0 applied | ✅ VM |
| REC-4.6 | Frozen PSI counter (psi absent), machine under load | warning + fallback, no false alert | ✅ VM + VPS |

## R5 — CPU optimizer

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-5.1 | Active manager (tuned / PPD / auto-cpufreq) | governor and EPP untouched, no duplication | ✅ VPS |
| REC-5.2 | No manager, cpufreq absent | logged degradation | ✅ VM + VPS |
| REC-5.3 | irqbalance absent | logged recommendation | ✅ VM + VPS |
| REC-5.4 | Critical CPU pressure, hog matching a pattern | configured nice demotion, restored on release | ✅ VM |

## R6 — systemd integration

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-6.1 | Type=notify service startup | READY=1 received, service active < 2 s | ✅ VM + VPS |
| REC-6.2 | Watchdog armed, service active past WatchdogSec | no watchdog kill | ✅ VM + VPS |
| REC-6.3 | SIGHUP | reload or argued refusal | ✅ VM + VPS |
| REC-6.4 | `systemd-analyze security` | low exposure | ✅ 1.9/10 |

## R7 — Footprint (performance type)

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| REC-7.1 | Idle CPU measured by cgroup, 120 s window | < 1 % | ✅ 0.018 % |
| REC-7.2 | RSS in operation | < 30 MB | ✅ ~2.3 MB |
| REC-7.3 | Detection latency (4-core load) | < 30 s | ✅ 9.1-12.5 s |

## Security

| ID | Procedure | Expected | Verdict |
|---|---|---|---|
| SEC-1 | Dependency audit (RustSec) | 0 alerts | ✅ |
| SEC-2 | Repository secret scan | no leak | ✅ |
| SEC-3 | Hostile config refused without panic | exit code 1, explicit message | ✅ |
| SEC-4 | Exec without shell, environment limited to SG_* | guest variables invisible | ✅ |

## Summary

- 207 automated cases green + system acceptance on 2 environments.
- 0 serious incident open; campaign incidents fixed with regressions
  (see `docs/campagne-tests-istqb-29119.md`).
- Residual risk documented in the campaign report.
