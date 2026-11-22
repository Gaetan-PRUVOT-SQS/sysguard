#!/bin/sh
# Construit le paquet Debian depuis build/sysguard (make construire avant).
set -eu

RACINE=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$RACINE/Cargo.toml")
ARCH=amd64
STAGE="$RACINE/build/deb"
MAINTENEUR=${MAINTENEUR:-SysGuard project}

[ -n "$VERSION" ] || {
	echo "version absente de Cargo.toml" >&2
	exit 1
}
[ -x "$RACINE/build/sysguard" ] || {
	echo "build/sysguard absent : lancer make construire" >&2
	exit 1
}

rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/local/bin" \
	"$STAGE/usr/lib/systemd/system" "$STAGE/etc/sysguard" \
	"$STAGE/usr/share/doc/sysguard"

install -m 755 "$RACINE/build/sysguard" "$STAGE/usr/local/bin/sysguard"
install -m 644 "$RACINE/packaging/sysguard.service" \
	"$STAGE/usr/lib/systemd/system/sysguard.service"
install -m 644 "$RACINE/packaging/sysguard-opt.service" \
	"$STAGE/usr/lib/systemd/system/sysguard-opt.service"
install -m 640 "$RACINE/configs/sysguard.yaml.example" \
	"$STAGE/etc/sysguard/sysguard.yaml.example"

cat >"$STAGE/DEBIAN/control" <<CONTROLE
Package: sysguard
Version: ${VERSION}-1
Architecture: ${ARCH}
Maintainer: ${MAINTENEUR}
Section: admin
Priority: optional
Depends: libc6 (>= 2.32), systemd
Description: Daemon Linux de supervision avec reaction automatique et optimiseur.
 Supervision CPU, memoire, swap, load et disques avec regles a duree et
 cooldown, actions log/webhook/exec, optimiseur continu PSI, zram et CPU.
CONTROLE

cat >"$STAGE/DEBIAN/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: sysguard
Source: https://github.com/Gaetan-PRUVOT-SQS/sysguard

Copyright: SysGuard project authors
License: voir le depot amont
COPYRIGHT

cat >"$STAGE/DEBIAN/postinst" <<AFTER_INSTALL
#!/bin/sh
set -eu
id -u sysguard >/dev/null 2>&1 || useradd -r -s /usr/sbin/nologin sysguard
systemctl daemon-reload
AFTER_INSTALL

cat >"$STAGE/DEBIAN/prerm" <<AVANT_RETRAIT
#!/bin/sh
set -eu
systemctl stop sysguard.service sysguard-opt.service 2>/dev/null || true
AVANT_RETRAIT

cat >"$STAGE/DEBIAN/postrm" <<APRES_RETRAIT
#!/bin/sh
set -eu
systemctl daemon-reload
if [ "\$1" = purge ]; then
    id -u sysguard >/dev/null 2>&1 && userdel sysguard || true
    rm -rf /etc/sysguard
fi
APRES_RETRAIT

chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/prerm" "$STAGE/DEBIAN/postrm"

DEB="$RACINE/build/sysguard_${VERSION}-1_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$STAGE" "$DEB"
echo "$DEB"
