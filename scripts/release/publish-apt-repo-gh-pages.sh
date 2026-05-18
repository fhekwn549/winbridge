#!/usr/bin/env bash
# Publish dist/apt as a static APT repository on the gh-pages branch.

set -euo pipefail
umask 022

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
    cat <<USAGE
Usage: $0 [--help|--dry-run]

Builds dist/apt when needed, then publishes that directory to the gh-pages
branch. GitHub Pages must serve the gh-pages branch root for the public APT URL
to work.
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

require_cmd git "Install git."

APT_ROOT="${WINBRIDGE_APT_REPO_DIR:-$REPO_ROOT/dist/apt}"
if [ "$DRY_RUN" -eq 1 ]; then
    echo "$SCRIPT_DIR/build-apt-repo.sh"
    echo "git worktree add <tmp> origin/gh-pages || create orphan gh-pages"
    echo "copy dist/apt to gh-pages branch root"
    echo "git push origin HEAD:gh-pages"
    exit 0
fi

if [ ! -f "$APT_ROOT/dists/stable/Release" ]; then
    "$SCRIPT_DIR/build-apt-repo.sh"
fi

worktree="$(mktemp -d)"
cleanup() {
    git -C "$REPO_ROOT" worktree remove --force "$worktree" >/dev/null 2>&1 || true
}
trap cleanup EXIT

if git -C "$REPO_ROOT" ls-remote --exit-code --heads origin gh-pages >/dev/null 2>&1; then
    git -C "$REPO_ROOT" worktree add "$worktree" origin/gh-pages
else
    git -C "$REPO_ROOT" worktree add --detach "$worktree"
    git -C "$worktree" checkout --orphan gh-pages
fi

find "$worktree" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -a "$APT_ROOT/." "$worktree/"

cat > "$worktree/README.md" <<'README'
# winbridge APT repository

This branch is generated from `scripts/release/publish-apt-repo-gh-pages.sh`.

Current repository layout:

```text
dists/stable/main/binary-amd64/Packages.gz
pool/main/w/winbridge/
```

The repository is unsigned at the moment. Use only for early validation.
README

git -C "$worktree" add -A
if git -C "$worktree" diff --cached --quiet; then
    echo "[INFO] gh-pages APT repository already up to date"
else
    git -C "$worktree" commit -m "Publish winbridge APT repository"
    git -C "$worktree" push origin HEAD:gh-pages
fi

echo "[INFO] gh-pages APT repository published"
