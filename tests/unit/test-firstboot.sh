#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIRSTBOOT="$REPO_ROOT/config/firstboot.ps1"

[ -f "$FIRSTBOOT" ] || { echo "FAIL: firstboot.ps1 missing"; exit 1; }

content=$(<"$FIRSTBOOT")

case "$content" in
    *"C:\\winbridge\\position-kakaotalk.ps1"*|*"C:\winbridge\position-kakaotalk.ps1"*) ;;
    *) echo "FAIL: firstboot does not install position-kakaotalk.ps1"; exit 1 ;;
esac

case "$content" in
    *"WinbridgeOpenKakaoTalk"*|*"position-kakaotalk.ps1"*"CurrentVersion\\Run"*) ;;
    *) echo "FAIL: firstboot does not register foreground KakaoTalk launcher"; exit 1 ;;
esac

case "$content" in
    *"position-kakaotalk.log"*) ;;
    *) echo "FAIL: foreground KakaoTalk launcher does not write a log"; exit 1 ;;
esac

case "$content" in
    *"function Find-KakaoTalkExe"* ) ;;
    *) echo "FAIL: foreground KakaoTalk launcher uses a fixed KakaoTalk.exe path"; exit 1 ;;
esac

case "$content" in
    *"function Enable-TaskbarAutoHide"* ) ;;
    *) echo "FAIL: foreground KakaoTalk launcher does not preserve taskbar autohide behavior"; exit 1 ;;
esac

case "$content" in
    *"function Hide-Taskbar"* ) ;;
    *) echo "FAIL: foreground KakaoTalk launcher does not hide the taskbar on each run"; exit 1 ;;
esac

case "$content" in
    *"KakaoTalk HKCU Run 등록"*) echo "FAIL: firstboot still documents raw KakaoTalk autostart"; exit 1 ;;
esac

echo "PASS: test-firstboot.sh"
