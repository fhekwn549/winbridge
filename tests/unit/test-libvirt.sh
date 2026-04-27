#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/02-setup-libvirt.sh"

[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }

# --help mentions libvirt + key keywords
help_out=$("$TARGET" --help 2>&1)
echo "$help_out" | grep -qi "libvirt"      || { echo "FAIL: --help missing 'libvirt'"; exit 1; }
echo "$help_out" | grep -qi "pool\|매핑\|네트워크" || { echo "FAIL: --help missing key topic"; exit 1; }

# --dry-run runs without sudo, mentions intended operations
out=$("$TARGET" --dry-run 2>&1) || { echo "FAIL: --dry-run errored: $out"; exit 1; }
echo "$out" | grep -qi "dry"                   || { echo "FAIL: --dry-run no 'dry' indicator"; exit 1; }
echo "$out" | grep -q  "192.168.122.50"        || { echo "FAIL: --dry-run missing default IP"; exit 1; }
echo "$out" | grep -qE "52:54:00|MAC|mac"      || { echo "FAIL: --dry-run missing MAC info"; exit 1; }
echo "$out" | grep -qi "winbridge"             || { echo "FAIL: --dry-run missing pool/VM name"; exit 1; }
echo "$out" | grep -qi "apparmor\|abstractions" || { echo "FAIL: --dry-run missing AppArmor mention"; exit 1; }

# Override env vars work in dry-run
out2=$(WINBRIDGE_VM_IP=192.168.122.99 "$TARGET" --dry-run 2>&1)
echo "$out2" | grep -q "192.168.122.99" \
    || { echo "FAIL: WINBRIDGE_VM_IP override not honored"; exit 1; }

echo "PASS: test-libvirt.sh"
