#!/usr/bin/env bash
# scripts/host/01-download-iso.sh
# Windows Server 2022 Eval ISO 다운로드 + sha256 검증.
# sentinel + verify 하이브리드 idempotency.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_ISO_URL:=}"
: "${WINBRIDGE_ISO_SHA256:=}"
: "${WINBRIDGE_ISO_DEST:=$HOME/.cache/winbridge/server2022.iso}"
: "${WINBRIDGE_SENTINEL_DIR:=$HOME/.cache/winbridge}"

usage() {
    cat <<USAGE
사용법: $0 [--help]
환경변수 (필수):
  WINBRIDGE_ISO_URL       Windows Server 2022 Eval ISO 다운로드 URL
  WINBRIDGE_ISO_SHA256    예상 sha256 (소문자 hex)
환경변수 (선택):
  WINBRIDGE_ISO_DEST      저장 경로 (기본 ~/.cache/winbridge/server2022.iso)
  WINBRIDGE_SENTINEL_DIR  sentinel 디렉토리 (기본 ~/.cache/winbridge)
USAGE
}

[ "${1:-}" = "--help" ] && { usage; exit 0; }

# sha256은 항상 필수 (검증용). URL은 다운로드가 필요할 때만 필수.
[ -z "$WINBRIDGE_ISO_SHA256" ] && { log_error "WINBRIDGE_ISO_SHA256 미설정"; exit 1; }

require_cmd sha256sum "coreutils 패키지 (대부분 기본 설치)"

mkdir -p "$WINBRIDGE_SENTINEL_DIR" "$(dirname "$WINBRIDGE_ISO_DEST")"

SENTINEL="$WINBRIDGE_SENTINEL_DIR/01-iso-downloaded.done"

verify_sha256() {
    [ -f "$WINBRIDGE_ISO_DEST" ] || return 1
    local actual
    actual=$(sha256sum "$WINBRIDGE_ISO_DEST" | cut -d' ' -f1)
    [ "$actual" = "$WINBRIDGE_ISO_SHA256" ]
}

# Sentinel + verify 하이브리드
if [ -f "$SENTINEL" ] && verify_sha256; then
    log_info "ISO 이미 존재 + sha256 일치, skip ($WINBRIDGE_ISO_DEST)"
    exit 0
fi

# 사용자가 직접 다운로드해 둔 경우 등 sentinel은 없지만 ISO+sha256 일치 → sentinel 생성 후 통과
if [ ! -f "$SENTINEL" ] && verify_sha256; then
    log_info "ISO 존재 + sha256 일치 (sentinel 부재). sentinel 생성 후 skip ($WINBRIDGE_ISO_DEST)"
    touch "$SENTINEL"
    exit 0
fi

# 여기 도달하면 다운로드가 필요 → URL 필수
[ -z "$WINBRIDGE_ISO_URL" ] && {
    log_error "WINBRIDGE_ISO_URL 미설정 (다운로드 필요한 상태)"
    log_error "  WINBRIDGE_ISO_DEST=$WINBRIDGE_ISO_DEST 에 ISO가 없거나 sha256 불일치"
    exit 1
}
require_cmd curl "sudo apt install -y curl"

if [ -f "$SENTINEL" ]; then
    log_warn "sentinel은 있으나 sha256 불일치 또는 파일 부재 → 재다운로드"
    rm -f "$SENTINEL"
fi

log_info "ISO 다운로드 시작 ($WINBRIDGE_ISO_URL → $WINBRIDGE_ISO_DEST)"
PART="${WINBRIDGE_ISO_DEST}.part"

if curl -fL --retry 3 --retry-delay 5 -C - -o "$PART" "$WINBRIDGE_ISO_URL"; then
    mv "$PART" "$WINBRIDGE_ISO_DEST"
else
    log_error "다운로드 실패"
    rm -f "$PART"
    exit 1
fi

if ! verify_sha256; then
    actual=$(sha256sum "$WINBRIDGE_ISO_DEST" | cut -d' ' -f1)
    log_error "sha256 불일치"
    log_error "  expected: $WINBRIDGE_ISO_SHA256"
    log_error "  actual:   $actual"
    rm -f "$WINBRIDGE_ISO_DEST"
    exit 1
fi

touch "$SENTINEL"
log_info "다운로드 + 검증 완료 ($WINBRIDGE_ISO_DEST)"
