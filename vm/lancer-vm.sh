#!/bin/sh
set -eu

racine=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
build="$racine/build"

"$racine/preparer-vm.sh"

exec qemu-system-x86_64 \
	-accel kvm \
	-cpu host \
	-smp 4 \
	-m 6144 \
	-drive "file=$build/disque.qcow2,if=virtio,format=qcow2" \
	-drive "file=$build/seed.iso,if=virtio,media=cdrom,format=raw,read-only=on" \
	-netdev "user,id=net0,hostfwd=tcp:127.0.0.1:2222-:22" \
	-device virtio-net-pci,netdev=net0 \
	-nographic
