#!/usr/bin/env bash
# scripts/lib/common.sh
# winbridge 호스트 스크립트들이 source로 로드하는 공통 함수.

# 색상 코드 (stderr가 TTY일 때만 활성화)
if [ -t 2 ]; then
    _C_RESET=$'\033[0m'
    _C_INFO=$'\033[34m'
    _C_WARN=$'\033[33m'
    _C_ERR=$'\033[31m'
else
    _C_RESET=
    _C_INFO=
    _C_WARN=
    _C_ERR=
fi

log_info()  { echo "${_C_INFO}[INFO]${_C_RESET} $*" >&2; }
log_warn()  { echo "${_C_WARN}[WARN]${_C_RESET} $*" >&2; }
log_error() { echo "${_C_ERR}[ERROR]${_C_RESET} $*" >&2; }

# require_cmd <name> [hint]
# 명령 부재 시 hint 메시지 출력 후 비-0 종료.
require_cmd() {
    local cmd="$1"
    local hint="${2:-}"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        log_error "필수 명령 부재: $cmd"
        [ -n "$hint" ] && log_error "  설치: $hint"
        return 1
    fi
}

# wait_for <bash-condition-string> <timeout-sec> <interval-sec>
# 조건이 참이 될 때까지 polling. timeout이면 비-0 종료.
wait_for() {
    local cond="$1"
    local timeout="$2"
    local interval="$3"
    local elapsed=0
    while ! eval "$cond" 2>/dev/null; do
        sleep "$interval"
        elapsed=$(awk "BEGIN{print $elapsed+$interval}")
        if awk "BEGIN{exit !($elapsed >= $timeout)}"; then
            log_error "wait_for 타임아웃: $cond (${timeout}s)"
            return 1
        fi
    done
}

# render_template <path>
# stdin 환경변수로 ${VAR} 치환된 결과를 stdout으로 출력. envsubst 의존.
render_template() {
    local tpl="$1"
    if ! command -v envsubst >/dev/null 2>&1; then
        log_error "envsubst 부재. 'sudo apt install -y gettext-base'로 설치 필요"
        return 1
    fi
    envsubst < "$tpl"
}
