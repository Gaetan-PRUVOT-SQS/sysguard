# Réécriture de SysGuard en C (v1)

## Métier

SysGuard v0.1 (Go) est un daemon de supervision (CPU, mémoire, swap, load,
disques ; règles à durée et cooldown ; actions log/webhook/exec) livré et
validé en VM le 2026-11-20. Demande du 2026-11-21 : 30 % de performances en
plus que les équivalents natifs Linux. L'exploitant a choisi « tout battre,
réécriture en C » : CPU et mémoire face à sar et vmstat, latence face au
script natif.

Le benchmark du 2026-11-20 fixe l'état de départ : CPU au repos 0,027 %
(7 fois mieux que le script natif de même rôle, mais derrière sar 0,011 %
et vmstat 0,007 %), mémoire 16,3 Mo cgroup contre 0,4 à 1,8 Mo chez les
natifs, latence de détection 15 s contre 12 s. Le plancher du runtime Go
(13,8 Mo RSS mesurés) rend l'objectif mémoire inaccessible en Go : la
réécriture en C est la seule voie pour tenir les 30 % sur tous les axes.

## Changement

Le binaire C devient la ligne principale (v1.0.0). Le daemon Go v0.1 reste
dans le projet comme oracle de différentiel jusqu'à la validation en VM
(T7) : mêmes configs, mêmes règles, comparaison des comportements. Mêmes
sémantiques que la cartographie Go du 2026-11-21 : for évalué en durée
réelle, cooldown compté depuis le dernier tir avec re-tir à chaque
expiration, tick immédiat au démarrage, montages en échec omis du
snapshot, toute erreur /proc abandonne le tick sans évaluation.

## Choix et alternatives

- Parser YAML maison en sous-ensemble strict (scalaires, listes, cartes,
  durées, refus explicite des constructions non supportées) contre libyaml
  (absente du poste, dépendance paquet en VM et en packaging, RSS) ou un
  changement de format (la spec fait foi : YAML).
- Webhook : client HTTP/1.1 maison (POST JSON, timeout, statut 2xx). HTTPS
  via mbedTLS dans une tranche dédiée : bibliothèque dynamique, pages
  résidentes seulement pendant une action.
- Exec sans shell comme en Go : fork/exec, découpage par espaces,
  variables SG_* dans l'environnement.
- Corrections documentées du portage : le timeout webhook par action sera
  appliqué (le Go le valide sans l'utiliser) ; avertissement au démarrage
  si check_interval dépasse le délai watchdog (non validé en Go).
- Tick 2 s dans la config de benchmark pour tenir la latence, soit un
  budget d'environ 0,09 ms CPU par tick. Si la mesure T8 montre le budget
  inatteignable, le critère latence se renégocie avec les chiffres,
  explicitement.
- Mémoire : arène unique par durée de vie, état après init sans allocation
  par tick (plancher RSS).
- Compilation : gcc -std=c17 -D_POSIX_C_SOURCE=202405L -Wall -Wextra
  -Werror plus extras ; durcissement release -D_FORTIFY_SOURCE=3
  -fstack-protector-strong -fstack-clash-protection -fcf-protection=full
  -fPIE -pie -Wl,-z,relro,-z,now -Wl,-z,noexecstack (-fhardened refusé par
  Ubuntu qui prédéfinit FORTIFY). Tests avec ASan + UBSan, valgrind séparé.
- Norminette 42 appliquée à tout C/H du projet ; INVALID_HEADER accepté
  (seul écart prévu, bandeau jamais posé) ; le contrôle make norminette
  filtre ce nom d'erreur et échoue sur tout autre.
- Casts (void) justifiés : écritures d'usage/version (parité avec Go qui
  ignore aussi ces erreurs) ; clock_gettime, gmtime_r et strftime
  (structures pré-remplies à zéro : l'échec donne un horodatage époque
  déterministe au lieu d'un contenu indéterminé) ; fclose des captures de
  tests (l'échec se traduit par des assertions KO en aval).
- arena_init borne capacite à SIZE_MAX/2 (l'entrée externe est validée à
  la frontière) : l'addition utilise + alignement - 1 devient prouvable
  sans débordement, son contrôle supprimé ; le contrôle decal + taille
  reste et est testé avec un alignement de 2^63.
- ASAN_OPTIONS=allocator_may_return_null=1 pour les tests : le cas d'échec
  calloc demande SIZE_MAX/2, refusé par défaut par ASan.

## Tests

Stratégie fondée sur les risques : le risque de la tranche T1 est la
propriété mémoire et les chemins d'échec. Techniques : partitions
(entrées nulles, nulles-capacité, bornes de taille, alignements
puissance de deux et non-puissance), valeurs limites (capacite
SIZE_MAX/2 et SIZE_MAX, taille SIZE_MAX, alignement 2^63), chaîne
d'échec calloc jusqu'à l'épuisement puis destruction.

Résultats du 2026-11-21 (sorties fraîches de la session) :

- make test (racine, Go + C) : 7 paquets Go ok, C : 23 + 17 + 7 = 47 cas,
  0 KO, sous ASan + UBSan.
- make valgrind : 0 erreur sur les 3 binaires de test.
- make couverture : lignes 97,3 % (108/111), branches 93,9 % (62/66),
  seuils 90 % fixés avant campagne via gcovr --fail-under-line et
  --fail-under-branch.
- make norminette : OK, INVALID_HEADER seul écart.
- Binaire : build/sysguard version → « sysguard 1.0.0 » code 0 ; sans
  argument → 1 ; commande inconnue → 2 ; help/-h/--help → 0.

Défaut trouvé pendant la tranche : arena_alloc alignait l'offset dans le
bloc au lieu d'aligner l'adresse effective (KO à -O0, OK par hasard sous
ASan). Corrigé par alignement depuis l'adresse de base ; le cas de
couverture -O0 verrouille la régression.

## Traçabilité

- Demande : 2026-11-21, « 30 % de performances en plus que les systèmes
  équivalents natifs Linux » ; choix « tout battre, réécriture en C »
  posé par l'exploitant.
- Baseline : benchmark/resultats/resultats-2026-11-20.md, résumé dans
  docs/etat.md.
- Critères de fin (T8, protocole identique, même VM Debian 13) : CPU
  ≤ 5,6 ms par fenêtre de 120 s contre 8 ms pour vmstat ; RSS < 1,19 Mo
  contre 1,7 Mo ; mémoire cgroup < 283 Ko contre 404 Ko ; latence
  < 8,4 s contre 12 s. Les critères spec §11 restent exigés.
- Cartographie Go : rapport d'exploration du 2026-11-21 (sémantiques,
  pièges : timeout webhook ignoré, premier tick CPU à 0, alertes
  répétées à chaque cooldown).
- Plan : 8 tranches dans docs/etat.md ; T1 socle faite, T2 config YAML
  ensuite.
