#!/usr/bin/env bash
# scripts/host/04-wait-for-install.sh
# Windows 무인 설치 + firstboot.ps1 + 최종 재부팅까지 대기.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_VM_NAME:=winbridge-srv2022}"
: "${WINBRIDGE_VM_IP:=192.168.122.50}"
: "${WINBRIDGE_TIMEOUT:=3600}"
: "${WINBRIDGE_LIBVIRT_URI:=qemu:///system}"

usage() {
    cat <<USAGE
사용법: $0 [--help|--timeout SEC]
'$WINBRIDGE_VM_NAME'의 무인 설치 + firstboot.ps1 + 재부팅 완료 대기.
완료 기준:
  1. VM running 상태 (60s 내)
  2. RDP 3389 응답 (timeout 내, 기본 ${WINBRIDGE_TIMEOUT}s)
  3. firstboot 완료 + reboot 안정화 추가 120s 대기
  4. RDP 재응답 확인
USAGE
}

case "${1:-}" in
    --help) usage; exit 0 ;;
    --timeout) WINBRIDGE_TIMEOUT="${2:?--timeout SEC 필요}" ;;
esac

require_cmd virsh
require_cmd nc "sudo apt install -y netcat-openbsd"

start=$(date +%s)
log_info "VM 무인 설치 대기 시작 ($WINBRIDGE_VM_NAME, timeout ${WINBRIDGE_TIMEOUT}s)"

# 1. VM running 상태 대기 (60s)
log_info "[1/4] VM running 대기 (60s)..."
if ! wait_for "sudo virsh -c $WINBRIDGE_LIBVIRT_URI domstate $WINBRIDGE_VM_NAME 2>/dev/null | grep -q running" 60 5; then
    log_error "VM이 60초 내 'running' 상태가 되지 않음"
    log_error "  진단: sudo virsh -c $WINBRIDGE_LIBVIRT_URI domstate $WINBRIDGE_VM_NAME"
    exit 1
fi
log_info "  running 확인"

# 2. RDP 3389 첫 응답 대기 (큰 폴링: 무인 설치 + 첫 부팅 + firstboot.ps1 = 30~50분 예상)
log_info "[2/4] RDP 3389 첫 응답 대기 (~30~50분 예상, timeout ${WINBRIDGE_TIMEOUT}s)..."
ELAPSED=0
INTERVAL=30
while ! nc -z -w3 "$WINBRIDGE_VM_IP" 3389 2>/dev/null; do
    sleep $INTERVAL
    ELAPSED=$((ELAPSED + INTERVAL))
    if [ $ELAPSED -ge "$WINBRIDGE_TIMEOUT" ]; then
        log_error "timeout: RDP 3389 ready 안 됨 (${WINBRIDGE_TIMEOUT}s 경과)"
        log_error "  진단: virt-viewer -c $WINBRIDGE_LIBVIRT_URI $WINBRIDGE_VM_NAME"
        log_error "  Windows 설치 화면 또는 firstboot.ps1 진행 상황 확인"
        exit 1
    fi
    if [ $((ELAPSED % 300)) -eq 0 ]; then
        log_info "  ${ELAPSED}s 경과, 계속 대기..."
    fi
done
elapsed_first=$(( $(date +%s) - start ))
log_info "  RDP 3389 첫 응답 OK (${elapsed_first}s)"

# 3. firstboot.ps1 + 재부팅 안정화 대기 (firstboot 마지막에 shutdown /r /t 30 예약함)
log_info "[3/4] firstboot 완료 + 재부팅 안정화 120s 대기..."
sleep 120

# 4. RDP 재응답 확인 (재부팅 후)
log_info "[4/4] 재부팅 후 RDP 재응답 확인 (60s 내)..."
if ! wait_for "nc -z -w3 $WINBRIDGE_VM_IP 3389" 60 5; then
    log_error "재부팅 후 RDP 응답 안 함 (60s 내)"
    log_error "  firstboot.ps1이 explorer Shell 차단 후 정상 재시작했는지 확인 필요"
    exit 1
fi

elapsed_total=$(( $(date +%s) - start ))
log_info "VM 설치 완료 대기 종료 (총 ${elapsed_total}s)"
log_info "다음: 05-verify-guest.sh로 RemoteApp/단독 앱 검증"
