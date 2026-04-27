#!/usr/bin/env bash
# install.sh
# winbridge P2A 진입점 오케스트레이터. Phase 0 결정에 따라 B-2 폴백 단일 흐름.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"
# shellcheck source=scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

CRED_DIR="$HOME/.config/winbridge"
CRED_FILE="$CRED_DIR/credentials"

usage() {
    cat <<USAGE
사용법: ./install.sh [--help]

필수 환경변수 (manual-checks.md 결과로 사용자가 알아낸 값):
  WINBRIDGE_ISO_URL       Server 2022 Eval ISO 다운로드 URL
  WINBRIDGE_ISO_SHA256    그 ISO의 sha256 (소문자 hex)

선택 환경변수 (기본값 있음):
  WINBRIDGE_ISO_DEST      ISO 저장 경로 (기본 ~/.cache/winbridge/server2022.iso)
                          이미 다운받아둔 ISO가 있으면 그 경로로 지정하면 재다운로드 안 함
  WINBRIDGE_VM_NAME       기본 winbridge-srv2022
  WINBRIDGE_VM_RAM_KB     기본 4194304 (4GB)
  WINBRIDGE_VM_VCPU       기본 2
  WINBRIDGE_VM_IP         기본 192.168.122.50

진행:
  1. ~/.config/winbridge/credentials에 random Administrator 비밀번호 생성/로드
  2. 00-check-prerequisites.sh
  3. 01-download-iso.sh (이미 있고 sha256 일치면 skip)
  4. 02-setup-libvirt.sh (virbr0 정적 매핑 + home pool + AppArmor, 멱등)
  5. 03-create-vm.sh (OEM ISO + qcow2 + libvirt define + start)
  6. 04-wait-for-install.sh (RDP 응답 + 재부팅 안정화 대기, ~30~50분)
  7. 05-verify-guest.sh (RDP 인증/세션 가능 확인)
  8. 마지막에 RDP 창 띄움 — 카톡 단독 표시 시각 확인 + 폰 페어링
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

# 1. 자격 증명 로드 또는 생성
mkdir -p "$CRED_DIR"
chmod 700 "$CRED_DIR"
if [ -f "$CRED_FILE" ]; then
    # shellcheck source=/dev/null
    source "$CRED_FILE"
    log_info "기존 자격 증명 로드 ($CRED_FILE)"
else
    require_cmd openssl "sudo apt install -y openssl"
    WINBRIDGE_ADMIN_PASSWORD=$(openssl rand -hex 16)  # 32-char hex, special char 회피
    {
        echo "# winbridge credentials, generated $(date -Iseconds)"
        echo "WINBRIDGE_ADMIN_PASSWORD=$WINBRIDGE_ADMIN_PASSWORD"
    } > "$CRED_FILE"
    chmod 600 "$CRED_FILE"
    log_info "자격 증명 생성: $CRED_FILE"
fi
export WINBRIDGE_ADMIN_PASSWORD

# 단계 실행 헬퍼
run_step() {
    local label="$1" script="$2"
    log_info ""
    log_info "=== $label ==="
    if ! "$REPO_ROOT/scripts/host/$script"; then
        log_error ""
        log_error "==== 실패: $label ===="
        log_error "  스크립트: $script"
        log_error "  진단 후 ./install.sh를 다시 실행하면 마지막 성공 단계 이후부터 재개됩니다"
        log_error "  (각 스크립트는 idempotent로 작성됨)"
        exit 1
    fi
}

run_step "00-check-prerequisites" "00-check-prerequisites.sh"
run_step "01-download-iso"        "01-download-iso.sh"
run_step "02-setup-libvirt"       "02-setup-libvirt.sh"
run_step "03-create-vm"           "03-create-vm.sh"
run_step "04-wait-for-install"    "04-wait-for-install.sh"
run_step "05-verify-guest"        "05-verify-guest.sh"

# 8. 마지막에 RDP 창 띄움 (시각 확인 + 페어링)
log_info ""
log_info "=== 설치 완료 ==="
log_info ""
log_info "지금부터 RDP 창을 띄웁니다."
log_info "카톡 창이 단독으로 보이고 Windows 데스크톱은 안 보여야 정상입니다."
log_info "처음이면 폰 카톡으로 QR 스캔 또는 전화번호 인증을 진행하세요."
log_info ""
log_info "창을 닫으면 install.sh가 종료됩니다. VM은 계속 실행 중입니다."
log_info "(Ctrl+C로 RDP 창 띄우기를 건너뛰어도 install.sh는 정상 종료됩니다)"
log_info ""
sleep 3

: "${WINBRIDGE_VM_IP:=192.168.122.50}"

# FreeRDP 결정
if command -v xfreerdp3 >/dev/null 2>&1; then
    RDP=(xfreerdp3)
elif command -v xfreerdp >/dev/null 2>&1; then
    RDP=(xfreerdp)
elif command -v flatpak >/dev/null 2>&1 && flatpak info com.freerdp.FreeRDP >/dev/null 2>&1; then
    RDP=(flatpak run com.freerdp.FreeRDP)
else
    log_warn "FreeRDP 부재로 자동 표시 생략. 수동으로 다음 명령:"
    log_warn "  flatpak run com.freerdp.FreeRDP /v:$WINBRIDGE_VM_IP /u:Administrator /p:'<credentials 참조>' /cert:ignore /dynamic-resolution"
    exit 0
fi

# /p: 보안 경고 그대로 (단순화). 향후 /from-stdin으로 변경 검토.
"${RDP[@]}" /v:"$WINBRIDGE_VM_IP:3389" \
    /u:Administrator /p:"$WINBRIDGE_ADMIN_PASSWORD" \
    /cert:ignore /dynamic-resolution || true

log_info "RDP 창 종료. install.sh 정상 종료."
log_info "VM은 계속 실행 중입니다. 종료/일시정지는 P2B의 stop-session.sh로 (현재는 'sudo virsh -c qemu:///system shutdown winbridge-srv2022')"
