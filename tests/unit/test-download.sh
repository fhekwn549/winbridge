#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/01-download-iso.sh"

[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }

# --help mentions download
"$TARGET" --help 2>&1 | grep -qi "download\|다운로드" \
    || { echo "FAIL: --help missing 'download/다운로드' keyword"; exit 1; }

# Missing required env vars => exit 1
rc=0; ( WINBRIDGE_ISO_URL="" WINBRIDGE_ISO_SHA256="" "$TARGET" ) >/dev/null 2>&1 || rc=$?
[ "$rc" -eq 0 ] && { echo "FAIL: should require WINBRIDGE_ISO_URL/SHA256"; exit 1; }

# Mock fixture
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "fake iso content $(date +%s%N)" > "$TMPDIR/fake.iso"
SHA=$(sha256sum "$TMPDIR/fake.iso" | cut -d' ' -f1)

# First run: should download and create sentinel
WINBRIDGE_ISO_URL="file://$TMPDIR/fake.iso" \
WINBRIDGE_ISO_SHA256="$SHA" \
WINBRIDGE_ISO_DEST="$TMPDIR/winbridge.iso" \
WINBRIDGE_SENTINEL_DIR="$TMPDIR/cache" \
"$TARGET" >/dev/null 2>&1 \
    || { echo "FAIL: first download failed"; exit 1; }

[ -f "$TMPDIR/winbridge.iso" ] || { echo "FAIL: ISO file missing after download"; exit 1; }
[ -f "$TMPDIR/cache/01-iso-downloaded.done" ] || { echo "FAIL: sentinel missing"; exit 1; }

# Second run: skip (sentinel + verify pass) and indicate skip in output
out=$(WINBRIDGE_ISO_URL="file://$TMPDIR/fake.iso" \
      WINBRIDGE_ISO_SHA256="$SHA" \
      WINBRIDGE_ISO_DEST="$TMPDIR/winbridge.iso" \
      WINBRIDGE_SENTINEL_DIR="$TMPDIR/cache" \
      "$TARGET" 2>&1)
echo "$out" | grep -qi "skip\|이미\|already" \
    || { echo "FAIL: second run missing skip indicator. Output: $out"; exit 1; }

# sha256 mismatch (sentinel must be invalidated, file deleted, exit non-zero)
TMPDIR2=$(mktemp -d)
trap 'rm -rf "$TMPDIR" "$TMPDIR2"' EXIT
rc=0
WINBRIDGE_ISO_URL="file://$TMPDIR/fake.iso" \
WINBRIDGE_ISO_SHA256="0000000000000000000000000000000000000000000000000000000000000000" \
WINBRIDGE_ISO_DEST="$TMPDIR2/winbridge.iso" \
WINBRIDGE_SENTINEL_DIR="$TMPDIR2/cache" \
"$TARGET" >/dev/null 2>&1 || rc=$?
[ "$rc" -eq 0 ] && { echo "FAIL: should fail on sha256 mismatch"; exit 1; }
[ -f "$TMPDIR2/winbridge.iso" ] && { echo "FAIL: bad ISO not deleted on mismatch"; exit 1; }

# Sentinel-but-mismatched-file scenario: pre-create sentinel with wrong file, expect re-download
TMPDIR3=$(mktemp -d)
trap 'rm -rf "$TMPDIR" "$TMPDIR2" "$TMPDIR3"' EXIT
mkdir -p "$TMPDIR3/cache"
echo "stale content" > "$TMPDIR3/winbridge.iso"
touch "$TMPDIR3/cache/01-iso-downloaded.done"
WINBRIDGE_ISO_URL="file://$TMPDIR/fake.iso" \
WINBRIDGE_ISO_SHA256="$SHA" \
WINBRIDGE_ISO_DEST="$TMPDIR3/winbridge.iso" \
WINBRIDGE_SENTINEL_DIR="$TMPDIR3/cache" \
"$TARGET" >/dev/null 2>&1 \
    || { echo "FAIL: should re-download when sentinel stale"; exit 1; }
ACTUAL_SHA=$(sha256sum "$TMPDIR3/winbridge.iso" | cut -d' ' -f1)
[ "$ACTUAL_SHA" = "$SHA" ] || { echo "FAIL: stale file not replaced after re-download"; exit 1; }

echo "PASS: test-download.sh"
