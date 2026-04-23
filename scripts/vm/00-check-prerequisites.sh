#!/usr/bin/env bash
# check prerequisites on the host (packages, kernel features, permissions)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

case "${1:-}" in
    -h|--help)
        cat <<EOF
Usage: $0

check prerequisites — verify the host machine has everything needed to provision a Windows VM:
  - CPU virtualization (AMD-V or VT-x) available
  - /dev/kvm accessible by current user
  - apt packages installed (qemu, libvirt, virtinst, freerdp2/3)
  - virtiofsd binary present (either /usr/libexec or /usr/lib/qemu)
  - xfreerdp3 (>=3) or xfreerdp (>=2) in PATH
  - user in 'kvm' and 'libvirt' groups
  - libvirtd service active
  - 50+ GB free in \$HOME

Exits non-zero on first missing requirement with a fix hint.
EOF
        exit 0
        ;;
esac

log_info "Checking CPU virtualization..."
if ! grep -qE '(svm|vmx)' /proc/cpuinfo; then
    die "CPU does not expose AMD-V/VT-x. Enable virtualization in BIOS/UEFI."
fi

log_info "Checking /dev/kvm..."
[ -c /dev/kvm ] || die "/dev/kvm missing. sudo apt install qemu-kvm and reboot."
if [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
    die "/dev/kvm not accessible. Run: sudo usermod -aG kvm \$USER && newgrp kvm"
fi

log_info "Checking required packages..."
REQUIRED_PKGS=(
    qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients
    virtinst bridge-utils cpu-checker xmlstarlet libxml2-utils jq curl
)
MISSING=()
for pkg in "${REQUIRED_PKGS[@]}"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || MISSING+=("$pkg")
done
if [ ${#MISSING[@]} -gt 0 ]; then
    die "missing apt packages: ${MISSING[*]}. Install: sudo apt install ${MISSING[*]}"
fi

log_info "Checking virtiofsd binary..."
VIRTIOFSD_CANDIDATES=(
    /usr/libexec/virtiofsd           # Ubuntu 23.04+, Rust version
    /usr/lib/qemu/virtiofsd          # Ubuntu 22.04 (bundled in qemu-system-common)
)
VIRTIOFSD_BIN=""
for c in "${VIRTIOFSD_CANDIDATES[@]}"; do
    if [ -x "$c" ]; then VIRTIOFSD_BIN="$c"; break; fi
done
[ -n "$VIRTIOFSD_BIN" ] || die "virtiofsd binary not found. On Ubuntu 22.04 it should be at /usr/lib/qemu/virtiofsd (inside qemu-system-common). On 23.04+ it's /usr/libexec/virtiofsd from the virtiofsd package. Install qemu-system-x86 or the virtiofsd package."
log_info "virtiofsd found at $VIRTIOFSD_BIN"

log_info "Checking FreeRDP..."
# Prefer xfreerdp3 (version 3+), accept xfreerdp (version 2+) as Ubuntu 22.04 fallback.
FRDP_BIN=""
FRDP_MIN_MAJOR=0
if command -v xfreerdp3 >/dev/null 2>&1; then
    FRDP_BIN=xfreerdp3
    FRDP_MIN_MAJOR=3
elif command -v xfreerdp >/dev/null 2>&1; then
    FRDP_BIN=xfreerdp
    FRDP_MIN_MAJOR=2
else
    die "neither xfreerdp3 nor xfreerdp found. Install freerdp2-x11 (default Ubuntu 22.04) or freerdp3-x11."
fi
FRDP_VER=$("$FRDP_BIN" --version 2>&1 | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)
[ -n "$FRDP_VER" ] || die "cannot parse FreeRDP version from $FRDP_BIN"
FRDP_MAJOR=${FRDP_VER%%.*}
[ "$FRDP_MAJOR" -ge "$FRDP_MIN_MAJOR" ] || die "FreeRDP $FRDP_BIN major=$FRDP_MAJOR found; need >= $FRDP_MIN_MAJOR"
log_info "FreeRDP OK: $FRDP_BIN $FRDP_VER"

log_info "Checking user groups (via /etc/group, authoritative)..."
current_user=$(id -un)
for grp in kvm libvirt; do
    members=$(getent group "$grp" 2>/dev/null | cut -d: -f4)
    if ! printf ',%s,' "$members" | grep -q ",$current_user,"; then
        die "user '$current_user' not registered in '$grp' group. Run: sudo usermod -aG $grp \$USER"
    fi
done

# Warn (do not fail) if current shell session doesn't yet have the groups applied.
shell_groups=$(id -nG)
missing_from_shell=()
for grp in kvm libvirt; do
    if ! printf ' %s ' "$shell_groups" | grep -q " $grp "; then
        missing_from_shell+=("$grp")
    fi
done
if [ ${#missing_from_shell[@]} -gt 0 ]; then
    log_warn "groups registered in /etc/group but NOT in current shell: ${missing_from_shell[*]}"
    log_warn "  this is fine as long as you re-login or open a new terminal before running any scripts that rely on these groups"
fi

log_info "Checking libvirtd service..."
systemctl is-active --quiet libvirtd || die "libvirtd not active. Run: sudo systemctl enable --now libvirtd"

log_info "Checking disk space in \$HOME..."
free_kb=$(df --output=avail -k "$HOME" | tail -n1 | tr -d ' ')
need_kb=$((50 * 1024 * 1024))
[ "$free_kb" -ge "$need_kb" ] || die "need 50GB free in \$HOME, have $((free_kb / 1024 / 1024))GB"

log_info "Creating winbridge directories..."
mkdir -p "$WINBRIDGE_IMAGES_DIR" "$WINBRIDGE_DOWNLOADS_DIR" "$WINBRIDGE_DATA_DIR" "$WINBRIDGE_ARCHIVE_DIR"

log_info "All prerequisites satisfied."
