use crate::config::WinbridgeConfig;
use crate::rdp::RdpHeadlessProbe;
use crate::vm::{libvirt_backend::LibvirtBackendImpl, AttachedCdrom, GuestDiagnostics, VmManager};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

const RDP_PORT: u16 = 3389;
const TCP_CHECK_TIMEOUT: Duration = Duration::from_secs(3);
const RDP_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctorStatus {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl DoctorStatus {
    fn label(self) -> &'static str {
        match self {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
            DoctorStatus::Skip => "SKIP",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
    pub next_action: Option<String>,
}

impl DoctorCheck {
    pub fn new(status: DoctorStatus, name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            next_action: None,
        }
    }

    pub fn with_next_action(mut self, next_action: impl Into<String>) -> Self {
        self.next_action = Some(next_action.into());
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn push(&mut self, check: DoctorCheck) {
        self.checks.push(check);
    }

    pub fn checks(&self) -> &[DoctorCheck] {
        &self.checks
    }
}

pub fn format_report(report: &DoctorReport) -> String {
    let mut out = String::from("winbridge doctor\n");
    for check in report.checks() {
        let _ = writeln!(
            out,
            "[{}] {} - {}",
            check.status.label(),
            check.name,
            check.detail
        );
        if let Some(next_action) = &check.next_action {
            let _ = writeln!(out, "      next: {next_action}");
        }
    }
    out
}

pub async fn diagnose_host() -> DoctorReport {
    let mut report = DoctorReport::default();

    let cfg = match WinbridgeConfig::load() {
        Ok(cfg) => {
            report.push(DoctorCheck::new(
                DoctorStatus::Ok,
                "credentials",
                format!("{} loaded", WinbridgeConfig::credentials_path().display()),
            ));
            cfg
        }
        Err(err) => {
            report.push(
                DoctorCheck::new(DoctorStatus::Fail, "credentials", err.to_string())
                    .with_next_action("run ./install.sh or create ~/.config/winbridge/credentials"),
            );
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "vm",
                "credentials unavailable",
            ));
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "rdp",
                "credentials unavailable",
            ));
            add_guest_manual_check(&mut report);
            return report;
        }
    };

    let backend = match LibvirtBackendImpl::open(&cfg.libvirt_uri) {
        Ok(backend) => {
            report.push(DoctorCheck::new(
                DoctorStatus::Ok,
                "libvirt",
                format!("connected to {}", cfg.libvirt_uri),
            ));
            Arc::new(backend)
        }
        Err(err) => {
            report.push(
                DoctorCheck::new(DoctorStatus::Fail, "libvirt", err.to_string())
                    .with_next_action("check libvirt service and user group membership"),
            );
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "vm",
                "libvirt unavailable",
            ));
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "rdp",
                "libvirt unavailable",
            ));
            add_guest_manual_check(&mut report);
            return report;
        }
    };

    let manager = VmManager::new(backend, cfg.vm_name.clone());
    let vm_state = match manager.state().await {
        Ok(state) => {
            let status = if state.is_active() {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            };
            report.push(DoctorCheck::new(
                status,
                "vm state",
                format!("{} is {:?}", cfg.vm_name, state),
            ));
            state
        }
        Err(err) => {
            report.push(
                DoctorCheck::new(DoctorStatus::Fail, "vm state", err.to_string())
                    .with_next_action("verify WINBRIDGE_VM_NAME and virsh list --all"),
            );
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "rdp",
                "VM state unavailable",
            ));
            add_guest_manual_check(&mut report);
            return report;
        }
    };

    match manager.attached_cdroms().await {
        Ok(cdroms) => add_cdrom_attachment_check(&mut report, "persistent", &cdroms),
        Err(err) => report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "vm persistent cdrom attachments",
                format!("unable to inspect VM XML: {err}"),
            )
            .with_next_action("run virsh --connect qemu:///system domblklist --details"),
        ),
    }

    let mut guest_checks_complete = false;
    if !vm_state.is_active() {
        report.push(DoctorCheck::new(
            DoctorStatus::Skip,
            "vm live cdrom attachments",
            "VM is not active; live XML unavailable",
        ));
        report.push(
            DoctorCheck::new(
                DoctorStatus::Skip,
                "qemu guest agent",
                "VM is not active; guest service-session checks skipped",
            )
            .with_next_action("start or resume the VM before running guest diagnostics"),
        );
    } else {
        match manager.live_attached_cdroms().await {
            Ok(cdroms) => add_cdrom_attachment_check(&mut report, "live", &cdroms),
            Err(err) => report.push(
                DoctorCheck::new(
                    DoctorStatus::Warn,
                    "vm live cdrom attachments",
                    format!("unable to inspect live VM XML: {err}"),
                )
                .with_next_action("run virsh --connect qemu:///system domblklist --details"),
            ),
        }
        match manager.qemu_guest_ping().await {
            Ok(()) => {
                report.push(DoctorCheck::new(
                    DoctorStatus::Ok,
                    "qemu guest agent",
                    "guest-ping succeeded; guest checks run in the qemu-ga service session",
                ));
                match manager.guest_diagnostics().await {
                    Ok(diagnostics) => {
                        add_guest_diagnostic_checks(&mut report, &diagnostics);
                        guest_checks_complete = true;
                    }
                    Err(err) => report.push(
                        DoctorCheck::new(DoctorStatus::Warn, "guest checks", err.to_string())
                            .with_next_action(
                                "run scripts/windows/diagnose-winbridge.ps1 inside Windows",
                            ),
                    ),
                }
            }
            Err(err) => report.push(
                DoctorCheck::new(DoctorStatus::Warn, "qemu guest agent", err.to_string())
                    .with_next_action("install virtio-win guest tools and restart the VM"),
            ),
        }
    }

    match check_tcp_port(&cfg.vm_ip, RDP_PORT).await {
        Ok(()) => report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "rdp tcp",
            format!("{}:{RDP_PORT} reachable", cfg.vm_ip),
        )),
        Err(detail) => {
            report.push(
                DoctorCheck::new(DoctorStatus::Fail, "rdp tcp", detail)
                    .with_next_action("start the VM and verify Windows Remote Desktop is enabled"),
            );
            report.push(DoctorCheck::new(
                DoctorStatus::Skip,
                "rdp handshake",
                "TCP unavailable",
            ));
            if !guest_checks_complete {
                add_guest_manual_check(&mut report);
            }
            return report;
        }
    }

    match check_rdp_handshake(&cfg.vm_ip, &cfg.admin_password).await {
        Ok(detail) => report.push(DoctorCheck::new(DoctorStatus::Ok, "rdp handshake", detail)),
        Err(detail) => report.push(
            DoctorCheck::new(DoctorStatus::Fail, "rdp handshake", detail)
                .with_next_action("check Administrator password and Windows RDP login state"),
        ),
    }

    if !guest_checks_complete {
        add_guest_manual_check(&mut report);
    }
    report
}

async fn check_tcp_port(host: &str, port: u16) -> Result<(), String> {
    let address = format!("{host}:{port}");
    match tokio::time::timeout(TCP_CHECK_TIMEOUT, tokio::net::TcpStream::connect(&address)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(format!("{address} connect failed: {err}")),
        Err(_) => Err(format!(
            "{address} connect timed out after {}s",
            TCP_CHECK_TIMEOUT.as_secs()
        )),
    }
}

async fn check_rdp_handshake(host: &str, password: &str) -> Result<String, String> {
    let username =
        std::env::var("WINBRIDGE_ADMIN_USER").unwrap_or_else(|_| "Administrator".to_string());
    let probe = RdpHeadlessProbe::new(host, RDP_PORT, &username, password)
        .map_err(|err| err.to_string())?;

    match tokio::time::timeout(RDP_PROBE_TIMEOUT, probe.probe()).await {
        Ok(Ok(result)) => Ok(format!(
            "login ok, desktop={}x{}x{}",
            result.width, result.height, result.bits_per_pixel
        )),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "probe timed out after {}s",
            RDP_PROBE_TIMEOUT.as_secs()
        )),
    }
}

fn add_guest_manual_check(report: &mut DoctorReport) {
    report.push(
        DoctorCheck::new(
            DoctorStatus::Warn,
            "guest checks",
            "visible RDP user checks require running scripts/windows/diagnose-winbridge.ps1 inside Windows",
        )
        .with_next_action("wallpaper: C:\\winbridge\\repair-wallpaper.ps1; KakaoTalk: C:\\winbridge\\position-kakaotalk.ps1 -Restart"),
    );
}

fn add_guest_diagnostic_checks(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    add_wallpaper_check(report, diagnostics);
    add_kakaotalk_check(report, diagnostics);
    add_themes_check(report, diagnostics);
    add_disk_check(report, diagnostics);
    add_update_check(report, diagnostics);
}

fn add_cdrom_attachment_check(report: &mut DoctorReport, scope: &str, cdroms: &[AttachedCdrom]) {
    let check_name = format!("vm {scope} cdrom attachments");
    if cdroms.is_empty() {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            check_name,
            format!("no {scope} CD-ROM attachments"),
        ));
        return;
    }

    let missing: Vec<_> = cdroms
        .iter()
        .filter(|cdrom| cdrom.source.as_ref().is_some_and(|_| !cdrom.source_exists))
        .collect();
    if !missing.is_empty() {
        let detail = missing
            .iter()
            .map(|cdrom| {
                format!(
                    "{}={}",
                    cdrom.target,
                    cdrom.source.as_deref().unwrap_or("<empty>")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        report.push(
            DoctorCheck::new(
                DoctorStatus::Fail,
                check_name,
                format!("missing {scope} CD-ROM source(s) can block VM start/resume: {detail}"),
            )
            .with_next_action("detach stale CD-ROMs with virsh detach-disk <vm> <target>"),
        );
        return;
    }

    let detail = cdroms
        .iter()
        .map(|cdrom| {
            format!(
                "{}={}",
                cdrom.target,
                cdrom.source.as_deref().unwrap_or("<empty>")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    report.push(
        DoctorCheck::new(
            DoctorStatus::Warn,
            check_name,
            format!("{scope} CD-ROM attachment(s) remain; deleting these files later can block VM start/resume: {detail}"),
        )
        .with_next_action("detach install/driver media after setup if they are no longer needed"),
    );
}

fn add_wallpaper_check(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    let wallpaper = &diagnostics.wallpaper;
    let path = wallpaper.path.trim();
    if path.is_empty() {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session wallpaper",
                "no wallpaper path configured in qemu-ga service session; visible RDP wallpaper may differ",
            )
            .with_next_action("if visible RDP wallpaper is broken, run winbridge repair-wallpaper"),
        );
    } else if wallpaper.source_reachable {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "guest service-session wallpaper",
            format!("source reachable in qemu-ga service session: {path}"),
        ));
    } else if wallpaper.theme_cache_reachable {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session wallpaper",
                format!(
                    "source missing in qemu-ga service session but theme cache exists ({} bytes): {path}",
                    wallpaper.theme_cache_bytes
                ),
            )
            .with_next_action("if visible RDP wallpaper is broken, run winbridge repair-wallpaper"),
        );
    } else {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Fail,
                "guest service-session wallpaper",
                format!(
                    "source missing and theme cache unavailable in qemu-ga service session: {path}"
                ),
            )
            .with_next_action("run winbridge repair-wallpaper or set the wallpaper again manually"),
        );
    }
}

fn add_kakaotalk_check(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    let kakao = &diagnostics.kakaotalk;
    if kakao.has_main_window {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "guest service-session kakaotalk",
            format!(
                "{} process(es), main window visible to qemu-ga service session",
                kakao.process_count
            ),
        ));
    } else if kakao.process_count > 0 {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session kakaotalk",
                format!(
                    "{} process(es), no main window visible to qemu-ga service session",
                    kakao.process_count
                ),
            )
            .with_next_action("if tray Open Winbridge does not show the visible window, run winbridge repair-kakao"),
        );
    } else {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session kakaotalk",
                "process not visible to qemu-ga service session",
            )
            .with_next_action("open Winbridge from tray first; if the visible window is broken, run winbridge repair-kakao"),
        );
    }
}

fn add_themes_check(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    let status = diagnostics.themes.status.trim();
    if status.eq_ignore_ascii_case("Running") {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "guest service-session themes",
            "Themes service running",
        ));
    } else {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session themes",
                format!("Themes service is {status}"),
            )
            .with_next_action("start the Windows Themes service"),
        );
    }
}

fn add_disk_check(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    let free_gb = diagnostics.disk.free_gb;
    if free_gb >= 5.0 {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "guest service-session disk",
            format!("C: free space {free_gb:.1} GiB"),
        ));
    } else {
        report.push(
            DoctorCheck::new(
                DoctorStatus::Warn,
                "guest service-session disk",
                format!("C: low free space {free_gb:.1} GiB"),
            )
            .with_next_action("free Windows disk space before updates or VM snapshots"),
        );
    }
}

fn add_update_check(report: &mut DoctorReport, diagnostics: &GuestDiagnostics) {
    let updates = &diagnostics.updates;
    if updates.reboot_pending {
        let detail = if updates.windows_update_reboot_required {
            "Windows Update reboot required"
        } else {
            "Windows reboot pending"
        };
        report.push(
            DoctorCheck::new(DoctorStatus::Warn, "guest service-session updates", detail)
                .with_next_action(
                    "finish Windows Update and restart Windows before relying on repair checks",
                ),
        );
    } else {
        report.push(DoctorCheck::new(
            DoctorStatus::Ok,
            "guest service-session updates",
            "no pending reboot detected",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::{
        GuestDiskDiagnostics, GuestKakaoTalkDiagnostics, GuestServiceDiagnostics,
        GuestUpdateDiagnostics, GuestWallpaperDiagnostics,
    };

    #[test]
    fn report_formats_status_detail_and_next_action() {
        let mut report = DoctorReport::default();
        report.push(
            DoctorCheck::new(DoctorStatus::Fail, "rdp tcp", "connect failed")
                .with_next_action("start VM"),
        );

        let text = format_report(&report);

        assert!(text.contains("winbridge doctor"));
        assert!(text.contains("[FAIL] rdp tcp - connect failed"));
        assert!(text.contains("next: start VM"));
    }

    #[test]
    fn report_exposes_checks_in_order() {
        let mut report = DoctorReport::default();
        report.push(DoctorCheck::new(DoctorStatus::Ok, "first", "done"));
        report.push(DoctorCheck::new(DoctorStatus::Skip, "second", "blocked"));

        let checks = report.checks();

        assert_eq!(checks[0].name, "first");
        assert_eq!(checks[1].status, DoctorStatus::Skip);
    }

    #[test]
    fn guest_diagnostics_warns_when_wallpaper_source_is_missing_but_cached() {
        let mut report = DoctorReport::default();
        let diagnostics = GuestDiagnostics {
            wallpaper: GuestWallpaperDiagnostics {
                path: "W:\\Downloads\\winbridge_desktop".to_string(),
                source_reachable: false,
                theme_cache_reachable: true,
                theme_cache_bytes: 1_696_236,
            },
            themes: GuestServiceDiagnostics {
                status: "Running".to_string(),
            },
            kakaotalk: GuestKakaoTalkDiagnostics {
                process_count: 1,
                has_main_window: true,
            },
            disk: GuestDiskDiagnostics { free_gb: 12.0 },
            updates: GuestUpdateDiagnostics {
                reboot_pending: false,
                windows_update_reboot_required: false,
            },
        };

        add_guest_diagnostic_checks(&mut report, &diagnostics);

        let checks = report.checks();
        assert_eq!(checks[0].name, "guest service-session wallpaper");
        assert_eq!(checks[0].status, DoctorStatus::Warn);
        assert!(checks[0].detail.contains("qemu-ga service session"));
        assert_eq!(checks[1].status, DoctorStatus::Ok);
        assert_eq!(checks[2].status, DoctorStatus::Ok);
        assert_eq!(checks[3].status, DoctorStatus::Ok);
        assert_eq!(checks[4].status, DoctorStatus::Ok);
    }

    #[test]
    fn guest_diagnostics_flags_missing_kakaotalk_window() {
        let mut report = DoctorReport::default();
        let diagnostics = GuestDiagnostics {
            wallpaper: GuestWallpaperDiagnostics {
                path: "C:\\winbridge\\wallpaper.jpg".to_string(),
                source_reachable: true,
                theme_cache_reachable: true,
                theme_cache_bytes: 100,
            },
            themes: GuestServiceDiagnostics {
                status: "Stopped".to_string(),
            },
            kakaotalk: GuestKakaoTalkDiagnostics {
                process_count: 2,
                has_main_window: false,
            },
            disk: GuestDiskDiagnostics { free_gb: 3.0 },
            updates: GuestUpdateDiagnostics {
                reboot_pending: true,
                windows_update_reboot_required: true,
            },
        };

        add_guest_diagnostic_checks(&mut report, &diagnostics);

        let checks = report.checks();
        assert_eq!(checks[1].name, "guest service-session kakaotalk");
        assert_eq!(checks[1].status, DoctorStatus::Warn);
        assert!(checks[1].detail.contains("qemu-ga service session"));
        assert!(checks[1]
            .next_action
            .as_ref()
            .unwrap()
            .contains("tray Open Winbridge"));
        assert_eq!(checks[2].status, DoctorStatus::Warn);
        assert_eq!(checks[3].status, DoctorStatus::Warn);
        assert_eq!(checks[4].name, "guest service-session updates");
        assert_eq!(checks[4].status, DoctorStatus::Warn);
    }

    #[test]
    fn guest_diagnostics_does_not_fail_when_service_session_cannot_see_kakaotalk() {
        let mut report = DoctorReport::default();
        let diagnostics = GuestDiagnostics {
            wallpaper: GuestWallpaperDiagnostics {
                path: "C:\\winbridge\\wallpaper.jpg".to_string(),
                source_reachable: true,
                theme_cache_reachable: true,
                theme_cache_bytes: 100,
            },
            themes: GuestServiceDiagnostics {
                status: "Running".to_string(),
            },
            kakaotalk: GuestKakaoTalkDiagnostics {
                process_count: 0,
                has_main_window: false,
            },
            disk: GuestDiskDiagnostics { free_gb: 12.0 },
            updates: GuestUpdateDiagnostics {
                reboot_pending: false,
                windows_update_reboot_required: false,
            },
        };

        add_guest_diagnostic_checks(&mut report, &diagnostics);

        let checks = report.checks();
        assert_eq!(checks[1].name, "guest service-session kakaotalk");
        assert_eq!(checks[1].status, DoctorStatus::Warn);
        assert!(checks[1].detail.contains("qemu-ga service session"));
        assert!(checks[1]
            .next_action
            .as_ref()
            .unwrap()
            .contains("open Winbridge"));
    }

    #[test]
    fn cdrom_attachment_check_fails_missing_sources() {
        let mut report = DoctorReport::default();
        add_cdrom_attachment_check(
            &mut report,
            "persistent",
            &[AttachedCdrom {
                target: "sdb".to_string(),
                source: Some("/missing/server.iso".to_string()),
                source_exists: false,
            }],
        );

        let checks = report.checks();
        assert_eq!(checks[0].name, "vm persistent cdrom attachments");
        assert_eq!(checks[0].status, DoctorStatus::Fail);
        assert!(checks[0]
            .detail
            .contains("missing persistent CD-ROM source"));
        assert!(checks[0]
            .next_action
            .as_ref()
            .unwrap()
            .contains("detach-disk"));
    }

    #[test]
    fn cdrom_attachment_check_warns_existing_sources() {
        let mut report = DoctorReport::default();
        add_cdrom_attachment_check(
            &mut report,
            "live",
            &[AttachedCdrom {
                target: "sdc".to_string(),
                source: Some("/tmp/driver.iso".to_string()),
                source_exists: true,
            }],
        );

        let checks = report.checks();
        assert_eq!(checks[0].status, DoctorStatus::Warn);
        assert!(checks[0].detail.contains("live CD-ROM"));
    }
}
