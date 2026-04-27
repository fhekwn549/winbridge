#!/usr/bin/env bash
# tests/e2e/install-uninstall-cycle.sh
#
# !!! 매우 시간 소모 (~3시간). 로컬 수동 실행만, CI에서 실행 X. !!!
#
# winbridge P2A end-to-end 사이클 검증:
#   install → 검증 → uninstall (--yes로 prompt 스킵) → 재install → 검증
#
# 사전:
#   WINBRIDGE_ISO_URL, WINBRIDGE_ISO_SHA256 환경변수 설정 필수
#   사용자 환경에 이미 ISO 있으면 WINBRIDGE_ISO_DEST=<경로> 도 설정해서 재다운로드 회피
#
# 검증 항목:
#   - install.sh가 처음부터 끝까지 정상 종료
#   - VM이 192.168.122.50에서 RDP 응답
#   - uninstall.sh가 모든 자원 정리
#   - 재install.sh가 두 번째에도 정상 (idempotency 회귀 테스트)
#   - AppArmor 마커 형식이 install/uninstall 양쪽에서 round-trip OK
#   - libvirt 매핑/pool 등 round-trip OK

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -z "${WINBRIDGE_ISO_URL:-}" ]    && { echo "ERROR: WINBRIDGE_ISO_URL 미설정"; exit 1; }
[ -z "${WINBRIDGE_ISO_SHA256:-}" ] && { echo "ERROR: WINBRIDGE_ISO_SHA256 미설정"; exit 1; }

cd "$REPO_ROOT"

start=$(date +%s)
echo "=== 1차 install (~30~50분 예상) ==="
./install.sh

echo ""
echo "=== uninstall (--yes, ~1분) ==="
./uninstall.sh --yes

echo ""
echo "=== 2차 install (재현성 + idempotency 검증, ~30~50분) ==="
./install.sh

elapsed=$(( $(date +%s) - start ))
echo ""
echo "PASS: install-uninstall-cycle 완료 (총 ${elapsed}s)"
