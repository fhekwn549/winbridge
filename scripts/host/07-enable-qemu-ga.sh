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
: "${WINBRIDGE_VIRTIO_CDROM_DEV:=sde}"

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

grant_iso_access() {
    if ! command -v setfacl >/dev/null 2>&1; then
        log_warn "setfacl 없음; libvirt-qemu가 virtio ISO 경로를 읽지 못하면 VM start가 실패할 수 있음"
        return 0
    fi

    local dir
    dir="$(dirname "$WINBRIDGE_VIRTIO_ISO_PATH")"
    setfacl -m u:libvirt-qemu:x "$HOME" 2>/dev/null || true
    setfacl -m u:libvirt-qemu:x "$HOME/.cache" 2>/dev/null || true
    setfacl -m u:libvirt-qemu:x "$dir" 2>/dev/null || true
    setfacl -m u:libvirt-qemu:r "$WINBRIDGE_VIRTIO_ISO_PATH" 2>/dev/null || true
}

grant_iso_access

VIRTIO_XML="$WINBRIDGE_BUILD_DIR/qemu-ga-virtio-cdrom.xml"
CHANNEL_XML="$WINBRIDGE_BUILD_DIR/qemu-ga-channel.xml"

cat > "$VIRTIO_XML" <<XML
<disk type="file" device="cdrom">
  <driver name="qemu" type="raw"/>
  <source file="$WINBRIDGE_VIRTIO_ISO_PATH"/>
  <target dev="$WINBRIDGE_VIRTIO_CDROM_DEV" bus="sata"/>
  <readonly/>
</disk>
XML

cat > "$CHANNEL_XML" <<'XML'
<channel type="unix">
  <target type="virtio" name="org.qemu.guest_agent.0"/>
</channel>
XML

if [ $DRY_RUN -eq 1 ]; then
    log_info "[dry-run] virsh -c $WINBRIDGE_LIBVIRT_URI attach-device $WINBRIDGE_VM_NAME $CHANNEL_XML --config"
    log_info "[dry-run] virsh -c $WINBRIDGE_LIBVIRT_URI attach-device $WINBRIDGE_VM_NAME $VIRTIO_XML --config"
    log_info "[dry-run] live VM이면 --live도 시도"
    log_info "[dry-run] channel XML:"
    sed 's/^/  /' "$CHANNEL_XML"
    log_info "[dry-run] virtio ISO XML:"
    sed 's/^/  /' "$VIRTIO_XML"
    exit 0
fi

virsh_attach() {
    local xml="$1"
    local mode="$2"
    local output

    if output=$(virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$xml" "$mode" 2>&1); then
        log_info "attached $xml $mode"
        return 0
    fi

    case "$output" in
        *"already exists"*|*"target ${WINBRIDGE_VIRTIO_CDROM_DEV} already exists"*)
            log_warn "already attached or target busy: $xml $mode"
            return 0
            ;;
        *"cdrom/floppy device hotplug isn't supported"*)
            log_warn "CD-ROM live hotplug unsupported; VM restart will apply config attach"
            return 1
            ;;
        *"authentication unavailable"*|*"access denied"*|*"permission denied"*)
            log_warn "virsh without sudo failed; trying sudo: $output"
            sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" attach-device "$WINBRIDGE_VM_NAME" "$xml" "$mode"
            return
            ;;
        *)
            log_error "attach failed: $xml $mode"
            log_error "$output"
            return 1
            ;;
    esac
}

virsh_attach "$CHANNEL_XML" --config
virsh_attach "$VIRTIO_XML" --config

if virsh -c "$WINBRIDGE_LIBVIRT_URI" domstate "$WINBRIDGE_VM_NAME" | grep -qi running; then
    virsh_attach "$CHANNEL_XML" --live
    virsh_attach "$VIRTIO_XML" --live || true
fi

log_info "QEMU guest agent channel/virtio-win ISO attach attempted"
log_info "Windows에서 virtio-win guest tools 또는 guest-agent\\qemu-ga-x86_64.msi 설치 후 VM 재시작"
