# État de SysGuard

## Phase initiale (v0.1) — 2026-11-20

Premier daemon complet livré : collecte CPU/mémoire/swap/load/disque,
règles à durée et cooldown, actions log/webhook/exec, YAML strict, SIGHUP,
systemd notify + watchdog conditionné, unit durcie, CLI run/check/dry-run/
version (--once), configs d'exemple, scripts VM.

### Validation en VM (Debian 13 genericcloud, KVM, 4 vCPU, 6 144 Mo, disque 20 G)

| Critère (spec §11) | Résultat | Preuve |
|---|---|---|
| Mémoire < 30 Mo | 13,8 Mo RSS | `ps -o rss` (14 088 Ko) |
| CPU < 1 % au repos | 0 tick sur 10 s (0 %) | delta utime+stime sur /proc/pid/stat |
| Démarrage < 2 s | 4 ms restart → active | boucle `is-active`, Type=notify (attend READY=1) |
| Config validée, messages clairs | check/version/dry-run dans la VM, refus explicites | sortie CLI |
| Watchdog | actif au-delà de WatchdogSec=60 sans kill, ping conditionné à la collecte | uptime monotonic > 60 s |
| Déclenchement réel | vm-mem : log + exec + webhook JSON reçu sur l'hôte | journalctl + vm/build/webhook-recus.log |
| SIGHUP | « configuration rechargée » puis nouvelle règle déclenchée au tick suivant | journalctl 17:54:24-29 |
| Durcissement | exposition systemd-analyze 1,9/10, UMask 0077 | systemd-analyze security |

Note VM : la seed cloud-init est rejetée si le fichier ne porte pas le nom
exact `user-data` sur l'ISO.

Relancer la VM : `vm/lancer-vm.sh` (prépare disque 20G + seed si besoin),
déployer : `make construire && vm/deploie.sh`. Récepteur webhook de test :
`python3 vm/serveur-webhook.py 9999 vm/build/webhook-recus.log`.

## Benchmark Linux natif contre SysGuard — 2026-11-20

Même VM (4 vCPU, 6 144 Mo), fenêtres de 120-125 s, CPU mesuré par cgroup
(`cpu.stat`), mémoire par cgroup + RSS ; latence = 4 cœurs saturés jusqu'à la
première trace de détection, même règle chez les deux candidats capables de
détecter (`cpu > 50 %`, `for` 5 s, cooldown 20 s). Bruts :
`benchmark/resultats/resultats-2026-11-20.md`.

| Candidat | Rôle | CPU (fenêtre) | Mémoire | Latence détection |
|---|---|---|---|---|
| veille-natif.sh (shell POSIX, ~60 lignes) | collecte + mêmes règles | 234 ms → **0,195 %** | RSS 1,7 Mo, cgroup 428 Ko | 12 s |
| vmstat 5 (procps-ng) | collecte seule | 8 ms → **0,007 %** | cgroup 404 Ko | N/A |
| sar 5 (sysstat 12.7.5) | collecte seule | 13 ms → **0,011 %** | cgroup 580 Ko | N/A |
| SysGuard | collecte + règles + actions | 32 ms → **0,027 %** | RSS 14,7 Mo, cgroup 16,3 Mo | 15 s |

Lecture :

- Le CPU au repos de SysGuard est négligeable (0,027 %) et **7 fois plus bas
  que le script natif** (0,195 %), qui paye un fork/exec awk + date par tick.
  sar et vmstat, collecteurs C sans règles, restent les plus légers en CPU.
- La RAM de SysGuard mesurée ce jour (14,7 Mo RSS, ~19 Mo pic cgroup) est
  le plancher du prototype initial : 30 à 40 fois plus que les candidats
  natifs (0,4-1,8 Mo).
  Dans un homelab, les deux ordres de grandeur restent anecdotiques devant un
  Prometheus (~centaines de Mo) ou netdata.
- Latence de détection 12 s contre 15 s : la même sémantique (tick 5 s +
  `for` 5 s) donne un délai de 1 à 3 ticks selon la phase du tick au moment
  de la charge ; l'écart est dans la phase, pas dans la conception.
- Ce que les candidats natifs n'ont pas : règles à durée et cooldown gérées,
  webhook/exec avec timeout, config validée strictement, sd_notify + watchdog
  conditionné, unit durcie (exposition 1,9/10), tests et couverture.

## Réécriture intermédiaire (v1) — 2026-11-21

Objectif : 30 % mieux que les équivalents natifs sur CPU, mémoire et
latence (cibles, protocole du benchmark du 20/11 : CPU ≤ 5,6 ms, RSS
< 1,19 Mo, cgroup < 283 Ko, latence < 8,4 s à tick 2 s). Phase arrêtée en
cours de tests quand le pivot Rust a été demandé ; ses choix (plancher
mémoire visé, durcissement release, norme de codage stricte, horloge
injectée) ont nourri le portage Rust.

### Reste (roadmap)

- Conditions multiples (AND), métriques réseau/proc/temp
- Endpoint de status, historique des déclenchements
- Type=notify-reload (systemd ≥ 253) à évaluer
- Export Prometheus, cgroups

## Portage Rust intégral (v1.1) — 2026-11-21

Demande du jour : « on le passe intégralement en rust ». La crate Rust à la
racine (édition 2024, `Cargo.toml` + `src/`) devient la ligne principale et
remplace les implémentations antérieures (retirées du dépôt plus tard dans
la journée, cf. section dédiée). Justification :
docs/justifications/rewrite-rust.md.

Livré : parseur YAML strict, config complète (36 refus documentés portés
avec leurs messages), collecte /proc + statfs (FFI libc, seul unsafe),
moteur for/cooldown fidèle, actions log/dry-run/exec/webhook, daemon
(signaux par drapeaux + sommeil complet de l'intervalle), sd_notify +
watchdog conditionné, CLI complète (les options seules impliquent `run`,
conformément à la spec §5), Makefile racine cargo, README mis à jour.

Preuves fraîches du 2026-11-21 :

- `make test` : 136 cas verts ; `make lint` (clippy deny) et `cargo fmt
  --check` propres ; `make couverture` : lignes 97,2 %, régions 95,2 %
  (seuils 90 % en --fail-under) ; `make audit` : 0 advisory (7 crates).
- Validation VM Debian 13 : service actif, RSS 2 148 Ko contre 13,8 Mo
  avant portage, webhook JSON templated reçu sur l'hôte, SIGHUP « configuration
  rechargée » puis nouvelle règle déclenchée au tick suivant, retour
  config d'origine au second HUP, uptime > 60 s sans kill watchdog,
  exposition systemd-analyze 1,9/10, démarrage Type=notify < 2 s.
- Benchmark (protocole du 20/11, tick 2 s, fenêtre 127 s ramenée à 120 s) :
  CPU cgroup 26 ms (0,022 %) contre 32 ms avant portage ; RSS 2 288 Ko contre
  14,7 Mo ; mémoire cgroup 480 Ko contre 16,3 Mo ; latence de détection à
  4 vCPU saturés 7,0 s contre 15 s avant portage et 12 s (script natif).

Anecdotes corrigées avec leurs régressions pendant le portage :

- webhook : l'implémentation intermédiaire n'appliquait pas le template au
  message (`{{.Value}}` littéral reçu sur l'hôte) ; corrigé au comportement
  de la spec, test en place.
- options seules sans commande : une commande explicite était exigée,
  l'unit systemd échouait au premier déploiement ; corrigé au comportement
  de la spec §5 (`run` implicite), test en place.
- CPU : la garde `dIdle <= 0` rapportait 0 % sous saturation pleine
  (delta idle exactement 0) et la règle `cpu > 50 %` ne pouvait pas tirer ;
  corrigé par la formule standard (delta non-idle / delta total), fixture
  `stat-sature` en place. Repéré par la mesure de latence, jamais par les
  tests unitaires.
- CPU au repos : la première version attendait l'intervalle par tranches
  de 50 ms (réactivité signal) ; 2 500 réveils coûtaient 165 ms CPU par
  fenêtre. Remplacé par un sommeil complet de l'intervalle + drapeaux
  (un appel système par tick) : 26 ms. Le réveil par signal attend
  désormais au plus un tick.

Gate secrets du tour : 73 détections gitleaks triées — 71 dans l'image
disque VM binaire (clés du système invité), 1 clé SSH jetable de VM
(régénérable, jamais publiée), 1 hash de boot pris pour une clé. Justifié
dans `.cyber-ignore` ; exclusion par chemin de `vm/build/` posée dans
`.gitleaks.toml` (chargé par gitleaks depuis la racine scannée — le
`.gitleaksignore` initial dépendait du cwd du hook et n'était pas lu) ;
`.gitignore` complété (`target/`). Règles par défaut re-testées actives
(faux secret détecté puis retiré). Rescan depuis un cwd étranger :
no leaks found.

Critères initiaux (-30 % vs natifs) : latence tenue (7,0 s < 8,4 s),
mémoire et CPU absolus non tenus en Rust (plancher du runtime et coût par
tick supérieurs aux cibles) : CPU 26 ms vs cible 5,6 ms, RSS 2,29 Mo vs
cible 1,19 Mo, cgroup 480 Ko vs cible 283 Ko. Renégociation explicite avec
les chiffres : contre la version du 20/11, le Rust fait -18 % CPU, -84 %
RSS, -97 % cgroup, -53 % latence, et les critères de la spec §11 passent
tous (RSS < 30 Mo, CPU < 1 %, démarrage < 2 s). Le retrait des sources
antérieures est acté plus bas.

## Retrait des implémentations antérieures — 2026-11-21

Demande de l'exploitant : pivot complet en Rust, plus aucune trace des
implémentations antérieures dans le dépôt. Retirés : tous les répertoires
et fichiers sources des deux implémentations précédentes, le script de
différentiel et sa cible Makefile. Sauvegarde préalable vérifiée par
extraction et diff (tar.gz sous `~/.local/share/sauvegardes/`).
`make test` et `make construire` relancés verts après retrait.

## Benchmark natif contre SysGuard Rust — 2026-11-21

Même VM et protocole du 20/11, SysGuard Rust en service (tick 5 s,
config-vm.yaml), script natif à règles égales, vmstat et sar en référence
collecte seule. Latence mesurée sur deux essais avec une charge réparée
(`benchmark/charge.sh`, l'ancienne charge ne démarrait pas). Bruts :
`benchmark/resultats/resultats-2026-11-21.md`.

| Candidat | CPU (fenêtre) | Mémoire cgroup | RSS | Latence |
|---|---|---|---|---|
| veille-natif.sh (shell POSIX, même rôle) | 194 ms → **0,162 %** | 436 Ko | 1852 Ko | 8,5 / 8,9 s |
| vmstat 5 (procps-ng) | 8 ms → **0,007 %** | 448 Ko | N/A | N/A |
| sar 5 30 (sysstat) | 11 ms → **0,009 %** | 924 Ko | N/A | N/A |
| SysGuard Rust | 21 ms → **0,018 %** | 496 Ko | 2356 Ko | 12,5 / 9,1 s |

Lecture : SysGuard Rust coûte 9 fois moins de CPU que le script natif de
même rôle, mémoire cgroup comparable, RSS supérieure (exécutable plus
gros), latence du même ordre (phase des ticks). Contre la version du
20/11 : CPU -34 %, cgroup -97 %, RSS -84 %, latence -15 à -40 %. vmstat et sar
(collecte seule) restent devant en CPU pur.

## Optimiseur continu (PSI, zram) — 2026-11-21

Demande : « le daemon doit optimiser en continue le système, cpu, ram (zram
et compression intelligente intégré au daemon) ». Recherche web + skill
reglage-swap-linux synthétisées dans
docs/justifications/optimisation-continue.md. Livré : `src/pression.rs`
(PSI, /proc/swaps, mm_stat), section `optimisation:` stricte,
`src/optimisation.rs` (socle statique idempotent, seuils PSI à hystérésis,
recompression zram, mode dégradé sans privilèges), commande `optimise`,
unité durcie `packaging/sysguard-opt.service` (root, sans réseau, sans
ProtectKernelTunables).

Preuves VM Debian 13 (noyau 6.12) :

- sans privilèges : recommandation journalisée, code 0 ; en root :
  vm.swappiness 60 → 150 appliqué et relu ;
- auto sans zram : swappiness intact (« pas de zram, auto ») ; avec zram
  (modprobe + disksize + swapon prio 100) : swappiness = 133 (doc noyau)
  et page-cluster = 0 appliqués ;
- comp_algorithm à chaud sur device actif : EBUSY noyau → message dédié
  « regler au boot via zram-generator » ;
- PSI figé détecté sur l'image cloud Debian (psi absent du cmdline,
  compteur total immobile sous charge) → avertissement + repli, test en
  place ; avec `psi=1` : cycle complet prouvé — critique à 47,6 % some
  (seuil 5 % tenu 10 s, stress-ng 9 G sur 6 G + swapfile), action
  templated exécutée, retombée à 1,7 % sous la bande de sortie (40 % du
  seuil), aucune retombée prématurée pendant la décroissance.

Découvertes corrigées avec leurs tests : `actions_critique` (liste)
refusée à tort par le contrôle scalaire (vu en VM) ; PSI figé sinon
invisible (compteur immobile = repli).

`make test` : 160 cas verts ; couverture 94,6 % lignes / 94,3 % régions
(seuils 90 en --fail-under) ; clippy deny et fmt propres ; audit 0 advisory.

## Optimisation CPU (parallélisation, répartition) — 2026-11-22

Demande : « CPU : parallélisation intelligente du cpu, répartition
intelligente des charges de travail pour efficience maximale et stabilité
système ». Recherche (Tuned/Red Hat : profils statiques, tuning dynamique
déconseillé ; EEVDF 6.6 : l'épinglement mono-cœur sort de l'équilibrage ;
nice 19 vs SCHED_IDLE : part garantie contre risque de famine ; cgroup v2 :
CPUWeight/CPUQuota côté systemd) synthétisée dans
docs/justifications/optimisation-cpu.md.

Livré : `src/processus.rs` (comptabilité /proc/[pid]/stat, deltas en ms),
FFI bornés (sched_setaffinity, setpriority, sysconf) dans src/sys.rs,
section `cpu:` stricte (gouverneur, affinites admin-déclarées, seuils
symétriques mémoire, demotion opt-in par patterns avec exceptions),
socle CPU (gouverneur + affinités idempotentes réappliquées à chaque pas
— les processus nés après le démarrage sont rattrapés), réponse graduée :
pression critique → alerte nommant le principal consommateur + actions,
démotion du top si pattern reconnu, restauration à nice 0 à la retombée.
Détection de PSI figé corrigée : elle ne juge que sous charge (load1 ≥ 0,5)
— au repos, un total PSI immobile est normal.

Preuves VM Debian 13 : cpufreq absent (qemu) → dégradation journalisée ;
affinité admin appliquée et relue (taskset : 0-3 → 0,1, preuve avec
pgrep -x — la première lecture était polluée par le wrapper sudo) ;
pression CPU réelle (6 hogs montraitement) : critique à 60,6 % tenu
10 s, top nommé dans le journal, trois relégations nice 19 progressives
et idempotentes ; restauration à la retombée prouvée par les tests
(processus tués avant la retombée en VM : rien à restaurer, correct).

Découvertes corrigées avec leurs tests : détection PSI figé en faux
positif au repos (garde load1) ; champ nice mal indexé (indice 16 après
la comm, pas 17) ; parsing des clés-listes (actions_critique, patterns)
refusées à tort par le contrôle scalaire.

`make test` : 168 cas verts ; couverture 92,2 % lignes / 91,3 % régions
(seuils 90 tenus) ; clippy deny et fmt propres ; audit 0 advisory.

## Provisionnement zram et arbitrage des doublons — 2026-11-22

Demande : « compléter ce que le noyau ne fait pas, zram et compression ram
si on l'a pas, conserver le plus performant en cas de doublon ». Recherche
(zram-generator, Chris Down sur zswap vs zram) dans
docs/justifications/provisionnement-zram.md. Livré : état de compression
(src/compression.rs : swaps typés, generator installé/configuré, zswap),
matrice d'arbitrage au socle, provisionnement opt-in complet
(modprobe → algorithme → disksize → mkswap → swapon priorité 100 →
zram-generator.conf), clé `zswap: desactive`.

Preuves VM Debian 13 : machine sans swap → provisionné end-to-end
(/dev/zram0, 2 965 Mio auto, zstd auto (4 vCPU), priorité 100, conf
écrite) ; swap disque présent → refus argumenté avec inversion LRU et
zones nommées ; zram + zswap actif → avertissement double compression
(seulement avec zram) ; conf sans binaire generator → provisionnement
runtime quand même + avertissement d'installation ; après installation de
systemd-zram-generator : le generator applique la conf de l'optimiseur
(unité active, 3 036 156 Ko, zstd, priorité 100).

Découvertes corrigées avec leurs tests : la détection de PSI figé se
déclenchait au repos (total immobile légitime) → garde load1 ≥ 0,5 ;
les méthodes réelles du trait n'étaient jamais couvertes (tests réels
tests/real_cible.rs : lectures /proc, renice/affinité sur soi-même,
échec swapon) ; le champ nice de /proc/stat est l'indice 16 après la
comm ; les clés-listes refusées par le contrôle scalaire (second cas).

`make test` : 193 cas verts ; couverture 94,4 % lignes / 93,7 % régions ;
clippy deny et fmt propres ; audit 0 advisory.

## Arbitrage CPU (gestionnaires, EPP, irqbalance) — 2026-11-22

Complément de la v1.3 : le socle CPU détecte maintenant les gestionnaires
de fréquence actifs (tuned, power-profiles-daemon, auto-cpufreq) et ne
touche à rien s'il y en a un (jamais deux gestionnaires — auto-cpufreq
désactive PPD à l'installation, conflit documenté). Nouvelle clé `epp`
(energy_performance_preference, HWP/amd-pstate) appliquée sur tous les
cœurs qui la supportent, après arbitrage. irqbalance absent →
recommandation journalisée (le daemon ne gère pas les IRQ lui-même).

Preuves VM : power-profiles-daemon et irqbalance installés et actifs →
« CPU gere par power-profiles-daemon (gouverneur/epp non appliques, pas de
doublon) », gouverneur intact ; PPD arrêté → recommandation irqbalance +
cpufreq absent (VM qemu) → dégradation. L'hôte de dev fait lui-même
tourner power-profiles-daemon : les tests unitaires auraient échoué sans
l'injection de l'état des services dans la fausse cible.

Anecdote corrigée : le faux service_actif appelait le vrai systemctl
(l'override n'avait pas pris après formatage) — la fausse cible
n'isolait pas du host. Tests : 196 verts ; couverture 94,4/93,5 ; clippy
deny, fmt propres ; audit 0 advisory.

## Campagne de tests ISTQB / ISO 29119 — 2026-11-22

Campagne intégrale sur v1.4 (doc : docs/campagne-tests-istqb-29119.md) :
stratégie par les risques (7 risques, I×P), 10 modèles de test
(partitions, syntaxique, limites, transitions, table, aléatoire à graine,
métamorphique, supposition d'erreur, fondé sur exigences), traçabilité
base → modèle → éléments → cas. Compléments écrits pour la campagne :
500 entrées aléatoires YAML (graine 20_261_122, zéro panique), aller-retour
des durées, invariance aux commentaires, déterminisme du parse config,
8 suppositions d'erreur de durées, 6 mutants duration.rs tous tués
(restore sha identique), tests réels de CibleReelle.

Incident I-1 (gravité 2) : fin de charge, le total PSI gèle + load1 encore
haut → « PSI figé » à tort, retombée perdue. Cause : la garde load1 seule
ne prouve pas la charge. Correctif : figé seulement si le total n'a JAMAIS
bougé depuis le démarrage. Régression `fin_de_charge_gel_ne_declare_pas_fige`,
revalidation VM : critique ✓ retombée ✓ fige=0.

Verdict : 202 cas automatisés verts, 6/6 mutants tués, couverture
94,49/93,55/93,75, statique OK (clippy deny, fmt, audit 0, secrets no
leaks), système validé en VM fraîche. Critères de fin tous tenus.

## Campagne système VPS réel — 2026-11-22

Même campagne exécutée sur le VPS réel (Ubuntu 26.04.1, 16,7 Gio,
swapfile 8 Gio, cpufreq virtuel lecture seule, PSI compteur figé) —
doc : docs/campagne-vps-29119.md. Résultats : déploiement ✓, dégradé
(nobody) ✓, arbitrage refuse le provisionnement zram (swapfile disque,
inversion LRU) avec zones nommées ✓, service superviseur actif + SIGHUP ✓,
gouverneur refusé par les 8 cœurs → dégradation ✓, PSI figé détecté sous
charge après I-2 ✓. Prérequis documenté : psi=1 au boot pour l'optimiseur.
Corrections d'environnement pendant la campagne : stress-ng et
systemd-zram-generator installés sur le VPS ; le compte utilisateur non
privilégié référencé par sudoers est absent (root seul).

Incident I-2 (gravité 2, trouvé par la campagne VPS) : le drapeau
psi_a_bouge se levait au premier pas au calme (branche Some(_) du match
confluaient « total différent » et « charge faible ») → PSI figé jamais
déclaré sur machine au repos puis chargée. Sémantique corrigée (figé
requiert total identique au premier ET load1 ≥ 0,5 ET jamais bougé),
démontré sur VPS réel sous charge 20 G.

Incident I-3 (processuel) : déploiements trainant des binaires périmés
(clé cpu: refusée en VM ; ETXTBSY en VPS) — chaîne construire→copier→
installer réexécutée dans l'ordre.

`make test` : 203 cas verts (177 unitaires + 26 intégration) ; couverture
94,4/93,5 ; clippy deny, fmt propres ; audit 0 advisory.

## Dépôt git initialisé — 2026-11-22

`git init` (branche main, identité locale de l'exploitant) après le
développement : l'historique est une **reconstruction par phases** — 13
commits regroupant les fichiers par phase réelle (socle, YAML, config,
collecte, évaluation, actions, daemon/CLI, optimiseur mémoire, CPU, tests
réels, docs/packaging/VM). Les états intermédiaires ne compilent pas tous
(l'état final du HEAD est la référence) ; la construction a révélé et
réparé un lib.rs tronqué par le script d'import. Marqueur
`.mode-side-project` exclu via .git/info/exclude. Aucun remote : push sur
demande explicite.

## Campagne parseur YAML (ISO 29119) — 2026-11-22

Campagne dédiée au parseur YAML (composant au risque le plus élevé) :
doc docs/campagne-yaml-29119.md. Deux défauts trouvés par sondes
hors suite et corrigés avec régressions :

- **I-4** : BOM UTF-8 en tête de fichier → clé fantôme, config rejetée
  avec message abscons (configs éditées sous Windows). Corrigé : BOM
  retiré silencieusement.
- **I-5** : nidification profonde (5 000 niveaux) → **stack overflow,
  abort du daemon** (crash complet sur entrée hostile). Corrigé : cap de
  profondeur 32, rejet propre « profondeur YAML excessive ».

Durcissements testés : BOM retiré, 100 niveaux → rejet propre, 10
niveaux → OK, fuzz enrichi (3 graines × 300 entrées, unicode
multi-octets). Couverture yaml.rs : 97,9 % lignes, 100 % branches,
96,8 % régions. Suite : 179 unitaires verts, 6/6 suites.

## Revue post-campagne : 4 correctifs — 2026-11-22

Revue externe de la v1.4 (unité opt bloquante, JSON, modèle PSI,
cohérence documentaire). Quatre correctifs avec validation :

- **JSON webhooks** : `json_echappe` couvre désormais tous les caractères
  de contrôle U+0000-U+001F (RFC 8259) ; un message avec retour à la
  ligne produisait un corps JSON invalide. Test avec \n, \t, \u0007.
- **Modèle PSI séparé** : `PressionMemoire` (some/full/total) et
  `PressionCpu` (some/total) remplacent le champ unique trompeur où la
  pression CPU atterrissait dans `memoire_some`.
- **Unité sysguard-opt** : profil de privilèges minimal réécrit —
  CapabilityBoundingSet limité à CAP_SYS_ADMIN (swapon) + CAP_SYS_MODULE
  (modprobe enfant), ReadWritePaths=/etc/systemd, suppression de
  PrivateDevices / ProtectKernelModules (incompatibles avec les
  opérations zram), le reste du durcissement conservé. Validation VM :
  provisionnement complet DANS le sandbox (zram0 prio 100, zstd,
  swappiness 133, page-cluster 0, conf generator écrite), exposition
  3,1/10.
- **Version et comptes** : version produit 1.4.0 (Cargo.toml = source de
  vérité), specs v0.1 marquées HISTORIQUES, comptes de tests harmonisés
  (203 puis 207 avec les ajouts YAML).

Découverte VM en validant le sandbox : **root sans CAP_DAC_OVERRIDE est
soumis aux permissions DAC** — la config 640 sysguard:sysguard était
illisible par l'unité opt (root, hors groupe). Correctif : config
appartenant à root, groupe sysguard en lecture (640 root:sysguard),
aucune capacité DAC ajoutée. Règle d'exploitation : les fichiers d'un
service à capacités bornées appartiennent au service ou à son groupe.

`make test` : 203 cas verts (181 unitaires + 26 intégration) ;
couverture ≥ 90 ; clippy deny, fmt propres ; audit 0.

## Stabilisation post-revue de l'exploitant — 2026-11-22

Revue en cinq points (privilèges systemd, webhook, descendance exec,
distribution, CI). Livré et justifié dans
docs/justifications/stabilisation.md :

- **CI confinée** : job `campagne` en read (auto-correctifs fmt/clippy,
  relance complète, patch en artifact), job `correctif` seul en write
  (commit mécanique), secret `CI_PUSH_TOKEN` optionnel pour un jeton
  dédié. Workflow release sur tag `v*` : campagne verte puis tar.gz +
  paquet .deb attachés.
- **exec descendance** : spawn en `process_group(0)`, timeout =
  `kill(-pgid, SIGKILL)`. Régression : script avec arrière-plan, preuve
  `/proc/<pid>` absent ; mutation prouvée (sans kill du groupe le test
  échoue).
- **webhook anti-SSRF** : destinations internes refusées par défaut
  (loopback, privées, lien local, CGNAT, TEST-NET, métadonnées, v6
  équivalentes), clé `local: oui` opt-in, hôte/chemin bornés aux
  caractères visibles (anti-injection d'en-tête), connexion sur les
  adresses résolues et vérifiées (anti rebinding DNS). TLS maintenu en
  tranche future (pas de TLS maison, rustls contre le choix zéro-dep).
- **distribution** : paquet .deb (dpkg-deb, postinst/prerm/postrm),
  `scripts/installer.sh` / `desinstaller.sh`, mainteneur du paquet via
  variable de dépôt `DEB_MAINTAINER` (aucune info personnelle poussée).
  Cycles réels prouvés sur la machine de dev : `dpkg -i` → `-r` → `-P`
  et archive tar.gz → installer → desinstaller --purge. L'exemple de
  config proposait un webhook `https://` que l'envoi refuse : corrigé en
  `http://` avec commentaire SSRF. Point ouvert signalé : pas de fichier
  LICENSE dans le dépôt (copyright du paquet à trancher).

`make campagne` : 212 cas verts (186 unitaires + 26 intégration et
réels), code de sortie 0 ; couverture lignes 94,57 / régions 93,85 ;
clippy deny, fmt propres ; audit 0.

### P0 VPS réel : sysguard-opt dans le sandbox systemd (2026-11-22)

Déploiement de l'unité opt sur le VPS (binaire 1.4.0 à jour, empreinte
sha256 vérifiée locale/distance avant install, stop superviseur avant
remplacement). Preuves dans le sandbox systemd réel :

- capacités : CapBnd = CapPrm = CapEff = 0x210000 → cap_sys_module,
  cap_sys_admin, rien d'autre (profondeur du bounding set exacte) ;
- arbitrage : avec `zram_provisionner: oui` posé exprès, « swap deja
  present (/swapfile (disque)), provisionnement zram non applique (swap
  disque + zram = inversion LRU) », swap du VPS intact ; config remise à
  `non` ensuite ;
- SIGHUP superviseur : « configuration rechargée » (la config partagée
  a gagné la section `optimisation`, inerte pour le superviseur) ;
- exposition 1,9 (superviseur) / 3,1 (opt), watchdog actif, les deux
  unités enabled + active.

**Incident I-6 (gravité 2, corrigé pendant le déploiement)** :
`install -d -m 750 /etc/sysguard` a posé le dossier root:root — le
superviseur (utilisateur sysguard, sans CAP_DAC_OVERRIDE) ne pouvait
plus traverser le dossier → « fichier illisible », crash au démarrage.
La règle DAC connue sur les fichiers s'étend au dossier d'exploitation :
`/etc/sysguard` doit appartenir à root:sysguard (750). Correctif posé et
relance verte ; la règle est à porter dans l'installateur (`installer.sh`
fait déjà `chown root:sysguard` sur la config mais laisse le dossier par
défaut — à harmoniser au prochain tour de distribution).
