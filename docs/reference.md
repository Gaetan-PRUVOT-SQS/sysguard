# Référence SysGuard v1.4

Document de référence de la version livrée : ce que le produit fait
aujourd'hui. Les exigences historiques restent dans `sysguard-specs.md`
(marquées HISTORIQUES) ; en cas d'écart, ce document et le code font foi.
Guide d'usage : `README.md`. Recette : `tests/README.md`.

## CLI

| Commande | Rôle |
|---|---|
| `sysguard --config F [run]` | daemon (les options seules impliquent `run`, spec §5) |
| `sysguard check --config F` | validation, « configuration valide » et code 0, refus explicite et code 1 |
| `sysguard dry-run --config F [--once]` | évalue sans exécuter webhook/exec (log et dry-run journalisés) |
| `sysguard optimise --config F [--once]` | optimiseur (refuse sans section `optimisation` ni `cpu`) |
| `sysguard version` | « sysguard 1.4.0 », code 0 |

Codes de sortie : 0 ok, 1 erreur applicative (config, action, évaluation),
2 usage inconnu. `--once` : une seule évaluation puis sortie. Options
seules sans commande → `run`.

## Configuration — `global`

| Clé | Défaut | Rôle |
|---|---|---|
| `check_interval` | `15s` | période d'évaluation des règles |
| `log_level` | `info` | `debug`/`info`/`warn`/`error`, journal JSON structuré |
| `exec_timeout` | `10s` | timeout par défaut des actions `exec` |
| `webhook_timeout` | `10s` | timeout par défaut des actions `webhook` |

Clés inconnues refusées partout (config stricte, 36 refus documentés).
SIGHUP recharge la config à chaud tant que le nombre de règles ne change
pas ; retour à la config d'origine au second HUP en cas d'échec.

## Règles

Métriques : `cpu.usage` (0-100), `mem.usage` (0-100), `mem.available`
(Mio), `swap.usage` (0-100), `load.load1`, et par point de montage
`disk.usage`, `disk.free` (octets), `disk.inodes` (exigent `mount`).

Opérateurs : `<`, `<=`, `>`, `>=`, `==`. Une règle se déclenche quand sa
condition est vraie depuis au moins `for` et pas plus souvent que son
`cooldown` (compteur remis à zéro dès que la condition devient fausse).
Tick immédiat au démarrage ; une erreur /proc abandonne le tick sans
évaluation ; montages en échec omis du snapshot.

## Actions

`log` (niveau dérivé de la sévérité ou `level`), `webhook` (POST JSON,
`http://` seul à l'envoi), `exec` (commande absolue, sans shell).

Variables de template : `{{.RuleName}}`, `{{.Severity}}`, `{{.Value}}`,
`{{.Threshold}}`, `{{.Hostname}}`, `{{.Timestamp}}`. L'`exec` reçoit
`SG_RULE_NAME`, `SG_SEVERITY`, `SG_VALUE`, `SG_THRESHOLD`,
`SG_HOSTNAME`, `SG_TIMESTAMP`, rien d'autre (`env_clear`).

Durcissements webhook : hôte borné aux caractères d'hôte (anti-injection
d'en-tête), chemin ASCII visible sans espace ni contrôle, destinations
internes refusées par défaut (loopback, privé, lien local, CGNAT,
TEST-NET, métadonnées, équivalents IPv6) avec `local: oui` opt-in,
connexion sur les adresses résolues et vérifiées. Durcissements exec :
spawn en `process_group(0)`, timeout = SIGKILL du groupe entier
(descendance comprise), refus des champs croisés (`url` sur exec,
`command` sur log ou `local` hors webhook).

## Optimiseur mémoire — `optimisation:`

Socle statique idempotent à chaque pas : swappiness, page-cluster.
Auto : swappiness `133` et page-cluster `0` si zram présent (doc noyau),
sinon intacts. PSI mémoire avec hystérésis : critique (seuil % de full
avg10 tenu `critique_duree`) → actions, retombée sous `critique_sortie`
(40 % du seuil). Détection de PSI figé (compteur total jamais bougé et
load1 ≥ 0,5) → repli journalisé. Provisionnement opt-in
(`zram_provisionner: oui`) : modprobe → algorithme (zstd ≥ 4 cœurs) →
disksize (`min(ram/2, 4 G)`) → mkswap → swapon priorité 100 →
`/etc/systemd/zram-generator.conf`. Arbitrage : swap disque présent →
refus (inversion LRU), zram présent → vérification, generator installé
et configuré → pas de doublon, `zswap: desactive` coupe la double
compression. Sans privilèges : recommandations journalisées, code 0.

## Optimiseur CPU — `cpu:`

Socle : gouverneur et `epp` appliqués si aucun gestionnaire de fréquence
actif (tuned, power-profiles-daemon, auto-cpufreq → jamais de doublon,
gestionnaires détectés) ; affinités admin-déclarées réappliquées à chaque
pas (rattrape les processus nés après le démarrage). Pression CPU PSI :
critique → alerte nommant le principal consommateur + `actions_critique`,
démotion nice opt-in du top si son nom matche `patterns` (hors
`exceptions`), restauration nice 0 à la retombée. cpufreq absent →
dégradation journalisée. irqbalance absent → recommandation (le daemon ne
gère pas les IRQ).

## Unités systemd

`sysguard.service` : utilisateur sysguard, `Type=notify`, `WatchdogSec=60`
(ping conditionné à la collecte réussie), sandbox strict, exposition
1,9/10. `sysguard-opt.service` : root, `CapabilityBoundingSet=CAP_SYS_ADMIN
CAP_SYS_MODULE` (vérifié réel : CapBnd `0x210000`), `ReadWritePaths=/etc/systemd`,
exposition 3,1/10. Les deux unités lisent la même config. Règle DAC : le
dossier et les fichiers d'un service à capacités bornées appartiennent à
root:sysguard (`/etc/sysguard` 750, config 640) — un démon sans
CAP_DAC_OVERRIDE reste soumis aux permissions, dossier compris.

## Dégradé et prérequis

PSI désactivé au boot (`psi` absent du cmdline) : compteurs figés →
repli journalisé, l'optimiseur complet demande `psi=1`. cpufreq virtuel
lecture seule (VPS virtualisé) : gouverneur refusé par les cœurs →
dégradation. Swap disque présent : pas de zram (arbitrage).

## Sécurité

Dépendances : 2 crates (`libc`, `signal-hook`), audit RustSec 0 advisory.
`deny(unsafe_code)` crate entier, `unsafe` borné aux FFI commentés de
`src/sys.rs` (statfs, sysconf, setpriority, sched_setaffinity, swapon,
kill de groupe). Config hostile : jamais de panique (fuzz à graines,
cap de profondeur YAML 32, BOM retiré). Secret : aucun dans le dépôt
(gitleaks). JSON webhook : échappement RFC 8259 complet des caractères de
contrôle.

## Tests et couverture

212 cas (186 unitaires + 26 intégration/réels), couverture ≥ 90 % lignes
et régions (94,5 / 93,8 mesurés), clippy deny, fmt, audit. Campagnes
ISTQB / ISO 29119 : générale, VPS réel, parseur YAML — rapports dans
`docs/`. CI : campagne complète à chaque push + matrice Debian 13 /
Ubuntu 24.04.
