# winbridge

> Run the Windows-only apps that have no viable web alternative — as if they were native Linux apps.
>
> 웹으로 해결되지 않는 Windows 전용 앱을 리눅스에서 네이티브처럼.

**현재 상태: P2A POC 검증 완료** — `./install.sh`로 무인 자동 설치 (~30~50분), 카톡 자동 시작/단독 표시/RDP 안정 접속/폰 페어링까지 end-to-end 동작. 프로젝트명은 가제입니다.

---

## What

A bash-driven installer that provisions a headless **Windows Server 2022 Evaluation** VM under libvirt/KVM and launches **KakaoTalk** via FreeRDP. The guest keeps the standard `explorer.exe` shell (한글 IME 호환에 필수) but hides the taskbar and desktop icons — so the Linux desktop sees a near-clean KakaoTalk window.

The MVP supports **KakaoTalk only**. Multi-app TOML profiles are out of P2A scope.

## Motivation

### Why not Wine

Wine is a remarkable translation layer, but it carries structural limits:

- Korean IME integration remains fragile across Wine versions
- Font mapping issues — the classic "squares instead of Hangul"
- Every KakaoTalk update can break Wine compatibility
- Version-to-version regressions are frequent

winbridge sidesteps the compatibility question entirely by running the actual Windows binary on a real Windows kernel. The cost is VM resources; the win is that your app works exactly as it does on Windows.

### Why not an unofficial protocol client

Libraries that reverse-engineer KakaoTalk's LOCO protocol (e.g. `node-kakao`) have been **unmaintained since 2022**, and using them carries a real account-ban risk because Kakao actively detects non-official clients. winbridge uses the official Windows KakaoTalk binary over RDP — from the server's perspective, the connection is indistinguishable from a Windows user.

### Why this positioning

winbridge focuses narrowly on **"native-exe-only apps without a viable web alternative"** — currently KakaoTalk only. Explicit non-goals: 3D games, apps with good web/Linux alternatives, system-level utilities, protocol-level reimplementation.

## How it works (P2A — current implementation)

### 채택 경로: B-2 폴백

Phase 0 검토에서 RemoteApp 단독 윈도우 surfacing이 게스트 측 제약(Server 2022 RDS 라이선스/세션 모델 등)으로 실패함을 확인했다. 따라서 P2A는 **B-2 폴백 단일 default**로 채택한다.

- 게스트 Windows는 `explorer.exe` 셸을 유지 (한글 IME 핫키가 셸 컨텍스트 의존)
- 작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김 + Server Manager 자동 시작 차단으로 시각적 노이즈 최소화
- 로그온 시 KakaoTalk이 HKCU\Run으로 자동 실행
- Linux 호스트는 RDP 세션을 FreeRDP로 띄움 (`./start-session.sh`)

### 설치 흐름 (`./install.sh`)

1. `~/.config/winbridge/credentials`에 random Administrator 비밀번호 생성/로드
2. `scripts/host/00-check-prerequisites.sh` — KVM/libvirt/FreeRDP/virt-install 점검
3. `scripts/host/01-download-iso.sh` — Server 2022 Eval ISO 다운로드 + sha256 검증 (이미 있고 일치하면 skip)
4. `scripts/host/02-setup-libvirt.sh` — `qemu:///system` virbr0 정적 DHCP 매핑, home storage pool, AppArmor (멱등)
5. `scripts/host/03-create-vm.sh` — `autounattend.xml` + `firstboot.ps1`을 담은 OEM ISO 생성, qcow2 디스크 생성, libvirt define + start
6. `scripts/host/04-wait-for-install.sh` — 무인 설치/재부팅 안정화 (~30~50분), RDP 응답 대기
7. `scripts/host/05-verify-guest.sh` — RDP 인증/세션 가능 확인
8. FreeRDP 창을 띄워 카톡 단독 표시 확인 + 폰 페어링

이후 재접속:

```bash
./start-session.sh    # FreeRDP 한 줄 wrapper (자격 증명 자동 로드, /kbd:0x00000409 안전망 포함)
```

제거: `./uninstall.sh` (각 단계 y/N 컨펌, `-y`로 자동 응답).

### Architecture overview

```
┌─ Linux host (Ubuntu 22.04) ─────────────────────────────────────┐
│                                                                 │
│  install.sh (bash 오케스트레이터)                               │
│   ├─ libvirt(qemu:///system) ── virsh ──▶ libvirtd ─KVM─▶ [VM]  │
│   └─ FreeRDP (xfreerdp3)     ── RDP ───────────────────▶ [VM]  │
│                                                                 │
│  [VM] Windows Server 2022 Eval (기본 4 GB RAM, 2 vCPU, qcow2)   │
│   ├─ explorer.exe 셸 (한글 IME 호환 위해 유지)                  │
│   ├─ 작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김                │
│   └─ KakaoTalk.exe (HKCU\Run으로 자동 시작)                     │
└─────────────────────────────────────────────────────────────────┘
```

설계 원칙:

- **단일 앱(KakaoTalk) 한정.** 다중 앱·TOML 프로파일은 P2A 범위 밖
- **bash + 멱등 스크립트.** Rust daemon이나 Cargo workspace, 별도 데몬 프로세스는 P2A에 없음
- **데이터 영속성은 VM qcow2 내부.** virtiofs 호스트 공유는 P2A에 없음 (P2B 후보)
- **수동 단계는 manual-checks 절차로 격리.** ISO URL/sha256은 사용자가 직접 확보

## Quick start

전제: Ubuntu 22.04 + KVM 활성 + `xfreerdp3` + `libvirt-daemon-system`/`virt-install`/`genisoimage`/`qemu-utils` 설치, 8 GB+ RAM, 50 GB+ 여유 디스크.

```bash
# manual-checks 절차로 확보한 값 입력
export WINBRIDGE_ISO_URL='https://...'                                       # Server 2022 Eval ISO URL
export WINBRIDGE_ISO_SHA256='...'                                            # 소문자 hex
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"     # 선택

./install.sh
# 30~50분 후 카톡 단독 RDP 창이 뜨면 폰으로 QR 페어링
```

제거:

```bash
./uninstall.sh        # 각 단계 컨펌
./uninstall.sh -y     # 자동 컨펌 (regression용)
```

## Target environment

- Ubuntu 22.04 LTS (검증 환경: `docs/environments/ubuntu-22.04-nvidia-amd.md`)
- AMD-V 또는 Intel VT-x with `/dev/kvm` accessible
- libvirt `qemu:///system`
- FreeRDP 3 (`xfreerdp3`)
- 8 GB+ RAM, 50 GB+ free disk

## 알려진 한계 (P2A)

- KakaoTalk 1개 앱 한정
- VM 자동 시작/idle suspend/만료(slmgr) 자동 관리 없음
- **Korean IME(한국어 입력) 자동화 없음.** xfreerdp v2.x가 게스트측 한국어 IME 활성 상태에서
  RDP 채널 협상 중 segfault하는 호환 이슈가 있고, Ubuntu 22.04엔 v3 패키지가 부재. 회피책:
  - 호스트(Linux)에서 한글 입력 → 복사 → RDP 카톡 채팅창에 `Ctrl+V` (RDP 클립보드 자동)
  - 영문/이모지/스티커는 RDP 안에서 직접 사용
- D-Bus 알림 브리지, 트레이/뱃지, 전역 핫키 없음 (P2B)
- virtiofs 데이터 영속성/호스트 공유 없음 (P2B)

## Roadmap

- **P2A (현재):** Server 2022 Eval 무인 설치 자동화, explorer 셸 + 작업표시줄 숨김 폴백, install/uninstall/start-session 진입점, 카톡 실행 가능성 POC 검증
- **P2B (예정):** Korean IME 자동화 (xfreerdp v3 / SPICE-vdagent 단독 / NoMachine / RustDesk 등 원격 프로토콜 재검토), VM 자동 시작/만료 관리, 알림 브리지, 데이터 영속성 (virtiofs), 추가 앱 후보
- **장기:** 본격 사용을 위한 Rust 기반 데몬/UI 재구현 (현재 bash POC를 대체)

## Data persistence and Windows licensing

Windows in the VM runs from the freely-available **Windows Server 2022 Evaluation** image (180-day eval). 만료 후 자동 재설치/rearm 자동화는 P2B 항목이다.

P2A 단계에서는 KakaoTalk 데이터가 VM 내부 qcow2 디스크에 저장된다. 호스트 측 영속 공유(virtiofs)는 P2B 후보이며 현재 구현에 없다.

Users are responsible for complying with Microsoft's evaluation license.

---

## Legal notice

**This project is independent of and not affiliated with Kakao Corp., any messenger service provider, or Microsoft.** It is a personal desktop integration tool.

- **Windows licensing** — winbridge neither bundles nor distributes Windows. Users must obtain a valid Windows license (e.g. the freely downloadable Windows Server 2022 evaluation from Microsoft) to run the VM. Compliance with Microsoft's license terms is the user's responsibility.
- **KakaoTalk and other apps** — winbridge does not modify, patch, or reverse-engineer any application. It runs the official Windows binary, unmodified, inside a VM. Users must comply with each application's Terms of Service. This project must not be used to violate any service's ToS, automate abuse, or circumvent paid features.
- **Scope limitations (design-enforced):** The bundled installer implements only *passive* operation of the official KakaoTalk Windows client. This project explicitly does **not** implement unofficial protocol reimplementation, scraping of expired server content, bypassing server-side retention policies, or any form of automated message traffic.
- **No warranty.** Provided as-is under the MIT License. See [`LICENSE`](LICENSE).

If you believe this project violates a specific right or agreement, please open an issue before any other action.

## License

[MIT](LICENSE) © 2026 fhekwn549
