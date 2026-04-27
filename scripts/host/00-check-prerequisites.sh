#!/usr/bin/env bash
# scripts/host/00-check-prerequisites.sh
# winbridge P2A install 진행 가능 여부를 검증.
# install.sh의 첫 단계에서 호출. 실패 시 사용자에게 회복 안내 출력 후 비-0 종료.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

usage() {
    cat <<USAGE
사용법: $0 [--help]

호스트가 winbridge P2A install을 진행할 prerequisites를 만족하는지 검증.

체크 항목:
  - 패키지: virsh, virt-install, qemu-system-x86_64, qemu-img,
            envsubst, genisoimage, setfacl, FreeRDP(xfreerdp3/xfreerdp/flatpak)
  - 사용자가 libvirt 그룹 멤버
  - libvirtd 서비스 활성 (warning, error 아님)
  - 디스크 여유: \$HOME ≥ 70GB, / ≥ 5GB
  - 세션 타입 (X11 검증 / Wayland·기타 best-effort)
  - 커널 버전 (5.x 미만은 best-effort 경고)

종료 코드:
  0  모두 통과 (warning은 통과로 간주)
  1  errors > 0 (필수 항목 누락)
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    usage
    exit 0
fi

errors=0
warnings=0

# ---- 1. 패키지 ----
log_info "패키지 검증..."
require_cmd virsh "sudo apt install -y libvirt-clients"             || ((errors++))
require_cmd virt-install "sudo apt install -y virtinst"             || ((errors++))
require_cmd qemu-system-x86_64 "sudo apt install -y qemu-system-x86" || ((errors++))
require_cmd envsubst "sudo apt install -y gettext-base"             || ((errors++))
require_cmd genisoimage "sudo apt install -y genisoimage"           || ((errors++))
require_cmd qemu-img "sudo apt install -y qemu-utils"               || ((errors++))
require_cmd setfacl "sudo apt install -y acl"                       || ((errors++))

# FreeRDP: xfreerdp3 / xfreerdp / flatpak com.freerdp.FreeRDP 중 하나면 OK
if command -v xfreerdp3 >/dev/null 2>&1; then
    log_info "  FreeRDP 3.x 감지 (xfreerdp3)"
elif command -v xfreerdp >/dev/null 2>&1; then
    log_info "  FreeRDP 2.x 감지 (xfreerdp). 3.x 권장이지만 동작 가능"
elif command -v flatpak >/dev/null 2>&1 && flatpak info com.freerdp.FreeRDP >/dev/null 2>&1; then
    log_info "  FreeRDP 3.x flatpak 감지 (com.freerdp.FreeRDP)"
else
    log_error "FreeRDP 부재. 다음 중 하나 설치 필요:"
    log_error "  sudo apt install -y freerdp2-x11"
    log_error "  flatpak install -y flathub com.freerdp.FreeRDP"
    ((errors++))
fi

# ---- 2. libvirt 그룹 멤버 ----
log_info "사용자 그룹 검증..."
# id -nG는 즉시 반영 (groups는 lag 있음)
if ! id -nG "$USER" | tr ' ' '\n' | grep -qx libvirt; then
    log_error "사용자 '$USER'가 'libvirt' 그룹 비포함."
    log_error "  실행: sudo usermod -aG libvirt $USER && newgrp libvirt"
    log_error "  적용 위해 재로그인 필요"
    ((errors++))
else
    log_info "  libvirt 그룹 멤버 OK"
fi

# ---- 3. libvirtd 데몬 (warning) ----
if ! systemctl is-active --quiet libvirtd 2>/dev/null; then
    log_warn "libvirtd 서비스 비활성. 활성화 권장:"
    log_warn "  sudo systemctl enable --now libvirtd"
    ((warnings++))
else
    log_info "  libvirtd 활성 OK"
fi

# ---- 4. 디스크 여유 (Phase 0의 root 100% 사고 반영) ----
log_info "디스크 여유 검증..."
home_gb=$(df -BG --output=avail "$HOME" | tail -1 | tr -d 'G ')
root_gb=$(df -BG --output=avail / | tail -1 | tr -d 'G ')

if [ "${home_gb:-0}" -lt 70 ]; then
    log_error "\$HOME 디스크 여유 ${home_gb}GB < 필요 70GB"
    log_error "  내역: ISO 5GB + qcow2 60GB sparse + 빌드 여유"
    ((errors++))
else
    log_info "  \$HOME 여유: ${home_gb}GB OK"
fi

if [ "${root_gb:-0}" -lt 5 ]; then
    log_error "/ (root) 디스크 여유 ${root_gb}GB < 필요 5GB"
    log_error "  내역: libvirt overhead + AppArmor 프로파일 + 빌드 산출물"
    ((errors++))
else
    log_info "  / 여유: ${root_gb}GB OK"
fi

# ---- 5. 세션 타입 ----
session_type="${XDG_SESSION_TYPE:-unknown}"
case "$session_type" in
    x11)
        log_info "세션: X11 (검증된 환경)"
        ;;
    wayland)
        log_warn "세션: Wayland. xfreerdp3는 XWayland 경유 동작."
        log_warn "  폴백 모드 (창 데코 제거)는 best-effort"
        ((warnings++))
        ;;
    *)
        log_warn "세션 타입 불명($session_type). best-effort"
        ((warnings++))
        ;;
esac

# ---- 6. 커널 버전 ----
kver_full="$(uname -r)"
kver_major="$(echo "$kver_full" | cut -d. -f1)"
if ! [[ "$kver_major" =~ ^[0-9]+$ ]]; then
    log_warn "커널 버전 파싱 실패 ($kver_full). best-effort"
    ((warnings++))
elif [ "$kver_major" -lt 5 ]; then
    log_warn "커널 < 5.x ($kver_full). best-effort. 진행은 가능"
    ((warnings++))
else
    log_info "커널: $kver_full OK"
fi

# ---- 결과 합산 ----
echo "" >&2
if [ "$errors" -gt 0 ]; then
    log_error "검증 실패: ${errors}개 오류, ${warnings}개 경고"
    exit 1
elif [ "$warnings" -gt 0 ]; then
    log_warn "검증 통과 (경고 ${warnings}개). 진행 가능"
    exit 0
else
    log_info "검증 모두 통과"
    exit 0
fi
