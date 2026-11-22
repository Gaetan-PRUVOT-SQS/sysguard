#!/bin/sh
# sature nb_coeurs : une boucle occupee par coeur, pour la mesure de latence
set -eu

coeurs=${1:-4}

i=0
while [ "$i" -lt "$coeurs" ]; do
	(while :; do :; done) &
	i=$((i + 1))
done
wait
