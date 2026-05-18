$ErrorActionPreference = 'Stop'

$BaseDir = 'C:\winbridge'
$QueueDir = Join-Path $BaseDir 'url-queue'
$ForwarderPath = Join-Path $BaseDir 'open-url-on-host.ps1'
$LogPath = Join-Path $BaseDir 'url-forwarder.log'

New-Item -Path $BaseDir -ItemType Directory -Force | Out-Null
New-Item -Path $QueueDir -ItemType Directory -Force | Out-Null

function Set-RegistryDefaultValue {
    param(
        [string]$Path,
        [string]$Value
    )
    $relative = $Path -replace '^HKCU:\\', ''
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($relative)
    if (-not $key) {
        throw "failed to open registry key for write: $Path"
    }
    try {
        $key.SetValue('', $Value, [Microsoft.Win32.RegistryValueKind]::String)
    } finally {
        $key.Close()
    }
}

$Forwarder = @'
param(
    [Parameter(Mandatory = $true)]
    [string]$Url
)

$ErrorActionPreference = 'Stop'
$BaseDir = 'C:\winbridge'
$QueueDir = Join-Path $BaseDir 'url-queue'
$LogPath = Join-Path $BaseDir 'url-forwarder.log'

function Log {
    param([string]$Message)
    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
    "$ts $Message" | Out-File -FilePath $LogPath -Append -Encoding utf8
}

try {
    New-Item -Path $QueueDir -ItemType Directory -Force | Out-Null
    $uri = [Uri]$Url
    if ($uri.Scheme -ne 'http' -and $uri.Scheme -ne 'https') {
        throw "blocked scheme: $($uri.Scheme)"
    }

    $file = Join-Path $QueueDir ("{0:yyyyMMddHHmmssfff}-{1}.url" -f (Get-Date), [Guid]::NewGuid())
    Set-Content -Path $file -Value $uri.AbsoluteUri -Encoding UTF8 -NoNewline
    Log "queued $($uri.AbsoluteUri)"
} catch {
    Log "failed: $($_.Exception.Message)"
    exit 2
}
'@

Set-Content -Path $ForwarderPath -Value $Forwarder -Encoding UTF8

$Command = 'powershell.exe -ExecutionPolicy Bypass -NoProfile -WindowStyle Hidden -File "' + $ForwarderPath + '" "%1"'
$ProgId = 'Winbridge.UrlForwarder'
$ProgKey = "HKCU:\Software\Classes\$ProgId"
$ClientKey = 'HKCU:\Software\Clients\StartMenuInternet\WinbridgeUrlForwarder'
$CapabilitiesKey = "$ClientKey\Capabilities"

New-Item -Path "$ProgKey\shell\open\command" -Force | Out-Null
Set-RegistryDefaultValue -Path $ProgKey -Value 'Winbridge URL Forwarder'
Set-RegistryDefaultValue -Path "$ProgKey\shell\open\command" -Value $Command

New-Item -Path "$CapabilitiesKey\URLAssociations" -Force | Out-Null
Set-RegistryDefaultValue -Path $ClientKey -Value 'Winbridge URL Forwarder'
Set-RegistryDefaultValue -Path "$ClientKey\shell\open\command" -Value $Command
Set-ItemProperty -Path $CapabilitiesKey -Name 'ApplicationName' -Value 'Winbridge URL Forwarder'
Set-ItemProperty -Path $CapabilitiesKey -Name 'ApplicationDescription' -Value 'Open Windows VM links on the Linux host through winbridge.'
Set-ItemProperty -Path "$CapabilitiesKey\URLAssociations" -Name 'http' -Value $ProgId
Set-ItemProperty -Path "$CapabilitiesKey\URLAssociations" -Name 'https' -Value $ProgId
New-Item -Path 'HKCU:\Software\RegisteredApplications' -Force | Out-Null
Set-ItemProperty -Path 'HKCU:\Software\RegisteredApplications' -Name 'Winbridge URL Forwarder' -Value 'Software\Clients\StartMenuInternet\WinbridgeUrlForwarder\Capabilities'

foreach ($Scheme in @('http', 'https')) {
    $SchemeKey = "HKCU:\Software\Classes\$Scheme"
    New-Item -Path "$SchemeKey\shell\open\command" -Force | Out-Null
    Set-RegistryDefaultValue -Path $SchemeKey -Value "URL:$Scheme"
    Set-ItemProperty -Path $SchemeKey -Name 'URL Protocol' -Value ''
    Set-RegistryDefaultValue -Path "$SchemeKey\shell\open\command" -Value $Command

    $UserChoice = "HKCU:\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\$Scheme\UserChoice"
    Remove-Item -Path $UserChoice -Recurse -Force -ErrorAction SilentlyContinue
}

"installed url forwarder: $ForwarderPath" | Out-File -FilePath $LogPath -Append -Encoding utf8
Write-Host "Winbridge URL forwarder installed. If Windows asks for a default browser, choose Winbridge URL Forwarder."
