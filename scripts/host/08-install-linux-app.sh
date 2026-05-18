#!/usr/bin/env bash
# Build and install the Linux-side winbridge app into the current user's profile.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_INSTALL_BIN_DIR:=$HOME/.local/bin}"
: "${WINBRIDGE_INSTALL_NAME:=winbridge}"

usage() {
    cat <<USAGE
Usage: $0 [--help|--dry-run]

Builds the release binary and installs the Linux app integration for this user:
  - $WINBRIDGE_INSTALL_BIN_DIR/$WINBRIDGE_INSTALL_NAME
  - ~/.local/share/applications/dev.winbridge.Winbridge.desktop
  - ~/.local/share/icons/hicolor/256x256/apps/winbridge.png
  - ~/.local/bin/kakaotalk
  - ~/.config/autostart/dev.winbridge.Winbridge.desktop

The desktop launcher runs:
  $WINBRIDGE_INSTALL_BIN_DIR/$WINBRIDGE_INSTALL_NAME start --mode app --display stable-slots

Internally this runs:
  $WINBRIDGE_INSTALL_BIN_DIR/$WINBRIDGE_INSTALL_NAME install-desktop-entry --exec $WINBRIDGE_INSTALL_BIN_DIR/$WINBRIDGE_INSTALL_NAME
USAGE
}

DRY_RUN=0
case "${1:-}" in
    --help) usage; exit 0 ;;
    --dry-run) DRY_RUN=1 ;;
    "") ;;
    *) usage >&2; exit 2 ;;
esac

INSTALL_BIN="$WINBRIDGE_INSTALL_BIN_DIR/$WINBRIDGE_INSTALL_NAME"
RELEASE_BIN="$REPO_ROOT/target/release/winbridge"

if [ "$DRY_RUN" -eq 1 ]; then
    log_info "[dry-run] cargo build --release"
    log_info "[dry-run] install -m 755 $RELEASE_BIN $INSTALL_BIN"
    log_info "[dry-run] $INSTALL_BIN install-desktop-entry --exec $INSTALL_BIN"
    exit 0
fi

require_cmd cargo "Install Rust first: https://rustup.rs"
require_cmd install "sudo apt install -y coreutils"

log_info "Building winbridge release binary..."
(cd "$REPO_ROOT" && cargo build --release)

mkdir -p "$WINBRIDGE_INSTALL_BIN_DIR"
install -m 755 "$RELEASE_BIN" "$INSTALL_BIN"

log_info "Installing desktop launcher and autostart entry..."
"$INSTALL_BIN" install-desktop-entry --exec "$INSTALL_BIN"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

log_info "Linux app installed:"
log_info "  binary: $INSTALL_BIN"
log_info "  launcher: ~/.local/share/applications/dev.winbridge.Winbridge.desktop"
log_info "  autostart: ~/.config/autostart/dev.winbridge.Winbridge.desktop"
log_info "You can now launch winbridge from the app icon even when the VM is off."
