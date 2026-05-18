#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DEB="$REPO_ROOT/scripts/release/build-deb.sh"
PUBLISH_RELEASE="$REPO_ROOT/scripts/release/publish-github-release.sh"

dry_out="$("$BUILD_DEB" --dry-run)"
echo "$dry_out" | grep -q "cargo build --release" \
    || { echo "FAIL: build-deb dry-run missing release build"; exit 1; }
echo "$dry_out" | grep -q "dpkg-deb --build --root-owner-group" \
    || { echo "FAIL: build-deb dry-run missing dpkg-deb build"; exit 1; }

grep -q "Package: winbridge" "$BUILD_DEB" \
    || { echo "FAIL: Debian control missing package name"; exit 1; }
grep -q 'Architecture: $ARCH' "$BUILD_DEB" \
    || { echo "FAIL: Debian control missing architecture"; exit 1; }
grep -q "Depends: .*libgtk-4-1" "$BUILD_DEB" \
    || { echo "FAIL: Debian control missing GTK runtime dependency"; exit 1; }
grep -q "Depends: .*libvirt0" "$BUILD_DEB" \
    || { echo "FAIL: Debian control missing libvirt runtime dependency"; exit 1; }
grep -q "Recommends: gnome-shell-extension-appindicator" "$BUILD_DEB" \
    || { echo "FAIL: Debian control missing tray extension recommendation"; exit 1; }
grep -q 'Exec=$BIN_PATH start --mode app --display stable-slots' "$BUILD_DEB" \
    || { echo "FAIL: package desktop entry does not launch app mode"; exit 1; }
grep -q "dev.winbridge.WinbridgeApp.desktop" "$BUILD_DEB" \
    || { echo "FAIL: package missing reverse-DNS desktop entry"; exit 1; }
grep -q "winbridge.desktop" "$BUILD_DEB" \
    || { echo "FAIL: package missing simple desktop alias"; exit 1; }
grep -q "gtk-update-icon-cache -f -t /usr/share/icons/hicolor" "$BUILD_DEB" \
    || { echo "FAIL: maintainer scripts do not refresh icon cache"; exit 1; }

publish_dry_out="$("$PUBLISH_RELEASE" --dry-run)"
echo "$publish_dry_out" | grep -q "gh release create" \
    || { echo "FAIL: publish dry-run missing GitHub Release creation"; exit 1; }
echo "$publish_dry_out" | grep -q "build-deb.sh" \
    || { echo "FAIL: publish dry-run missing package build"; exit 1; }

echo "PASS: Debian package scripts are wired"
