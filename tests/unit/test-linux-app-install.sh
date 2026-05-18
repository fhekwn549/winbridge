#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$REPO_ROOT/scripts/host/08-install-linux-app.sh"

[ -x "$TARGET" ] || { echo "FAIL: $TARGET missing or not executable"; exit 1; }

help_out=$("$TARGET" --help 2>&1)
echo "$help_out" | grep -q "install-desktop-entry" \
    || { echo "FAIL: --help missing desktop entry install"; exit 1; }
echo "$help_out" | grep -q "autostart" \
    || { echo "FAIL: --help missing autostart"; exit 1; }
echo "$help_out" | grep -q "start --mode app" \
    || { echo "FAIL: --help missing app launcher command"; exit 1; }

dry_out=$(WINBRIDGE_INSTALL_BIN_DIR=/tmp/winbridge-bin "$TARGET" --dry-run 2>&1)
echo "$dry_out" | grep -q "cargo build --release" \
    || { echo "FAIL: --dry-run missing release build"; exit 1; }
echo "$dry_out" | grep -q "/tmp/winbridge-bin/winbridge" \
    || { echo "FAIL: --dry-run does not honor WINBRIDGE_INSTALL_BIN_DIR"; exit 1; }
echo "$dry_out" | grep -q "install-desktop-entry --exec /tmp/winbridge-bin/winbridge" \
    || { echo "FAIL: --dry-run does not install desktop entry with stable binary path"; exit 1; }

grep -q '08-install-linux-app.sh' "$REPO_ROOT/install.sh" \
    || { echo "FAIL: install.sh does not install the Linux app"; exit 1; }
grep -q 'HOME/.local/bin/winbridge' "$REPO_ROOT/install.sh" \
    || { echo "FAIL: install.sh does not advertise the stable installed binary"; exit 1; }
grep -q 'WINBRIDGE_BIN="$HOME/.local/bin/winbridge"' "$REPO_ROOT/uninstall.sh" \
    || { echo "FAIL: uninstall.sh does not remove the installed winbridge binary"; exit 1; }
grep -q 'dev.winbridge.WinbridgeApp.desktop' "$REPO_ROOT/scripts/host/08-install-linux-app.sh" \
    || { echo "FAIL: Linux app installer does not install a winbridge desktop entry"; exit 1; }
grep -q 'winbridge.png' "$REPO_ROOT/scripts/host/08-install-linux-app.sh" \
    || { echo "FAIL: Linux app installer does not install the winbridge icon"; exit 1; }

echo "PASS: test-linux-app-install.sh"
