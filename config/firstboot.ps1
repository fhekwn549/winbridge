# config/firstboot.ps1
# Windows Server 2022 첫 부팅 시 OEM 드라이브(E:\)에서 자동 실행.
# Phase 0 결정에 따라 B-2 폴백 모드 단일 흐름 (P2A: 카톡 실행 가능성 POC):
#   RDP 활성화 → 카톡 설치 → explorer 셸 유지 + 카톡 자동 시작 → Server Manager 차단
#   → 작업표시줄 자동 숨김 + 데스크톱 아이콘 숨김 → 재부팅
# 한국어 IME 자동 등록은 P2A 범위 밖 (xfreerdp v2.x RDP 협상 segfault → P2B 이관).
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

# Step 1b: QEMU Guest Agent 설치 (virtio-win ISO가 연결된 경우)
try {
    $virtioRoots = Get-PSDrive -PSProvider FileSystem |
        ForEach-Object { $_.Root } |
        Where-Object {
            (Test-Path (Join-Path $_ 'virtio-win-guest-tools.exe')) -or
            (Test-Path (Join-Path $_ 'guest-agent\qemu-ga-x86_64.msi')) -or
            (Test-Path (Join-Path $_ 'vioserial'))
        }

    $virtioRoot = $virtioRoots | Select-Object -First 1
    if ($virtioRoot) {
        Log "[INFO] virtio-win media detected: $virtioRoot"

        $vioserial = Join-Path $virtioRoot 'vioserial'
        if (Test-Path $vioserial) {
            Get-ChildItem -Path $vioserial -Recurse -Filter '*.inf' -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\amd64\\' } |
                ForEach-Object {
                    pnputil.exe /add-driver $_.FullName /install | Out-Null
                }
            Log "[OK] virtio serial driver install attempted"
        }

        $guestTools = Join-Path $virtioRoot 'virtio-win-guest-tools.exe'
        if (Test-Path $guestTools) {
            $proc = Start-Process -FilePath $guestTools -ArgumentList '/install', '/quiet', '/norestart' -Wait -PassThru
            Log "[OK] virtio-win guest tools installer exit code $($proc.ExitCode)"
        }

        $qgaMsi = Join-Path $virtioRoot 'guest-agent\qemu-ga-x86_64.msi'
        if (Test-Path $qgaMsi) {
            $proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList "/i `"$qgaMsi`" /qn /norestart" -Wait -PassThru
            Log "[OK] qemu-ga MSI installer exit code $($proc.ExitCode)"
        }

        Set-Service -Name 'QEMU-GA' -StartupType Automatic -ErrorAction SilentlyContinue
        Start-Service -Name 'QEMU-GA' -ErrorAction SilentlyContinue
        Log "[OK] QEMU Guest Agent install/start attempted"
    } else {
        Log "[WARN] virtio-win media not found; QEMU Guest Agent not installed"
    }
} catch {
    Log "[WARN] QEMU Guest Agent setup failed: $($_.Exception.Message)"
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

# Step 6: 카톡 foreground 실행/위치 보정 스크립트 설치 + 자동 시작 등록
try {
    $positionScriptPath = 'C:\winbridge\position-kakaotalk.ps1'
    $positionScript = @'
param(
    [int]$Left = 0,
    [int]$Top = 0,
    [int]$Width = 960,
    [int]$Height = 720,
    [switch]$Restart
)

$ErrorActionPreference = 'Stop'
$LogPath = 'C:\winbridge\position-kakaotalk.log'

function Log {
    param([string]$Message)
    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
    "$ts $Message" | Out-File -FilePath $LogPath -Append -Encoding utf8
}

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class WinbridgeWindow {
    [DllImport("user32.dll")]
    public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int width, int height, bool repaint);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int command);

    [DllImport("user32.dll", SetLastError=true)]
    public static extern IntPtr FindWindow(string className, string windowName);
}
"@

function Find-KakaoTalkExe {
    $candidates = @(
        "$env:ProgramFiles\Kakao\KakaoTalk\KakaoTalk.exe",
        "${env:ProgramFiles(x86)}\Kakao\KakaoTalk\KakaoTalk.exe"
    )

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return $candidate
        }
    }

    $searchRoots = @()
    foreach ($root in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ($root) {
            $searchRoots += Join-Path $root 'Kakao'
        }
    }

    foreach ($base in $searchRoots) {
        if (-not (Test-Path $base)) {
            continue
        }

        $found = Get-ChildItem -Path $base -Recurse -Filter 'KakaoTalk.exe' -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($found) {
            return $found.FullName
        }
    }

    throw 'KakaoTalk.exe not found.'
}

function Get-KakaoTalkMainProcess {
    Get-Process -Name 'KakaoTalk' -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
}

function Wait-KakaoTalkMainProcess {
    param([int]$Attempts = 60)

    for ($i = 0; $i -lt $Attempts; $i++) {
        Start-Sleep -Milliseconds 250
        $process = Get-KakaoTalkMainProcess
        if ($process) {
            return $process
        }
    }

    return $null
}

function Stop-KakaoTalkProcesses {
    Get-Process -Name 'KakaoTalk' -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

function Enable-TaskbarAutoHide {
    $advanced = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    if (-not (Test-Path $advanced)) {
        New-Item -Path $advanced -Force | Out-Null
    }
    Set-ItemProperty -Path $advanced -Name 'HideIcons' -Value 1 -Type DWord -Force

    foreach ($key in @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects2'
    )) {
        if (-not (Test-Path $key)) {
            continue
        }

        $settings = (Get-ItemProperty -Path $key -Name Settings -ErrorAction SilentlyContinue).Settings
        if ($settings -and $settings.Length -gt 8) {
            $settings[8] = $settings[8] -bor 0x01
            Set-ItemProperty -Path $key -Name Settings -Value $settings -Force
        }
    }
}

try {
    Log 'position-kakaotalk.ps1 START'
    if ($Restart) {
        Stop-KakaoTalkProcesses
        Start-Sleep -Milliseconds 500
    }

    $process = Get-KakaoTalkMainProcess
    if (-not $process) {
        $kakaoExe = Find-KakaoTalkExe
        foreach ($attempt in 1..2) {
            Start-Process -FilePath $kakaoExe
            $process = Wait-KakaoTalkMainProcess
            if ($process) {
                break
            }
        }
    }

    if (-not $process -or $process.MainWindowHandle -eq 0) {
        throw 'KakaoTalk main window not found.'
    }

    $SW_RESTORE = 9
    Enable-TaskbarAutoHide
    [WinbridgeWindow]::ShowWindow($process.MainWindowHandle, $SW_RESTORE) | Out-Null
    [WinbridgeWindow]::MoveWindow($process.MainWindowHandle, $Left, $Top, $Width, $Height, $true) | Out-Null
    [WinbridgeWindow]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
    Log "KakaoTalk positioned at $Left,$Top ${Width}x${Height}."
} catch {
    Log "[FAIL] $($_.Exception.Message)"
    exit 10
}
'@

    Set-Content -Path $positionScriptPath -Value $positionScript -Encoding UTF8

    $wallpaperScriptPath = 'C:\winbridge\repair-wallpaper.ps1'
    $wallpaperScript = @'
param(
    [string]$StablePath = 'C:\winbridge\wallpaper.jpg'
)

$ErrorActionPreference = 'Stop'

New-Item -Path (Split-Path -Parent $StablePath) -ItemType Directory -Force | Out-Null

$desktop = Get-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -ErrorAction SilentlyContinue
$current = if ($desktop -and $desktop.Wallpaper) { [string]$desktop.Wallpaper } else { '' }
$themeCache = Join-Path $env:APPDATA 'Microsoft\Windows\Themes\TranscodedWallpaper'

if ($current -and (Test-Path -LiteralPath $current)) {
    Copy-Item -LiteralPath $current -Destination $StablePath -Force
} elseif (Test-Path -LiteralPath $themeCache) {
    Copy-Item -LiteralPath $themeCache -Destination $StablePath -Force
} else {
    throw 'No reachable wallpaper source or TranscodedWallpaper cache found.'
}

Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name Wallpaper -Value $StablePath
Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name WallpaperStyle -Value '10'
Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name TileWallpaper -Value '0'
Start-Service -Name Themes -ErrorAction SilentlyContinue

Add-Type -Namespace Winbridge -Name Wallpaper -MemberDefinition '[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool SystemParametersInfo(int action, int param, string value, int flags);'
if (-not [Winbridge.Wallpaper]::SystemParametersInfo(20, 0, $StablePath, 3)) {
    throw 'SystemParametersInfo failed.'
}

Write-Host "Wallpaper repaired: $StablePath"
'@
    Set-Content -Path $wallpaperScriptPath -Value $wallpaperScript -Encoding UTF8

    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    Remove-ItemProperty -Path $runKey -Name 'KakaoTalk' -ErrorAction SilentlyContinue
    Set-ItemProperty -Path $runKey `
        -Name 'WinbridgeOpenKakaoTalk' `
        -Value "powershell.exe -ExecutionPolicy Bypass -NoProfile -WindowStyle Hidden -File `"$positionScriptPath`" *> `"C:\winbridge\position-kakaotalk-run.log`"" `
        -Type String
    Log "[OK] KakaoTalk foreground HKCU Run 등록: $positionScriptPath"
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

# 한국어 IME 자동 등록은 P2A 범위 밖 (P2B 또는 Rust 구현 단계로 이관).
#   사유: 게스트 OS 측 한국어 IME 활성 → xfreerdp v2.x RDP 채널 협상에서 segfault.
#   xfreerdp v3은 Ubuntu 22.04 패키지 부재 (24.04+), source build 비용 큼.
#   P2A는 카톡 자동 설치/실행 가능성 검증(POC)이므로 한국어 입력은 다음 우회로 충당:
#     호스트(Linux)에서 한글 입력 → 복사 → RDP 카톡 채팅창에 Ctrl+V (RDP 클립보드 자동).
#   본격 한국어 IME 자동화는 P2B에서 SPICE-vdagent 단독 / NoMachine / RustDesk 등 재검토.

# Step 8: 데스크톱 아이콘 숨김 + 작업표시줄 자동 숨김
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
$advKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"
if (-not (Test-Path $advKey)) {
    New-Item -Path $advKey -Force | Out-Null
}
Set-ItemProperty -Path $advKey -Name "HideIcons" -Value 1 -Type DWord -Force
$sr3 = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StuckRects3"
if (Test-Path $sr3) {
    $b = (Get-ItemProperty -Path $sr3 -Name Settings).Settings
    if ($b.Length -gt 8) {
        $b[8] = $b[8] -bor 0x01
        Set-ItemProperty -Path $sr3 -Name Settings -Value $b -Force
    }
}
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

# Step 9: Ubuntu host file share shortcut
try {
    $sharePath = '\\192.168.122.1\winbridge'
    $shortcutTargets = @(
        (Join-Path $env:USERPROFILE 'Desktop\WinbridgeShare.lnk'),
        (Join-Path $env:USERPROFILE 'Links\WinbridgeShare.lnk'),
        (Join-Path $env:APPDATA 'Microsoft\Windows\Network Shortcuts\WinbridgeShare.lnk')
    )

    $shell = New-Object -ComObject WScript.Shell
    foreach ($shortcutPath in $shortcutTargets) {
        $parent = Split-Path -Parent $shortcutPath
        if (-not (Test-Path $parent)) {
            New-Item -Path $parent -ItemType Directory -Force | Out-Null
        }

        $shortcut = $shell.CreateShortcut($shortcutPath)
        $shortcut.TargetPath = $sharePath
        $shortcut.Description = 'Ubuntu WinbridgeShare'
        $shortcut.Save()
    }
    Log "[OK] WinbridgeShare 바로가기 생성: $sharePath"
} catch {
    Log "[WARN] WinbridgeShare 바로가기 생성 실패: $($_.Exception.Message)"
}

# Step 10: URL forwarder install
try {
    $urlForwarderSource = Join-Path $PSScriptRoot 'install-url-forwarder.ps1'
    if (Test-Path $urlForwarderSource) {
        $urlForwarderTarget = 'C:\winbridge\install-url-forwarder.ps1'
        Copy-Item -Path $urlForwarderSource -Destination $urlForwarderTarget -Force
        & $urlForwarderTarget | Out-File -FilePath 'C:\winbridge\install-url-forwarder-firstboot.log' -Append -Encoding utf8
        Log "[OK] URL forwarder 설치"
    } else {
        Log "[WARN] install-url-forwarder.ps1 not found on OEM media"
    }
} catch {
    Log "[WARN] URL forwarder 설치 실패: $($_.Exception.Message)"
}

# Step 11: status SUCCESS + 재부팅 예약
# (SPICE guest tools 설치는 VirtIO PnP 미서명 컨펌으로 자동화 끊김 → P2B로 이관)
'SUCCESS' | Out-File -FilePath $StatusPath -Encoding ascii -NoNewline
Log "=== firstboot.ps1 SUCCESS, 30초 후 재부팅 ==="
Start-Process -FilePath 'shutdown.exe' -ArgumentList '/r /t 30 /c "winbridge firstboot done, rebooting"' -NoNewWindow
exit 0
