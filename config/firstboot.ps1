# config/firstboot.ps1
# Windows Server 2022 첫 부팅 시 OEM 드라이브(E:\)에서 자동 실행.
# Phase 0 결정에 따라 B-2 폴백 모드 단일 흐름:
#   RDP 활성화 → 카톡 설치 → 카톡 자동 시작 → SPICE guest tools → explorer 차단 → 재부팅
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

# Step 5: 카톡 자동 시작 등록 (HKCU Run)
try {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
        -Name 'KakaoTalk' -Value "`"$KakaoExe`"" -Type String
    Log "[OK] KakaoTalk HKCU Run 등록"
} catch {
    FailWith 'KAKAO_AUTOSTART' $_.Exception.Message
}

# Step 6: SPICE guest tools 설치 (필수 X, 실패 시 warning)
$SpiceUrl   = 'https://www.spice-space.org/download/windows/spice-guest-tools/spice-guest-tools-latest.exe'
$SpiceSetup = 'C:\winbridge\spice-guest-tools.exe'
try {
    Invoke-WebRequest -Uri $SpiceUrl -OutFile $SpiceSetup -UseBasicParsing
    $proc = Start-Process -FilePath $SpiceSetup -ArgumentList '/S' -Wait -PassThru
    if ($proc.ExitCode -eq 0) {
        Log "[OK] SPICE guest tools 설치 완료"
    } else {
        Log "[WARN] SPICE guest tools 설치 exit code $($proc.ExitCode) - 클립보드 공유 미동작 가능, 부가 기능이라 진행"
    }
} catch {
    Log "[WARN] SPICE guest tools 설치 스킵: $($_.Exception.Message) - 부가 기능"
}

# Step 7: explorer shell 차단 (B-2 폴백 핵심)
try {
    Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' `
        -Name 'Shell' -Value '' -Type String
    Log "[OK] explorer Shell 차단 (다음 부팅부터 적용)"
} catch {
    FailWith 'SHELL_BLOCK' $_.Exception.Message
}

# Step 8: status SUCCESS + 재부팅 예약
'SUCCESS' | Out-File -FilePath $StatusPath -Encoding ascii -NoNewline
Log "=== firstboot.ps1 SUCCESS, 30초 후 재부팅 ==="
Start-Process -FilePath 'shutdown.exe' -ArgumentList '/r /t 30 /c "winbridge firstboot done, rebooting"' -NoNewWindow
exit 0
