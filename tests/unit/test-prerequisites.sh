#!/usr/bin/env bash
# tests/unit/test-prerequisites.sh
# 00-check-prerequisites.sh smoke test.
# 호스트 환경이 완벽할 필요는 없음 — 스크립트가 올바르게 동작하는지만 검증.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/00-check-prerequisites.sh"

# 존재 + 실행권한
[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }

# --help: 주제 키워드 포함 확인
help_out=$("$TARGET" --help 2>&1)
echo "$help_out" | grep -qi "prerequisites\|사전조건\|호스트" \
    || { echo "FAIL: --help missing topic keyword"; exit 1; }

# 일반 실행: clean env에서는 0, 미흡한 env에서는 1 (단 진단 메시지 포함 필수).
rc=0
out=$("$TARGET" 2>&1) || rc=$?
case "$rc" in
    0)
        echo "OK: prerequisites passed cleanly"
        ;;
    1)
        case "$out" in
            *"필수 명령 부재"*|*"libvirt"*|*"디스크 여유"*|*"검증 실패"*)
                echo "OK: errors reported with recognizable diagnostic"
                ;;
            *)
                echo "FAIL: exit 1 but output not informative:"
                echo "$out"
                exit 1
                ;;
        esac
        ;;
    *)
        echo "FAIL: unexpected exit code $rc"
        echo "$out"
        exit 1
        ;;
esac

echo "PASS: test-prerequisites.sh"
