#!/bin/sh
set -eu

racine=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
build="$racine/build"
telechargement="$build/img-download"
image_source="$telechargement/debian-13-genericcloud-amd64.qcow2"
url_image="https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2"

mkdir -p "$build" "$telechargement"

if [ ! -f "$image_source" ]; then
	curl -fsSL -o "$image_source" "$url_image"
	curl -fsSL -o "$telechargement/SHA512SUMS" "https://cloud.debian.org/images/cloud/trixie/latest/SHA512SUMS"
	(cd "$telechargement" && sha512sum --check SHA512SUMS) | grep -F genericcloud
fi

if [ ! -f "$build/cle-vm" ]; then
	ssh-keygen -t ed25519 -N "" -f "$build/cle-vm" -q
fi

if [ ! -f "$build/disque.qcow2" ]; then
	cp "$image_source" "$build/disque.qcow2"
	qemu-img resize "$build/disque.qcow2" 20G
fi

sed "s|__CLE_PUBLIQUE__|$(cat "$build/cle-vm.pub")|" "$racine/user-data" >"$build/user-data-rendu"

rm -f "$build/seed.iso"
genisoimage -quiet -output "$build/seed.iso" -volid cidata -joliet -rock \
	-graft-points "user-data=$build/user-data-rendu" "meta-data=$racine/meta-data"

echo "VM prete : $build/disque.qcow2 (20G), seed $build/seed.iso"
