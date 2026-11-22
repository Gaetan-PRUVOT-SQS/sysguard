#!/bin/sh
set -eu

racine=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
build="$racine/build"
projet="$racine/.."
binaire="$projet/build/sysguard"
cle="$build/cle-vm"

ssh_vers_vm() {
	ssh -i "$cle" -p 2222 -o IdentitiesOnly=yes \
		-o StrictHostKeyChecking=no -o UserKnownHostsFile="$build/known_hosts" \
		-o ConnectTimeout=5 "$@"
}

scp_vers_vm() {
	scp -i "$cle" -P 2222 -o IdentitiesOnly=yes \
		-o StrictHostKeyChecking=no -o UserKnownHostsFile="$build/known_hosts" -o ConnectTimeout=5 "$@"
}

if [ ! -x "$binaire" ]; then
	echo "binaire absent : lance make construire d'abord" >&2
	exit 1
fi

attendre_ssh() {
	essais=0
	while [ "$essais" -lt 60 ]; do
		if ssh_vers_vm debian@127.0.0.1 true 2>/dev/null; then
			return 0
		fi
		essais=$((essais + 1))
		sleep 5
	done
	echo "ssh injoignable apres 5 minutes" >&2
	return 1
}

attendre_ssh
echo "VM joignable en ssh, deploiement..."

scp_vers_vm "$binaire" "debian@127.0.0.1:/tmp/sysguard"
scp_vers_vm "$racine/config-vm.yaml" "debian@127.0.0.1:/tmp/sysguard.yaml"
scp_vers_vm "$projet/packaging/sysguard.service" "debian@127.0.0.1:/tmp/sysguard.service"

ssh_vers_vm debian@127.0.0.1 sh -s <<'REMOTE'
set -eu
sudo useradd -r -d /nonexistent -s /usr/sbin/nologin sysguard 2>/dev/null || true
sudo mkdir -p /etc/sysguard
sudo install -m 755 /tmp/sysguard /usr/local/bin
sudo install -m 640 -o sysguard -g sysguard /tmp/sysguard.yaml /etc/sysguard/
sudo install -m 644 /tmp/sysguard.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now sysguard
sudo systemctl is-active sysguard
REMOTE

echo "deploiement termine"
