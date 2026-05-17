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
