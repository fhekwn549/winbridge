# winbridge Installation Guide

This guide walks through a first-time winbridge installation and the daily commands for opening KakaoTalk. The verified host target is Ubuntu 22.04.5 LTS. Ubuntu 24.04 LTS is pending direct validation in <https://github.com/fhekwn549/winbridge/issues/2>.

The Windows VM setup still uses this repository's host scripts. The Linux app itself can be installed from the release `.deb` package, from the early APT repository, or from source.

Korean guide: [INSTALL.ko.md](INSTALL.ko.md)

## 1. Base Environment

Verified environment:

- Ubuntu 22.04.5 LTS
- 8 GB RAM or more
- 50 GB free disk space or more
- Hardware virtualization enabled in BIOS/UEFI
- Internet access

Expected but not yet verified:

- Ubuntu 24.04 LTS

Check that KVM is available.

```bash
ls -l /dev/kvm
```

If `/dev/kvm` does not exist, enable AMD-V or Intel VT-x in BIOS/UEFI first.

## 2. Install Host Packages

```bash
sudo apt update
sudo apt install -y \
  git \
  curl \
  openssl \
  netcat-openbsd \
  build-essential \
  pkg-config \
  libssl-dev \
  libvirt-daemon-system \
  libvirt-clients \
  virtinst \
  qemu-system-x86 \
  qemu-utils \
  genisoimage \
  gettext-base \
  acl \
  freerdp2-x11 \
  desktop-file-utils \
  libgtk-4-dev \
  libgraphene-1.0-dev \
  libpango1.0-dev \
  libvirt-dev \
  gnome-shell-extension-appindicator
```

This package list covers VM creation, source builds, and local package builds. If you only install the release `.deb`, it pulls the runtime dependencies, but the VM creation scripts still need the host tools above.

Add the current user to the `libvirt` group.

```bash
sudo usermod -aG libvirt "$USER"
newgrp libvirt
```

If `virsh -c qemu:///system list --all` still fails with a permission error, log out and log back in.

```bash
virsh -c qemu:///system list --all
```

## 3. Install Rust

Skip this step if `cargo` is already installed.

```bash
command -v cargo || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Verify Rust:

```bash
cargo --version
```

## 4. Clone Source Code

```bash
cd "$HOME"
git clone https://github.com/fhekwn549/winbridge.git
cd "$HOME/winbridge"
```

If the repository is already cloned, run only:

```bash
cd "$HOME/winbridge"
git pull
```

## 5. Prepare the Windows Server 2022 ISO

Download the Windows Server 2022 ISO from the official Microsoft Evaluation Center.

<https://www.microsoft.com/en-us/evalcenter/download-windows-server-2022>

Set the path to the ISO file downloaded in your browser. Change the filename if yours is different.

```bash
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"
```

Check that the file exists.

```bash
ls -lh "$WINBRIDGE_ISO_DEST"
```

Calculate the checksum.

```bash
export WINBRIDGE_ISO_SHA256="$(sha256sum "$WINBRIDGE_ISO_DEST" | awk '{print $1}')"
echo "$WINBRIDGE_ISO_SHA256"
```

If you already downloaded the ISO in your browser, no download URL is needed.

```bash
unset WINBRIDGE_ISO_URL
```

If you copied a direct download URL from the Microsoft page and want winbridge to handle the download, run the commands below. Direct Microsoft download URLs can expire over time.

```bash
export WINBRIDGE_ISO_URL='https://download.microsoft.com/.../SERVER_EVAL_x64FRE_en-us.iso'
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"
curl -fL -o "$WINBRIDGE_ISO_DEST" "$WINBRIDGE_ISO_URL"
export WINBRIDGE_ISO_SHA256="$(sha256sum "$WINBRIDGE_ISO_DEST" | awk '{print $1}')"
```

## 6. Install the VM

Start the installer. The unattended Windows setup usually takes about 30-50 minutes.

```bash
./install.sh
```

The installer automatically performs these steps:

- Creates a generated Windows Administrator password at `~/.config/winbridge/credentials`
- Checks KVM, libvirt, FreeRDP, and VM build tools
- Verifies the Windows ISO checksum
- Configures libvirt networking and storage
- Creates the Windows VM and runs unattended setup
- Installs KakaoTalk and registers it for automatic startup
- Registers Winbridge URL Forwarder as a Windows default-app candidate for `http` and `https`
- Enables Windows taskbar auto-hide
- Verifies RDP access

When installation finishes, an RDP window opens. On first use, pair or authenticate Windows KakaoTalk with QR login or phone-number login.

## 7. Install the Linux App

### Option A: Install the release package

Download the latest `.deb` from:

<https://github.com/fhekwn549/winbridge/releases/tag/v0.1.0>

Install it:

```bash
sudo apt install ./winbridge_0.1.0_amd64.deb
```

This installs:

- `/usr/bin/winbridge`
- `/usr/share/applications/dev.winbridge.WinbridgeApp.desktop`
- `/usr/share/applications/winbridge.desktop`
- `/usr/share/icons/hicolor/256x256/apps/winbridge.png`

### Option B: Install from the early APT repository

The repository is currently unsigned and intended for early validation.

```bash
echo 'deb [arch=amd64 trusted=yes] https://fhekwn549.github.io/winbridge stable main' | sudo tee /etc/apt/sources.list.d/winbridge.list
sudo apt update
sudo apt install winbridge
```

### Option C: Install from source

```bash
scripts/host/08-install-linux-app.sh
```

This builds `target/release/winbridge`, installs it to `~/.local/bin/winbridge`, installs the winbridge app launcher, and registers the same launcher for login autostart.

If the winbridge icon does not appear in the GNOME app list or Dock, log out and log back in once. The launcher runs `~/.local/bin/winbridge start --mode app`, so clicking it can start or resume the VM even when the VM is not already running.

Build a local Debian package:

```bash
scripts/release/build-deb.sh
```

Build the static APT repository files:

```bash
scripts/release/build-apt-repo.sh
```

## 8. Open Guest Links on the Host Browser

winbridge can make `http` and `https` links clicked inside Windows KakaoTalk open in the Linux host browser instead of Edge inside the VM.

New VM installs already register `Winbridge URL Forwarder` inside Windows as a default-app candidate. For an existing VM or after updating winbridge, refresh it with:

```bash
winbridge install-url-forwarder
```

If running from source before installing the package, use:

```bash
cargo run -- install-url-forwarder
```

Then, inside the Windows VM:

1. Open **Settings -> Apps -> Default apps**.
2. Search for **Winbridge URL Forwarder**.
3. Choose it for both `http` and `https`.
4. Test from KakaoTalk by clicking a web link. It should open on the Linux host browser.

Windows protects this final default-app choice with a `UserChoice` hash. winbridge registers the app candidate automatically, but the final `http`/`https` selection must be done manually once. If Windows falls back to Edge after a reboot, repeat the same selection.

## 9. Run

Start the tray manager.

```bash
winbridge
```

Click `Open Winbridge` from the top tray icon, or launch winbridge from the app list.

You can also open only the KakaoTalk window from the terminal.

```bash
winbridge start --mode app
```

Use desktop mode when you need the full Windows desktop.

```bash
winbridge start --mode desktop
```

## 10. Stop and Restart

Closing only the KakaoTalk RDP window keeps the VM running in the background.

To pause the VM:

```bash
winbridge stop
```

Check VM status:

```bash
winbridge status
```

Run diagnostics:

```bash
winbridge doctor
```

Repair Winbridge window placement or wallpaper state:

```bash
winbridge repair-kakao
winbridge repair-wallpaper
```

`doctor` entries named `guest service-session ...` come from the qemu-ga Windows service session. If the visible RDP window is healthy, those warnings alone do not require repair.

Retrofit qemu-ga on an existing VM:

```bash
scripts/host/07-enable-qemu-ga.sh
```

Then install `virtio-win-guest-tools.exe` or `guest-agent\qemu-ga-x86_64.msi` inside Windows and restart the VM.

Lifecycle config example:

```toml
[lifecycle]
close-window = "keep-running"
quit = "managed-save"
idle-timeout-minutes = 30
```

Restart the tray process:

```bash
pkill -f 'winbridge'
winbridge
```

## 11. If the Windows Taskbar Reappears

New VM installations apply Windows taskbar auto-hide automatically.

If the taskbar keeps showing in an existing VM, run the contents of `scripts/windows/position-kakaotalk.ps1` once in Windows PowerShell.

## 12. Uninstall

If installed from `.deb` or APT:

```bash
sudo apt remove winbridge
```

If installed from the early APT repository and you want to remove the source entry:

```bash
sudo rm -f /etc/apt/sources.list.d/winbridge.list
sudo apt update
```

If installed from source or if you want to remove the VM and generated host resources, use the repository uninstaller.

Uninstall with confirmation prompts:

```bash
./uninstall.sh
```

Uninstall without prompts:

```bash
./uninstall.sh -y
```

## Troubleshooting

If libvirt fails with a permission error:

```bash
id -nG "$USER"
virsh -c qemu:///system list --all
```

Check the RDP port:

```bash
nc -zv -w 3 192.168.122.50 3389
```

Check running winbridge processes:

```bash
pgrep -af 'winbridge'
```
