#!/usr/bin/env bash
# Build a static APT repository from winbridge Debian package artifacts.

set -euo pipefail
umask 022

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
    cat <<USAGE
Usage: $0 [--help|--dry-run]

Builds:
  dist/apt/pool/main/w/winbridge/winbridge_<version>_amd64.deb
  dist/apt/dists/stable/main/binary-amd64/Packages
  dist/apt/dists/stable/main/binary-amd64/Packages.gz
  dist/apt/dists/stable/Release

This creates an unsigned static APT repository. Until repository signing is
added, test clients must configure it with arch=amd64 and trusted=yes.
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

require_cmd dpkg "Install dpkg."
require_cmd dpkg-scanpackages "Install dpkg-dev."
require_cmd apt-ftparchive "Install apt-utils."
require_cmd gzip "Install gzip."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
ARCH="${WINBRIDGE_DEB_ARCH:-$(dpkg --print-architecture)}"
DEB_DIR="${WINBRIDGE_DEB_OUT_DIR:-$REPO_ROOT/dist}"
DEB_PATH="$DEB_DIR/winbridge_${VERSION}_${ARCH}.deb"
APT_ROOT="${WINBRIDGE_APT_REPO_DIR:-$REPO_ROOT/dist/apt}"
POOL_DIR="$APT_ROOT/pool/main/w/winbridge"
BINARY_DIR="$APT_ROOT/dists/stable/main/binary-$ARCH"
RELEASE_DIR="$APT_ROOT/dists/stable"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "$SCRIPT_DIR/build-deb.sh"
    echo "install -m 644 $DEB_PATH $POOL_DIR/"
    echo "dpkg-scanpackages --arch $ARCH pool > dists/stable/main/binary-$ARCH/Packages"
    echo "apt-ftparchive release dists/stable > dists/stable/Release"
    exit 0
fi

if [ ! -f "$DEB_PATH" ]; then
    "$SCRIPT_DIR/build-deb.sh"
fi

rm -rf "$APT_ROOT"
install -d "$POOL_DIR" "$BINARY_DIR"
install -m 644 "$DEB_PATH" "$POOL_DIR/"

(
    cd "$APT_ROOT"
    dpkg-scanpackages --arch "$ARCH" pool > "dists/stable/main/binary-$ARCH/Packages"
    gzip -n -9 -c "dists/stable/main/binary-$ARCH/Packages" > "dists/stable/main/binary-$ARCH/Packages.gz"
    apt-ftparchive \
        -o APT::FTPArchive::Release::Origin="winbridge" \
        -o APT::FTPArchive::Release::Label="winbridge" \
        -o APT::FTPArchive::Release::Suite="stable" \
        -o APT::FTPArchive::Release::Codename="stable" \
        -o APT::FTPArchive::Release::Architectures="$ARCH" \
        -o APT::FTPArchive::Release::Components="main" \
        -o APT::FTPArchive::Release::Description="winbridge APT repository" \
        release dists/stable > "$RELEASE_DIR/Release"
)

echo "[INFO] APT repository written: $APT_ROOT"
echo "[INFO] Local test source example:"
echo "deb [arch=$ARCH trusted=yes] file:$APT_ROOT stable main"
