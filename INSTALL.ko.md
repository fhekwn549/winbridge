# winbridge 설치 가이드

이 문서는 Ubuntu 22.04 기준으로 winbridge를 처음 설치하고 KakaoTalk을 실행하는 절차입니다. 가능한 그대로 복사해서 터미널에 붙여넣을 수 있게 작성했습니다.

영문 설치 가이드는 [INSTALL.md](INSTALL.md)를 보세요.

## 1. 기본 환경

권장 환경:

- Ubuntu 22.04
- 메모리 8 GB 이상
- 여유 디스크 50 GB 이상
- BIOS/UEFI에서 가상화 기능 활성화
- 인터넷 연결

KVM이 보이는지 확인합니다.

```bash
ls -l /dev/kvm
```

`/dev/kvm`이 없으면 BIOS/UEFI의 AMD-V 또는 Intel VT-x 설정을 먼저 켜야 합니다.

## 2. 패키지 설치

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

현재 사용자를 `libvirt` 그룹에 추가합니다.

```bash
sudo usermod -aG libvirt "$USER"
newgrp libvirt
```

그래도 `virsh -c qemu:///system list --all`이 권한 문제로 실패하면 로그아웃 후 다시 로그인하세요.

```bash
virsh -c qemu:///system list --all
```

## 3. Rust 설치

이미 `cargo`가 있다면 이 단계는 건너뛰어도 됩니다.

```bash
command -v cargo || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

확인:

```bash
cargo --version
```

## 4. 소스 코드 받기

```bash
cd "$HOME"
git clone https://github.com/fhekwn549/winbridge.git
cd "$HOME/winbridge"
```

이미 clone해 둔 저장소가 있다면 다음만 실행합니다.

```bash
cd "$HOME/winbridge"
git pull
```

## 5. Windows Server 2022 ISO 준비

Microsoft 공식 Evaluation Center에서 Windows Server 2022 ISO를 받습니다.

<https://www.microsoft.com/en-us/evalcenter/download-windows-server-2022>

브라우저로 받은 ISO 파일 경로를 설정합니다. 파일명이 다르면 실제 파일명에 맞게 바꾸세요.

```bash
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"
```

파일이 있는지 확인합니다.

```bash
ls -lh "$WINBRIDGE_ISO_DEST"
```

체크섬을 계산합니다.

```bash
export WINBRIDGE_ISO_SHA256="$(sha256sum "$WINBRIDGE_ISO_DEST" | awk '{print $1}')"
echo "$WINBRIDGE_ISO_SHA256"
```

브라우저로 이미 ISO를 받았다면 다운로드 URL은 필요 없습니다.

```bash
unset WINBRIDGE_ISO_URL
```

Microsoft 페이지에서 직접 다운로드 URL을 복사했고 winbridge가 다운로드까지 하게 만들고 싶다면 아래처럼 실행합니다. 직접 URL은 시간이 지나면 만료될 수 있습니다.

```bash
export WINBRIDGE_ISO_URL='https://download.microsoft.com/.../SERVER_EVAL_x64FRE_en-us.iso'
export WINBRIDGE_ISO_DEST="$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso"
curl -fL -o "$WINBRIDGE_ISO_DEST" "$WINBRIDGE_ISO_URL"
export WINBRIDGE_ISO_SHA256="$(sha256sum "$WINBRIDGE_ISO_DEST" | awk '{print $1}')"
```

## 6. VM 설치

설치를 시작합니다. Windows 무인 설치 때문에 보통 30-50분 정도 걸립니다.

```bash
./install.sh
```

설치 중에는 다음 작업이 자동으로 진행됩니다.

- `~/.config/winbridge/credentials`에 Windows Administrator 비밀번호 생성
- KVM/libvirt/FreeRDP/VM 생성 도구 점검
- Windows ISO 체크섬 검증
- libvirt 네트워크와 저장소 설정
- Windows VM 생성 및 무인 설치
- KakaoTalk 설치와 자동 시작 등록
- Windows 작업표시줄 자동 숨김 설정
- RDP 접속 검증

설치가 끝나면 RDP 창이 열립니다. 처음 실행이면 Windows KakaoTalk에서 QR 페어링 또는 전화번호 인증을 진행하세요.

## 7. winbridge 빌드

```bash
cargo build --release
```

## 8. 데스크톱 런처 설치

```bash
./target/release/winbridge install-desktop-entry --exec "$PWD/target/release/winbridge"
```

GNOME 앱 목록이나 Dock에서 KakaoTalk 아이콘이 보이지 않으면 한 번 로그아웃 후 다시 로그인하세요.

## 9. 실행

트레이 매니저를 실행합니다.

```bash
./target/release/winbridge
```

상단 트레이 아이콘에서 `Open KakaoTalk`을 누르거나, 앱 목록에서 KakaoTalk을 실행합니다.

터미널에서 바로 KakaoTalk 창만 열 수도 있습니다.

```bash
./target/release/winbridge start --mode app
```

Windows 전체 화면 설정이 필요하면 desktop 모드를 사용합니다.

```bash
./target/release/winbridge start --mode desktop
```

## 10. 종료와 재실행

KakaoTalk RDP 창만 닫아도 VM은 백그라운드에서 계속 실행됩니다.

VM을 일시정지하려면:

```bash
./target/release/winbridge stop
```

VM 상태 확인:

```bash
./target/release/winbridge status
```

진단:

```bash
./target/release/winbridge doctor
```

KakaoTalk 창이나 배경화면이 깨졌을 때:

```bash
./target/release/winbridge repair-kakao
./target/release/winbridge repair-wallpaper
```

`doctor`의 `guest service-session ...` 항목은 qemu-ga 서비스 세션에서 본 값입니다. 보이는 RDP 창이 정상이라면 해당 경고만으로 복구할 필요는 없습니다.

기존 VM에 qemu-ga를 추가하려면:

```bash
scripts/host/07-enable-qemu-ga.sh
```

그 다음 Windows 안에서 `virtio-win-guest-tools.exe` 또는 `guest-agent\qemu-ga-x86_64.msi`를 설치하고 VM을 재시작하세요.

lifecycle 설정 예시:

```toml
[lifecycle]
close-window = "keep-running"
quit = "managed-save"
idle-timeout-minutes = 30
```

트레이 프로세스를 다시 시작하려면:

```bash
pkill -f 'target/release/winbridge'
./target/release/winbridge
```

## 11. 작업표시줄이 다시 보일 때

새 VM 설치 과정에서는 Windows 작업표시줄 자동 숨김이 자동 적용됩니다.

이미 설치된 VM에서 작업표시줄이 계속 보이면 Windows PowerShell에서 `scripts/windows/position-kakaotalk.ps1` 내용을 한 번 실행하세요.

## 12. 제거

확인 질문을 보면서 제거:

```bash
./uninstall.sh
```

질문 없이 제거:

```bash
./uninstall.sh -y
```

## 문제 해결

권한 문제로 libvirt가 실패할 때:

```bash
id -nG "$USER"
virsh -c qemu:///system list --all
```

RDP 포트 확인:

```bash
nc -zv -w 3 192.168.122.50 3389
```

실행 중인 winbridge 프로세스 확인:

```bash
pgrep -af 'winbridge'
```
