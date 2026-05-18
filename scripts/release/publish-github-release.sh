#!/usr/bin/env bash
# Publish the current Debian package to a GitHub Release.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
    cat <<USAGE
Usage: $0 [--help] [--dry-run] [--tag vX.Y.Z] [--draft] [--prerelease]

Builds the .deb package when missing, creates or reuses a GitHub Release, and
uploads:
  dist/winbridge_<version>_<arch>.deb

Default tag:
  v<version from Cargo.toml>
USAGE
}

DRY_RUN=0
DRAFT=0
PRERELEASE=0
TAG=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --help) usage; exit 0 ;;
        --dry-run) DRY_RUN=1; shift ;;
        --draft) DRAFT=1; shift ;;
        --prerelease) PRERELEASE=1; shift ;;
        --tag)
            TAG="${2:-}"
            [ -n "$TAG" ] || { usage >&2; exit 2; }
            shift 2
            ;;
        *) usage >&2; exit 2 ;;
    esac
done

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: missing command: $1" >&2
        echo "       $2" >&2
        exit 1
    fi
}

require_cmd gh "Install GitHub CLI and authenticate with: gh auth login"
require_cmd dpkg "Install dpkg."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
ARCH="${WINBRIDGE_DEB_ARCH:-$(dpkg --print-architecture)}"
TAG="${TAG:-v$VERSION}"
DEB_PATH="${WINBRIDGE_DEB_OUT_DIR:-$REPO_ROOT/dist}/winbridge_${VERSION}_${ARCH}.deb"

if [ "$DRY_RUN" -eq 1 ]; then
    echo "$SCRIPT_DIR/build-deb.sh"
    echo "git push origin $TAG"
    echo "gh release create $TAG $DEB_PATH --title winbridge $TAG --notes-file <generated>"
    exit 0
fi

if [ ! -f "$DEB_PATH" ]; then
    "$SCRIPT_DIR/build-deb.sh"
fi

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
    git tag -a "$TAG" -m "winbridge $TAG"
fi

git push origin "$TAG"

notes_file="$(mktemp)"
trap 'rm -f "$notes_file"' EXIT
cat > "$notes_file" <<NOTES
winbridge $TAG

Tested support:
- Ubuntu 22.04.5 LTS

Pending validation:
- Ubuntu 24.04 LTS: https://github.com/fhekwn549/winbridge/issues/2

Install:
\`\`\`bash
sudo apt install ./$(basename "$DEB_PATH")
\`\`\`
NOTES

flags=(--title "winbridge $TAG" --notes-file "$notes_file")
[ "$DRAFT" -eq 1 ] && flags+=(--draft)
[ "$PRERELEASE" -eq 1 ] && flags+=(--prerelease)

if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$DEB_PATH" --clobber
else
    gh release create "$TAG" "$DEB_PATH" "${flags[@]}"
fi

echo "[INFO] GitHub Release ready: $TAG"
