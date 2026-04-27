#!/usr/bin/env bash
# scripts/host/03-create-vm.sh
# OEM ISO 생성 (autounattend + firstboot.ps1) + qcow2 디스크 + libvirt VM define+start.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_NAME:=winbridge-srv2022}"
: "${WINBRIDGE_VM_RAM_KB:=4194304}"
: "${WINBRIDGE_VM_VCPU:=2}"
: "${WINBRIDGE_VM_DISK_GB:=60}"
: "${WINBRIDGE_VM_MAC:=52:54:00:7B:01:01}"
: "${WINBRIDGE_HOME_POOL_DIR:=$HOME/.local/share/libvirt/images}"
: "${WINBRIDGE_VM_DISK_PATH:=$WINBRIDGE_HOME_POOL_DIR/$WINBRIDGE_VM_NAME.qcow2}"
: "${WINBRIDGE_ISO_PATH:=$HOME/.cache/winbridge/server2022.iso}"
: "${WINBRIDGE_HOSTNAME:=$WINBRIDGE_VM_NAME}"
: "${WINBRIDGE_TIMEZONE:=Korea Standard Time}"
: "${WINBRIDGE_BUILD_DIR:=$REPO_ROOT/build}"
: "${WINBRIDGE_LIBVIRT_URI:=qemu:///system}"

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

usage() {
    cat <<USAGE
사용법: $0 [--help|--dry-run]
OEM ISO 생성 후 qcow2 디스크 생성 + libvirt VM define + start.
필수 환경변수:
  WINBRIDGE_ADMIN_PASSWORD  Administrator 비밀번호 (install.sh가 random 생성/영속화)
선택 환경변수 (기본값 있음):
  WINBRIDGE_VM_NAME, WINBRIDGE_VM_RAM_KB, WINBRIDGE_VM_VCPU,
  WINBRIDGE_VM_DISK_GB, WINBRIDGE_VM_MAC, WINBRIDGE_HOME_POOL_DIR,
  WINBRIDGE_VM_DISK_PATH, WINBRIDGE_ISO_PATH, WINBRIDGE_HOSTNAME,
  WINBRIDGE_TIMEZONE, WINBRIDGE_BUILD_DIR, WINBRIDGE_LIBVIRT_URI
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

[ -z "${WINBRIDGE_ADMIN_PASSWORD:-}" ] && { log_error "WINBRIDGE_ADMIN_PASSWORD 미설정"; exit 1; }
[ -f "$WINBRIDGE_ISO_PATH" ] || { log_error "Server 2022 ISO 부재: $WINBRIDGE_ISO_PATH"; exit 1; }

require_cmd virsh
require_cmd virt-install || true   # virt-install 실제론 안 쓰지만 prerequisites 체크용
require_cmd genisoimage
require_cmd qemu-img
require_cmd envsubst

mkdir -p "$WINBRIDGE_BUILD_DIR" "$WINBRIDGE_HOME_POOL_DIR"

# 1. 기존 VM 정의 체크 (idempotent: 이미 있으면 명확한 에러 + uninstall 안내)
if [ $DRY_RUN -eq 0 ] && sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" dominfo "$WINBRIDGE_VM_NAME" >/dev/null 2>&1; then
    log_error "VM '$WINBRIDGE_VM_NAME' 이미 존재. 재생성하려면 ./uninstall.sh 먼저 실행"
    exit 1
fi

# 2. OEM 디렉토리 구성 (autounattend + firstboot.ps1)
OEM_DIR="$WINBRIDGE_BUILD_DIR/oem"
rm -rf "$OEM_DIR"
mkdir -p "$OEM_DIR"

export WINBRIDGE_HOSTNAME WINBRIDGE_ADMIN_PASSWORD WINBRIDGE_TIMEZONE
log_info "autounattend.xml 렌더..."
render_template "$REPO_ROOT/config/autounattend.xml.template" > "$OEM_DIR/autounattend.xml"

log_info "firstboot.ps1 복사..."
cp "$REPO_ROOT/config/firstboot.ps1" "$OEM_DIR/firstboot.ps1"

# 3. OEM ISO 생성
OEM_ISO="$WINBRIDGE_BUILD_DIR/oem.iso"
log_info "OEM ISO 생성 ($OEM_ISO)..."
genisoimage -quiet -joliet -rational-rock -output "$OEM_ISO" "$OEM_DIR"
log_info "  $(stat -c%s "$OEM_ISO") bytes"

# 4. qcow2 디스크 생성 (없으면)
if [ ! -f "$WINBRIDGE_VM_DISK_PATH" ]; then
    log_info "qcow2 디스크 생성 ($WINBRIDGE_VM_DISK_PATH, ${WINBRIDGE_VM_DISK_GB}G)..."
    qemu-img create -f qcow2 "$WINBRIDGE_VM_DISK_PATH" "${WINBRIDGE_VM_DISK_GB}G"
else
    log_info "qcow2 디스크 이미 존재, skip ($WINBRIDGE_VM_DISK_PATH)"
fi

# 5. libvirt VM XML 렌더
export WINBRIDGE_VM_NAME WINBRIDGE_VM_RAM_KB WINBRIDGE_VM_VCPU \
    WINBRIDGE_VM_DISK_PATH WINBRIDGE_VM_MAC WINBRIDGE_ISO_PATH
WINBRIDGE_OEM_ISO_PATH="$OEM_ISO" \
    render_template "$REPO_ROOT/config/libvirt-vm.xml.template" > "$WINBRIDGE_BUILD_DIR/vm.xml"
log_info "libvirt VM XML 렌더: $WINBRIDGE_BUILD_DIR/vm.xml"

if [ $DRY_RUN -eq 1 ]; then
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI define $WINBRIDGE_BUILD_DIR/vm.xml"
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI start $WINBRIDGE_VM_NAME"
    log_info "[dry-run] 렌더된 XML 미리보기:"
    grep -E "<(name|memory|vcpu|source|model|target dev)" "$WINBRIDGE_BUILD_DIR/vm.xml" | head -15
    exit 0
fi

# 6. VM define + start
sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" define "$WINBRIDGE_BUILD_DIR/vm.xml"
sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" start "$WINBRIDGE_VM_NAME"

log_info "VM '$WINBRIDGE_VM_NAME' 시작됨"
log_info "다음 단계: 04-wait-for-install.sh로 무인 설치 + firstboot.ps1 완료 대기 (~30~50분)"
