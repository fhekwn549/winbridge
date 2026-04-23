#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

out=$(bash ./01-download-artifacts.sh --help 2>&1) || true
[[ "$out" == *"download artifacts"* ]] || { echo "missing --help"; exit 1; }

# --check-only 옵션은 네트워크를 쓰지 않고 이미 받은 파일만 검증
out=$(bash ./01-download-artifacts.sh --check-only 2>&1) || true
# 아직 받은 게 없으니 missing 메시지 나와야 정상
[[ "$out" == *"missing"* ]] || [[ "$out" == *"not found"* ]] || {
    echo "expected missing-artifact message: $out"; exit 1;
}
echo "test-01 shell OK"
