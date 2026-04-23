#!/usr/bin/env bash
# winbridge VM provisioning shared helpers.
# Sourced by every scripts/vm/NN-*.sh script.

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
