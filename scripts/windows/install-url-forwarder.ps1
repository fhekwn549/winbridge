$ErrorActionPreference = 'Stop'

$BaseDir = 'C:\winbridge'
$QueueDir = Join-Path $BaseDir 'url-queue'
$ForwarderPath = Join-Path $BaseDir 'open-url-on-host.ps1'
$ForwarderExePath = Join-Path $BaseDir 'WinbridgeUrlForwarder.exe'
$ForwarderIconPath = Join-Path $BaseDir 'winbridge-kakaotalk.ico'
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

$ForwarderSource = @'
using System;
using System.IO;
using System.Text;

[assembly: System.Reflection.AssemblyTitle("Winbridge URL Forwarder")]
[assembly: System.Reflection.AssemblyProduct("Winbridge URL Forwarder")]
[assembly: System.Reflection.AssemblyDescription("Open Windows VM links on the Linux host through winbridge.")]

public static class Program
{
    public static int Main(string[] args)
    {
        string baseDir = @"C:\winbridge";
        string queueDir = Path.Combine(baseDir, "url-queue");
        string logPath = Path.Combine(baseDir, "url-forwarder.log");
        try
        {
            Directory.CreateDirectory(queueDir);
            if (args.Length < 1) throw new ArgumentException("missing URL argument");
            Uri uri = new Uri(args[0]);
            if (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps)
            {
                throw new ArgumentException("blocked scheme: " + uri.Scheme);
            }

            string file = Path.Combine(queueDir, DateTime.UtcNow.ToString("yyyyMMddHHmmssfff") + "-" + Guid.NewGuid().ToString("N") + ".url");
            File.WriteAllText(file, uri.AbsoluteUri, new UTF8Encoding(false));
            File.AppendAllText(logPath, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + " queued " + uri.AbsoluteUri + Environment.NewLine, Encoding.UTF8);
            return 0;
        }
        catch (Exception ex)
        {
            try
            {
                Directory.CreateDirectory(baseDir);
                File.AppendAllText(logPath, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + " failed: " + ex.Message + Environment.NewLine, Encoding.UTF8);
            }
            catch {}
            return 2;
        }
    }
}
'@

$ForwarderSourcePath = Join-Path $BaseDir 'WinbridgeUrlForwarder.cs'
Set-Content -Path $ForwarderSourcePath -Value $ForwarderSource -Encoding UTF8
$CscPath = @(
    "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
    "$env:WINDIR\Microsoft.NET\Framework\v4.0.30319\csc.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($CscPath) {
    $proc = Start-Process -FilePath $CscPath -ArgumentList '/nologo', '/target:winexe', ('/out:' + $ForwarderExePath), $ForwarderSourcePath -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "csc.exe failed to build WinbridgeUrlForwarder.exe, exit code $($proc.ExitCode)"
    }
} else {
    throw 'csc.exe not found; cannot build WinbridgeUrlForwarder.exe'
}

$Command = '"' + $ForwarderExePath + '" "%1"'
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
    Set-RegistryDefaultValue -Path "${Root}:\Software\Classes\Applications\WinbridgeUrlForwarder.exe\shell\open\command" -Value $Command

    New-Item -Path "$ClientKey\shell\open\command" -Force | Out-Null
    New-Item -Path "$ClientKey\shell\properties\command" -Force | Out-Null
    New-Item -Path "$CapabilitiesKey\URLAssociations" -Force | Out-Null
    New-Item -Path "$CapabilitiesKey\StartMenu" -Force | Out-Null

    Set-RegistryDefaultValue -Path $ClientKey -Value 'Winbridge URL Forwarder'
    Set-RegistryDefaultValue -Path "$ClientKey\shell\open\command" -Value $Command
    Set-RegistryDefaultValue -Path "$ClientKey\shell\properties\command" -Value $Command
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationName' -Value 'Winbridge URL Forwarder'
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationDescription' -Value 'Open Windows VM links on the Linux host through winbridge.'
    $ApplicationIcon = if (Test-Path $ForwarderIconPath) { $ForwarderIconPath } else { "$ForwarderExePath,0" }
    Set-RegistryStringValue -Path $CapabilitiesKey -Name 'ApplicationIcon' -Value $ApplicationIcon
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
