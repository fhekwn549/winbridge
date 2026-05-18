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
    $root = if ($Path -match '^HKLM:\\') {
        [Microsoft.Win32.Registry]::LocalMachine
    } else {
        [Microsoft.Win32.Registry]::CurrentUser
    }
    $relative = $Path -replace '^HKCU:\\', '' -replace '^HKLM:\\', ''
    $key = $root.CreateSubKey($relative)
    if (-not $key) {
        throw "failed to open registry key for write: $Path"
    }
    try {
        $key.SetValue('', $Value, [Microsoft.Win32.RegistryValueKind]::String)
    } finally {
        $key.Close()
    }
}

function Set-RegistryStringValue {
    param(
        [string]$Path,
        [string]$Name,
        [string]$Value
    )
    if (-not (Test-Path $Path)) {
        New-Item -Path $Path -Force | Out-Null
    }
    Set-ItemProperty -Path $Path -Name $Name -Value $Value -Type String
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

foreach ($Root in @('HKCU', 'HKLM')) {
    $ProgKey = "${Root}:\Software\Classes\$ProgId"
    $ClientKey = "${Root}:\Software\Clients\StartMenuInternet\WinbridgeUrlForwarder"
    $CapabilitiesKey = "$ClientKey\Capabilities"
    $RegisteredApplicationsKey = "${Root}:\Software\RegisteredApplications"

    New-Item -Path "$ProgKey\shell\open\command" -Force | Out-Null
    Set-RegistryDefaultValue -Path $ProgKey -Value 'Winbridge URL Forwarder'
    Set-RegistryDefaultValue -Path "$ProgKey\shell\open\command" -Value $Command
    Set-RegistryStringValue -Path $ProgKey -Name 'FriendlyTypeName' -Value 'Winbridge URL Forwarder'
    Set-RegistryStringValue -Path $ProgKey -Name 'URL Protocol' -Value ''

    New-Item -Path "$ClientKey\shell\open\command" -Force | Out-Null
    New-Item -Path "$ClientKey\shell\properties\command" -Force | Out-Null
    New-Item -Path "$CapabilitiesKey\URLAssociations" -Force | Out-Null
    New-Item -Path "$CapabilitiesKey\StartMenu" -Force | Out-Null

    Set-RegistryDefaultValue -Path $ClientKey -Value 'Winbridge URL Forwarder'
    Set-RegistryDefaultValue -Path "$ClientKey\shell\open\command" -Value $Command
    Set-RegistryDefaultValue -Path "$ClientKey\shell\properties\command" -Value $Command
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationName' -Value 'Winbridge URL Forwarder'
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationDescription' -Value 'Open Windows VM links on the Linux host through winbridge.'
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationIcon' -Value 'powershell.exe,0'
    Set-RegistryStringValue -Path "$CapabilitiesKey\StartMenu" -Name 'StartMenuInternet' -Value 'WinbridgeUrlForwarder'
    Set-RegistryStringValue -Path "$CapabilitiesKey\URLAssociations" -Name 'http' -Value $ProgId
    Set-RegistryStringValue -Path "$CapabilitiesKey\URLAssociations" -Name 'https' -Value $ProgId
    Set-RegistryStringValue -Path $RegisteredApplicationsKey -Name 'Winbridge URL Forwarder' -Value 'Software\Clients\StartMenuInternet\WinbridgeUrlForwarder\Capabilities'
}

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
