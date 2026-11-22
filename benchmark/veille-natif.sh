#!/bin/sh
# veille native : les memes regles que config-vm.yaml, en shell POSIX
# regles : cpu.usage > 50 pendant 5s (cooldown 20s), mem.usage > 1 (cooldown 25s)
set -eu

log="${1:?usage : veille-natif.sh FICHIER_LOG}"

intervalle=5
seuil_cpu=50
duree_cpu=5
cooldown_cpu=20
seuil_mem=1
cooldown_mem=25

prev_total=0
prev_idle=0
depuis_cpu=0
dernier_cpu=0
dernier_mem=0
premier=1

horodate() {
	date "+%Y-%m-%dT%H:%M:%S"
}

while :; do
	maintenant=$(date +%s)

	vals=$(awk '/^cpu / {t=0; for (i = 2; i <= 9; i++) t += $i; print t, $5 + $6}' /proc/stat)
	total=${vals%% *}
	idle=${vals##* }
	usage=0
	if [ "$premier" -eq 0 ] && [ "$total" -gt "$prev_total" ]; then
		dt=$((total - prev_total))
		di=$((idle - prev_idle))
		usage=$(((dt - di) * 100 / dt))
		[ "$usage" -lt 0 ] && usage=0
	fi
	prev_total=$total
	prev_idle=$idle
	premier=0

	if [ "$usage" -gt "$seuil_cpu" ]; then
		if [ "$depuis_cpu" -eq 0 ]; then
			depuis_cpu=$maintenant
		fi
		if [ $((maintenant - depuis_cpu)) -ge "$duree_cpu" ] && [ $((maintenant - dernier_cpu)) -ge "$cooldown_cpu" ]; then
			dernier_cpu=$maintenant
			printf '%s cpu %s%% pendant %ss\n' "$(horodate)" "$usage" "$duree_cpu" >>"$log"
		fi
	else
		depuis_cpu=0
	fi

	mem=$(awk '/^MemTotal:/ {t = $2} /^MemAvailable:/ {d = $2} END {if (t > 0) print int((t - d) * 100 / t); else print 0}' /proc/meminfo)
	if [ "$mem" -gt "$seuil_mem" ] && [ $((maintenant - dernier_mem)) -ge "$cooldown_mem" ]; then
		dernier_mem=$maintenant
		printf '%s memoire %s%%\n' "$(horodate)" "$mem" >>"$log"
		/bin/echo "reaction mem" >/dev/null
	fi

	sleep "$intervalle"
done
