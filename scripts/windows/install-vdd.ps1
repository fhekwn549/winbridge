# Experimental helper for manual virtual-display testing.
# Not called by install.sh. Requires a local VDD driver bundle and devcon.exe inside scripts/windows.

param(
    [int]$VirtualMonitorCount = 1
)

$ErrorActionPreference = 'Stop'

function Write-Step($Message) {
    Write-Host "[winbridge-vdd] $Message"
}

$sourceRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$driverSource = Join-Path $sourceRoot 'SignedDrivers\x86\VDD'
$devcon = Join-Path $sourceRoot 'Dependencies\devcon.exe'
$target = 'C:\VirtualDisplayDriver'

if (-not ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated Administrator PowerShell.'
}

if (-not (Test-Path $driverSource)) {
    throw "Driver source not found: $driverSource"
}

if (-not (Test-Path $devcon)) {
    throw "devcon.exe not found: $devcon"
}

Write-Step "Preparing $target"
New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item -Force -Path (Join-Path $driverSource '*') -Destination $target
attrib -R "$target\*" /S /D | Out-Null
& icacls $target /grant Administrators:F /T /C | Out-Null

$settingsPath = Join-Path $target 'vdd_settings.xml'
if (Test-Path $settingsPath) {
    Write-Step "Setting virtual monitor count to $VirtualMonitorCount"
    [xml]$settings = Get-Content -LiteralPath $settingsPath
    $settings.vdd_settings.monitors.count = [string]$VirtualMonitorCount

    $has1280x720 = $false
    foreach ($resolution in $settings.vdd_settings.resolutions.resolution) {
        if ($resolution.width -eq '1280' -and $resolution.height -eq '720') {
            $has1280x720 = $true
            break
        }
    }

    if (-not $has1280x720) {
        $resolution = $settings.CreateElement('resolution')
        $width = $settings.CreateElement('width')
        $width.InnerText = '1280'
        $height = $settings.CreateElement('height')
        $height.InnerText = '720'
        $refreshRate = $settings.CreateElement('refresh_rate')
        $refreshRate.InnerText = '60'
        [void]$resolution.AppendChild($width)
        [void]$resolution.AppendChild($height)
        [void]$resolution.AppendChild($refreshRate)
        [void]$settings.vdd_settings.resolutions.AppendChild($resolution)
    }

    $settings.Save($settingsPath)
}

$inf = Join-Path $target 'MttVDD.inf'
Write-Step "Installing driver with devcon"
& $devcon install $inf 'Root\MttVDD'
$exitCode = $LASTEXITCODE
Write-Step "devcon exit code: $exitCode"

Write-Step "Display adapters after install"
Get-PnpDevice -Class Display | Format-Table -AutoSize

Write-Step "Active screens"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Screen]::AllScreens | Format-List DeviceName,Bounds,Primary,WorkingArea

if ($exitCode -ne 0) {
    Write-Step "devcon returned non-zero exit code $exitCode; continuing because devcon can report 1 after successful root device installation."
}

Write-Step 'If the new display is not visible yet, reboot Windows and run Verify-Displays.ps1.'
