#!/usr/bin/env bash
# scripts/host/02-setup-libvirt.sh
# qemu:///system libvirt 설정: 정적 IP 매핑 + home storage pool + AppArmor abstractions.
# 멱등.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_NAME:=winbridge-srv2022}"
: "${WINBRIDGE_VM_IP:=192.168.122.50}"
: "${WINBRIDGE_VM_MAC:=52:54:00:7B:01:01}"
: "${WINBRIDGE_HOME_POOL_DIR:=$HOME/.local/share/libvirt/images}"
: "${WINBRIDGE_LIBVIRT_URI:=qemu:///system}"

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

usage() {
    cat <<USAGE
사용법: $0 [--help|--dry-run]
qemu:///system libvirt 호스트 설정. 멱등.
  - default 네트워크에 정적 IP 매핑 (mac=$WINBRIDGE_VM_MAC ip=$WINBRIDGE_VM_IP name=$WINBRIDGE_VM_NAME)
  - winbridge directory storage pool 등록 (target=$WINBRIDGE_HOME_POOL_DIR)
  - libvirt-qemu setfacl traverse + AppArmor abstractions 추가
환경변수: WINBRIDGE_VM_NAME, WINBRIDGE_VM_IP, WINBRIDGE_VM_MAC,
          WINBRIDGE_HOME_POOL_DIR, WINBRIDGE_LIBVIRT_URI
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

if [ $DRY_RUN -eq 0 ]; then
    require_cmd virsh
    require_cmd setfacl "sudo apt install -y acl"
fi

# ============== 1. 정적 IP 매핑 ==============
log_info "1. default 네트워크 정적 IP 매핑 ($WINBRIDGE_VM_NAME → $WINBRIDGE_VM_IP, mac $WINBRIDGE_VM_MAC)"

if [ $DRY_RUN -eq 0 ]; then
    if ! virsh -c "$WINBRIDGE_LIBVIRT_URI" net-info default >/dev/null 2>&1; then
        log_error "default 네트워크 부재. 다음 명령으로 생성 후 재실행:"
        log_error "  sudo virsh -c $WINBRIDGE_LIBVIRT_URI net-define /usr/share/libvirt/networks/default.xml"
        log_error "  sudo virsh -c $WINBRIDGE_LIBVIRT_URI net-autostart default"
        log_error "  sudo virsh -c $WINBRIDGE_LIBVIRT_URI net-start default"
        exit 1
    fi
    CURRENT_XML=$(virsh -c "$WINBRIDGE_LIBVIRT_URI" net-dumpxml default)

    if echo "$CURRENT_XML" | grep -q "name='$WINBRIDGE_VM_NAME'.*ip='$WINBRIDGE_VM_IP'"; then
        log_info "  매핑 이미 존재, skip"
    elif echo "$CURRENT_XML" | grep -q "ip='$WINBRIDGE_VM_IP'"; then
        log_error "  $WINBRIDGE_VM_IP 가 다른 호스트가 사용 중:"
        echo "$CURRENT_XML" | grep "ip='$WINBRIDGE_VM_IP'" >&2
        log_error "  WINBRIDGE_VM_IP를 다른 값으로 지정하거나 충돌 매핑 정리 필요"
        exit 1
    else
        if echo "$CURRENT_XML" | grep -q "name='$WINBRIDGE_VM_NAME'"; then
            log_warn "  기존 $WINBRIDGE_VM_NAME 매핑이 다른 IP로 존재 → 제거 후 재추가"
            sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" net-update default delete ip-dhcp-host \
                "<host name='$WINBRIDGE_VM_NAME'/>" --live --config 2>/dev/null || true
        fi
        NEW_HOST="<host mac='$WINBRIDGE_VM_MAC' name='$WINBRIDGE_VM_NAME' ip='$WINBRIDGE_VM_IP'/>"
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" net-update default add ip-dhcp-host "$NEW_HOST" --live --config
        log_info "  매핑 추가: $NEW_HOST"
    fi
else
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI net-update default add ip-dhcp-host"
    log_info "[dry-run]   <host mac='$WINBRIDGE_VM_MAC' name='$WINBRIDGE_VM_NAME' ip='$WINBRIDGE_VM_IP'/>"
fi

# ============== 2. home directory storage pool ==============
log_info "2. home storage pool 등록 (target=$WINBRIDGE_HOME_POOL_DIR)"

mkdir -p "$WINBRIDGE_HOME_POOL_DIR"

if [ $DRY_RUN -eq 0 ]; then
    # setfacl traverse
    sudo setfacl -m u:libvirt-qemu:x "$HOME"
    sudo setfacl -m u:libvirt-qemu:x "$HOME/.local" 2>/dev/null || true
    sudo setfacl -m u:libvirt-qemu:x "$HOME/.local/share" 2>/dev/null || true
    sudo setfacl -m u:libvirt-qemu:x "$HOME/.local/share/libvirt" 2>/dev/null || true
    sudo setfacl -m u:libvirt-qemu:rwx "$WINBRIDGE_HOME_POOL_DIR"
    log_info "  setfacl traverse + pool dir 권한 적용"

    if sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-info winbridge >/dev/null 2>&1; then
        existing_target=$(sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-dumpxml winbridge | grep -oP "(?<=<path>)[^<]+")
        if [ "$existing_target" = "$WINBRIDGE_HOME_POOL_DIR" ]; then
            log_info "  winbridge pool 이미 등록 (target $existing_target), skip"
        else
            log_warn "  winbridge pool 존재하나 target 다름 ($existing_target ≠ $WINBRIDGE_HOME_POOL_DIR). 자동 수정 안 함, 사용자 확인 필요"
        fi
    else
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-define-as winbridge dir --target "$WINBRIDGE_HOME_POOL_DIR"
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-build winbridge 2>/dev/null || true
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-start winbridge
        sudo virsh -c "$WINBRIDGE_LIBVIRT_URI" pool-autostart winbridge
        log_info "  winbridge pool 등록 + 활성"
    fi
else
    log_info "[dry-run] sudo setfacl -m u:libvirt-qemu:x $HOME/.local/share/libvirt"
    log_info "[dry-run] sudo virsh -c $WINBRIDGE_LIBVIRT_URI pool-define-as winbridge dir --target $WINBRIDGE_HOME_POOL_DIR"
fi

# ============== 3. AppArmor abstractions ==============
APPARMOR_LOCAL=/etc/apparmor.d/local/abstractions/libvirt-qemu
log_info "3. AppArmor abstractions ($APPARMOR_LOCAL)"

if [ $DRY_RUN -eq 0 ]; then
    sudo touch "$APPARMOR_LOCAL"
    if sudo grep -q "$HOME/.local/share/libvirt/images" "$APPARMOR_LOCAL" 2>/dev/null; then
        log_info "  AppArmor 이미 winbridge path 포함, skip"
    else
        sudo tee -a "$APPARMOR_LOCAL" >/dev/null <<EOF

# winbridge:BEGIN
# winbridge: home의 libvirt 디스크/ISO 접근 허용
"$HOME/.local/share/libvirt/images/**" rwk,
"$HOME/Downloads/*.iso" rk,
# winbridge:END
EOF
        log_info "  AppArmor abstractions에 winbridge path 추가"
        sudo systemctl reload apparmor 2>/dev/null || sudo service apparmor reload || \
            log_warn "  apparmor reload 실패. 수동: sudo systemctl reload apparmor"
    fi
else
    log_info "[dry-run] $APPARMOR_LOCAL에 다음 줄 추가:"
    log_info "[dry-run]   $HOME/.local/share/libvirt/images/** rwk,"
    log_info "[dry-run]   $HOME/Downloads/*.iso rk,"
fi

log_info "libvirt setup 완료"
