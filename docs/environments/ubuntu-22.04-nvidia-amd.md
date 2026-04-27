# Ubuntu 22.04 + NVIDIA + AMD CPU 환경 검증

본 사용자 환경에서 winbridge P2A를 검증한 기록. 다른 환경 사용자가 본인 환경 결과를 추가하여 동일 디렉토리에 PR로 기여할 수 있다.

## 환경

| 항목 | 값 |
|---|---|
| OS | Ubuntu 22.04.5 LTS |
| 커널 | 6.8.0-110-generic |
| 데스크탑 세션 | X11 GNOME |
| GPU | NVIDIA GeForce RTX 3070 Mobile (driver 580+) |
| CPU | AuthenticAMD x86_64 |
| RAM | 16GB+ (호스트 + VM 4GB 여유) |
| libvirt | 8.0+ (qemu:///system) |
| FreeRDP | 2.x (apt `freerdp2-x11`) + 3.x (flatpak `com.freerdp.FreeRDP`) |
| WSL? | 아니오 (베어메탈) |

## Phase 0 결과 (2026-04-27)

| 게이트 | 결과 | 비고 |
|---|---|---|
| Gate-1 카톡 호환 | **PASS** | Server 2022 Standard (Desktop Experience)에 카톡 PC 정상 설치/실행. 한글 폰트 정상 |
| Gate-2 (b) TSAppAllowList | **FAIL** | FreeRDP 3.x 로그 `[preConnect]: Update size to 2490x1573` → 서버가 RemoteApp 모드 무시. P1A와 동일 시그니처 |
| Gate-2 (a) RDS Role + Deployment | **FAIL** | `New-RDSessionDeployment`가 `The server is not joined to a domain.` 에러. 워크그룹 환경에서 RDS 정공법 불가 |
| **채택 경로** | **B-2 폴백 (단일 default)** | RemoteApp 다중 앱 지원 포기, 카톡 1개만 가능 |

## 환경 특이 이슈 / 발견 사항

### 1. VM은 qemu:///system + virbr0 필수
virt-manager 기본 연결이 `qemu:///session`이라 user-mode SLiRP (10.0.2.15)로 만들어짐. SLiRP는 호스트→VM 인바운드 차단이라 RDP 검증 불가. **Phase 0 spike VM을 system URI로 마이그레이션 필요했음** (`/tmp/migrate-vm-via-pool.sh`로 자동화). P2A 자동화는 처음부터 `qemu:///system` 가정.

### 2. home pool + setfacl + AppArmor (root 디스크 우회)
`/var/lib/libvirt/images/` (root 파티션, 55GB)이 100% 가득 사고 (cp가 sparse 보존 안 함). 해결: home의 `~/.local/share/libvirt/images/`를 system libvirt가 직접 사용. setfacl + AppArmor abstractions 추가 필요. P2A의 `02-setup-libvirt.sh`가 자동화.

### 3. NIC = e1000e (virtio 드라이버 ISO 불필요)
Phase 0 spike VM에서 e1000e가 정상 동작 검증. virtio NIC + Windows 드라이버 ISO 조합은 추가 부담이라 default e1000e 채택. `config/libvirt-vm.xml.template`에 명시.

### 4. VM 부팅 모드 = BIOS (legacy)
virt-manager 기본 새 VM이 BIOS. UEFI(OVMF)는 별도 옵션. P2A는 BIOS default. autounattend.xml의 디스크 파티셔닝도 BIOS 가정 (Primary 2개, EFI 없음).

### 5. 한국어 IME 수동 설치 필요
Server 2022 영어 ISO 기본 IME에 한국어 없음. 한글 출력은 폰트로 가능하나 입력 X. 사용자가 옵션 A(Windows 설정 → Time & Language → Language → Add Korean)로 수동 설치. **P2A 자동화 범위 밖, P2B 후속 검토** (`Install-Language ko-KR`).

### 6. SPICE guest tools (클립보드 공유)
호스트↔VM 클립보드 공유는 `spice-guest-tools-latest.exe` 게스트 설치 필요. P2A의 `firstboot.ps1`이 자동 다운로드/설치 (옵션, 실패 시 warning만).

### 7. 카톡 PC 설치 경로 = Program Files (x64)
64-bit 인스톨러는 `C:\Program Files\Kakao\KakaoTalk\KakaoTalk.exe` (Program Files (x86) 아님). `firstboot.ps1`이 `Get-ChildItem`으로 동적 검출.

### 8. 호스트 디스크 정리 필요
P1A 잔존 자산 (winbridge-spike VM 41G + Win11 ISO 6.7G + apt cache + journal) 정리로 root 16G 회복. P2A의 `00-check-prerequisites.sh`가 home ≥70GB + root ≥5GB 사전조건 검증.

## Phase 1 자동화 결과

| Task | 산출물 | Commit |
|---|---|---|
| T1.3 | `scripts/lib/common.sh` + 단위 테스트 | 4285108 + 77675f4 (wait_for awk injection fix) |
| T1.4 | `scripts/host/00-check-prerequisites.sh` + 테스트 | e938969 |
| T1.5 | `scripts/host/01-download-iso.sh` + 테스트 | 1bf1ee5 |
| T1.6 | `scripts/host/02-setup-libvirt.sh` + 테스트 | 71d1c3a |
| T1.7 | `config/autounattend.xml.template` | c277c4a |
| T1.8 | `config/firstboot.ps1` | 6d84d17 |
| T1.9 | `config/libvirt-vm.xml.template` | fbb720e |
| T1.10 | `scripts/host/03-create-vm.sh` | 2e3d9a5 |
| T1.11 | `scripts/host/04-wait-for-install.sh` | d5acb73 |
| T1.12 | `scripts/host/05-verify-guest.sh` | 899a441 |
| T1.13 | `install.sh` | d2de243 |
| T1.14 | `uninstall.sh` | b408511 |
| T1.15 | `tests/e2e/install-uninstall-cycle.sh` | a5d5722 |

## 알려진 한계 (P2B 후속에서 다룰 항목)

- **카톡 1개 한정**: B-2 폴백 default라 다중 앱 추가 불가. 다른 Windows 앱 띄우려면 도메인 컨트롤러 추가 + RDS 또는 RDP Wrapper 같은 비공식 솔루션 검토 필요.
- **자동 시작 X**: install.sh 실행 후 VM이 켜져있을 뿐. 호스트 부팅/로그인 시 자동 시작은 P2B의 systemd user unit.
- **만료 관리 X**: Server 2022 Eval 180일. rearm/만료 알림은 P2B.
- **카톡 채팅 백업 가이드 X**: 재설치 시 로컬 캐시 소실. P2B의 README/troubleshooting.
- **IME 자동 설치 X**: 위 #5 참조.
- **다른 디스트로**: Fedora/Arch 등은 best-effort. 패키지 명/AppArmor/SELinux 차이 가능.
- **Wayland 세션**: best-effort. xfreerdp는 XWayland 경유 동작하지만 폴백 모드의 시각 통합은 X11 우위.

## 기여

본인 환경 검증 후 같은 형식으로 `docs/environments/<distro>-<gpu>-<cpu>.md` PR 환영. 특히 Intel iGPU + Mesa 환경의 게이트 결과 (Wayland 네이티브 세션 가능성) 가치 큼.
