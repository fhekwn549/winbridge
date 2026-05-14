#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/06-setup-file-share.sh"

[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }

help_out=$("$TARGET" --help 2>&1)
echo "$help_out" | grep -qi "Windows VM" || { echo "FAIL: --help missing Windows VM"; exit 1; }
echo "$help_out" | grep -q -- "--home" || { echo "FAIL: --help missing --home"; exit 1; }
echo "$help_out" | grep -q "\\\\\\\\192.168.122.1\\\\winbridge" \
    || { echo "FAIL: --help missing default UNC path"; exit 1; }

out=$("$TARGET" --dry-run 2>&1)
echo "$out" | grep -qi "dry" || { echo "FAIL: --dry-run no dry indicator"; exit 1; }
echo "$out" | grep -q "$HOME/WinbridgeShare" \
    || { echo "FAIL: --dry-run missing default share dir"; exit 1; }
echo "$out" | grep -q "\\\\\\\\192.168.122.1\\\\winbridge" \
    || { echo "FAIL: --dry-run missing default Windows path"; exit 1; }

out2=$(WINBRIDGE_SHARE_DIR="$HOME/CustomShare" WINBRIDGE_SHARE_NAME=custom "$TARGET" --dry-run 2>&1)
echo "$out2" | grep -q "$HOME/CustomShare" \
    || { echo "FAIL: WINBRIDGE_SHARE_DIR override not honored"; exit 1; }
echo "$out2" | grep -q "\\\\\\\\192.168.122.1\\\\custom" \
    || { echo "FAIL: WINBRIDGE_SHARE_NAME override not honored"; exit 1; }

out3=$("$TARGET" --home --dry-run 2>&1)
echo "$out3" | grep -q "path=$HOME" \
    || { echo "FAIL: --home does not set share path to HOME"; exit 1; }
echo "$out3" | grep -q "\\\\\\\\192.168.122.1\\\\winbridge" \
    || { echo "FAIL: --home changed the default UNC path"; exit 1; }

echo "PASS: test-file-share.sh"
