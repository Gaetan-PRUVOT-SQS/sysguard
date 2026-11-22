#!/bin/sh
# latence de detection : charge de 4 coeurs, premiere trace de chaque
# candidat capable de regles (veille-natif.sh et sysguard), memes regles
# (cpu.usage > 50, for 5s, cooldown 20s). Run en root dans la VM.
set -eu

rm -f /tmp/veille.log
systemctl start sysguard
/tmp/bench-start.sh veille-natif /tmp/veille.log /tmp/veille-natif.sh /tmp/veille.log
sleep 6

t0=$(date +%s.%N)
t0us=$(awk -v t="$t0" 'BEGIN{printf "%d", t * 1000000}')
systemd-run --unit=bench-charge --property=RuntimeMaxSec=100 \
	/tmp/charge.sh 4 >/dev/null
sleep 6
echo "load=$(cut -d' ' -f1 /proc/loadavg) (charge verifiee)"

natif=""
sg=""
i=0
while [ "$i" -lt 45 ]; do
	if [ -z "$natif" ]; then
		ligne=$(grep "cpu " /tmp/veille.log 2>/dev/null | head -1 || true)
		if [ -n "$ligne" ]; then
			horo=$(printf '%s' "$ligne" | cut -d' ' -f1)
			hms=$(date -d "$horo" +%s%3N)
			natif=$(awk -v h="$hms" -v t0="$t0us" \
				'BEGIN{printf "%.0f", (h * 1000 - t0) / 1000}')
		fi
	fi
	if [ -z "$sg" ]; then
		ligne=$(journalctl -u sysguard -o json --since "@${t0%.*}" 2>/dev/null |
			grep "au-dessus" | head -1 || true)
		if [ -n "$ligne" ]; then
			sgus=$(printf '%s' "$ligne" |
				sed 's/.*__REALTIME_TIMESTAMP":"\([0-9]*\)".*/\1/')
			sg=$(awk -v s="$sgus" -v t0="$t0us" \
				'BEGIN{printf "%.0f", (s - t0) / 1000}')
		fi
	fi
	if [ -n "$natif" ] && [ -n "$sg" ]; then
		break
	fi
	i=$((i + 1))
	sleep 1
done

echo "latence_veille_natif=${natif:-n/a} ms"
echo "latence_sysguard_rust=${sg:-n/a} ms"
journalctl -u sysguard -o cat --since "@${t0%.*}" 2>/dev/null |
	grep "au-dessus" | head -1 || true

systemctl stop bench-charge 2>/dev/null || true
systemctl stop veille-natif
rm -f /tmp/veille.log
