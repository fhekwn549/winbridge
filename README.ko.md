# winbridge

> 웹으로 해결되지 않는 Windows 전용 앱을 리눅스에서 네이티브 앱처럼 실행합니다.

영문 문서: [README.md](README.md)

**현재 상태:** P2A POC 검증 완료. Windows Server 2022 Evaluation VM을 자동 구성하고, Windows용 KakaoTalk을 작은 Linux 앱 창처럼 띄우며, 트레이 런처, 키보드 입력, 양방향 텍스트 클립보드를 지원합니다.

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
- KakaoTalk은 Windows 로그온 시 `HKCU\Run`으로 자동 시작됩니다.
- Linux 호스트는 Rust `winbridge` 매니저로 트레이, 데스크톱 런처, 내장 RDP 뷰어, 키보드 입력, 텍스트 클립보드를 제공합니다.

## 설치

터미널 입력 기준 설치와 실행 절차는 [install.md](install.md)를 보세요.

## 아키텍처

```text
Linux 호스트 (Ubuntu 22.04)
  install.sh
    -> libvirt qemu:///system
    -> Windows Server 2022 Evaluation VM

  winbridge Rust 매니저
    -> 트레이 + KakaoTalk 데스크톱 런처
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
- 설치는 bash 스크립트가 담당하고, 일상 사용은 Rust 매니저가 담당합니다.

## 알려진 한계

- KakaoTalk만 지원합니다.
- 자동 유휴 절전 정책은 아직 없습니다.
- Windows Evaluation 만료 관리는 아직 없습니다.
- D-Bus 알림 브리지, 뱃지 브리지, 전역 핫키는 아직 없습니다.
- KakaoTalk 데이터의 호스트 공유 저장소는 아직 없습니다.

## 로드맵

- **P2A:** Windows Server 2022 무인 설치, KakaoTalk 앱 창, 트레이/런처, 키보드 입력, 양방향 텍스트 클립보드.
- **P2B:** VM 유휴 관리, 만료 관리, 알림 브리지, 영속성 개선, 추가 앱 후보 검토.
- **장기:** POC 스크립트를 더 완성도 높은 Rust 기반 데스크톱 통합으로 대체.

## 법적 고지

이 프로젝트는 Kakao Corp., 메신저 서비스 제공자, Microsoft와 무관한 독립 프로젝트입니다.

- 사용자는 유효한 Windows 라이선스를 확보하고 준수해야 합니다.
- winbridge는 공식 Windows KakaoTalk 바이너리를 수정 없이 실행합니다.
- 사용자는 각 애플리케이션의 약관을 준수해야 합니다.
- 이 프로젝트는 악용 자동화, 유료 기능 우회, 비공개 프로토콜 재구현, 서비스 측 보존 정책 우회에 사용되어서는 안 됩니다.

## 라이선스

[MIT](LICENSE) (c) 2026 fhekwn549
