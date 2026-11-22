#!/bin/sh
# latence de detection : meme charge (4 coeurs satures) pour tous les candidats
# capables de regles ; latence = t0 -> premiere trace de chaque candidat
# sysguard et netdata : journalctl -o json (__REALTIME_TIMESTAMP, epoch us)
# monit : action exec qui ajoute son epoch dans /tmp/monit-trace.epoch
# usage : bench-latence.sh NB_ESSAIS
set -eu

essais=${1:-3}

arret_charge()
{
	sudo systemctl stop bench-charge.service >/dev/null 2>&1 || true
}

premier_trace_sysguard()
{
	journalctl -u sysguard -o json --since "@$1" 2>/dev/null |
		grep '"MESSAGE"' |
		grep 'seuil' |
		head -1 |
		sed 's/.*__REALTIME_TIMESTAMP":"\([0-9]*\)".*/\1/'
}

premier_trace_netdata()
{
	journalctl -u netdata -o json --since "@$1" 2>/dev/null |
		grep 'bench_cpu_50' |
		grep 'WARNING' |
		head -1 |
		sed 's/.*__REALTIME_TIMESTAMP":"\([0-9]*\)".*/\1/'
}

arret_charge
rm -f /tmp/monit-trace.epoch
essaie=1
while [ "$essaie" -le "$essais" ]; do
	t0=$(date +%s.%N)
	t0us=$(awk -v t="$t0" 'BEGIN{printf "%d", t * 1000000}')
	# shellcheck disable=SC2016
	sudo systemd-run --unit=bench-charge --property=RuntimeMaxSec=100 \
		sh -c 'i=0; while [ "$i" -lt 4 ]; do (while :; do :) & i=$((i+1)); done; wait' \
		>/dev/null 2>&1
	sleep 45
	sg=$(premier_trace_sysguard "${t0%.*}")
	nd=$(premier_trace_netdata "${t0%.*}")
	mo=$(awk -v t0="$t0" '$1 > t0 {print; exit}' /tmp/monit-trace.epoch 2>/dev/null || true)
	sg_lat=$(awk -v sg="$sg" -v t0="$t0us" 'BEGIN{if (sg=="") print "n/a"; else printf "%.1f", (sg-t0)/1000}')
	nd_lat=$(awk -v nd="$nd" -v t0="$t0us" 'BEGIN{if (nd=="") print "n/a"; else printf "%.1f", (nd-t0)/1000}')
	mo_lat=$(awk -v mo="$mo" -v t0="$t0" 'BEGIN{if (mo=="") print "n/a"; else printf "%.1f", (mo-t0)*1000}')
	echo "essai=$essaie t0=$t0 sysguard=${sg_lat}ms monit=${mo_lat}ms netdata=${nd_lat}ms"
	arret_charge
	essaie=$((essaie + 1))
	sleep 30
done
