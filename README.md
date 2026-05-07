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

## Installation

For terminal-first installation and daily-use commands, see [INSTALL.md](INSTALL.md).

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
