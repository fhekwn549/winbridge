# winbridge

> Run Windows-only apps that have no viable web alternative as if they were native Linux apps.

winbridge is a Linux KakaoTalk / Ubuntu KakaoTalk bridge. It runs the official Windows KakaoTalk client in a managed Windows VM and opens it from Ubuntu like a native desktop app.

Korean documentation: [README.ko.md](README.ko.md)

**Current status:** Ubuntu 22.04.5 LTS is the verified host target. winbridge can provision a Windows Server 2022 Evaluation VM, run Windows KakaoTalk, expose it through a small Linux app window, provide app launchers and tray actions, handle keyboard input, bridge text clipboard in both directions, and forward guest browser links to the Linux host.

Ubuntu 24.04 LTS is expected to work but remains pending direct validation: <https://github.com/fhekwn549/winbridge/issues/2>.

The MVP supports **KakaoTalk only**. Multi-app profiles are out of scope for P2A.

Common search terms this project targets: KakaoTalk on Linux, KakaoTalk on Ubuntu, Linux KakaoTalk, Ubuntu KakaoTalk, 리눅스 카카오톡, 우분투 카카오톡.

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
- `firstboot.ps1` registers Winbridge URL Forwarder as a Windows default-app candidate for `http` and `https` links.
- KakaoTalk starts from `HKCU\Run` on Windows logon.
- The Linux host runs `winbridge`, a Rust manager with a tray entry, desktop launcher, embedded RDP viewer, keyboard input, text clipboard bridge, VM diagnostics, and repair commands.
- QEMU guest agent commands run in a Windows service session. GUI repair commands trigger an interactive Scheduled Task so KakaoTalk window positioning happens in the logged-in RDP user session.

## Installation

For terminal-first installation and daily-use commands, see [INSTALL.md](INSTALL.md).

### Release Package

The current release package is published at:

- <https://github.com/fhekwn549/winbridge/releases/tag/v0.1.0>

Install the downloaded package:

```bash
sudo apt install ./winbridge_0.1.0_amd64.deb
```

An early unsigned APT repository is also available:

```text
deb [arch=amd64 trusted=yes] https://fhekwn549.github.io/winbridge stable main
```

The APT repository is unsigned for now and should be treated as early validation infrastructure.

### Source Install

The Linux-side app installer places `winbridge` at `~/.local/bin/winbridge` and installs a winbridge launcher plus login autostart entry:

```bash
scripts/host/08-install-linux-app.sh
```

The launcher runs `winbridge start --mode app`, so clicking the winbridge icon can start or resume the VM before opening KakaoTalk.

### Build A Debian Package

Build a local `.deb` package:

```bash
scripts/release/build-deb.sh
```

The package is written to `dist/winbridge_<version>_amd64.deb` and installs the binary, desktop launchers, and icon under `/usr`. Build release packages on Ubuntu 22.04 when targeting both Ubuntu 22.04 and newer systems, because that keeps the runtime libc baseline compatible.

Build the static APT repository:

```bash
scripts/release/build-apt-repo.sh
```

Existing VMs can retrofit QEMU guest agent support:

```bash
scripts/host/07-enable-qemu-ga.sh
```

After attaching the channel/ISO, install `virtio-win-guest-tools.exe` or `guest-agent\qemu-ga-x86_64.msi` inside Windows and restart the VM.

## Operate

Check host, VM, RDP, qemu-ga, and guest-side health:

```bash
cargo run -- doctor
```

Repair guest-side state:

```bash
cargo run -- repair-kakao
cargo run -- repair-wallpaper
```

`doctor` labels guest checks as `guest service-session ...` when data comes from qemu-ga. Treat these checks as service-session diagnostics, not proof that the visible RDP user session is broken. If the visible KakaoTalk window works from the tray, no repair is needed.

Lifecycle defaults keep the VM running when the KakaoTalk window closes, managed-save on tray quit, and leave idle timeout disabled. Override them in `~/.config/winbridge/config.toml`:

```toml
[lifecycle]
close-window = "keep-running"     # or "managed-save"
quit = "managed-save"             # or "keep-running"
idle-timeout-minutes = 30         # omit to disable
```

`cargo run -- status` prints the VM state and lifecycle summary.

### Guest Links

winbridge can open `http` and `https` links clicked inside Windows KakaoTalk on the Linux host browser. New VM installs register `Winbridge URL Forwarder` automatically as a Windows default-app candidate. Existing VMs can install or refresh it with:

```bash
winbridge install-url-forwarder
```

If running from source, use:

```bash
cargo run -- install-url-forwarder
```

Windows protects the final `http`/`https` default-app choice with a `UserChoice` hash, so winbridge cannot safely force that selection. In the Windows VM, open **Settings -> Apps -> Default apps**, search for **Winbridge URL Forwarder**, and choose it for both `http` and `https`. If Windows falls back to Edge after a reboot, repeat that manual selection once.

## Architecture

```text
Linux host (Ubuntu 22.04.5 LTS verified, Ubuntu 24.04 pending)
  install.sh
    -> libvirt qemu:///system
    -> Windows Server 2022 Evaluation VM

  winbridge Rust manager
    -> tray + winbridge desktop launcher
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
- Source installation is handled by bash scripts; release installation is handled by the Debian package. Daily use is handled by the Rust manager.

## Known Limits

- KakaoTalk only.
- Ubuntu 24.04 support is not confirmed until issue #2 has direct execution evidence.
- The APT repository is unsigned and uses `trusted=yes` during early validation.
- Automatic idle suspend is available through optional config but disabled by default.
- No Windows evaluation expiration management.
- Tray action result notifications are local host notifications; no KakaoTalk message notification bridge, badge bridge, or global hotkey support yet.
- No host shared KakaoTalk data storage yet.

## Roadmap

- **Current:** Ubuntu 22.04.5 verified release package, GitHub Release `.deb`, early unsigned APT repository, app launcher/tray flow, URL forwarding, diagnostics, and repair commands.
- **Next:** Validate Ubuntu 24.04, add signed APT repository metadata, improve setup/doctor automation, and tighten package install checks.
- **Long term:** Replace more host setup scripts with Rust-managed desktop and VM integration.

## Legal Notice

This project is independent of and not affiliated with Kakao Corp., any messenger service provider, or Microsoft.

- Users must obtain and comply with a valid Windows license.
- winbridge runs the official Windows KakaoTalk binary unmodified.
- Users must comply with each application's Terms of Service.
- This project must not be used to automate abuse, bypass paid features, reimplement private protocols, or evade service-side retention policies.

## License

[MIT](LICENSE) (c) 2026 fhekwn549
