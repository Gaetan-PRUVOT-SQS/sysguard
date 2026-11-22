# SysGuard — Spécifications fonctionnelles et techniques

**Version** : 0.1 (draft)  
**Date** : 20 novembre 2026  
**Statut** : HISTORIQUE — spécification initiale du prototype Go. La
version courante du produit est celle de `Cargo.toml` ; les éléments Go
non portés (endpoint status, notifications desktop, export Prometheus)
restent en roadmap (§10).  

---

## 1. Contexte et objectif

SysGuard est un **daemon Linux léger** de supervision des ressources système avec capacités de réaction automatique.

Il s’adresse principalement aux :
- Homelabs
- Petits serveurs
- NAS
- Machines de production ne nécessitant pas une stack complète type Prometheus + Alertmanager

### Objectifs
- Surveiller les ressources critiques en temps réel
- Détecter les situations anormales avec hystérésis et durée minimale
- Déclencher des actions configurables (notifications, scripts, redémarrages…)
- Rester simple à déployer et à configurer
- S’intégrer nativement avec systemd

### Non-objectifs (v1)
- Remplacer Prometheus / Grafana
- Faire de l’observabilité distribuée multi-nœuds
- Gérer des métriques applicatives métier complexes
- Interface web complète

---

## 2. Fonctionnalités principales

### 2.1 Collecte de métriques
| Métrique                  | Source                          | Notes |
|---------------------------|---------------------------------|-------|
| CPU                       | `/proc/stat`                    | Global + par cœur optionnel |
| Mémoire / Swap            | `/proc/meminfo`                 | Pourcentage et valeurs absolues |
| Load average              | `/proc/loadavg`                 | |
| Espace disque             | `statfs`                        | Par point de montage |
| Inodes                    | `statfs`                        | |
| I/O disque                | `/proc/diskstats`               | |
| Réseau                    | `/proc/net/dev`                 | |
| Processus spécifiques     | `/proc/[pid]` ou cgroup         | CPU, mémoire, nombre de processus |
| Température               | `/sys/class/thermal` ou `lm-sensors` | Optionnel |

### 2.2 Règles de déclenchement
Une règle se compose de :
- Une ou plusieurs conditions (métrique + opérateur + seuil)
- Une durée minimale de violation (`for:`)
- Une action ou un groupe d’actions
- Un cooldown (anti-spam)
- Un niveau de sévérité (`info`, `warning`, `critical`)

Exemple de logique :
> Si `cpu.usage > 90%` pendant **au moins 2 minutes**, alors déclencher l’action.

### 2.3 Actions supportées
| Type              | Description                              | Exemple |
|-------------------|------------------------------------------|---------|
| `log`             | Écriture dans le journal                 | Niveau configurable |
| `webhook`         | POST JSON vers une URL                   | Discord, Slack, ntfy, custom |
| `exec`            | Exécution d’un script ou commande        | Avec timeout et variables |
| `systemd`         | `systemctl restart/stop/start`           | Service cible |
| `notify`          | Notification desktop (optionnel)         | Via `notify-send` ou D-Bus |

### 2.4 Fonctionnalités transverses
- Rechargement à chaud de la configuration (`SIGHUP`)
- Mode dry-run (simule les actions)
- Historique des déclenchements (mémoire + optionnellement fichier)
- Healthcheck interne (compatible systemd `WatchdogSec`)
- Statistiques d’exécution (nombre d’évaluations, triggers, erreurs)

---

## 3. Architecture

```
┌─────────────────┐
│   systemd       │
│  (service)      │
└────────┬────────┘
         │
┌────────▼────────┐
│   sysguard      │
│   (daemon)      │
│                 │
│  ┌───────────┐  │
│  │ Collector │  │
│  └─────┬─────┘  │
│        │        │
│  ┌─────▼─────┐  │
│  │ Evaluator │  │
│  └─────┬─────┘  │
│        │        │
│  ┌─────▼─────┐  │
│  │  Actions  │  │
│  └───────────┘  │
└─────────────────┘
```

### Composants internes
- **Collector** : récupère périodiquement les métriques
- **Evaluator** : évalue les règles avec état (durée de violation)
- **ActionRunner** : exécute les actions de façon asynchrone et sécurisée
- **ConfigManager** : charge et valide la configuration
- **SignalHandler** : gère SIGHUP, SIGTERM, SIGINT

---

## 4. Configuration

Format recommandé : **YAML** (ou TOML).

### Exemple minimal

```yaml
global:
  check_interval: 15s
  log_level: info

rules:
  - name: high-cpu
    severity: warning
    condition:
      metric: cpu.usage
      operator: ">"
      value: 90
    for: 2m
    cooldown: 10m
    actions:
      - type: webhook
        url: "https://ntfy.sh/mon-topic"
        message: "CPU élevé depuis 2 minutes"

  - name: disk-full
    severity: critical
    condition:
      metric: disk.usage
      mount: "/"
      operator: ">"
      value: 90
    for: 1m
    actions:
      - type: exec
        command: "/usr/local/bin/cleanup-logs.sh"
      - type: webhook
        url: "https://hooks.slack.com/..."
```

### Variables d’environnement supportées dans les actions
- `{{.RuleName}}`
- `{{.Severity}}`
- `{{.Value}}`
- `{{.Threshold}}`
- `{{.Hostname}}`
- `{{.Timestamp}}`

---

## 5. Interface en ligne de commande

```bash
sysguard                    # Démarre le daemon (mode normal)
sysguard --config /path     # Spécifie le fichier de config
sysguard check              # Valide la configuration et sort
sysguard dry-run            # Évalue les règles sans exécuter les actions
sysguard version
sysguard status             # Affiche l’état courant (si endpoint activé)
```

---

## 6. Intégration systemd

### Unit de service recommandée

```ini
[Unit]
Description=SysGuard - System resource supervisor
After=network.target

[Service]
Type=notify
ExecStart=/usr/local/bin/sysguard
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
WatchdogSec=60
User=sysguard
Group=sysguard

[Install]
WantedBy=multi-user.target
```

---

## 7. Sécurité

- Exécution avec un utilisateur dédié non privilégié (`sysguard`)
- Capacités minimales si besoin d’accès à certaines métriques
- Timeout strict sur les actions `exec`
- Validation stricte de la configuration au démarrage
- Pas d’exécution de commandes provenant d’entrées non maîtrisées
- Support optionnel de seccomp / apparmor (phase ultérieure)

---

## 8. Observabilité

- Logs structurés (JSON ou texte) vers journald
- Métriques internes exposables :
  - Nombre de checks
  - Nombre de triggers
  - Temps d’évaluation
  - Erreurs d’actions
- Endpoint HTTP local optionnel (`127.0.0.1:9101/metrics` au format Prometheus)

---

## 9. Stack technique recommandée

| Composant       | Recommandation     | Alternative |
|-----------------|--------------------|-------------|
| Langage         | **Go**             | Rust        |
| Config          | `yaml.v3` + validation | - |
| Logging         | `log/slog`         | zerolog     |
| Systemd         | `go-systemd` (notify) | - |
| HTTP client     | `net/http` standard | - |

---

## 10. Roadmap indicative

### v0.1 — MVP
- Collecte CPU, mémoire, disque
- Règles simples + webhook + exec
- Config YAML + systemd
- Rechargement SIGHUP

### v0.2
- Plus de métriques (réseau, processus, température)
- Cooldown + historique
- Endpoint de status

### v0.3+
- Support cgroups / conteneurs
- Export Prometheus
- Actions systemd natives
- UI web minimale (optionnelle)

---

## 11. Critères de succès

- Consommation mémoire < 30 Mo en fonctionnement normal
- CPU < 1 % en idle
- Démarrage < 2 secondes
- Configuration validée au démarrage avec messages d’erreur clairs
- Documentation d’installation et d’exemple de règles complète

---

*Document de spécifications initial — ouvert aux retours et itérations.*
