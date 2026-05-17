#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIRSTBOOT="$REPO_ROOT/config/firstboot.ps1"
POSITION_SCRIPT="$REPO_ROOT/scripts/windows/position-kakaotalk.ps1"
WALLPAPER_SCRIPT="$REPO_ROOT/scripts/windows/repair-wallpaper.ps1"

[ -f "$FIRSTBOOT" ] || { echo "FAIL: firstboot.ps1 missing"; exit 1; }
[ -f "$POSITION_SCRIPT" ] || { echo "FAIL: position-kakaotalk.ps1 missing"; exit 1; }
[ -f "$WALLPAPER_SCRIPT" ] || { echo "FAIL: repair-wallpaper.ps1 missing"; exit 1; }

content=$(<"$FIRSTBOOT")
position_content=$(<"$POSITION_SCRIPT")
wallpaper_content=$(<"$WALLPAPER_SCRIPT")

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
    *"C:\\winbridge\\repair-wallpaper.ps1"*|*"C:\winbridge\repair-wallpaper.ps1"*) ;;
    *) echo "FAIL: firstboot does not install repair-wallpaper.ps1"; exit 1 ;;
esac

case "$wallpaper_content" in
    *"TranscodedWallpaper"* ) ;;
    *) echo "FAIL: repair-wallpaper.ps1 does not recover from theme cache"; exit 1 ;;
esac

case "$wallpaper_content" in
    *"SystemParametersInfo"* ) ;;
    *) echo "FAIL: repair-wallpaper.ps1 does not apply wallpaper through user32"; exit 1 ;;
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
    *"QEMU Guest Agent"*) ;;
    *) echo "FAIL: firstboot does not mention QEMU Guest Agent installation"; exit 1 ;;
esac

case "$content" in
    *"qemu-ga-x86_64.msi"*) ;;
    *) echo "FAIL: firstboot does not install qemu-ga MSI"; exit 1 ;;
esac

case "$content" in
    *"vioserial"*) ;;
    *) echo "FAIL: firstboot does not install virtio serial driver"; exit 1 ;;
esac

case "$content" in
    *"function Hide-Taskbar"*|*"ShowWindow(\$handle, \$SW_HIDE)"*)
        echo "FAIL: foreground KakaoTalk launcher still force-hides the taskbar"
        exit 1
        ;;
esac

case "$position_content" in
    *"function Hide-Taskbar"*|*"ShowWindow(\$handle, \$SW_HIDE)"*)
        echo "FAIL: standalone position-kakaotalk.ps1 still force-hides the taskbar"
        exit 1
        ;;
esac

case "$position_content" in
    *"[int]\$Width = 960"*"[int]\$Height = 720"* ) ;;
    *) echo "FAIL: standalone position-kakaotalk.ps1 does not use the current app window size"; exit 1 ;;
esac

case "$position_content" in
    *"[switch]\$Restart"*) ;;
    *) echo "FAIL: standalone position-kakaotalk.ps1 does not expose restart recovery switch"; exit 1 ;;
esac

case "$position_content" in
    *"function Stop-KakaoTalkProcesses"*) ;;
    *) echo "FAIL: standalone position-kakaotalk.ps1 does not stop stale KakaoTalk processes"; exit 1 ;;
esac

case "$content" in
    *"[switch]\$Restart"*) ;;
    *) echo "FAIL: firstboot embedded position-kakaotalk.ps1 does not expose restart recovery switch"; exit 1 ;;
esac

case "$content" in
    *"function Stop-KakaoTalkProcesses"*) ;;
    *) echo "FAIL: firstboot embedded position-kakaotalk.ps1 does not stop stale KakaoTalk processes"; exit 1 ;;
esac

echo "$content" | grep -q "WinbridgeShare" \
    || { echo "FAIL: firstboot does not create a WinbridgeShare shortcut"; exit 1; }
echo "$content" | grep -q "\\\\\\\\192.168.122.1\\\\winbridge" \
    || { echo "FAIL: firstboot shortcut does not point at the default SMB share"; exit 1; }

case "$content" in
    *"KakaoTalk HKCU Run 등록"*) echo "FAIL: firstboot still documents raw KakaoTalk autostart"; exit 1 ;;
esac

echo "PASS: test-firstboot.sh"
