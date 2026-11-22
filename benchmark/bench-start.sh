#!/bin/sh
# lance un candidat dans une unit systemd ephemere, sortie captee
# usage : bench-start.sh UNITE FICHIER_SORTIE COMMANDE [ARGS...]
set -eu

unite=$1
sortie=$2
shift 2

# shellcheck disable=SC2016
systemd-run --unit="$unite" --remain-after-exit \
	sh -c 'exec "$@" >"$0" 2>&1' "$sortie" "$@"
