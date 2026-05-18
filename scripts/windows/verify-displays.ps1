# Experimental helper for manual virtual-display testing.
# Not called by install.sh. Requires a local VDD driver bundle and devcon.exe inside scripts/windows.

$ErrorActionPreference = 'Stop'

Write-Host '[winbridge-vdd] Display adapters'
Get-PnpDevice -Class Display | Format-Table -AutoSize

Write-Host '[winbridge-vdd] Desktop monitors'
Get-CimInstance Win32_DesktopMonitor | Select-Object Name, ScreenWidth, ScreenHeight | Format-Table -AutoSize

Write-Host '[winbridge-vdd] Active screens'
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Screen]::AllScreens | Format-List DeviceName,Bounds,Primary,WorkingArea

Write-Host '[winbridge-vdd] Active screen count:'
[System.Windows.Forms.Screen]::AllScreens.Count
