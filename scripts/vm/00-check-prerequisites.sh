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
  - apt packages installed (qemu, libvirt, virtinst, freerdp3)
  - xfreerdp3 >= 3.0.0 in PATH
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
    virtinst bridge-utils cpu-checker virtiofsd xmlstarlet libxml2-utils jq curl
)
MISSING=()
for pkg in "${REQUIRED_PKGS[@]}"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || MISSING+=("$pkg")
done
if [ ${#MISSING[@]} -gt 0 ]; then
    die "missing apt packages: ${MISSING[*]}. Install: sudo apt install ${MISSING[*]}"
fi

log_info "Checking FreeRDP 3.x..."
if ! command -v xfreerdp3 >/dev/null 2>&1; then
    die "xfreerdp3 not found. Ubuntu 22.04 ships FreeRDP 2.x; add PPA ppa:remmina-ppa-team/remmina-next and install freerdp3-x11."
fi
FRDP_VER=$(xfreerdp3 --version 2>&1 | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)
[ -n "$FRDP_VER" ] || die "cannot parse FreeRDP version"
FRDP_MAJOR=${FRDP_VER%%.*}
[ "$FRDP_MAJOR" -ge 3 ] || die "FreeRDP version $FRDP_VER found; need >= 3.0.0"

log_info "Checking user groups..."
user_groups=$(id -nG)
for grp in kvm libvirt; do
    if ! printf ' %s ' "$user_groups" | grep -q " $grp "; then
        die "user not in '$grp' group. Run: sudo usermod -aG $grp \$USER && re-login"
    fi
done

log_info "Checking libvirtd service..."
systemctl is-active --quiet libvirtd || die "libvirtd not active. Run: sudo systemctl enable --now libvirtd"

log_info "Checking disk space in \$HOME..."
free_kb=$(df --output=avail -k "$HOME" | tail -n1 | tr -d ' ')
need_kb=$((50 * 1024 * 1024))
[ "$free_kb" -ge "$need_kb" ] || die "need 50GB free in \$HOME, have $((free_kb / 1024 / 1024))GB"

log_info "Creating winbridge directories..."
mkdir -p "$WINBRIDGE_IMAGES_DIR" "$WINBRIDGE_DOWNLOADS_DIR" "$WINBRIDGE_DATA_DIR" "$WINBRIDGE_ARCHIVE_DIR"

log_info "All prerequisites satisfied."
