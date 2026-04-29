#!/usr/bin/env bash
# start-session.sh — 설치된 winbridge VM에 FreeRDP로 재접속하는 사용자 진입점.
# install.sh가 끝까지 한 번 자동 실행한 후, 다음부터는 이 스크립트로 카톡 창을 띄움.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
. scripts/lib/common.sh

CRED="$HOME/.config/winbridge/credentials"
if [[ ! -r "$CRED" ]]; then
    log_error "자격 증명 부재: $CRED — install.sh를 먼저 실행하세요."
    exit 1
fi
. "$CRED"

: "${WINBRIDGE_VM_IP:=192.168.122.50}"

if command -v xfreerdp3 >/dev/null 2>&1; then
    RDP=(xfreerdp3)
elif command -v xfreerdp >/dev/null 2>&1; then
    RDP=(xfreerdp)
else
    log_error "FreeRDP 부재. 'sudo apt install -y freerdp2-x11' 설치 후 재실행하세요."
    exit 1
fi

if [[ "${RDP[0]}" == xfreerdp3 ]]; then
    RDP_RES_OPT=(/dynamic-resolution)
else
    RDP_RES_OPT=(/size:1280x720)
fi

# /kbd:0x00000409 = 클라이언트 영문 keymap 강제. 서버측 한국어 IME 활성 시 xfreerdp v2.x가
#   RDP 채널 협상 중 segfault하는 케이스를 회피.
printf '%s\n' "$WINBRIDGE_ADMIN_PASSWORD" | "${RDP[@]}" /v:"$WINBRIDGE_VM_IP:3389" \
    /u:Administrator /from-stdin \
    /cert:ignore /kbd:0x00000409 "${RDP_RES_OPT[@]}"
