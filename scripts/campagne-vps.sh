#!/bin/sh
# Campagne système SysGuard sur le VPS perso (Ubuntu 26.04, 8,1 Gio, sans swap).
# Pré-requis : accès SSH opérationnel (clé déverrouillée dans l'agent), binaire
# build/sysguard à jour, config temporaire posée par l'appelant.
# usage : scripts/campagne-vps.sh [HOTE] [CLE]
set -eu

hote=${1:-gaetan@193.168.146.216}
cle=${2:-/home/gaetan/VPS Perso}
ssh_cmd() {
	ssh -i "$cle" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no \
		-o ConnectTimeout=10 "$hote" "$@"
}

echo "== V1 environnement (reconnaissance) =="
ssh_cmd sh -s <<'REMOTE'
set -eu
. /etc/os-release && echo "$PRETTY_NAME / $(uname -r)"
echo "RAM : $(awk '/MemTotal/ {print int($2/1024) " Mio"}' /proc/meminfo)"
echo "swap zones : $(tail -n +2 /proc/swaps | wc -l)"
echo "PSI : $(head -1 /proc/pressure/memory 2>/dev/null || echo ABSENT)"
echo "cpufreq : $(ls /sys/devices/system/cpu/cpu0/cpufreq 2>/dev/null | head -1 || echo ABSENT)"
for s in tuned.service power-profiles-daemon.service auto-cpufreq.service irqbalance.service; do
	echo "$s : $(systemctl is-active "$s" 2>/dev/null)"
done
echo "generator : $(ls /usr/lib/systemd/system-generators/zram-generator 2>/dev/null || echo ABSENT)"
echo "glibc : $(ldd --version | head -1 | awk '{print $NF}')"
REMOTE

echo "== V2 déploiement (root, config campagne sans provisionnement) =="
scp_cmd() {
	scp -i "$cle" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no "$@"
}
scp_cmd build/sysguard "root@${hote#*@}:/usr/local/bin/sysguard" 2>/dev/null ||
	scp_cmd build/sysguard "$hote:/tmp/sysguard"

echo "== V3 dégradé sans privilèges =="
ssh_cmd "/usr/local/bin/sysguard --once --config /tmp/campagne.yaml optimise || echo code=\$?"

echo "== V4 provisionnement root (aucun swap attendu) =="
ssh_cmd "sudo /usr/local/bin/sysguard --once --config /tmp/campagne.yaml optimise"
ssh_cmd "cat /proc/swaps | tail -n +2; cat /proc/sys/vm/swappiness; cat /proc/sys/vm/page-cluster; cat /etc/systemd/zram-generator.conf"

echo "== V5 arbitrage doublon (re-run) =="
ssh_cmd "sudo /usr/local/bin/sysguard --once --config /tmp/campagne.yaml optimise | grep -E 'deja present|non duplique' || true"

echo "== V6 service superviseur =="
ssh_cmd "sudo systemctl is-active sysguard || true"

echo "== campagne VPS : terminée (voir journal ci-dessus) =="
