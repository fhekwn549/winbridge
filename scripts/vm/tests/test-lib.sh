#!/usr/bin/env bash
# Minimal smoke test for lib.sh
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

# 1. log functions exist
type log_info >/dev/null
type log_error >/dev/null
type die >/dev/null

# 2. log_info writes to stderr with prefix
output=$(log_info "hello" 2>&1 >/dev/null)
[[ "$output" == *"[INFO]"*"hello"* ]] || { echo "log_info format wrong: $output"; exit 1; }

# 3. die exits non-zero with message
set +e
(die "test-exit" 2>/dev/null); rc=$?
set -e
[ "$rc" -eq 1 ] || { echo "die did not exit 1, got $rc"; exit 1; }

echo "lib.sh smoke OK"
