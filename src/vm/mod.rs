pub mod backend;
pub mod libvirt_backend;

use crate::error::{VmError, WinbridgeResult};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// 실행 중. RDP 즉시 가능.
    Active,
    /// managed-save 상태 (메모리 디스크에 dump). resume 필요.
    Saved,
    /// 종료된 상태. boot 필요.
    Off,
    /// 그 외 (paused, crashed 등) — 매니저 관점에서 알 수 없음.
    Other,
}

impl VmState {
    pub fn requires_start(self) -> bool {
        matches!(self, VmState::Off | VmState::Other)
    }

    pub fn requires_resume(self) -> bool {
        matches!(self, VmState::Saved)
    }

    pub fn is_active(self) -> bool {
        matches!(self, VmState::Active)
    }
}

pub struct VmManager {
    backend: Arc<dyn backend::LibvirtBackend>,
    vm_name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GuestDiagnostics {
    pub wallpaper: GuestWallpaperDiagnostics,
    pub themes: GuestServiceDiagnostics,
    pub kakaotalk: GuestKakaoTalkDiagnostics,
    pub disk: GuestDiskDiagnostics,
    pub updates: GuestUpdateDiagnostics,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GuestWallpaperDiagnostics {
    pub path: String,
    #[serde(rename = "sourceReachable")]
    pub source_reachable: bool,
    #[serde(rename = "themeCacheReachable")]
    pub theme_cache_reachable: bool,
    #[serde(rename = "themeCacheBytes")]
    pub theme_cache_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GuestServiceDiagnostics {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GuestKakaoTalkDiagnostics {
    #[serde(rename = "processCount")]
    pub process_count: u32,
    #[serde(rename = "hasMainWindow")]
    pub has_main_window: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GuestDiskDiagnostics {
    #[serde(rename = "freeGb")]
    pub free_gb: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GuestUpdateDiagnostics {
    #[serde(rename = "rebootPending")]
    pub reboot_pending: bool,
    #[serde(rename = "windowsUpdateRebootRequired")]
    pub windows_update_reboot_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedCdrom {
    pub target: String,
    pub source: Option<String>,
    pub source_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GuestExecStatus {
    exitcode: i64,
    stdout: String,
    stderr: String,
}

impl VmManager {
    pub fn new(backend: Arc<dyn backend::LibvirtBackend>, vm_name: impl Into<String>) -> Self {
        Self {
            backend,
            vm_name: vm_name.into(),
        }
    }

    pub async fn state(&self) -> WinbridgeResult<VmState> {
        self.backend.state(&self.vm_name).await
    }

    /// VM이 RDP 응답 가능한 실행 상태가 될 때까지 준비한다.
    pub async fn ensure_active(&self) -> WinbridgeResult<()> {
        let initial = self.backend.state(&self.vm_name).await?;
        if initial.is_active() {
            return Ok(());
        }

        if initial.requires_resume() {
            self.backend.resume_from_saved(&self.vm_name).await?;
        } else if initial.requires_start() {
            self.backend.start(&self.vm_name).await?;
        }

        self.poll_until_active(60).await
    }

    pub(crate) async fn poll_until_active(&self, timeout_secs: u64) -> WinbridgeResult<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            let state = self.backend.state(&self.vm_name).await?;
            if state.is_active() {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                return Err(VmError::StateTimeout {
                    operation: "ensure_active",
                    timeout_secs,
                }
                .into());
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    pub async fn managed_save(&self) -> WinbridgeResult<()> {
        self.backend.managed_save(&self.vm_name).await
    }

    /// ACPI shutdown을 먼저 시도하고, 제한 시간 안에 꺼지지 않으면 강제 종료한다.
    pub async fn graceful_shutdown(&self, timeout_secs: u64) -> WinbridgeResult<()> {
        self.backend.shutdown(&self.vm_name).await?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        loop {
            let state = self.backend.state(&self.vm_name).await?;
            if matches!(state, VmState::Off) {
                return Ok(());
            }

            if std::time::Instant::now() >= deadline {
                tracing::warn!("ACPI shutdown 응답 없음, destroy로 강제 종료");
                return self.backend.destroy(&self.vm_name).await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    pub async fn qemu_guest_ping(&self) -> WinbridgeResult<()> {
        self.backend
            .qemu_agent_command(&self.vm_name, r#"{"execute":"guest-ping"}"#, 5)
            .await?;
        Ok(())
    }

    pub async fn guest_diagnostics(&self) -> WinbridgeResult<GuestDiagnostics> {
        let output = self
            .run_powershell_capture(guest_diagnostics_powershell_command(), 10, 40)
            .await?;
        let stdout = output.stdout.trim_matches(char::from(0)).trim();
        serde_json::from_str(stdout).map_err(|err| {
            VmError::GuestAgent(format!(
                "failed to parse guest diagnostics JSON: {err}; stdout={stdout}"
            ))
            .into()
        })
    }

    pub async fn attached_cdroms(&self) -> WinbridgeResult<Vec<AttachedCdrom>> {
        let xml = self.backend.domain_xml(&self.vm_name).await?;
        Ok(attached_cdroms_from_domain_xml(&xml))
    }

    pub async fn live_attached_cdroms(&self) -> WinbridgeResult<Vec<AttachedCdrom>> {
        let xml = self.backend.live_domain_xml(&self.vm_name).await?;
        Ok(attached_cdroms_from_domain_xml(&xml))
    }

    pub async fn repair_kakaotalk(&self) -> WinbridgeResult<String> {
        self.qemu_guest_ping().await?;
        let command = kakaotalk_repair_guest_exec_command();
        let response = self
            .backend
            .qemu_agent_command(&self.vm_name, &command, 10)
            .await?;
        let pid = guest_exec_pid(&response)?;

        for _ in 0..40 {
            let status_command = json!({
                "execute": "guest-exec-status",
                "arguments": { "pid": pid }
            })
            .to_string();
            let response = self
                .backend
                .qemu_agent_command(&self.vm_name, &status_command, 10)
                .await?;
            if let Some(status) = guest_exec_status(&response)? {
                if status.exitcode == 0 {
                    return Ok(status.stdout.trim_matches(char::from(0)).trim().to_string());
                }
                return Err(VmError::GuestAgent(format!(
                    "KakaoTalk repair exited with code {}; stderr={}; stdout={}",
                    status.exitcode, status.stderr, status.stdout
                ))
                .into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        Err(VmError::GuestAgent("KakaoTalk repair timed out".to_string()).into())
    }

    pub async fn repair_wallpaper(&self) -> WinbridgeResult<String> {
        let output = self
            .run_powershell_capture(repair_wallpaper_powershell_command(), 10, 40)
            .await?;
        Ok(output.stdout.trim_matches(char::from(0)).trim().to_string())
    }

    async fn run_powershell_capture(
        &self,
        powershell: &str,
        timeout_secs: i32,
        poll_count: usize,
    ) -> WinbridgeResult<GuestExecStatus> {
        self.qemu_guest_ping().await?;
        let command = powershell_guest_exec_command(powershell, true);
        let response = self
            .backend
            .qemu_agent_command(&self.vm_name, &command, timeout_secs)
            .await?;
        let pid = guest_exec_pid(&response)?;

        for _ in 0..poll_count {
            let status_command = json!({
                "execute": "guest-exec-status",
                "arguments": { "pid": pid }
            })
            .to_string();
            let response = self
                .backend
                .qemu_agent_command(&self.vm_name, &status_command, timeout_secs)
                .await?;
            if let Some(status) = guest_exec_status(&response)? {
                if status.exitcode == 0 {
                    return Ok(status);
                }
                return Err(VmError::GuestAgent(format!(
                    "PowerShell exited with code {}; stderr={}; stdout={}",
                    status.exitcode, status.stderr, status.stdout
                ))
                .into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        Err(VmError::GuestAgent("PowerShell guest-exec timed out".to_string()).into())
    }
}

pub fn kakaotalk_repair_guest_exec_command() -> String {
    powershell_guest_exec_command(kakaotalk_repair_powershell_command(), true)
}

fn attached_cdroms_from_domain_xml(xml: &str) -> Vec<AttachedCdrom> {
    let mut cdroms = Vec::new();
    for disk in xml.split("<disk ").skip(1) {
        let Some(disk_body) = disk.split("</disk>").next() else {
            continue;
        };
        let disk = format!("<disk {disk_body}");
        if !disk.contains("device='cdrom'") && !disk.contains("device=\"cdrom\"") {
            continue;
        }

        let target = xml_attr_value(&disk, "target", "dev").unwrap_or_else(|| "unknown".into());
        let source = xml_attr_value(&disk, "source", "file");
        let source_exists = source
            .as_deref()
            .map(|path| Path::new(path).exists())
            .unwrap_or(false);
        cdroms.push(AttachedCdrom {
            target,
            source,
            source_exists,
        });
    }
    cdroms
}

fn xml_attr_value(xml: &str, element: &str, attr: &str) -> Option<String> {
    let marker = format!("<{element} ");
    let element_start = xml.find(&marker)?;
    let element_xml = &xml[element_start..xml[element_start..].find('>')? + element_start];
    for quote in ['\'', '"'] {
        let attr_marker = format!("{attr}={quote}");
        let Some(attr_start) = element_xml.find(&attr_marker) else {
            continue;
        };
        let attr_start = attr_start + attr_marker.len();
        let rest = &element_xml[attr_start..];
        let Some(attr_end) = rest.find(quote) else {
            continue;
        };
        return Some(rest[..attr_end].to_string());
    }
    None
}

fn powershell_guest_exec_command(command: &str, capture_output: bool) -> String {
    json!({
        "execute": "guest-exec",
        "arguments": {
            "path": "powershell.exe",
            "arg": [
                "-ExecutionPolicy",
                "Bypass",
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                command
            ],
            "capture-output": capture_output
        }
    })
    .to_string()
}

fn kakaotalk_repair_powershell_command() -> &'static str {
    r#"$ErrorActionPreference = 'Stop'
$script = 'C:\winbridge\position-kakaotalk.ps1'
if (-not (Test-Path $script)) { throw 'position-kakaotalk.ps1 not found.' }
$repairScript = 'C:\winbridge\repair-kakaotalk-interactive.ps1'
$repairContent = @'
$ErrorActionPreference = 'Stop'
$script = 'C:\winbridge\position-kakaotalk.ps1'
if (-not (Test-Path $script)) { throw 'position-kakaotalk.ps1 not found.' }
$content = Get-Content -Path $script -Raw -ErrorAction Stop
if ($content -match '\[switch\]\$Restart') {
    & $script -Restart
} else {
    Get-Process -Name 'KakaoTalk' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    & $script
}
'@
Set-Content -Path $repairScript -Value $repairContent -Encoding UTF8
$taskName = 'RepairKakaoTalk'
$taskPath = '\Winbridge\'
$argument = '-ExecutionPolicy Bypass -NoProfile -WindowStyle Hidden -File "' + $repairScript + '"'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $argument
$principal = New-ScheduledTaskPrincipal -UserId 'Administrator' -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 2)
Register-ScheduledTask -TaskPath $taskPath -TaskName $taskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskPath $taskPath -TaskName $taskName
Start-Sleep -Seconds 2
$info = Get-ScheduledTaskInfo -TaskPath $taskPath -TaskName $taskName
$task = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName
$result = [int]$info.LastTaskResult
if ($result -ne 0 -and $result -ne 267009) { throw "KakaoTalk interactive repair task result=$result state=$($task.State)" }
Write-Host "KakaoTalk interactive repair task triggered: state=$($task.State), result=$result""#
}

fn guest_diagnostics_powershell_command() -> &'static str {
    "$ErrorActionPreference = 'Stop'; \
$desktop = Get-ItemProperty -Path 'HKCU:\\Control Panel\\Desktop' -ErrorAction SilentlyContinue; \
$wallpaper = if ($desktop -and $desktop.Wallpaper) { [string]$desktop.Wallpaper } else { '' }; \
$themeCachePath = Join-Path $env:APPDATA 'Microsoft\\Windows\\Themes\\TranscodedWallpaper'; \
$themeCache = Get-Item -LiteralPath $themeCachePath -ErrorAction SilentlyContinue; \
$themes = Get-Service -Name Themes -ErrorAction SilentlyContinue; \
$kakao = @(Get-Process -Name KakaoTalk -ErrorAction SilentlyContinue); \
$main = $kakao | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1; \
$drive = Get-PSDrive -Name C -ErrorAction Stop; \
$rebootKeys = @( \
    'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending', \
    'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired' \
); \
$rebootPending = $false; \
foreach ($key in $rebootKeys) { if (Test-Path $key) { $rebootPending = $true } }; \
$sessionManager = Get-ItemProperty -Path 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Session Manager' -ErrorAction SilentlyContinue; \
if ($sessionManager -and $sessionManager.PendingFileRenameOperations) { $rebootPending = $true }; \
$wuRebootRequired = Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired'; \
[pscustomobject]@{ \
    wallpaper = [pscustomobject]@{ \
        path = $wallpaper; \
        sourceReachable = [bool]($wallpaper -and (Test-Path -LiteralPath $wallpaper)); \
        themeCacheReachable = [bool]$themeCache; \
        themeCacheBytes = if ($themeCache) { [uint64]$themeCache.Length } else { [uint64]0 } \
    }; \
    themes = [pscustomobject]@{ status = if ($themes) { [string]$themes.Status } else { 'missing' } }; \
    kakaotalk = [pscustomobject]@{ processCount = [uint32]$kakao.Count; hasMainWindow = [bool]$main }; \
    disk = [pscustomobject]@{ freeGb = [math]::Round($drive.Free / 1GB, 1) }; \
    updates = [pscustomobject]@{ rebootPending = [bool]$rebootPending; windowsUpdateRebootRequired = [bool]$wuRebootRequired } \
} | ConvertTo-Json -Compress -Depth 5"
}

fn repair_wallpaper_powershell_command() -> &'static str {
    "$ErrorActionPreference = 'Stop'; \
New-Item -Path 'C:\\winbridge' -ItemType Directory -Force | Out-Null; \
$desktop = Get-ItemProperty -Path 'HKCU:\\Control Panel\\Desktop' -ErrorAction SilentlyContinue; \
$current = if ($desktop -and $desktop.Wallpaper) { [string]$desktop.Wallpaper } else { '' }; \
$themeCache = Join-Path $env:APPDATA 'Microsoft\\Windows\\Themes\\TranscodedWallpaper'; \
$stable = 'C:\\winbridge\\wallpaper.jpg'; \
if ($current -and (Test-Path -LiteralPath $current)) { \
    Copy-Item -LiteralPath $current -Destination $stable -Force; \
} elseif (Test-Path -LiteralPath $themeCache) { \
    Copy-Item -LiteralPath $themeCache -Destination $stable -Force; \
} else { \
    throw 'No reachable wallpaper source or TranscodedWallpaper cache found.'; \
}; \
Set-ItemProperty -Path 'HKCU:\\Control Panel\\Desktop' -Name Wallpaper -Value $stable; \
Set-ItemProperty -Path 'HKCU:\\Control Panel\\Desktop' -Name WallpaperStyle -Value '10'; \
Set-ItemProperty -Path 'HKCU:\\Control Panel\\Desktop' -Name TileWallpaper -Value '0'; \
Start-Service -Name Themes -ErrorAction SilentlyContinue; \
Add-Type -Namespace Winbridge -Name Wallpaper -MemberDefinition '[DllImport(\"user32.dll\", SetLastError=true, CharSet=CharSet.Unicode)] public static extern bool SystemParametersInfo(int action, int param, string value, int flags);'; \
if (-not [Winbridge.Wallpaper]::SystemParametersInfo(20, 0, $stable, 3)) { \
    throw 'SystemParametersInfo failed.'; \
}; \
Write-Host \"Wallpaper repaired: $stable\""
}

fn guest_exec_pid(response: &str) -> WinbridgeResult<i64> {
    let value: serde_json::Value =
        serde_json::from_str(response).map_err(|err| VmError::GuestAgent(err.to_string()))?;
    value
        .get("return")
        .and_then(|value| value.get("pid"))
        .and_then(|value| value.as_i64())
        .ok_or_else(|| {
            VmError::GuestAgent(format!("guest-exec response missing pid: {response}")).into()
        })
}

fn guest_exec_status(response: &str) -> WinbridgeResult<Option<GuestExecStatus>> {
    let value: serde_json::Value =
        serde_json::from_str(response).map_err(|err| VmError::GuestAgent(err.to_string()))?;
    let Some(return_value) = value.get("return") else {
        return Err(VmError::GuestAgent(format!(
            "guest-exec-status response missing return: {response}"
        ))
        .into());
    };
    let exited = return_value
        .get("exited")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !exited {
        return Ok(None);
    }
    let exitcode = return_value
        .get("exitcode")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| {
            VmError::GuestAgent(format!("guest-exec-status missing exitcode: {response}"))
        })?;
    let stdout = decode_guest_exec_data(return_value.get("out-data"))?;
    let stderr = decode_guest_exec_data(return_value.get("err-data"))?;
    Ok(Some(GuestExecStatus {
        exitcode,
        stdout,
        stderr,
    }))
}

fn decode_guest_exec_data(value: Option<&serde_json::Value>) -> WinbridgeResult<String> {
    let Some(encoded) = value.and_then(|value| value.as_str()) else {
        return Ok(String::new());
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|err| VmError::GuestAgent(format!("guest-exec base64 decode failed: {err}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::backend::MockLibvirtBackend;
    use mockall::predicate::eq;
    use std::sync::Arc;

    #[test]
    fn off_requires_start() {
        assert!(VmState::Off.requires_start());
    }

    #[test]
    fn saved_requires_resume() {
        assert!(VmState::Saved.requires_resume());
    }

    #[test]
    fn active_needs_no_action() {
        assert!(VmState::Active.is_active());
    }

    #[tokio::test]
    async fn vm_manager_state_delegates_to_backend() {
        let mut mock = MockLibvirtBackend::new();
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .returning(|_| Box::pin(async { Ok(VmState::Saved) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        let state = mgr.state().await.unwrap();
        assert_eq!(state, VmState::Saved);
    }

    #[tokio::test]
    async fn ensure_active_resumes_saved_vm_then_polls_until_active() {
        let mut mock = MockLibvirtBackend::new();
        let mut seq = mockall::Sequence::new();

        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Saved) }));
        mock.expect_resume_from_saved()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(()) }));
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Active) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.ensure_active().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_active_no_op_when_already_active() {
        let mut mock = MockLibvirtBackend::new();
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .returning(|_| Box::pin(async { Ok(VmState::Active) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.ensure_active().await.unwrap();
    }

    #[tokio::test]
    async fn ensure_active_starts_off_vm() {
        let mut mock = MockLibvirtBackend::new();
        let mut seq = mockall::Sequence::new();

        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Off) }));
        mock.expect_start()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(()) }));
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Active) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.ensure_active().await.unwrap();
    }

    #[tokio::test]
    async fn poll_until_active_times_out_when_state_never_reaches_active() {
        let mut mock = MockLibvirtBackend::new();
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .returning(|_| Box::pin(async { Ok(VmState::Other) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        let err = mgr.poll_until_active(0).await.unwrap_err();
        assert!(format!("{}", err).contains("시간 초과"));
    }

    #[tokio::test]
    async fn managed_save_calls_backend() {
        let mut mock = MockLibvirtBackend::new();
        mock.expect_managed_save()
            .with(eq("test-vm"))
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.managed_save().await.unwrap();
    }

    #[tokio::test]
    async fn graceful_shutdown_calls_shutdown_then_polls_off() {
        let mut mock = MockLibvirtBackend::new();
        let mut seq = mockall::Sequence::new();

        mock.expect_shutdown()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(()) }));
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Off) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.graceful_shutdown(5).await.unwrap();
    }

    #[tokio::test]
    async fn graceful_shutdown_destroys_when_acpi_times_out() {
        let mut mock = MockLibvirtBackend::new();
        let mut seq = mockall::Sequence::new();

        mock.expect_shutdown()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(()) }));
        mock.expect_state()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(VmState::Active) }));
        mock.expect_destroy()
            .with(eq("test-vm"))
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Box::pin(async { Ok(()) }));

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        mgr.graceful_shutdown(0).await.unwrap();
    }

    #[test]
    fn kakaotalk_repair_command_runs_position_script_with_restart() {
        let command = kakaotalk_repair_guest_exec_command();

        assert!(command.contains("guest-exec"));
        assert!(command.contains("powershell.exe"));
        assert!(command.contains("\"capture-output\":true"));
        assert!(command.contains("C:\\\\winbridge\\\\position-kakaotalk.ps1"));
        assert!(command.contains("repair-kakaotalk-interactive.ps1"));
        assert!(command.contains("Register-ScheduledTask"));
        assert!(command.contains("Start-ScheduledTask"));
        assert!(command.contains("LogonType Interactive"));
        assert!(command.contains("-Restart"));
        assert!(command.contains("Stop-Process"));
    }

    #[test]
    fn guest_exec_pid_reads_qga_response() {
        assert_eq!(guest_exec_pid(r#"{"return":{"pid":42}}"#).unwrap(), 42);
    }

    #[test]
    fn guest_exec_status_reports_pending_and_exitcode() {
        assert_eq!(
            guest_exec_status(r#"{"return":{"exited":false}}"#).unwrap(),
            None
        );

        let status = guest_exec_status(r#"{"return":{"exited":true,"exitcode":0}}"#)
            .unwrap()
            .unwrap();
        assert_eq!(status.exitcode, 0);
        assert_eq!(status.stdout, "");
        assert_eq!(status.stderr, "");
    }

    #[test]
    fn guest_exec_status_decodes_captured_output() {
        let status = guest_exec_status(
            r#"{"return":{"exited":true,"exitcode":0,"out-data":"aGVsbG8=","err-data":"d2Fybgo="}}"#,
        )
        .unwrap()
        .unwrap();

        assert_eq!(status.stdout, "hello");
        assert_eq!(status.stderr, "warn\n");
    }

    #[test]
    fn guest_diagnostics_json_parses() {
        let diagnostics: GuestDiagnostics = serde_json::from_str(
            r#"{"wallpaper":{"path":"W:\\Downloads\\winbridge_desktop","sourceReachable":false,"themeCacheReachable":true,"themeCacheBytes":1696236},"themes":{"status":"Running"},"kakaotalk":{"processCount":1,"hasMainWindow":true},"disk":{"freeGb":18.5},"updates":{"rebootPending":true,"windowsUpdateRebootRequired":true}}"#,
        )
        .unwrap();

        assert!(!diagnostics.wallpaper.source_reachable);
        assert!(diagnostics.wallpaper.theme_cache_reachable);
        assert!(diagnostics.kakaotalk.has_main_window);
        assert_eq!(diagnostics.themes.status, "Running");
        assert!(diagnostics.updates.reboot_pending);
        assert!(diagnostics.updates.windows_update_reboot_required);
    }

    #[test]
    fn attached_cdroms_parse_domain_xml_sources() {
        let cdroms = attached_cdroms_from_domain_xml(
            r#"
            <domain>
              <devices>
                <disk type='file' device='disk'>
                  <source file='/tmp/disk.qcow2'/>
                  <target dev='sda' bus='sata'/>
                </disk>
                <disk type='file' device='cdrom'>
                  <source file='/missing/server.iso'/>
                  <target dev='sdb' bus='sata'/>
                </disk>
                <disk type="file" device="cdrom">
                  <target dev="sdc" bus="sata"/>
                </disk>
              </devices>
            </domain>
            "#,
        );

        assert_eq!(cdroms.len(), 2);
        assert_eq!(cdroms[0].target, "sdb");
        assert_eq!(cdroms[0].source, Some("/missing/server.iso".to_string()));
        assert!(!cdroms[0].source_exists);
        assert_eq!(cdroms[1].target, "sdc");
        assert_eq!(cdroms[1].source, None);
    }

    #[tokio::test]
    async fn live_attached_cdroms_uses_live_domain_xml() {
        let mut mock = MockLibvirtBackend::new();
        mock.expect_live_domain_xml()
            .with(eq("test-vm"))
            .times(1)
            .returning(|_| {
                Box::pin(async {
                    Ok(r#"<domain><devices><disk type="file" device="cdrom"><source file="/tmp/live.iso"/><target dev="sdc"/></disk></devices></domain>"#.to_string())
                })
            });

        let mgr = VmManager::new(Arc::new(mock), "test-vm");
        let cdroms = mgr.live_attached_cdroms().await.unwrap();

        assert_eq!(cdroms.len(), 1);
        assert_eq!(cdroms[0].target, "sdc");
        assert_eq!(cdroms[0].source, Some("/tmp/live.iso".to_string()));
    }

    #[test]
    fn guest_diagnostics_command_captures_output() {
        let command = powershell_guest_exec_command(guest_diagnostics_powershell_command(), true);

        assert!(command.contains("guest-exec"));
        assert!(command.contains("\"capture-output\":true"));
        assert!(command.contains("TranscodedWallpaper"));
        assert!(command.contains("KakaoTalk"));
        assert!(command.contains("RebootRequired"));
        assert!(command.contains("PendingFileRenameOperations"));
    }

    #[test]
    fn repair_wallpaper_command_uses_stable_path_and_theme_cache() {
        let command = powershell_guest_exec_command(repair_wallpaper_powershell_command(), true);

        assert!(command.contains("guest-exec"));
        assert!(command.contains("C:\\\\winbridge\\\\wallpaper.jpg"));
        assert!(command.contains("TranscodedWallpaper"));
        assert!(command.contains("SystemParametersInfo"));
    }
}
