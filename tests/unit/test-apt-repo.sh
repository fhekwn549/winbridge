#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_APT_REPO="$REPO_ROOT/scripts/release/build-apt-repo.sh"
PUBLISH_APT_REPO="$REPO_ROOT/scripts/release/publish-apt-repo-gh-pages.sh"

dry_out="$("$BUILD_APT_REPO" --dry-run)"
echo "$dry_out" | grep -q "build-deb.sh" \
    || { echo "FAIL: APT repo dry-run missing Debian package build"; exit 1; }
echo "$dry_out" | grep -q "dpkg-scanpackages --arch" \
    || { echo "FAIL: APT repo dry-run missing Packages index generation"; exit 1; }
echo "$dry_out" | grep -q "apt-ftparchive release" \
    || { echo "FAIL: APT repo dry-run missing Release generation"; exit 1; }

grep -q 'arch=$ARCH trusted=yes' "$BUILD_APT_REPO" \
    || { echo "FAIL: unsigned APT repo script does not explain arch-scoped trusted=yes setup"; exit 1; }
grep -q 'APT::FTPArchive::Release::Codename="stable"' "$BUILD_APT_REPO" \
    || { echo "FAIL: APT repo script does not set Release codename"; exit 1; }
grep -q "Packages.gz" "$BUILD_APT_REPO" \
    || { echo "FAIL: APT repo script does not create compressed package index"; exit 1; }
grep -q "pool/main/w/winbridge" "$BUILD_APT_REPO" \
    || { echo "FAIL: APT repo script does not use Debian pool layout"; exit 1; }

publish_dry_out="$("$PUBLISH_APT_REPO" --dry-run)"
echo "$publish_dry_out" | grep -q "git push origin HEAD:gh-pages" \
    || { echo "FAIL: APT repo publish dry-run missing gh-pages push"; exit 1; }
echo "$publish_dry_out" | grep -q "build-apt-repo.sh" \
    || { echo "FAIL: APT repo publish dry-run missing repo build"; exit 1; }

echo "PASS: APT repository script is wired"
