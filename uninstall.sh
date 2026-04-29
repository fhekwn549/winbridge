#!/usr/bin/env bash
# uninstall.sh
# winbridge VM/매핑/AppArmor/pool/ISO/credentials 제거. 사용자 컨펌 prompt 포함.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"
# shellcheck source=scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_NAME:=winbridge-srv2022}"
: "${WINBRIDGE_LIBVIRT_URI:=qemu:///system}"

ASSUME_YES=0

usage() {
    cat <<USAGE
사용법: $0 [--help|-y|--yes]
winbridge 설치 자원 제거. 각 단계마다 y/N 컨펌.
  --yes, -y    모든 prompt에 자동 y (CI/regression 테스트용)
USAGE
}

case "${1:-}" in
    --help) usage; exit 0 ;;
    -y|--yes) ASSUME_YES=1 ;;
esac

confirm() {
    if [ "$ASSUME_YES" -eq 1 ]; then
        return 0
    fi
    read -r -p "$1 [y/N] " ans
    [[ "$ans" =~ ^[yY]$ ]]
}

# Master confirm
if ! confirm "winbridge VM '$WINBRIDGE_VM_NAME' 및 관련 자원 모두 삭제. 진행?"; then
    log_info "취소"
    exit 0
fi

# 1. VM destroy + undefine + storage
log_info "1. VM 정리..."
if sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" dominfo "$WINBRIDGE_VM_NAME" >/dev/null 2>&1; then
    sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" destroy "$WINBRIDGE_VM_NAME" 2>/dev/null || true
    sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" undefine "$WINBRIDGE_VM_NAME" --remove-all-storage --nvram 2>/dev/null \
        || sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" undefine "$WINBRIDGE_VM_NAME" --remove-all-storage \
        || sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" undefine "$WINBRIDGE_VM_NAME"
    log_info "  VM 삭제: $WINBRIDGE_VM_NAME"
else
    log_info "  VM 부재, skip"
fi

# 2. libvirt 정적 매핑 제거
log_info "2. libvirt 정적 매핑 정리..."
if sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" net-dumpxml default 2>/dev/null | grep -q "name='$WINBRIDGE_VM_NAME'"; then
    sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" net-update default delete ip-dhcp-host \
        "<host name='$WINBRIDGE_VM_NAME'/>" --live --config 2>/dev/null \
        || log_warn "  매핑 제거 실패 (수동 정리 필요)"
    log_info "  매핑 제거"
else
    log_info "  매핑 부재, skip"
fi

# 3. AppArmor (선택)
APPARMOR_LOCAL=/etc/apparmor.d/local/abstractions/libvirt-qemu
log_info "3. AppArmor abstractions..."
if [ -f "$APPARMOR_LOCAL" ] && sudo grep -q "^# winbridge:BEGIN$" "$APPARMOR_LOCAL" 2>/dev/null; then
    if confirm "  $APPARMOR_LOCAL의 winbridge 블록 제거? (다른 winbridge 인스턴스 있으면 keep)"; then
        # BEGIN/END 마커 사이 전체 블록 제거 (round-trip safe)
        sudo sed -i '/^# winbridge:BEGIN$/,/^# winbridge:END$/d' "$APPARMOR_LOCAL"
        sudo systemctl reload apparmor 2>/dev/null || sudo service apparmor reload || \
            log_warn "  apparmor reload 실패. 수동: sudo systemctl reload apparmor"
        log_info "  AppArmor winbridge 블록 제거"
    else
        log_info "  AppArmor keep (사용자 선택)"
    fi
elif [ -f "$APPARMOR_LOCAL" ] && sudo grep -q "^# winbridge:" "$APPARMOR_LOCAL" 2>/dev/null; then
    # 구버전 (마커 없는) 흔적 호환 제거
    if confirm "  $APPARMOR_LOCAL에서 구버전 winbridge 흔적 발견. 제거? (회귀 호환)"; then
        sudo sed -i '/^# winbridge:/,+2d' "$APPARMOR_LOCAL"
        sudo systemctl reload apparmor 2>/dev/null || sudo service apparmor reload || true
        log_info "  AppArmor 구버전 흔적 제거"
    else
        log_info "  AppArmor keep (사용자 선택)"
    fi
else
    log_info "  AppArmor에 winbridge 항목 부재, skip"
fi

# 4. winbridge storage pool (선택)
log_info "4. winbridge storage pool..."
if sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-info winbridge >/dev/null 2>&1; then
    if confirm "  winbridge storage pool 제거? home 디렉토리 자체는 유지됨"; then
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-destroy winbridge 2>/dev/null || true
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-undefine winbridge
        log_info "  pool 제거"
    else
        log_info "  pool keep (사용자 선택)"
    fi
else
    log_info "  pool 부재, skip"
fi

# 5. ISO 캐시 + sentinel (선택)
log_info "5. ISO 캐시..."
if [ -d "$HOME/.cache/winbridge" ]; then
    if confirm "  ISO 캐시 (~/.cache/winbridge/) 삭제? 재설치 시 다시 다운로드"; then
        rm -rf "$HOME/.cache/winbridge"
        log_info "  캐시 제거"
    else
        log_info "  캐시 keep (사용자 선택)"
    fi
else
    log_info "  캐시 부재, skip"
fi

# 6. Build artifacts (무조건)
log_info "6. build/ 디렉토리..."
if [ -d "$REPO_ROOT/build" ]; then
    rm -rf "$REPO_ROOT/build"
    log_info "  build/ 제거"
else
    log_info "  build/ 부재, skip"
fi

# 7. Credentials (선택)
log_info "7. 자격 증명..."
if [ -f "$HOME/.config/winbridge/credentials" ]; then
    if confirm "  자격 증명 (~/.config/winbridge/credentials) 삭제? 재설치 시 새 random 비밀번호"; then
        rm -rf "$HOME/.config/winbridge"
        log_info "  자격 증명 제거"
    else
        log_info "  자격 증명 keep (사용자 선택)"
    fi
fi

log_info ""
log_info "uninstall 완료"
