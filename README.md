# winbridge

> Run the Windows-only apps that have no viable web alternative — as if they were native Linux apps.
>
> 웹으로 해결되지 않는 Windows 전용 앱을 리눅스에서 네이티브처럼.

**현재 상태: 설계 완료, 구현 전 (Pre-implementation).** 프로젝트명은 가제입니다.

---

## What

A Rust daemon that orchestrates a headless Windows VM (KVM) and projects individual app windows onto the Linux desktop via FreeRDP RemoteApp — with proper system tray, native notifications, global hotkeys, and host-side data persistence.

The first supported application is **KakaoTalk**. Additional apps are added via TOML profile files, not code changes.

## Motivation

### Why not Wine

Wine is a remarkable translation layer, but it carries structural limits:

- Korean IME integration remains fragile across Wine versions
- Font mapping issues — the classic "squares instead of Hangul"
- Every KakaoTalk update can break Wine compatibility
- Version-to-version regressions are frequent

winbridge sidesteps the compatibility question entirely by running the actual Windows binary on a real Windows kernel. The cost is VM resources; the win is that your app works exactly as it does on Windows — now and in the future, regardless of how the app evolves.

### Why not an unofficial protocol client

Libraries that reverse-engineer KakaoTalk's LOCO protocol (e.g. `node-kakao`) have been **unmaintained since 2022**, and using them carries a real account-ban risk because Kakao actively detects non-official clients. winbridge uses the official Windows KakaoTalk binary over RDP — from the server's perspective, the connection is indistinguishable from a Windows user.

### Why this positioning (what winbridge does *not* try to be)

winbridge focuses narrowly on a category we call **"native-exe-only apps without a viable web alternative"**. It is deliberately **not** a general-purpose Windows compatibility layer.

Explicit non-goals:

- **3D games** — RemoteApp streaming is 2D-centric. Games need Looking Glass or passthrough-class tech, which is out of scope.
- **Apps that already have good web or Linux alternatives** — MS Office, Notion, Slack, most productivity tools. Use the web/Linux version; winbridge adds nothing there.
- **System-level utilities** — disk managers, driver installers, firewall tools. These need host hardware access that RemoteApp cannot provide.
- **Protocol-level reimplementation** — no unofficial API use, no scraping, no ToS circumvention.

## How it works

### Key scenarios

**Launch (cold start):** You press `Super+K`. The daemon sees the VM is off, boots it headlessly (≈15s), spawns `xfreerdp3` in RemoteApp mode pointing at `KakaoTalk.exe`, and a single KakaoTalk window appears on your Linux desktop — indistinguishable from a native app window.

**Launch (warm start):** The VM is already running from a previous session. The same keypress takes ≈2s: no boot, just a new RemoteApp session inside the existing RDP connection.

**Incoming message:** FreeRDP reports that the window title changed to `"(3) 카카오톡"`. The daemon parses the digit, updates the tray icon with a red badge `3`. In Phase 2, a small helper inside the VM also forwards Windows Toast notifications over virtiofs, which the daemon re-emits as native D-Bus notifications — so your GNOME notification stack sees `홍길동: 안녕하세요` as a first-class notification.

**Idle suspend:** When the last window closes, a 5-minute idle timer starts. If no new launch happens, the VM is suspended to disk. Next launch resumes from suspend in ≈2s. You get "always available" UX without "always consuming RAM" cost.

### Architecture overview

Three processes cooperate as a small distributed system on one machine:

```
┌─ Linux host (Ubuntu 22.04, X11, GNOME) ─────────────────────────┐
│                                                                 │
│  winbridge (Rust daemon)                                        │
│   ├─ VM control     ──libvirt──▶ libvirtd / QEMU ──KVM──▶ [VM]  │
│   ├─ Window display ──spawn────▶ xfreerdp3 ───RDP───────▶ [VM]  │
│   ├─ Notifier       ──D-Bus───▶ GNOME Shell                     │
│   ├─ Tray           ──KSNI────▶ GNOME AppIndicator              │
│   ├─ Hotkeys        ──X11─────▶ XGrabKey                        │
│   └─ Archiver       ──inotify─▶ host filesystem                 │
│                                                                 │
│  [VM] Windows 11 Enterprise (headless, 40GB qcow2)              │
│   └─ KakaoTalk.exe  ── AppData junctioned to virtiofs mount ──┐ │
│                                                               │ │
│  Host filesystem                                              │ │
│   └─ ~/.local/share/winbridge/data/kakao/  ◀──────────────────┘ │
│                                            (virtiofs-shared)   │
└─────────────────────────────────────────────────────────────────┘
```

Design principles:

- **The VM is headless.** You never see a Windows desktop — only the individual app windows projected through RemoteApp.
- **Data is host-owned.** KakaoTalk writes its files into a virtiofs-shared directory that actually lives on the Linux filesystem. The VM can be rebuilt from scratch without losing data.
- **The daemon is a thin coordinator.** It does not render anything itself; it wires together libvirt, FreeRDP, D-Bus, X11, and the filesystem. The sum is more useful than any one of them alone.
- **New apps are a config change, not a code change.** Each supported Windows app is a TOML profile under `app-profiles/`.

### Code structure (Clean Architecture via Cargo workspace)

The daemon is split into four crates whose dependency direction is enforced by `Cargo.toml`:

```
crates/domain           pure Rust types and traits (ports). No external deps.
crates/application      use cases — orchestration logic. Depends on domain only.
crates/infrastructure   adapters — libvirt, FreeRDP, D-Bus, tray, inotify. Depends on domain only.
crates/daemon           binary — composition root that wires everything. Depends on all three.
```

A domain type accidentally importing `rclpy` — sorry, I mean `libvirt` — becomes a compile error. The architecture is verified by the compiler, not by convention.

## Data persistence and Windows licensing

Windows in the VM runs from a freely-available **Windows 11 Enterprise 90-day evaluation** image. The daemon schedules `slmgr.vbs /rearm` calls to extend the evaluation up to the allowed maximum (~360 days), and when that is exhausted it rebuilds the VM from scratch using an unattended-install script.

The rebuild is non-destructive to user data: all KakaoTalk data lives in a host directory shared to the VM via virtiofs, so wiping and reinstalling Windows does not touch chat archives, media, or settings. The only user-visible step after a rebuild is a one-time QR login from the mobile app.

Users are responsible for complying with Microsoft's evaluation license. Users who prefer a permanent license can supply their own legally-acquired key.

## Target environment

- Ubuntu 22.04 LTS (or compatible)
- AMD-V or Intel VT-x with `/dev/kvm` accessible
- X11 session (Wayland support is Phase 3)
- GNOME desktop with `gnome-shell-extension-appindicator`
- 8 GB+ RAM, 50 GB+ free disk

## Supported applications (planned)

| App | Phase | Status |
|---|---|---|
| KakaoTalk | 1 (MVP) | 설계 완료 |
| LINE | 2 | Planned |
| PotPlayer | 2 | Planned |
| 공인인증서 브라우저 (Edge) | 3 | Planned |

## Roadmap

- **Phase 1 (MVP):** Windows VM automation, RemoteApp window surfacing, global hotkey launch, basic tray, KakaoTalk profile.
- **Phase 2:** Unread badge, D-Bus notification bridge, daily archive sync, LINE/PotPlayer profiles.
- **Phase 3:** License auto-renewal & VM rebuild automation, settings GUI, Wayland hotkeys, 공인인증서 workflow.

---

## Legal notice

**This project is independent of and not affiliated with Kakao Corp., any messenger service provider, or Microsoft.** It is a personal desktop integration tool.

- **Windows licensing** — winbridge neither bundles nor distributes Windows. Users must obtain a valid Windows license (e.g. the freely downloadable Windows 11 Enterprise 90-day evaluation from Microsoft) to run the VM. Compliance with Microsoft's license terms is the user's responsibility.
- **KakaoTalk and other apps** — winbridge does not modify, patch, or reverse-engineer any application. It runs the official Windows binary, unmodified, inside a VM. Users must comply with each application's Terms of Service. This project must not be used to violate any service's ToS, automate abuse, or circumvent paid features.
- **Scope limitations (design-enforced):** The bundled profiles implement only *passive* preservation of data that the app itself writes to disk. This project explicitly does **not** implement unofficial protocol reimplementation, scraping of expired server content, bypassing server-side retention policies, or any form of automated message traffic.
- **No warranty.** Provided as-is under the MIT License. See [`LICENSE`](LICENSE).

If you believe this project violates a specific right or agreement, please open an issue before any other action.

## License

[MIT](LICENSE) © 2026 fhekwn549
