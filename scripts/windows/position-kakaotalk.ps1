param(
    [int]$Left = 0,
    [int]$Top = 0,
    [int]$Width = 960,
    [int]$Height = 720,
    [switch]$Restart
)

$ErrorActionPreference = 'Stop'

Add-Type @'
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
'@

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
        if (-not $base -or -not (Test-Path $base)) {
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

Write-Host "KakaoTalk positioned at $Left,$Top ${Width}x${Height}."
