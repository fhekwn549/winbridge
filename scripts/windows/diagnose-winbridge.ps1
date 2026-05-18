param()

$ErrorActionPreference = 'Continue'

function Write-Check {
    param(
        [ValidateSet('OK', 'WARN', 'FAIL', 'SKIP')]
        [string]$Status,
        [string]$Name,
        [string]$Detail,
        [string]$NextAction = ''
    )

    Write-Host "[$Status] $Name - $Detail"
    if ($NextAction) {
        Write-Host "      next: $NextAction"
    }
}

Write-Host 'winbridge Windows guest doctor'

$wallpaper = (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -ErrorAction SilentlyContinue).Wallpaper
if ([string]::IsNullOrWhiteSpace($wallpaper)) {
    Write-Check 'WARN' 'wallpaper' 'HKCU wallpaper path is empty' 'run C:\winbridge\repair-wallpaper.ps1 or set C:\winbridge\wallpaper.jpg manually'
} elseif (Test-Path $wallpaper) {
    Write-Check 'OK' 'wallpaper' "path reachable: $wallpaper"
} else {
    Write-Check 'FAIL' 'wallpaper' "path unreachable: $wallpaper" 'run C:\winbridge\repair-wallpaper.ps1'
}

$themeCache = Join-Path $env:APPDATA 'Microsoft\Windows\Themes\TranscodedWallpaper'
if (Test-Path $themeCache) {
    $cache = Get-Item $themeCache -ErrorAction SilentlyContinue
    Write-Check 'OK' 'wallpaper cache' "TranscodedWallpaper exists ($($cache.Length) bytes, $($cache.LastWriteTime))"
} else {
    Write-Check 'WARN' 'wallpaper cache' 'TranscodedWallpaper missing' 'reapply wallpaper from Windows Settings or set C:\winbridge\wallpaper.jpg'
}

foreach ($scope in @('HKLM', 'HKCU')) {
    $policyPath = "${scope}:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services"
    $policy = Get-ItemProperty $policyPath -Name fNoRemoteDesktopWallpaper -ErrorAction SilentlyContinue
    if ($null -eq $policy) {
        Write-Check 'OK' "rdp wallpaper policy $scope" 'fNoRemoteDesktopWallpaper not set'
    } elseif ($policy.fNoRemoteDesktopWallpaper -eq 0) {
        Write-Check 'OK' "rdp wallpaper policy $scope" 'wallpaper allowed'
    } else {
        Write-Check 'FAIL' "rdp wallpaper policy $scope" "fNoRemoteDesktopWallpaper=$($policy.fNoRemoteDesktopWallpaper)" 'set this policy to 0 or remove it'
    }
}

$themes = Get-Service -Name Themes -ErrorAction SilentlyContinue
if ($themes -and $themes.Status -eq 'Running') {
    Write-Check 'OK' 'Themes service' 'running'
} elseif ($themes) {
    Write-Check 'WARN' 'Themes service' "status=$($themes.Status)" 'start the Themes service if wallpapers or themes do not render'
} else {
    Write-Check 'WARN' 'Themes service' 'not found'
}

$kakaoProcesses = Get-Process -Name KakaoTalk -ErrorAction SilentlyContinue
if ($kakaoProcesses) {
    $main = $kakaoProcesses | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if ($main) {
        Write-Check 'OK' 'KakaoTalk window' "pid=$($main.Id), hwnd=$($main.MainWindowHandle)"
    } else {
        Write-Check 'WARN' 'KakaoTalk window' "process count=$($kakaoProcesses.Count), no main window" 'run C:\winbridge\position-kakaotalk.ps1 -Restart'
    }
} else {
    Write-Check 'FAIL' 'KakaoTalk process' 'not running' 'run C:\winbridge\position-kakaotalk.ps1 -Restart'
}

$systemDrive = Get-PSDrive -Name C -ErrorAction SilentlyContinue
if ($systemDrive) {
    $freeGb = [Math]::Round($systemDrive.Free / 1GB, 1)
    if ($freeGb -ge 5) {
        Write-Check 'OK' 'disk free' "C: ${freeGb}GB free"
    } else {
        Write-Check 'WARN' 'disk free' "C: ${freeGb}GB free" 'free disk space before Windows Update or KakaoTalk update'
    }
}
