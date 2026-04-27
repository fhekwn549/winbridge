# P2A 다음 세션 시작 가이드

**작성일:** 2026-04-27 (P2A Phase 1 자동화 코드 완료 시점)
**다음 세션에서 사용:** 본 문서를 새 세션 첫 메시지에 첨부하거나 `cat docs/resume-p2a.md`로 읽혀서 컨텍스트 복원.

---

## 현재 상태 한 줄

winbridge **P2A Phase 1 자동화 코드 작성 완료**. 실제 `install.sh` 실행 + 최종 코드 리뷰 + finishing-branch는 다음 세션에서.

## 진행 위치

```
브랜치: phase-2a-server-remoteapp (commits 5e9fc79..2ceb779, 13 커밋)
플랜: docs/superpowers/plans/2026-04-27-plan-p2a-server-remoteapp-install.md
스펙: docs/superpowers/specs/2026-04-27-winbridge-server-remoteapp-design.md
환경 기록: docs/environments/ubuntu-22.04-nvidia-amd.md
Phase 0 결과: docs/manual-checks.md
```

## Phase 0 (수동 검증) 결과 요약

| 게이트 | 결과 |
|---|---|
| Gate-1 카톡 호환 (Server 2022 + 카톡 PC) | **PASS** |
| Gate-2 (b) TSAppAllowList | FAIL (P1A와 동일 시그니처) |
| Gate-2 (a) RDS Role + Deployment | FAIL (워크그룹 환경에서 도메인 가입 요구) |
| **채택 경로** | **B-2 폴백 단일 default** (RemoteApp 분기 제거, 카톡 1개 한정) |

## Phase 1 완료 Task (16개)

| Task | 파일 | Commit |
|---|---|---|
| T1.1 | 브랜치 phase-2a-server-remoteapp 생성 | (no commit) |
| T1.2 | scaffold + .gitignore | 5e9fc79 |
| T1.3 | scripts/lib/common.sh + 테스트 (+wait_for awk fix) | 4285108, 77675f4 |
| T1.4 | 00-check-prerequisites.sh + 테스트 | e938969 |
| T1.5 | 01-download-iso.sh + 테스트 | 1bf1ee5 |
| T1.6 | 02-setup-libvirt.sh + 테스트 | 71d1c3a |
| T1.7 | autounattend.xml.template | c277c4a |
| T1.8 | firstboot.ps1 (B-2 단일) | 6d84d17 |
| T1.9 | libvirt-vm.xml.template (BIOS+e1000e) | fbb720e |
| T1.10 | 03-create-vm.sh | 2e3d9a5 |
| T1.11 | 04-wait-for-install.sh | d5acb73 |
| T1.12 | 05-verify-guest.sh | 899a441 |
| T1.13 | install.sh 오케스트레이터 | d2de243 |
| T1.14 | uninstall.sh | b408511 |
| T1.15 | tests/e2e/install-uninstall-cycle.sh | a5d5722 |
| T1.16 | docs/environments/ubuntu-22.04-nvidia-amd.md | 2ceb779 |

## 다음 세션 — 두 옵션 중 선택

### 옵션 A. 최종 리뷰 + finishing-branch (코드 검증 우선)

```
1. subagent-driven-development 스킬: dispatch 최종 code-reviewer subagent
   - 13개 commit 전체를 종합 검토
   - 잠재 이슈 (set -e 호환, idempotency 회귀, 에러 메시지 일관성 등)
2. superpowers:finishing-a-development-branch 스킬 invoke
   - PR 만들지 / merge할지 / 더 다듬을지 결정
3. (선택) 실제 install.sh 1회 실행
```

### 옵션 B. 실제 install.sh 한 번 돌려보기 (e2e 검증 우선)

```
사전: WINBRIDGE_ISO_URL, WINBRIDGE_ISO_SHA256 환경변수 설정.
사용자 환경엔 이미 ISO 있으니 WINBRIDGE_ISO_DEST도 지정.

1. cd /home/fhekwn549/winbridge
2. git checkout phase-2a-server-remoteapp  (이미면 skip)
3. 환경변수:
   export WINBRIDGE_ISO_URL='https://software-static.download.prss.microsoft.com/...'
   export WINBRIDGE_ISO_SHA256='<sha256 from manual-checks.md>'
   export WINBRIDGE_ISO_DEST=$HOME/Downloads/SERVER_EVAL_x64FRE_en-us.iso
4. ./install.sh
5. ~30~50분 대기 후 RDP 창 자동 표시. 카톡만 단독 보이는지 확인.
6. 폰으로 페어링 (QR 또는 전화번호)
7. 채팅 1건 송수신 검증
```

성공 시 → 옵션 A로. 실패 시 → 디버깅 + plan 갱신.

## 빠른 시작 (다음 세션 첫 명령)

```bash
cd /home/fhekwn549/winbridge
git status
git log --oneline phase-2a-server-remoteapp ^main | head -20
cat docs/resume-p2a.md   # 본 문서
```

다음 세션 Claude에게 보낼 첫 메시지 예시:

```
P2A 작업 재개. docs/resume-p2a.md 읽고 컨텍스트 복원해줘.
오늘 세션엔 옵션 A로 진행 (최종 리뷰 + finishing-branch).
```

또는

```
P2A 작업 재개. 옵션 B로 install.sh 실제 실행하고 카톡 페어링까지 가고 싶어.
ISO sha256은 ____________ (manual-checks.md 채워둔 값).
```

## P2B 후속 (Phase 2/3, 별도 plan)

본 P2A에서 의도적으로 제외한 항목 — 별도 plan으로 다룰 예정:

- 사용자 로그인 시 systemd user unit 자동 시작 (`winbridge-session.service`)
- VM `managedsave` (hibernate) 라이프사이클
- 다른 Windows 앱 추가 (`register-app.sh`) — 단 B-2 모드 한계로 카톡 1개라 제한적
- 만료 알림 + 자동 rearm (`check-expiry.sh`, `rearm.sh`)
- qemu-guest-agent 도입 → 게스트 명령/파일 채널
- 한국어 IME 자동 설치 (`Install-Language ko-KR`)
- README.md 사용자 가이드 본문
- CI 구성 (.github/workflows/test.yml)

## 알려진 미해결 / 짚고 갈 점

1. **install.sh 마지막의 `/p:` 비밀번호 노출** — `ps`에서 잠시 보임. 향후 `/from-stdin`으로 전환 검토.
2. **05-verify-guest.sh의 5초 sleep** — 호스트 부하/네트워크 지연 시 false-positive PASS 가능성. e2e 1회 후 시간 조정.
3. **AppArmor sed 마커 round-trip** — install.sh의 02 단계 작성 형식과 uninstall.sh의 sed 패턴 일치 검증 e2e 필요.
4. **03-create-vm.sh dry-run 부작용** — qcow2/oem.iso 실제 생성. preview only 시맨틱 X.
5. **certificate grep 시그니처 폭** — 05-verify-guest의 `/cert:ignore`가 정상 흐름에서 노란 경고 표시 가능.

## 정리하지 않은 호스트 자원

- `winbridge-spike-p2a` VM (qemu:///system, 60GB qcow2 sparse 실제 ~12GB) — Phase 0 디버깅용. P2A install.sh 검증 후 정리 권장:
  ```
  sudo virsh -c qemu:///system destroy winbridge-spike-p2a
  sudo virsh -c qemu:///system undefine winbridge-spike-p2a --remove-all-storage
  ```
  단 install.sh가 default로 같은 이름(`winbridge-srv2022`)을 쓰므로 충돌은 없음.
- `~/Downloads/SERVER_EVAL_x64FRE_en-us.iso` (4.7GB) — 옵션 B 진행 시 그대로 재사용.

## 메모리 (auto-memory) 업데이트 필요 사항

본 세션 종료 시점 메모리에 추가/갱신할 가치 있는 사실:
- "P2A는 워크그룹 환경에서 RemoteApp 활성화 못 함을 두 번째로 확인" (P1A에 이어). RDS Deployment는 도메인 가입 필수.
- "Server 2022 데스크톱 에디션 + TSAppAllowList 트릭은 데스크톱/Server 모두 막힘" (Phase 0 검증).
- 폴백 모드(B-2)가 본 P2A의 유일 동작 경로.
