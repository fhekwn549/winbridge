#!/usr/bin/env bash
# download Windows ISO, VirtIO drivers, KakaoTalk installer
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

CHECK_ONLY=false
case "${1:-}" in
    -h|--help)
        cat <<EOF
Usage: $0 [--check-only]

download artifacts needed to install the Windows VM:
  - Windows 11 Enterprise evaluation ISO (~6 GB)
  - VirtIO-Win drivers ISO (~600 MB)
  - KakaoTalk Windows installer (~90 MB)
Destination: \$WINBRIDGE_HOME/downloads/

  --check-only   verify artifacts are present (no download)
EOF
        exit 0
        ;;
    --check-only) CHECK_ONLY=true ;;
esac

# Artifact definitions
# NOTE: Microsoft's eval ISO URL rotates; the script below uses the Microsoft
# Evaluation Center landing page and parses out a link. If that fails, user
# must manually download from https://www.microsoft.com/en-us/evalcenter/ and
# place the ISO at WIN11_ISO path below.
readonly WIN11_ISO="$WINBRIDGE_DOWNLOADS_DIR/Win11_Enterprise_Eval.iso"
readonly VIRTIO_ISO="$WINBRIDGE_DOWNLOADS_DIR/virtio-win.iso"
readonly KAKAO_EXE="$WINBRIDGE_DOWNLOADS_DIR/KakaoTalk_Setup.exe"

check_artifact() {
    local path=$1 name=$2 min_bytes=$3
    if [ ! -f "$path" ]; then
        log_warn "$name missing: $path"
        return 1
    fi
    local size; size=$(stat -c%s "$path")
    if [ "$size" -lt "$min_bytes" ]; then
        log_warn "$name too small ($size < $min_bytes bytes): $path"
        return 1
    fi
    log_info "$name OK ($((size / 1024 / 1024)) MB)"
    return 0
}

download_atomic() {
    local dest=$1 url=$2 label=$3
    log_info "Downloading $label..."
    if curl -fL --retry 3 -C - -o "${dest}.part" "$url"; then
        mv "${dest}.part" "$dest"
    else
        log_error "$label download failed; removing partial file"
        rm -f "${dest}.part"
        return 1
    fi
}

mkdir -p "$WINBRIDGE_DOWNLOADS_DIR"

if $CHECK_ONLY; then
    ok=true
    check_artifact "$WIN11_ISO"   "Windows 11 ISO"    5500000000 || ok=false
    check_artifact "$VIRTIO_ISO"  "VirtIO drivers"     300000000 || ok=false
    check_artifact "$KAKAO_EXE"   "KakaoTalk"          50000000  || ok=false
    $ok || exit 1
    exit 0
fi

# VirtIO drivers: stable URL from Red Hat
if ! check_artifact "$VIRTIO_ISO" "VirtIO drivers" 300000000 2>/dev/null; then
    download_atomic "$VIRTIO_ISO" \
        "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/virtio-win.iso" \
        "VirtIO drivers"
fi

# KakaoTalk: stable CDN URL
if ! check_artifact "$KAKAO_EXE" "KakaoTalk" 50000000 2>/dev/null; then
    download_atomic "$KAKAO_EXE" \
        "https://app-pc.kakaocdn.net/talk/win32/KakaoTalk_Setup.exe" \
        "KakaoTalk installer"
fi

# Windows 11 ISO: Microsoft rotates the URL. Try a known-good direct link; if it
# fails, instruct the user to download manually.
if ! check_artifact "$WIN11_ISO" "Windows 11 ISO" 5500000000 2>/dev/null; then
    log_warn "Windows 11 ISO must be downloaded manually."
    log_warn "  1. Visit https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise"
    log_warn "  2. Fill out the form, pick 'ISO - Enterprise'"
    log_warn "  3. Language: English (International). Architecture: 64-bit."
    log_warn "  4. Save as: $WIN11_ISO"
    log_warn ""
    log_warn "After download, re-run: $0"
    exit 2
fi

log_info "All artifacts present."
