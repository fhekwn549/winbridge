# config/firstboot.ps1
# Windows Server 2022 첫 부팅 시 OEM 드라이브(E:\)에서 자동 실행.
# Phase 0 결정에 따라 B-2 폴백 모드 단일 흐름:
#   RDP 활성화 → 카톡 설치 → explorer 셸 유지 + 카톡 자동 시작 → Server Manager 차단
#   → 작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김 → 재부팅
# explorer를 셸로 두는 이유: ctfmon/IME 핫키 등록이 explorer 셸 컨텍스트 의존.
# 이전 'Shell=KakaoTalk' 설계는 한글 IME 핫키가 죽어 입력 불가 → 폐기.
# WINBRIDGE_MODE 분기 없음, fallback-apply.ps1 별도 파일 없음.
# autounattend.xml의 FirstLogonCommands가 1회 호출.

$ErrorActionPreference = 'Stop'
New-Item -Path 'C:\winbridge' -ItemType Directory -Force | Out-Null
$LogPath    = 'C:\winbridge\firstboot.log'
$StatusPath = 'C:\winbridge\firstboot.status'

function Log {
    param([string]$Message)
    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
    "$ts $Message" | Out-File -FilePath $LogPath -Append -Encoding utf8
}

function FailWith {
    param([string]$Code, [string]$Message)
    Log "[FAIL] $Code - $Message"
    "FAIL_$Code" | Out-File -FilePath $StatusPath -Encoding ascii -NoNewline
    exit 10
}

Log "=== firstboot.ps1 START ==="

# Step 1: RDP 활성화
try {
    Set-ItemProperty 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name fDenyTSConnections -Value 0
    Enable-NetFirewallRule -DisplayGroup 'Remote Desktop' -ErrorAction SilentlyContinue
    Enable-NetFirewallRule -DisplayGroup '원격 데스크톱' -ErrorAction SilentlyContinue
    Log "[OK] RDP 활성화"
} catch {
    FailWith 'RDP_ENABLE' $_.Exception.Message
}

# Step 2: 카톡 PC 다운로드
$KakaoUrl   = if ($env:KAKAOTALK_URL) { $env:KAKAOTALK_URL } else { 'https://app-pc.kakaocdn.net/talk/win32/KakaoTalk_Setup.exe' }
$KakaoSetup = 'C:\winbridge\KakaoTalk_Setup.exe'
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $KakaoUrl -OutFile $KakaoSetup -UseBasicParsing
    if (-not (Test-Path $KakaoSetup) -or (Get-Item $KakaoSetup).Length -lt 1MB) {
        throw "다운로드 산출물이 너무 작음 ($KakaoSetup) - URL 변경 가능성"
    }
    $sizeMB = [Math]::Round((Get-Item $KakaoSetup).Length / 1MB, 1)
    Log "[OK] KakaoTalk 다운로드: $KakaoSetup ($sizeMB MB)"
} catch {
    FailWith 'KAKAO_DOWNLOAD' $_.Exception.Message
}

# Step 3: 카톡 사일런트 설치 (NSIS /S)
try {
    $proc = Start-Process -FilePath $KakaoSetup -ArgumentList '/S' -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "사일런트 설치 exit code $($proc.ExitCode) - /S 미지원 가능성 (R2 리스크)"
    }
    Log "[OK] KakaoTalk 사일런트 설치 완료"
} catch {
    FailWith 'KAKAO_INSTALL' $_.Exception.Message
}

# Step 4: 카톡 실제 경로 동적 검출
$KakaoExe = $null
foreach ($base in @('C:\Program Files\Kakao', 'C:\Program Files (x86)\Kakao')) {
    if (Test-Path $base) {
        $found = Get-ChildItem -Path $base -Recurse -Filter KakaoTalk.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) {
            $KakaoExe = $found.FullName
            Log "[OK] KakaoTalk 경로 검출: $KakaoExe"
            break
        }
    }
}
if (-not $KakaoExe) {
    FailWith 'KAKAO_NOT_FOUND' 'C:\Program Files{,(x86)}\Kakao 어디에도 KakaoTalk.exe 부재'
}

# Step 5: Winlogon Shell을 explorer.exe로 명시 (Windows 기본값이지만 안전성 위해 명시)
# 한글 IME 핫키(Win+Space, 한/영) 등록은 ctfmon이 explorer 셸 컨텍스트에서 실행될 때만 동작.
try {
    Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' `
        -Name 'Shell' -Value 'explorer.exe' -Type String
    Log "[OK] Winlogon Shell = explorer.exe (IME 호환)"
} catch {
    FailWith 'SHELL_SET' $_.Exception.Message
}

# Step 6: 카톡 자동 시작 등록 (HKCU Run, explorer가 처리)
try {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
        -Name 'KakaoTalk' -Value "`"$KakaoExe`"" -Type String
    Log "[OK] KakaoTalk HKCU Run 등록"
} catch {
    FailWith 'KAKAO_AUTOSTART' $_.Exception.Message
}

# Step 7: Server Manager 자동 시작 차단 (정책 + Scheduled Task)
try {
    $smPolicy = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Server\ServerManager'
    if (-not (Test-Path $smPolicy)) { New-Item -Path $smPolicy -Force | Out-Null }
    Set-ItemProperty -Path $smPolicy -Name 'DoNotOpenAtLogon' -Value 1 -Type DWord

    $smUser = 'HKCU:\Software\Microsoft\ServerManager'
    if (-not (Test-Path $smUser)) { New-Item -Path $smUser -Force | Out-Null }
    Set-ItemProperty -Path $smUser -Name 'DoNotOpenServerManagerAtLogon' -Value 1 -Type DWord
    Set-ItemProperty -Path $smUser -Name 'CheckedUnattendLaunchSetting' -Value 0 -Type DWord

    Disable-ScheduledTask -TaskName 'ServerManager' -TaskPath '\Microsoft\Windows\Server Manager\' -ErrorAction Stop | Out-Null
    Log "[OK] Server Manager 자동 시작 차단 (정책 + Scheduled Task)"
} catch {
    Log "[WARN] Server Manager 차단 일부 실패: $($_.Exception.Message)"
}

# Step 8: 한국어 IME 추가 + 디폴트 입력기 한국어로
# (autounattend에 InputLocale=0412 두면 xfreerdp v2.9 RDP 협상에서 segfault 재현됨 2026-04-29.
#  RDP 협상 시점은 영문 keymap만 노출, OS 부팅 후 PowerShell로 한국어 IME만 추가해 우회.)
try {
    $list = New-WinUserLanguageList -Language 'en-US'
    $list.Add('ko-KR')
    Set-WinUserLanguageList -LanguageList $list -Force
    Set-WinDefaultInputMethodOverride -InputTip "0412:{A028AE76-01B1-46C2-99C4-ACD9858AE02F}{B5FE1F02-D5F2-4445-9C03-C568F23C99A1}"
    Log "[OK] 한국어 IME 추가 + 디폴트 입력 한국어로"
} catch {
    Log "[WARN] 한국어 IME 추가 실패: $($_.Exception.Message)"
}

# Step 9: 데스크톱 아이콘 숨김 + 작업표시줄 자동 숨김
# StuckRects3 키는 explorer가 작업표시줄 처음 그릴 때 lazy 생성. firstboot.ps1 시점엔 부재라
# 즉시 시도 + RunOnce 폴백 등록으로 다음 부팅에서 보장.
try {
    $advKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    if (-not (Test-Path $advKey)) { New-Item -Path $advKey -Force | Out-Null }
    Set-ItemProperty -Path $advKey -Name 'HideIcons' -Value 1 -Type DWord

    $sr3 = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3'
    if (Test-Path $sr3) {
        $bytes = (Get-ItemProperty -Path $sr3 -Name Settings).Settings
        if ($bytes.Length -gt 8) {
            $bytes[8] = $bytes[8] -bor 0x01
            Set-ItemProperty -Path $sr3 -Name Settings -Value $bytes
            Log "[OK] 작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김 즉시 적용"
        }
    }

    # RunOnce 폴백 — 다음 부팅의 AutoLogon 시점에 한 번 더 시도 (StuckRects3 안정화 후)
    $fallbackCmd = @'
$sr3 = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3"
if (Test-Path $sr3) {
    $b = (Get-ItemProperty -Path $sr3 -Name Settings).Settings
    if ($b.Length -gt 8) {
        $b[8] = $b[8] -bor 0x01
        Set-ItemProperty -Path $sr3 -Name Settings -Value $b
    }
}
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "HideIcons" -Value 1 -Type DWord -Force
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
'@
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($fallbackCmd))
    $ro = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
    if (-not (Test-Path $ro)) { New-Item -Path $ro -Force | Out-Null }
    Set-ItemProperty -Path $ro -Name 'WinbridgeHideShell' -Value "powershell -NoProfile -EncodedCommand $enc" -Type String
    Log "[OK] 작업표시줄/데스크톱 숨김 RunOnce 폴백 등록 (다음 부팅 보장)"
} catch {
    Log "[WARN] 데스크톱/작업표시줄 숨김 일부 실패: $($_.Exception.Message)"
}

# Step 10: status SUCCESS + 재부팅 예약
# (SPICE guest tools 설치는 VirtIO PnP 미서명 컨펌으로 자동화 끊김 → P2B로 이관)
'SUCCESS' | Out-File -FilePath $StatusPath -Encoding ascii -NoNewline
Log "=== firstboot.ps1 SUCCESS, 30초 후 재부팅 ==="
Start-Process -FilePath 'shutdown.exe' -ArgumentList '/r /t 30 /c "winbridge firstboot done, rebooting"' -NoNewWindow
exit 0
