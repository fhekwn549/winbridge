#!/usr/bin/env bash
# DEPRECATED (P1A 레거시): scripts/vm/NN-*.sh가 공유하는 helpers.
# P2A 채택 흐름은 scripts/host/ 하위에서 scripts/lib/common.sh를 사용한다.
# 신규 작업은 scripts/lib/common.sh 사용.
# winbridge VM provisioning shared helpers.
# Sourced by every scripts/vm/NN-*.sh script.

# Guard against double-sourcing. Sub-scripts may source lib.sh directly even
# if the caller already did; without this guard the readonly declarations
# below would fail on the second pass.
[[ -n "${_WINBRIDGE_LIB_SOURCED:-}" ]] && return 0
readonly _WINBRIDGE_LIB_SOURCED=1

# Colors for terminals that support them.
if [ -t 2 ]; then
    readonly _C_RED=$'\033[31m'
    readonly _C_YELLOW=$'\033[33m'
    readonly _C_GREEN=$'\033[32m'
    readonly _C_RESET=$'\033[0m'
else
    readonly _C_RED=""
    readonly _C_YELLOW=""
    readonly _C_GREEN=""
    readonly _C_RESET=""
fi

log_info()  { printf '%s[INFO]%s  %s\n'  "$_C_GREEN"  "$_C_RESET" "$*" >&2; }
log_warn()  { printf '%s[WARN]%s  %s\n'  "$_C_YELLOW" "$_C_RESET" "$*" >&2; }
log_error() { printf '%s[ERROR]%s %s\n' "$_C_RED"    "$_C_RESET" "$*" >&2; }

die() { log_error "$*"; exit 1; }

# Retry a command N times with exponential backoff.
# Usage: retry <max_tries> <base_sleep_seconds> <cmd...>
retry() {
    local max=$1 base=$2; shift 2
    local attempt=0
    until "$@"; do
        attempt=$((attempt + 1))
        if [ "$attempt" -ge "$max" ]; then
            return 1
        fi
        local sleep_s=$((base * (2 ** (attempt - 1))))
        log_warn "retry $attempt/$max after ${sleep_s}s"
        sleep "$sleep_s"
    done
}

# Require a command to be in PATH.
require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# Virsh wrapper that scopes to user session (we use qemu:///session, not system).
vsh() { virsh --connect qemu:///session "$@"; }

# Check domain exists.
domain_exists() { vsh dominfo "$1" >/dev/null 2>&1; }

# Path shared across scripts.
readonly WINBRIDGE_HOME="${WINBRIDGE_HOME:-$HOME/.local/share/winbridge}"
readonly WINBRIDGE_IMAGES_DIR="$WINBRIDGE_HOME/images"
readonly WINBRIDGE_DOWNLOADS_DIR="$WINBRIDGE_HOME/downloads"
readonly WINBRIDGE_DATA_DIR="$WINBRIDGE_HOME/data"
readonly WINBRIDGE_ARCHIVE_DIR="$WINBRIDGE_HOME/archive"
readonly WINBRIDGE_DOMAIN_NAME="${WINBRIDGE_DOMAIN_NAME:-winbridge-win11}"
