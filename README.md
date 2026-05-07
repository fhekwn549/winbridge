# winbridge

> Run Windows-only apps that have no viable web alternative as if they were native Linux apps.

Korean documentation: [README.ko.md](README.ko.md)

**Current status:** P2A proof of concept is complete. The project can provision a Windows Server 2022 Evaluation VM, run Windows KakaoTalk, expose it through a small Linux app window, provide a tray launcher, handle keyboard input, and bridge text clipboard in both directions.

The MVP supports **KakaoTalk only**. Multi-app profiles are out of scope for P2A.

## What

winbridge provisions a headless Windows Server 2022 Evaluation VM under libvirt/KVM and runs the official Windows KakaoTalk client inside it. A Rust host manager wakes the VM and opens KakaoTalk through an embedded RDP viewer so it behaves like a small Linux desktop app.

The guest keeps the standard `explorer.exe` shell for compatibility, but hides the Windows taskbar and desktop icons to keep the app window clean.

## Motivation

### Why not Wine

Wine is useful, but KakaoTalk on Wine has recurring structural issues:

- Korean IME integration is fragile across Wine versions.
- Font mapping can break Hangul rendering.
- KakaoTalk updates can break Wine compatibility.
- Version-to-version regressions are frequent.

winbridge avoids that compatibility layer by running the real Windows binary on a real Windows kernel.

### Why not an unofficial protocol client

Libraries that reverse-engineer KakaoTalk's LOCO protocol, such as `node-kakao`, have been unmaintained since 2022. They also carry real account risk because Kakao can detect unofficial clients. winbridge uses the official Windows KakaoTalk binary over RDP.

## How It Works

P2A uses a VM-based fallback instead of RemoteApp. RemoteApp-style single-window surfacing was tested and blocked by Windows Server 2022 RDS licensing/session constraints.

- The Windows guest keeps `explorer.exe` as its shell.
- `firstboot.ps1` installs KakaoTalk, disables Server Manager at logon, hides desktop icons, and enables taskbar auto-hide.
- KakaoTalk starts from `HKCU\Run` on Windows logon.
- The Linux host runs `winbridge`, a Rust manager with a tray entry, desktop launcher, embedded RDP viewer, keyboard input, and text clipboard bridge.

### Install Flow

1. `~/.config/winbridge/credentials` stores a generated Administrator password.
2. `scripts/host/00-check-prerequisites.sh` checks KVM, libvirt, FreeRDP, and VM build tools.
3. `scripts/host/01-download-iso.sh` downloads and verifies the Windows Server 2022 Evaluation ISO.
4. `scripts/host/02-setup-libvirt.sh` configures libvirt networking, storage, and AppArmor access.
5. `scripts/host/03-create-vm.sh` builds the OEM ISO, creates the qcow2 disk, defines the VM, and starts it.
6. `scripts/host/04-wait-for-install.sh` waits for unattended Windows setup and reboot stabilization.
7. `scripts/host/05-verify-guest.sh` verifies RDP authentication/session creation.
8. The Rust manager can then open the KakaoTalk app window for pairing and daily use.

## Architecture

```text
Linux host (Ubuntu 22.04)
  install.sh
    -> libvirt qemu:///system
    -> Windows Server 2022 Evaluation VM

  winbridge Rust manager
    -> tray + KakaoTalk desktop launcher
    -> embedded RDP viewer
    -> keyboard input
    -> bidirectional text clipboard

Windows guest
  explorer.exe shell
  taskbar auto-hide + hidden desktop icons
  KakaoTalk.exe autostarted from HKCU\Run
```

Design constraints:

- KakaoTalk only for P2A.
- VM state and KakaoTalk data live inside the qcow2 disk.
- Host shared persistence such as virtiofs is deferred.
- Installation is handled by bash scripts; daily use is handled by the Rust manager.

## Build And Run

Install build prerequisites:

```bash
sudo apt install -y \
  libgtk-4-dev \
  libgraphene-1.0-dev \
  libpango1.0-dev \
  libvirt-dev \
  pkg-config \
  libssl-dev
```

Build:

```bash
cargo build --release
```

Install the KakaoTalk desktop launcher and icon:

```bash
./target/release/winbridge install-desktop-entry --exec "$PWD/target/release/winbridge"
```

Run the tray manager:

```bash
./target/release/winbridge
```

Useful commands:

```bash
./target/release/winbridge start
./target/release/winbridge start --mode app
./target/release/winbridge start --mode desktop
./target/release/winbridge start --mode desktop --display experimental-multimon
./target/release/winbridge stop
./target/release/winbridge status
```

`app` mode opens a 480x680 KakaoTalk-focused RDP session and uses the `dev.winbridge.KakaoTalk` application identity. `desktop` mode opens a larger Windows management session for settings, debugging, and updates.

GNOME users may need the AppIndicator extension for tray icon support:

```bash
sudo apt install -y gnome-shell-extension-appindicator
```

## Quick Start

Requirements:

- Ubuntu 22.04
- KVM enabled and `/dev/kvm` accessible
- libvirt `qemu:///system`
- FreeRDP 2 or 3 for install verification and manual management
- 8 GB or more RAM
- 50 GB or more free disk space

Provide the Windows ISO location and checksum:

```bash
export WINBRIDGE_ISO_URL='https://...'
export WINBRIDGE_ISO_SHA256='...'
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"
```

Install:

```bash
./install.sh
```

After the VM is ready, build and run the Rust manager:

```bash
cargo build --release
./target/release/winbridge install-desktop-entry --exec "$PWD/target/release/winbridge"
./target/release/winbridge
```

Uninstall:

```bash
./uninstall.sh
./uninstall.sh -y
```

## Existing VM Taskbar Reset

New VM installs apply Windows taskbar auto-hide through `config/firstboot.ps1`.

For an existing VM, run the contents of `scripts/windows/position-kakaotalk.ps1` in Windows PowerShell if the taskbar becomes visible again.

## Known Limits

- KakaoTalk only.
- No automatic idle suspend policy.
- No Windows evaluation expiration management.
- No D-Bus notification bridge, badge bridge, or global hotkey support yet.
- No host shared KakaoTalk data storage yet.

## Roadmap

- **P2A:** Windows Server 2022 unattended install, KakaoTalk app window, tray/launcher, keyboard input, and bidirectional text clipboard.
- **P2B:** VM idle management, expiration management, notification bridge, persistence improvements, and candidate support for additional apps.
- **Long term:** Replace the proof-of-concept scripts with a more complete Rust-managed desktop integration.

## Legal Notice

This project is independent of and not affiliated with Kakao Corp., any messenger service provider, or Microsoft.

- Users must obtain and comply with a valid Windows license.
- winbridge runs the official Windows KakaoTalk binary unmodified.
- Users must comply with each application's Terms of Service.
- This project must not be used to automate abuse, bypass paid features, reimplement private protocols, or evade service-side retention policies.

## License

[MIT](LICENSE) (c) 2026 fhekwn549
