#!/usr/bin/env bash
# scripts/host/05-verify-guest.sh
# B-2 폴백 모드 검증: RDP 응답 + RDP 세션 생성 가능 여부.
# 카톡 프로세스 검증은 P2B에서 (qemu-guest-agent 도입 후).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_IP:=192.168.122.50}"
: "${WINBRIDGE_VM_USER:=Administrator}"

usage() {
    cat <<USAGE
사용법: $0 [--help]
B-2 폴백 모드 게스트 검증.
체크: 1) RDP 3389 응답, 2) RDP 세션 생성 가능 (xfreerdp 5초 띄우고 종료)
필수 환경변수: WINBRIDGE_ADMIN_PASSWORD
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

[ -z "${WINBRIDGE_ADMIN_PASSWORD:-}" ] && { log_error "WINBRIDGE_ADMIN_PASSWORD 미설정"; exit 1; }

require_cmd nc "sudo apt install -y netcat-openbsd"

# FreeRDP 바이너리 결정
if command -v xfreerdp3 >/dev/null 2>&1; then
    RDP=(xfreerdp3)
elif command -v xfreerdp >/dev/null 2>&1; then
    RDP=(xfreerdp)
elif command -v flatpak >/dev/null 2>&1 && flatpak info com.freerdp.FreeRDP >/dev/null 2>&1; then
    RDP=(flatpak run com.freerdp.FreeRDP)
else
    log_error "FreeRDP 부재. 'sudo apt install -y freerdp2-x11' 또는 flatpak"
    exit 1
fi

# 1. RDP 3389 응답
log_info "[1/2] RDP 3389 응답 확인..."
if ! nc -z -w3 "$WINBRIDGE_VM_IP" 3389 2>/dev/null; then
    log_error "RDP 3389 응답 안 함 (VM IP $WINBRIDGE_VM_IP)"
    exit 1
fi
log_info "  OK"

# 2. RDP 세션 생성 시도 (5초 띄우고 kill)
# 비밀번호는 /p: 대신 stdin으로 전달 (ps/proc 노출 회피)
log_info "[2/2] RDP 세션 5초 시도..."
LOG=/tmp/winbridge-verify.log
if [[ "${RDP[0]}" == xfreerdp3 ]]; then
    RDP_RES_OPT=(/dynamic-resolution)
else
    RDP_RES_OPT=(/size:1280x720)
fi
# /kbd:0x00000409 = 클라이언트 영문 keymap 강제. 서버측 한국어 IME 활성 시
#   xfreerdp v2.x가 RDP 협상 중 segfault하는 케이스를 회피.
printf '%s\n' "$WINBRIDGE_ADMIN_PASSWORD" | "${RDP[@]}" /v:"$WINBRIDGE_VM_IP:3389" \
    /u:"$WINBRIDGE_VM_USER" /from-stdin \
    /cert:ignore /kbd:0x00000409 "${RDP_RES_OPT[@]}" \
    > "$LOG" 2>&1 &
RDP_PID=$!

sleep 5

# 종료
kill -TERM "$RDP_PID" 2>/dev/null || true
wait "$RDP_PID" 2>/dev/null || true

# 인증/세션 에러 시그니처 검사
if grep -qiE "authentication|access denied|connection refused|certificate" "$LOG" 2>/dev/null; then
    log_warn "RDP 세션 시도 중 잠재 이슈 발견:"
    grep -iE "authentication|access denied|connection refused|certificate" "$LOG" | head -3 | sed 's/^/  /'
fi

# 명백한 실패 시그니처
if grep -qE "ERR_|FATAL|Authentication failure|Logon failure" "$LOG" 2>/dev/null; then
    log_error "RDP 세션 생성 실패. 로그: $LOG"
    grep -E "ERR_|FATAL|Authentication failure|Logon failure" "$LOG" | head -5 | sed 's/^/  /'
    exit 1
fi

log_info "  RDP 세션 5초 동안 정상 (인증 OK)"
log_info ""
log_info "B-2 폴백 모드 자동 검증 통과."
log_info "최종 시각 확인은 install.sh가 마지막에 RDP 창을 띄워 사용자가 직접 확인:"
log_info "  - 카톡 창이 단독으로 표시되는가"
log_info "  - Windows 데스크톱/taskbar가 안 보이는가 (explorer 차단됨)"
