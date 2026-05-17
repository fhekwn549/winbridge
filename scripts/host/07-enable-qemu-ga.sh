#!/usr/bin/env bash
# scripts/host/07-enable-qemu-ga.sh
# Existing VM retrofit: attach virtio-win ISO and QEMU guest agent channel.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_NAME:=winbridge-srv2022}"
: "${WINBRIDGE_LIBVIRT_URI:=qemu:///system}"
: "${WINBRIDGE_VIRTIO_ISO_PATH:=${WINBRIDGE_VIRTIO_ISO_DEST:-$HOME/.cache/winbridge/virtio-win.iso}}"
: "${WINBRIDGE_BUILD_DIR:=$REPO_ROOT/build}"

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

usage() {
    cat <<USAGE
사용법: $0 [--help|--dry-run]
기존 VM에 QEMU guest agent용 libvirt channel과 virtio-win ISO를 붙입니다.

필수:
  WINBRIDGE_VIRTIO_ISO_PATH 또는 WINBRIDGE_VIRTIO_ISO_DEST

이후 Windows 안에서 다음 중 하나를 실행하세요:
  virtio-win-guest-tools.exe
  guest-agent\\qemu-ga-x86_64.msi
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

[ -f "$WINBRIDGE_VIRTIO_ISO_PATH" ] || {
    log_error "virtio-win ISO 부재: $WINBRIDGE_VIRTIO_ISO_PATH"
    log_error "먼저 WINBRIDGE_ENABLE_QEMU_GA=1 scripts/host/01-download-iso.sh 실행"
    exit 1
}

require_cmd virsh
mkdir -p "$WINBRIDGE_BUILD_DIR"

VIRTIO_XML="$WINBRIDGE_BUILD_DIR/qemu-ga-virtio-cdrom.xml"
CHANNEL_XML="$WINBRIDGE_BUILD_DIR/qemu-ga-channel.xml"

cat > "$VIRTIO_XML" <<XML
<disk type="file" device="cdrom">
  <driver name="qemu" type="raw"/>
  <source file="$WINBRIDGE_VIRTIO_ISO_PATH"/>
  <target dev="sdd" bus="sata"/>
  <readonly/>
</disk>
XML

cat > "$CHANNEL_XML" <<'XML'
<channel type="unix">
  <target type="virtio" name="org.qemu.guest_agent.0"/>
</channel>
XML

if [ $DRY_RUN -eq 1 ]; then
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI attach-device $WINBRIDGE_VM_NAME $CHANNEL_XML --config"
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI attach-device $WINBRIDGE_VM_NAME $VIRTIO_XML --config"
    log_info "[dry-run] live VM이면 --live도 시도"
    log_info "[dry-run] channel XML:"
    sed 's/^/  /' "$CHANNEL_XML"
    log_info "[dry-run] virtio ISO XML:"
    sed 's/^/  /' "$VIRTIO_XML"
    exit 0
fi

sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$CHANNEL_XML" --config || true
sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$VIRTIO_XML" --config || true

if sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" domstate "$WINBRIDGE_VM_NAME" | grep -qi running; then
    sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$CHANNEL_XML" --live || true
    sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$VIRTIO_XML" --live || true
fi

log_info "QEMU guest agent channel/virtio-win ISO attach attempted"
log_info "Windows에서 virtio-win guest tools 또는 guest-agent\\qemu-ga-x86_64.msi 설치 후 VM 재시작"
