# Stabilisation post-revue (v1.4.0) — 2026-11-22

Revue de l'exploitant en phase stabilisation : pas de nouvelle
fonctionnalité, durcir ce qui existe, corriger la CI, préparer la
distribution.

## Métier

SysGuard est déployé (VM + VPS réel) et publié. Les points de la revue :
privilèges du démon dans systemd, webhook durci, actions exec aux
descendances maîtrisées, installation reproductible.

## Changements

- **exec : groupe de processus** (`src/action/exec.rs`, `src/sys.rs`).
  L'enfant part avec `process_group(0)` (std, pas de FFI) ; au timeout,
  `kill(-pgid, SIGKILL)` via `sys::tuer_groupe` tue toute sa descendance.
  Alternatives écartées : cgroups (inutilement lourd pour une action
  ponctuelle), prctl CHILD_SUBREAPER + balayage /proc (course et
  complexité pour un cas couvert par les groupes).
- **webhook : anti-SSRF et validation stricte** (`src/action/webhook.rs`,
  `src/config/actions.rs`, `src/config/mod.rs`). Destinations internes
  refusées par défaut (loopback, privées, lien local, CGNAT 100.64/10,
  TEST-NET, benchmarking, métadonnées, unique-local et link-local v6,
  documentation v6, multicast, broadcast, unspecified) ; clé `local: oui`
  opt-in par action pour l'homelab. L'hôte et le chemin sont bornés aux
  caractères visibles sans espace ni contrôle (anti-injection d'en-tête,
  crochets IPv6 hors grammaire refusés). La connexion part sur les
  adresses déjà résolues et vérifiées (pas de re-résolution : anti
  rebinding DNS). `booleen()` montée dans `config/mod.rs` (usage partagé).
- **TLS écarté, assumé** : écrire du TLS maison est dangereux, rustls
  tirerait une pile de dépendances contre le choix « deux dépendances
  seulement » documenté dans rewrite-rust.md. Le README reste explicite :
  envoi `http://` seul, TLS en tranche future.
- **Distribution** (`scripts/paquet-deb.sh`, `scripts/installer.sh`,
  `scripts/desinstaller.sh`, `.github/workflows/release.yml`). Paquet
  .deb construit par dpkg-deb (postinst crée le compte, prerm stoppe,
  postrm purge l'utilisateur et /etc/sysguard), archive tar.gz, workflow
  release déclenché par tag `v*` après campagne verte. Le champ
  Maintainer vient de la variable de dépôt `DEB_MAINTAINER` (aucune
  information personnelle poussée dans les fichiers ; défaut neutre).
  Pas de fichier LICENSE dans le dépôt : le copyright du paquet renvoie
  à trancher, point signalé.
- **CI : privilège d'écriture confiné** (`.github/workflows/ci.yml`).
  Deux jobs : `campagne` en `contents: read` (auto-correctifs, relance,
  patch en artifact), `correctif` en `contents: write` limité au commit
  mécanique, jeton dédié optionnel via secret `CI_PUSH_TOKEN`
  (`${{ secrets.CI_PUSH_TOKEN || github.token }}`).

## Tests

ISO 29119 adapté, oracle = état système réel et suite du projet :

- descendance exec : script lançant un arrière-plan, timeout à 400 ms,
  preuve par `/proc/<pid>` de l'arrière-plan absent ; **mutation** :
  sans kill du groupe (fallback leader seul), le test échoue
  (« la descendance doit être morte »), restauré puis vert.
- SSRF : table de décision des plages internes/publiques (26 cas), hôte
  et chemin injectables refusés (espace, LF, CR, crochets), deux essais
  réels : `127.0.0.1:9999` sans `local` → « destination interne
  refusee » ; avec `local: oui` sur un port fermé → « connexion
  impossible » (la branche SSRF laisse passer la connexion).
- distribution : `dpkg-deb -I/-c` du paquet construit, cycle réel sur la
  machine de dev : `dpkg -i` (binaire, unités, compte créé) →
  `dpkg -r` (compte conservé, fichiers du paquet retirés) → `dpkg -P`
  (compte et /etc/sysguard retirés) ; cycle équivalent depuis l'archive
  tar.gz avec `installer.sh` puis `desinstaller.sh --purge`.

`make campagne` verte après l'ensemble (fmt, clippy deny, tests, couverture
≥ 90 %, audit 0 advisory, build release `--locked`). Risque résiduel :
l'installation .deb n'est pas exercée en conteneur Debian/Ubuntu propre
(P2 multi-distributions) ; les scripts d'installation sont shellcheckés
mais l'installation réelle depuis une release GitHub se fera au premier
tag.
