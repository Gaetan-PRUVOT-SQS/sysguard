# SysGuard v0.1 — justification

## Métier

Superviser les ressources d'un homelab ou d'un petit serveur avec réaction automatique (log, webhook, script), sans la pile Prometheus + Alertmanager. La spec (`sysguard-specs.md`, v0.1 draft) fixe le périmètre du MVP : collecte CPU/mémoire/disque, règles à durée, actions log/webhook/exec, config YAML, SIGHUP, systemd.

## Ce qui est livré

- Collecte : CPU (/proc/stat, delta entre deux lectures), mémoire et swap (/proc/meminfo), load1 (/proc/loadavg), disques (statfs sur les montages nommés par les règles). Parseurs purs sur chaînes, testables sans /proc.
- Évaluation : machine à états par règle — compteur de violation remis à zéro dès que la condition redevient fausse, déclenchement quand la violation tient `for`, cooldown entre deux déclenchements. Horloge monotone via `time.Now().Sub` (insensible aux changements d'heure).
- Actions : `log` (niveau dérivé de la sévérité ou explicite), `webhook` (POST JSON, `http.Client.Timeout`, statut 2xx requis), `exec` (commande rendue, découpée sans shell, variables `SG_*`, timeout par contexte, `WaitDelay` de 2 s). Actions lancées en goroutines, la collecte ne bloque jamais.
- Config : YAML strict (`KnownFields` — une clé inconnue refuse la config), validation complète (métriques, opérateurs, URL http(s), chemins absolus, durées positives, noms uniques). `check`, `dry-run` (+ `--once`), `version`.
- systemd : `Type=notify`, `READY=1` au démarrage, `STOPPING=1` à l'arrêt, `WATCHDOG=1` au tiers de `WATCHDOG_USEC` et seulement si la dernière collecte a réussi (ping aveugle = piège documenté). SIGHUP : rechargement validé avant application, ancienne config conservée si le nouveau fichier est cassé.
- Unit durcie : voir README (sandbox strict compatible exec d'actions).

## Choix et alternatives

- **Go** (recommandé par la spec §9, alternative Rust) : un seul binaire statique de 9 Mo, goroutines pour les actions asynchrones, `log/slog` standard. Coût : runtime plus gros qu'un équivalent C, non pertinent ici.
- **sd_notify maison** (~30 lignes, socket unixgram, `@` abstraite gérée par `net.Dial`) plutôt que `coreos/go-systemd/v22` : une dépendance de moins pour une surface minuscule. go-systemd absorbe plus de cas limites (`WATCHDOG_PID`), à reprendre si le besoin apparaît.
- **`Type=notify` + `ExecReload=/bin/kill -HUP`** (comme la spec) plutôt que `Type=notify-reload` (systemd ≥ 253, plus propre : systemd envoie lui-même le SIGHUP et attend `RELOADING=1`/`READY=1`) : portabilité des systemd plus anciens. Noté pour v0.2.
- **Ping watchdog au tiers** de `WATCHDOG_USEC` : la moitié exacte a produit des redémarrages intempestifs documentés (Bernat) ; `sd_watchdog_enabled(3)` ne garantit que la moitié comme minimum.
- **Une condition par règle** : le modèle interne (`valeurMetrique` + `comparer`) se prête à une liste AND en v0.2 sans changer l'état.
- **Pas d'hystérésis de retour** (seuil de retour distinct) : standard chez netdata/zabbix/prometheus, reporté v0.2 ; le cooldown limite déjà le spam.
- **Utilisateur système dédié** plutôt que `DynamicUser` : le daemon exécute des scripts d'action fournis par l'admin et porte un fichier de config avec des droits fixés ; le UID stable est prévisible. DynamicUser reste l'option à privilégier pour un service sans exec ni état.
- **`ProtectProc` absent de l'unit** : les métriques par processus (/proc/[pid]) sont prévues en v0.2 ; l'invisibilité des /proc des autres processus les casserait. À reposer en v0.2 avec cgroups.
- **Webhook JSON générique** (champs rule/severity/value/threshold/hostname/timestamp/message) plutôt qu'un format par cible (Discord/Slack/ntfy) : un seul code, l'adaptation par cible attendra un besoin réel.

## Tests

Stratégie fondée sur les risques (ISO 29119 adaptée) : le risque principal porte sur la validité de la config (entrées administrateur), la machine à états des règles (cœur du produit) et la fiabilité des actions (exec, webhook). Couverture exigée : ≥ 90 % d'instructions.

Techniques par unité :

- config : tables de décision sur 27 cas de refus (clé inconnue, métrique/opérateur/sévérité inconnus, mount requis/interdit, durées négatives ou nulles explicites, URL non http(s), chemin relatif, noms dupliqués, YAML cassé) + valeurs de défaut.
- collect : parseurs sur fixtures de /proc, delta CPU (première lecture, delta nul, horloge en arrière), pourcentages mémoire/swap/inodes, cas d'erreur (clé absente, valeur non numérique).
- eval : transitions d'états (déclenchement immédiat `for: 0`, limite exacte de durée, remise à zéro au retour à la normale, cooldown), valeurs limites des cinq opérateurs, erreurs de métrique.
- action : rendu des six variables, log par niveau, exec réel (/bin/true, /bin/false, timeout à 100 ms, variables SG_*, commande rendue, binaire absent), webhook contre un serveur httptest (payload vérifié champ par champ, statut 500, serveur injoignable, dry-run qui n'envoie rien).
- daemon : tick avec faux collecteur, collecte en échec, montage manquant, action en échec, rechargement (config cassée refusée, ancienne conservée), boucle `Run` (arrêt par contexte, watchdog avec et sans collecte réussie, SIGHUP réel).
- CLI : codes de sortie (0/1/2), boucle complète démarrée puis arrêtée par SIGTERM réel.

Résultat (go test -coverprofile, sortie du 2026-11-20) : **93,8 % d'instructions** au total, tous paquets entre 92 % et 98 %. L'outil Go ne rapporte ni branches ni issues de conditions : cette mesure manque, le critère branche reste ouvert. Le code non couvert se concentre sur `main()` (os.Exit, intestable) et les cas d'erreur de lecture système.

Le ping watchdog à 1/3 et conditionné à la collecte réussie, le découpage sans shell des commandes exec et le refus des clés YAML inconnues proviennent directement de la recherche de bonnes pratiques du 2026-11-20 (sources dans les skills daemon-go-systemd, systemd-durcissement, regles-supervision).

## Benchmark contre le Linux natif

Campagne du 2026-11-20, même VM, fenêtres 120-125 s, comptabilité par cgroup (protocole et bruts dans `benchmark/resultats/resultats-2026-11-20.md`) : CPU au repos 0,027 % (SysGuard) contre 0,195 % pour un script shell équivalent (fork/exec awk+date par tick), 0,011 % (sar) et 0,007 % (vmstat) pour des collecteurs C sans règles ; RAM 14,7 Mo RSS contre 0,4-1,8 Mo côté natif (plancher du runtime Go) ; latence de détection 15 s contre 12 s à sémantique identique (écart = phase de tick). Le coût SysGuard reste borné (7 fois moins de CPU que le shell, RAM anecdotique en homelab) et il apporte ce que les natifs n'ont pas : `for`/cooldown gérés, actions webhook/exec, config validée, watchdog conditionné, unit durcie.

## Validation en VM

Campagne du 2026-11-20 dans une VM Debian 13 genericcloud (KVM, 4 vCPU, 6 144 Mo, disque 20 G), binaire statique déployé avec l'unit durcie, utilisateur système `sysguard` : mémoire 13,8 Mo RSS, CPU 0 tick sur 10 s au repos, redémarrage → actif en 4 ms (`Type=notify`, l'état active attend READY=1), service actif au-delà de `WatchdogSec=60` (pings conditionnés), règle réelle déclenchée avec les trois types d'action (log + exec + webhook JSON reçu sur l'hôte), SIGHUP suivi de « configuration rechargée » et déclenchement de la règle ajoutée au tick suivant, exposition `systemd-analyze security` 1,9/10. Détail et preuves : `docs/etat.md`.

## Risque résiduel

- Une suite verte ne prouve pas l'absence de défaut : les chemins d'erreur de statfs réel (montage qui disparaît au vol) sont couverts par injection de fonction, pas par un vrai démontage.
- Le sandbox de l'unit n'a pas été confronté à des scripts d'action réels lourds (bash avec redirections) : à éprouver avec les scripts de production avant un déploiement sérieux.
- La comparaison `==` sur des flottants est exacte par conception, documentée dans le README.
