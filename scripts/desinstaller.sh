#!/bin/sh
# Désinstalle SysGuard. Root requis. --purge retire aussi la config et
# l'utilisateur dédié.
set -eu

[ "$(id -u)" -eq 0 ] || {
	echo "root requis" >&2
	exit 1
}

PURGE=non
for arg in "$@"; do
	case "$arg" in
	--purge) PURGE=oui ;;
	*)
		echo "option inconnue : $arg" >&2
		exit 2
		;;
	esac
done

systemctl stop sysguard.service sysguard-opt.service 2>/dev/null || true
systemctl disable sysguard.service sysguard-opt.service 2>/dev/null || true
rm -f /etc/systemd/system/sysguard.service /etc/systemd/system/sysguard-opt.service
rm -f /usr/local/bin/sysguard
systemctl daemon-reload

if [ "$PURGE" = oui ]; then
	rm -rf /etc/sysguard
	id -u sysguard >/dev/null 2>&1 && userdel sysguard || true
	echo "desinstalle et purge"
else
	echo "desinstalle (la config et le compte sysguard restent : --purge pour tout retirer)"
fi
