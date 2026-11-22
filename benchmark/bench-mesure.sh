#!/bin/sh
# mesure cpu (cgroup) et memoire d'une unite sur une fenetre donnee
# usage : bench-mesure.sh UNITE DUREE_SECONDES
set -eu

unite=$1
duree=$2
cg="/sys/fs/cgroup/system.slice/$unite.service"

attente=0
while [ ! -r "$cg/cpu.stat" ] && [ "$attente" -lt 30 ]; do
	sleep 1
	attente=$((attente + 1))
done
if [ ! -r "$cg/cpu.stat" ]; then
	echo "erreur : $cg/cpu.stat illisible apres ${attente}s"
	exit 1
fi
if [ "$attente" -gt 0 ]; then
	echo "attente_cgroup=${attente}s"
fi

u0=$(awk '/^usage_usec/ {print $2}' "$cg/cpu.stat")
sleep "$duree"
u1=$(awk '/^usage_usec/ {print $2}' "$cg/cpu.stat")

echo "cpu_ms=$(((u1 - u0) / 1000)) sur ${duree}s"
awk -v u="$((u1 - u0))" -v d="$((duree * 1000000))" \
	'BEGIN {printf "cpu_pct=%.3f\n", u * 100 / d}'
echo "mem_current_ko=$(($(cat "$cg/memory.current") / 1024))"
awk '$1 == "anon" {print "mem_anon_ko=" int($2 / 1024)}' "$cg/memory.stat"
[ -r "$cg/memory.peak" ] && echo "mem_peak_ko=$(($(cat "$cg/memory.peak") / 1024))"
if systemctl is-active --quiet "$unite"; then
	echo "etat=actif"
else
	echo "etat=inactif"
fi
