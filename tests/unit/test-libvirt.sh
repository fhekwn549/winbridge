#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/02-setup-libvirt.sh"
QGA_TARGET="$REPO_ROOT/scripts/host/07-enable-qemu-ga.sh"

[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }
[ -x "$QGA_TARGET" ] || { echo "FAIL: $QGA_TARGET missing or not executable"; exit 1; }

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

# VM XML template supports optional QEMU guest agent channel.
grep -q "WINBRIDGE_QEMU_GA_CHANNEL_XML" "$REPO_ROOT/config/libvirt-vm.xml.template" \
    || { echo "FAIL: libvirt template missing QEMU guest agent channel placeholder"; exit 1; }
grep -q "WINBRIDGE_VIRTIO_DISK_XML" "$REPO_ROOT/config/libvirt-vm.xml.template" \
    || { echo "FAIL: libvirt template missing virtio-win ISO placeholder"; exit 1; }

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
touch "$TMPDIR/virtio-win.iso"
qga_out=$(WINBRIDGE_VIRTIO_ISO_PATH="$TMPDIR/virtio-win.iso" "$QGA_TARGET" --dry-run 2>&1)
echo "$qga_out" | grep -q "org.qemu.guest_agent.0" \
    || { echo "FAIL: qemu-ga dry-run missing guest agent channel"; exit 1; }
echo "$qga_out" | grep -qi "virtio" \
    || { echo "FAIL: qemu-ga dry-run missing virtio ISO"; exit 1; }
grep -q "setfacl" "$QGA_TARGET" \
    || { echo "FAIL: qemu-ga retrofit does not grant libvirt-qemu ISO access"; exit 1; }

echo "PASS: test-libvirt.sh"
