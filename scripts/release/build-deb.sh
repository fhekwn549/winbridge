#!/usr/bin/env bash
# Build a local Debian package for winbridge.

set -euo pipefail
umask 022

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
    cat <<USAGE
Usage: $0 [--help|--dry-run]

Builds:
  dist/winbridge_<version>_<arch>.deb

The package installs:
  /usr/bin/winbridge
  /usr/share/applications/dev.winbridge.WinbridgeApp.desktop
  /usr/share/applications/winbridge.desktop
  /usr/share/icons/hicolor/256x256/apps/winbridge.png

Package builds intended to support Ubuntu 22.04 and newer should be produced on
Ubuntu 22.04 so the glibc baseline remains compatible.
USAGE
}

DRY_RUN=0
case "${1:-}" in
    --help) usage; exit 0 ;;
    --dry-run) DRY_RUN=1 ;;
    "") ;;
    *) usage >&2; exit 2 ;;
esac

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: missing command: $1" >&2
        echo "       $2" >&2
        exit 1
    fi
}

require_cmd cargo "Install Rust first: https://rustup.rs"
require_cmd dpkg-deb "Install dpkg-dev/core Debian packaging tools."
require_cmd dpkg "Install dpkg."
require_cmd install "Install coreutils."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
if [ -z "$VERSION" ]; then
    echo "ERROR: failed to read package version from Cargo.toml" >&2
    exit 1
fi

ARCH="${WINBRIDGE_DEB_ARCH:-$(dpkg --print-architecture)}"
if [ "$ARCH" != "amd64" ]; then
    echo "ERROR: unsupported Debian architecture: $ARCH" >&2
    echo "       winbridge packaging is currently verified for amd64 only." >&2
    exit 1
fi

PKG_NAME="winbridge"
OUT_DIR="${WINBRIDGE_DEB_OUT_DIR:-$REPO_ROOT/dist}"
WORK_DIR="$REPO_ROOT/target/deb/${PKG_NAME}_${VERSION}_${ARCH}"
PKG_ROOT="$WORK_DIR/pkg"
DEB_PATH="$OUT_DIR/${PKG_NAME}_${VERSION}_${ARCH}.deb"
BIN_PATH="/usr/bin/winbridge"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "cargo build --release"
    echo "install -m 755 target/release/winbridge $PKG_ROOT$BIN_PATH"
    echo "dpkg-deb --build --root-owner-group $PKG_ROOT $DEB_PATH"
    exit 0
fi

echo "[INFO] Building winbridge release binary..."
(cd "$REPO_ROOT" && cargo build --release)

rm -rf "$WORK_DIR"
mkdir -p "$PKG_ROOT/DEBIAN" "$OUT_DIR"

install -Dm755 "$REPO_ROOT/target/release/winbridge" "$PKG_ROOT$BIN_PATH"
install -Dm644 "$REPO_ROOT/assets/icons/winbridge.png" \
    "$PKG_ROOT/usr/share/icons/hicolor/256x256/apps/winbridge.png"

desktop_entry() {
    cat <<DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=winbridge
Comment=Open Winbridge through the Windows VM
Exec=$BIN_PATH start --mode app --display stable-slots
Icon=winbridge
Terminal=false
Categories=Network;InstantMessaging;
StartupNotify=true
StartupWMClass=dev.winbridge.WinbridgeApp
DESKTOP
}

install -d "$PKG_ROOT/usr/share/applications"
desktop_entry > "$PKG_ROOT/usr/share/applications/dev.winbridge.WinbridgeApp.desktop"
desktop_entry > "$PKG_ROOT/usr/share/applications/winbridge.desktop"

install -d "$PKG_ROOT/usr/share/doc/winbridge"
gzip -n -9 -c > "$PKG_ROOT/usr/share/doc/winbridge/changelog.gz" <<CHANGELOG
winbridge ($VERSION) unstable; urgency=medium

  * Build winbridge Debian package.

 -- fhekwn549 <fhekwn549@users.noreply.github.com>  Mon, 18 May 2026 00:00:00 +0900
CHANGELOG

install -m 644 "$REPO_ROOT/LICENSE" "$PKG_ROOT/usr/share/doc/winbridge/copyright"

cat > "$PKG_ROOT/DEBIAN/control" <<CONTROL
Package: winbridge
Version: $VERSION
Section: net
Priority: optional
Architecture: $ARCH
Maintainer: fhekwn549 <fhekwn549@users.noreply.github.com>
Depends: libc6 (>= 2.35), libgtk-4-1, libvirt0, libvirt-daemon-system, libvirt-clients, qemu-system-x86, qemu-utils
Recommends: gnome-shell-extension-appindicator
Description: Linux-native Windows app bridge for KakaoTalk
 winbridge runs the official Windows KakaoTalk client inside a libvirt/KVM
 Windows VM and exposes it as a small Linux desktop app through RDP.
CONTROL

cat > "$PKG_ROOT/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi

exit 0
POSTINST

cat > "$PKG_ROOT/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi

exit 0
POSTRM

chmod 755 "$PKG_ROOT/DEBIAN/postinst" "$PKG_ROOT/DEBIAN/postrm"
chmod 644 "$PKG_ROOT/DEBIAN/control"

echo "[INFO] Building Debian package..."
dpkg-deb --build --root-owner-group "$PKG_ROOT" "$DEB_PATH"

echo "[INFO] Debian package written: $DEB_PATH"
