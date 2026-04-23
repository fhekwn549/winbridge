#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# 스크립트 자체가 실행 가능해야 하고 --help 인자로 사용법을 출력해야 한다.
out=$(bash ./00-check-prerequisites.sh --help 2>&1) || true
[[ "$out" == *"check prerequisites"* ]] || { echo "missing --help text"; exit 1; }
echo "test-00 shell OK"
