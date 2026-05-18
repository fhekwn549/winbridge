# winbridge

> 웹으로 해결되지 않는 Windows 전용 앱을 리눅스에서 네이티브 앱처럼 실행합니다.

영문 문서: [README.md](README.md)

**현재 상태:** Ubuntu 22.04.5 LTS가 검증된 호스트 대상입니다. Windows Server 2022 Evaluation VM을 자동 구성하고, Windows용 KakaoTalk을 작은 Linux 앱 창처럼 띄우며, 앱 런처와 트레이 동작, 키보드 입력, 양방향 텍스트 클립보드, 게스트 링크의 Linux 호스트 브라우저 전달을 지원합니다.

Ubuntu 24.04 LTS는 동작 가능성이 높지만 아직 직접 검증 전입니다: <https://github.com/fhekwn549/winbridge/issues/2>.

현재 MVP는 **KakaoTalk만** 지원합니다. 여러 앱을 TOML 프로파일로 관리하는 기능은 P2A 범위 밖입니다.

## 개요

winbridge는 libvirt/KVM 위에 헤드리스 Windows Server 2022 Evaluation VM을 만들고, 그 안에서 공식 Windows KakaoTalk 클라이언트를 실행합니다. Linux 호스트의 Rust 매니저가 VM을 깨우고 내장 RDP 뷰어로 KakaoTalk을 열어 작은 Linux 데스크톱 앱처럼 보이게 합니다.

게스트 Windows는 호환성을 위해 표준 `explorer.exe` 셸을 유지합니다. 대신 Windows 작업표시줄은 자동 숨김으로 설정하고 데스크톱 아이콘은 숨겨 앱 창의 시각적 노이즈를 줄입니다.

## 배경

### Wine을 쓰지 않는 이유

Wine은 유용하지만 KakaoTalk에서는 구조적인 문제가 반복됩니다.

- Wine 버전에 따라 한국어 IME 연동이 불안정합니다.
- 글꼴 매핑 문제로 한글이 깨질 수 있습니다.
- KakaoTalk 업데이트가 Wine 호환성을 깨뜨릴 수 있습니다.
- 버전 간 회귀가 자주 발생합니다.

winbridge는 Windows 호환 계층을 거치지 않고 실제 Windows 바이너리를 실제 Windows 커널 위에서 실행합니다.

### 비공식 프로토콜 클라이언트를 쓰지 않는 이유

`node-kakao`처럼 KakaoTalk LOCO 프로토콜을 역공학한 라이브러리는 2022년 이후 유지보수가 사실상 중단되었습니다. 또한 Kakao가 비공식 클라이언트를 감지할 수 있어 계정 리스크가 있습니다. winbridge는 공식 Windows KakaoTalk 바이너리를 RDP로 사용합니다.

## 동작 방식

P2A는 RemoteApp 대신 VM 기반 폴백 경로를 사용합니다. RemoteApp 형태의 단일 창 노출은 Windows Server 2022의 RDS 라이선스/세션 모델 제약으로 막히는 것을 확인했습니다.

- Windows 게스트는 `explorer.exe` 셸을 유지합니다.
- `firstboot.ps1`이 KakaoTalk 설치, Server Manager 자동 시작 차단, 데스크톱 아이콘 숨김, 작업표시줄 자동 숨김을 적용합니다.
- `firstboot.ps1`이 `http`/`https` 링크용 Windows 기본앱 후보로 `Winbridge URL Forwarder`를 등록합니다.
- KakaoTalk은 Windows 로그온 시 `HKCU\Run`으로 자동 시작됩니다.
- Linux 호스트는 Rust `winbridge` 매니저로 트레이, 데스크톱 런처, 내장 RDP 뷰어, 키보드 입력, 텍스트 클립보드, VM 진단, 복구 명령을 제공합니다.
- QEMU guest agent 명령은 Windows 서비스 세션에서 실행됩니다. GUI 복구 명령은 interactive Scheduled Task를 트리거해 로그인된 RDP 사용자 세션에서 KakaoTalk 창 위치 조정을 실행합니다.

## 설치

터미널 입력 기준 설치와 실행 절차는 [INSTALL.ko.md](INSTALL.ko.md)를 보세요.

### 릴리즈 패키지

현재 릴리즈 패키지:

- <https://github.com/fhekwn549/winbridge/releases/tag/v0.1.0>

다운로드한 패키지 설치:

```bash
sudo apt install ./winbridge_0.1.0_amd64.deb
```

초기 unsigned APT 저장소도 사용할 수 있습니다.

```text
deb [arch=amd64 trusted=yes] https://fhekwn549.github.io/winbridge stable main
```

현재 APT 저장소는 서명 전이므로 초기 검증용으로 봐야 합니다.

### 소스 설치

Linux 앱 설치 스크립트는 `winbridge`를 `~/.local/bin/winbridge`에 넣고 winbridge 런처와 로그인 자동시작 항목을 등록합니다.

```bash
scripts/host/08-install-linux-app.sh
```

런처는 `winbridge start --mode app`을 실행하므로 winbridge 아이콘 클릭만으로 VM 시작 또는 재개 후 KakaoTalk을 열 수 있습니다.

### Debian 패키지 빌드

로컬 `.deb` 패키지 생성:

```bash
scripts/release/build-deb.sh
```

패키지는 `dist/winbridge_<version>_amd64.deb`에 생성되며 `/usr` 아래에 바이너리, 데스크톱 런처, 아이콘을 설치합니다. Ubuntu 22.04와 그 이후 버전을 함께 대상으로 삼는 릴리즈 패키지는 Ubuntu 22.04에서 빌드하는 것이 안전합니다. 런타임 libc 기준선을 낮게 유지하기 위해서입니다.

정적 APT 저장소 생성:

```bash
scripts/release/build-apt-repo.sh
```

기존 VM에 QEMU guest agent를 붙일 때:

```bash
scripts/host/07-enable-qemu-ga.sh
```

채널/ISO를 붙인 뒤 Windows 안에서 `virtio-win-guest-tools.exe` 또는 `guest-agent\qemu-ga-x86_64.msi`를 설치하고 VM을 재시작합니다.

## 운영

호스트, VM, RDP, qemu-ga, 게스트 상태를 확인합니다.

```bash
cargo run -- doctor
```

게스트 상태 복구:

```bash
cargo run -- repair-kakao
cargo run -- repair-wallpaper
```

`doctor`가 `guest service-session ...`으로 표시하는 항목은 qemu-ga 서비스 세션에서 본 진단입니다. 보이는 RDP 사용자 세션이 고장났다는 뜻이 아닙니다. 트레이 `Open Winbridge`로 창이 정상 표시되면 복구할 필요가 없습니다.

lifecycle 기본값은 KakaoTalk 창 닫기 시 VM 유지, 트레이 종료 시 managed-save, idle timeout 비활성입니다. `~/.config/winbridge/config.toml`에서 바꿀 수 있습니다.

```toml
[lifecycle]
close-window = "keep-running"     # 또는 "managed-save"
quit = "managed-save"             # 또는 "keep-running"
idle-timeout-minutes = 30         # 생략하면 비활성
```

`cargo run -- status`는 VM 상태와 lifecycle 요약을 출력합니다.

### 게스트 링크

winbridge는 Windows KakaoTalk 안에서 누른 링크를 Linux 호스트 브라우저로 열 수 있습니다. 새 VM 설치는 `Winbridge URL Forwarder`를 Windows 기본앱 후보로 자동 등록합니다. 기존 VM은 아래 명령으로 설치 또는 갱신합니다.

```bash
cargo run -- install-url-forwarder
```

Windows는 최종 `http`/`https` 기본앱 선택을 `UserChoice` hash로 보호하므로 winbridge가 안전하게 강제 설정할 수 없습니다. Windows VM 안에서 Settings -> Apps -> Default apps에서 `http`와 `https`를 각각 `Winbridge URL Forwarder`로 한 번 선택하세요. 재부팅 뒤 Edge로 돌아가면 같은 선택을 한 번 다시 적용하세요.

## 아키텍처

```text
Linux 호스트 (Ubuntu 22.04.5 LTS 검증됨, Ubuntu 24.04 검증 대기)
  install.sh
    -> libvirt qemu:///system
    -> Windows Server 2022 Evaluation VM

  winbridge Rust 매니저
    -> 트레이 + winbridge 데스크톱 런처
    -> 내장 RDP 뷰어
    -> 키보드 입력
    -> 양방향 텍스트 클립보드

Windows 게스트
  explorer.exe 셸
  작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김
  HKCU\Run에서 KakaoTalk.exe 자동 시작
```

설계 제약:

- P2A는 KakaoTalk 단일 앱만 대상으로 합니다.
- VM 상태와 KakaoTalk 데이터는 qcow2 디스크 내부에 저장됩니다.
- virtiofs 같은 호스트 공유 영속성은 후속 단계로 미룹니다.
- 소스 설치는 bash 스크립트가 담당하고, 릴리즈 설치는 Debian 패키지가 담당합니다. 일상 사용은 Rust 매니저가 담당합니다.

## 알려진 한계

- KakaoTalk만 지원합니다.
- Ubuntu 24.04 지원은 issue #2의 직접 실행 증거가 쌓이기 전까지 확정하지 않습니다.
- APT 저장소는 아직 unsigned이며 초기 검증 동안 `trusted=yes`를 사용합니다.
- 자동 유휴 절전 정책은 설정으로 사용할 수 있지만 기본값은 비활성입니다.
- Windows Evaluation 만료 관리는 아직 없습니다.
- 트레이 작업 결과 알림은 호스트 로컬 알림입니다. KakaoTalk 메시지 알림 브리지, 뱃지 브리지, 전역 핫키는 아직 없습니다.
- KakaoTalk 데이터의 호스트 공유 저장소는 아직 없습니다.

## 로드맵

- **현재:** Ubuntu 22.04.5 검증 릴리즈 패키지, GitHub Release `.deb`, 초기 unsigned APT 저장소, 앱 런처/트레이 흐름, URL forwarding, 진단과 복구 명령.
- **다음:** Ubuntu 24.04 검증, signed APT 저장소 메타데이터 추가, setup/doctor 자동화 개선, 패키지 설치 검증 강화.
- **장기:** 더 많은 호스트 설정 스크립트를 Rust 기반 데스크톱/VM 통합으로 대체.

## 법적 고지

이 프로젝트는 Kakao Corp., 메신저 서비스 제공자, Microsoft와 무관한 독립 프로젝트입니다.

- 사용자는 유효한 Windows 라이선스를 확보하고 준수해야 합니다.
- winbridge는 공식 Windows KakaoTalk 바이너리를 수정 없이 실행합니다.
- 사용자는 각 애플리케이션의 약관을 준수해야 합니다.
- 이 프로젝트는 악용 자동화, 유료 기능 우회, 비공개 프로토콜 재구현, 서비스 측 보존 정책 우회에 사용되어서는 안 됩니다.

## 라이선스

[MIT](LICENSE) (c) 2026 fhekwn549
