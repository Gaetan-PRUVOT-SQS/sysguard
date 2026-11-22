#!/bin/sh
# Installe SysGuard depuis une archive release (tar.gz) : binaire, unités
# systemd, exemple de config. Root requis. --activer démarre le service.
set -eu

USAGE="usage : installer.sh <archive.tar.gz> [--activer]"

[ "$(id -u)" -eq 0 ] || {
	echo "root requis" >&2
	echo "$USAGE" >&2
	exit 1
}

ACTIVER=non
ARCHIVE=""
for arg in "$@"; do
	case "$arg" in
	--activer) ACTIVER=oui ;;
	-*)
		echo "option inconnue : $arg" >&2
		echo "$USAGE" >&2
		exit 2
		;;
	*) ARCHIVE=$arg ;;
	esac
done
[ -f "$ARCHIVE" ] || {
	echo "archive absente : $ARCHIVE" >&2
	echo "$USAGE" >&2
	exit 1
}

STAGE=$(mktemp -d /dev/shm/sysguard-install.XXXXXX)
trap 'rm -rf "$STAGE"' EXIT INT TERM
tar -xzf "$ARCHIVE" -C "$STAGE"
DOSSIER=$(find "$STAGE" -mindepth 1 -maxdepth 1 -type d | head -n 1)
[ -n "$DOSSIER" ] && [ -x "$DOSSIER/sysguard" ] || {
	echo "archive invalide : binaire sysguard absent" >&2
	exit 1
}

install -m 755 "$DOSSIER/sysguard" /usr/local/bin/sysguard
install -m 644 "$DOSSIER/sysguard.service" /etc/systemd/system/sysguard.service
install -m 644 "$DOSSIER/sysguard-opt.service" /etc/systemd/system/sysguard-opt.service
mkdir -p /etc/sysguard
[ -f /etc/sysguard/sysguard.yaml ] ||
	install -m 640 "$DOSSIER/sysguard.yaml.example" /etc/sysguard/sysguard.yaml
id -u sysguard >/dev/null 2>&1 || useradd -r -s /usr/sbin/nologin sysguard
chown root:sysguard /etc/sysguard/sysguard.yaml 2>/dev/null || true

systemctl daemon-reload
if [ "$ACTIVER" = oui ]; then
	systemctl enable --now sysguard.service
	echo "installe et actif : /etc/sysguard/sysguard.yaml a adapter puis systemctl restart sysguard"
else
	echo "installe : adapter /etc/sysguard/sysguard.yaml puis systemctl enable --now sysguard"
fi
