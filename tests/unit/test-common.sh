#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# shellcheck source=../../scripts/lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

assert_eq() {
    if [ "$1" != "$2" ]; then
        echo "FAIL [$3]: expected '$2', got '$1'" >&2
        exit 1
    fi
}

# log_info: writes to stderr, contains the message
out=$(log_info "hello" 2>&1)
case "$out" in
    *hello*) ;;
    *) echo "FAIL: log_info output missing 'hello'"; exit 1 ;;
esac

# require_cmd: existing command passes
require_cmd bash || { echo "FAIL: require_cmd bash should succeed"; exit 1; }

# require_cmd: missing command returns non-zero
# set -e 때문에 ( ... ) 실패가 외부 종료를 유발하므로 종료코드를 변수로 캡처.
rc=0; ( require_cmd this_cmd_does_not_exist 2>/dev/null ) || rc=$?
[ "$rc" -eq 0 ] && { echo "FAIL: require_cmd succeeded for missing cmd"; exit 1; }

# wait_for: condition immediately true
out=$(timeout 3 bash -c 'source "'"$REPO_ROOT"'/scripts/lib/common.sh" && wait_for "test 1 = 1" 2 0.1 && echo OK')
assert_eq "$out" "OK" "wait_for immediate"

# wait_for: timeout returns non-zero
rc=0; ( timeout 3 bash -c 'source "'"$REPO_ROOT"'/scripts/lib/common.sh" && wait_for "test 1 = 2" 1 0.1' 2>/dev/null ) || rc=$?
[ "$rc" -eq 0 ] && { echo "FAIL: wait_for succeeded on timeout"; exit 1; }

# render_template: substitutes only WINBRIDGE_* variables and preserves other shell syntax
TPL=$(mktemp)
trap 'rm -f "$TPL"' EXIT
# shellcheck disable=SC2016  # template literal processed by envsubst
echo 'host=${WINBRIDGE_HOSTNAME} port=${WINBRIDGE_PORT} wb=${WB_HOSTNAME} ps_var=$p last_arg=$_' > "$TPL"
out=$(WINBRIDGE_HOSTNAME=foo WINBRIDGE_PORT=42 WB_HOSTNAME=bad WB_PORT=99 render_template "$TPL")
assert_eq "$out" 'host=foo port=42 wb=${WB_HOSTNAME} ps_var=$p last_arg=$_' "render_template WINBRIDGE allowlist"

# wait_for: 잘못된 timeout 형식 거부 (return 2)
rc=0; ( wait_for "test 1 = 1" "abc" 0.1 ) 2>/dev/null || rc=$?
[ "$rc" -eq 2 ] || { echo "FAIL: wait_for should return 2 for non-numeric timeout, got $rc"; exit 1; }

# wait_for: 잘못된 interval 형식 거부 (return 2)
rc=0; ( wait_for "test 1 = 1" 1 "1)} END{print 1" ) 2>/dev/null || rc=$?
[ "$rc" -eq 2 ] || { echo "FAIL: wait_for should return 2 for malicious interval, got $rc"; exit 1; }

echo "PASS: test-common.sh"
