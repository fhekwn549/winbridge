use clap::Parser;
use gtk4::prelude::*;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use winbridge::{cli, config, desktop, doctor, error, rdp, tray, vm};

const RDP_PORT: u16 = 3389;
const RDP_READY_TIMEOUT: Duration = Duration::from_secs(180);
const RDP_READY_POLL_INTERVAL: Duration = Duration::from_secs(2);
const RDP_HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
const WINBRIDGE_APPLICATION_ID: &str = "dev.winbridge.Winbridge";

#[derive(Debug, PartialEq, Eq)]
enum TrayAction {
    Open {
        mode: cli::WindowMode,
    },
    OpenReady {
        mode: cli::WindowMode,
        vm_ip: String,
        password: String,
    },
    OpenFinished,
    WindowOpened,
    WindowClosed,
    IdleTimeout {
        generation: u64,
    },
    RepairKakao,
    RepairWallpaper,
    Pause,
    Shutdown,
    ManagedSaveThenQuit,
    Notify {
        title: String,
        body: String,
    },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RdpWindowCloseAction {
    CloseWindowOnly,
    ManagedSave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessSignalAction {
    ManagedSaveThenQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartProcessPolicy {
    ExitWithLastWindow,
    HoldTrayAfterWindowClose,
}

#[derive(Debug, Default)]
struct TrayOpenGate {
    in_progress: bool,
}

impl TrayOpenGate {
    fn try_begin(&mut self) -> bool {
        if self.in_progress {
            return false;
        }

        self.in_progress = true;
        true
    }

    fn finish(&mut self) {
        self.in_progress = false;
    }
}

type DisplayResolver = Arc<dyn Fn(cli::WindowMode) -> cli::DisplayStrategy>;

fn rdp_window_close_action(policy: config::CloseWindowPolicy) -> RdpWindowCloseAction {
    match policy {
        config::CloseWindowPolicy::KeepRunning => RdpWindowCloseAction::CloseWindowOnly,
        config::CloseWindowPolicy::ManagedSave => RdpWindowCloseAction::ManagedSave,
    }
}

fn process_signal_action() -> ProcessSignalAction {
    ProcessSignalAction::ManagedSaveThenQuit
}

fn tray_quit_action(policy: config::QuitPolicy) -> TrayAction {
    match policy {
        config::QuitPolicy::ManagedSave => TrayAction::ManagedSaveThenQuit,
        config::QuitPolicy::KeepRunning => TrayAction::Quit,
    }
}

fn start_process_policy(mode: cli::WindowMode) -> StartProcessPolicy {
    match mode {
        cli::WindowMode::App => StartProcessPolicy::HoldTrayAfterWindowClose,
        cli::WindowMode::Desktop => StartProcessPolicy::ExitWithLastWindow,
    }
}

fn lifecycle_idle_timeout(lifecycle: config::LifecycleConfig) -> Option<Duration> {
    lifecycle
        .idle_timeout_minutes
        .map(|minutes| Duration::from_secs(minutes.saturating_mul(60)))
        .filter(|duration| !duration.is_zero())
}

fn close_window_policy_label(policy: config::CloseWindowPolicy) -> &'static str {
    match policy {
        config::CloseWindowPolicy::KeepRunning => "keep-running",
        config::CloseWindowPolicy::ManagedSave => "managed-save",
    }
}

fn quit_policy_label(policy: config::QuitPolicy) -> &'static str {
    match policy {
        config::QuitPolicy::ManagedSave => "managed-save",
        config::QuitPolicy::KeepRunning => "keep-running",
    }
}

fn lifecycle_summary(lifecycle: config::LifecycleConfig) -> String {
    let idle = lifecycle
        .idle_timeout_minutes
        .map(|minutes| format!("{minutes}m"))
        .unwrap_or_else(|| "disabled".to_string());
    format!(
        "close-window={}, quit={}, idle-timeout={idle}",
        close_window_policy_label(lifecycle.close_window),
        quit_policy_label(lifecycle.quit)
    )
}

fn repair_notification_body(action: &str, result: Result<&str, &str>) -> String {
    match result {
        Ok(detail) if detail.trim().is_empty() => {
            format!("{action} completed. Next: open KakaoTalk or run doctor if the issue remains.")
        }
        Ok(detail) => format!(
            "{action} completed: {}. Next: open KakaoTalk or run doctor if the issue remains.",
            detail.trim()
        ),
        Err(err) => format!("{action} failed: {err}. Next: run doctor or open Windows desktop."),
    }
}

fn action_notification_body(action: &str, result: Result<&str, &str>) -> String {
    match result {
        Ok(detail) if detail.trim().is_empty() => {
            format!("{action} completed. Next: run status or doctor if needed.")
        }
        Ok(detail) => format!(
            "{action} completed: {}. Next: run status or doctor if needed.",
            detail.trim()
        ),
        Err(err) => format!("{action} failed: {err}. Next: run doctor or open Windows desktop."),
    }
}

fn send_desktop_notification(app: &gtk4::Application, title: &str, body: &str) {
    let notification = gtk4::gio::Notification::new(title);
    notification.set_body(Some(body));
    app.send_notification(None, &notification);
}

fn rdp_window_close_handler(
    action: RdpWindowCloseAction,
    action_tx: async_channel::Sender<TrayAction>,
) -> Arc<dyn Fn() + Send + Sync> {
    match action {
        RdpWindowCloseAction::CloseWindowOnly => Arc::new(move || {
            tracing::debug!("RDP window closed; VM state left unchanged");
            let _ = action_tx.try_send(TrayAction::WindowClosed);
        }),
        RdpWindowCloseAction::ManagedSave => Arc::new(move || {
            tracing::info!("RDP window closed; VM managed-save requested");
            let _ = action_tx.try_send(TrayAction::WindowClosed);
            let _ = action_tx.try_send(TrayAction::Pause);
        }),
    }
}

async fn handle_process_signal_action(
    action: ProcessSignalAction,
    manager: Arc<vm::VmManager>,
    action_tx: async_channel::Sender<TrayAction>,
) {
    match action {
        ProcessSignalAction::ManagedSaveThenQuit => {
            handle_managed_save_then_quit(manager, action_tx, "after process signal").await;
        }
    }
}

async fn handle_managed_save_then_quit(
    manager: Arc<vm::VmManager>,
    action_tx: async_channel::Sender<TrayAction>,
    reason: &'static str,
) {
    tracing::info!("saving VM before quitting winbridge {reason}");
    if let Err(err) = manager.managed_save().await {
        tracing::error!("VM managed save before quitting winbridge failed: {err}");
    }
    let _ = action_tx.try_send(TrayAction::Quit);
}

fn spawn_process_signal_handler(
    manager: Arc<vm::VmManager>,
    action_tx: async_channel::Sender<TrayAction>,
) {
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM received"),
            _ = sigint.recv() => tracing::info!("SIGINT received"),
        }

        handle_process_signal_action(process_signal_action(), manager, action_tx).await;
    });
}

fn spawn_tray_action_loop(
    app: gtk4::Application,
    cfg: Arc<config::WinbridgeConfig>,
    manager: Arc<vm::VmManager>,
    handle: tokio::runtime::Handle,
    action_tx: async_channel::Sender<TrayAction>,
    action_rx: async_channel::Receiver<TrayAction>,
    display_resolver: DisplayResolver,
) {
    glib::MainContext::default().spawn_local(async move {
        let mut open_gate = TrayOpenGate::default();
        let idle_timeout = lifecycle_idle_timeout(cfg.lifecycle);
        let mut idle_generation = 0_u64;
        let mut rdp_window_open = false;

        while let Ok(action) = action_rx.recv().await {
            match action {
                TrayAction::Open { mode } => {
                    idle_generation = idle_generation.wrapping_add(1);
                    if !open_gate.try_begin() {
                        tracing::debug!("RDP open request ignored while another open is pending");
                        continue;
                    }

                    let cfg = cfg.clone();
                    let manager = manager.clone();
                    let handle = handle.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.ensure_active().await {
                            tracing::error!("VM wake failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "Open KakaoTalk failed".to_string(),
                                body: action_notification_body(
                                    "Open KakaoTalk",
                                    Err(&err.to_string()),
                                ),
                            });
                            let _ = action_tx.try_send(TrayAction::OpenFinished);
                            return;
                        }
                        if let Err(err) = wait_for_rdp_ready(&cfg.vm_ip, &cfg.admin_password).await
                        {
                            tracing::error!("RDP readiness wait failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "Open KakaoTalk failed".to_string(),
                                body: action_notification_body(
                                    "Open KakaoTalk",
                                    Err(&err.to_string()),
                                ),
                            });
                            let _ = action_tx.try_send(TrayAction::OpenFinished);
                            return;
                        }

                        let _ = action_tx.try_send(TrayAction::OpenReady {
                            mode,
                            vm_ip: cfg.vm_ip.clone(),
                            password: cfg.admin_password.clone(),
                        });
                    });
                }
                TrayAction::OpenReady {
                    mode,
                    vm_ip,
                    password,
                } => {
                    if mode == cli::WindowMode::App {
                        if let Some(window) = app.active_window() {
                            window.present();
                            open_gate.finish();
                            continue;
                        }
                    }

                    let on_close = rdp_window_close_handler(
                        rdp_window_close_action(cfg.lifecycle.close_window),
                        action_tx.clone(),
                    );
                    let options = rdp_window_options(mode, display_resolver(mode));

                    if let Err(err) =
                        rdp::RdpWindow::open(&app, &vm_ip, &password, options, on_close)
                    {
                        tracing::error!("RDP window open failed: {err}");
                        let _ = action_tx.try_send(TrayAction::Notify {
                            title: "Open KakaoTalk failed".to_string(),
                            body: action_notification_body("Open KakaoTalk", Err(&err.to_string())),
                        });
                    } else {
                        let _ = action_tx.try_send(TrayAction::WindowOpened);
                    }
                    open_gate.finish();
                }
                TrayAction::OpenFinished => open_gate.finish(),
                TrayAction::WindowOpened => {
                    rdp_window_open = true;
                    idle_generation = idle_generation.wrapping_add(1);
                }
                TrayAction::WindowClosed => {
                    rdp_window_open = false;
                    idle_generation = idle_generation.wrapping_add(1);
                    if let Some(timeout) = idle_timeout {
                        let generation = idle_generation;
                        let action_tx = action_tx.clone();
                        handle.spawn(async move {
                            tokio::time::sleep(timeout).await;
                            let _ = action_tx.try_send(TrayAction::IdleTimeout { generation });
                        });
                    }
                }
                TrayAction::IdleTimeout { generation } => {
                    if generation != idle_generation || rdp_window_open {
                        continue;
                    }
                    let manager = manager.clone();
                    handle.spawn(async move {
                        tracing::info!("VM idle timeout reached; managed-save requested");
                        if let Err(err) = manager.managed_save().await {
                            tracing::error!("VM idle managed save failed: {err}");
                        }
                    });
                }
                TrayAction::RepairKakao => {
                    let manager = manager.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.ensure_active().await {
                            tracing::error!("VM wake before KakaoTalk repair failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "KakaoTalk repair failed".to_string(),
                                body: repair_notification_body(
                                    "KakaoTalk repair",
                                    Err(&err.to_string()),
                                ),
                            });
                            return;
                        }
                        match manager.repair_kakaotalk().await {
                            Ok(detail) => {
                                tracing::info!("KakaoTalk repair requested: {detail}");
                                let _ = action_tx.try_send(TrayAction::Notify {
                                    title: "KakaoTalk repair requested".to_string(),
                                    body: repair_notification_body("KakaoTalk repair", Ok(&detail)),
                                });
                            }
                            Err(err) => {
                                tracing::error!("KakaoTalk repair failed: {err}");
                                let _ = action_tx.try_send(TrayAction::Notify {
                                    title: "KakaoTalk repair failed".to_string(),
                                    body: repair_notification_body(
                                        "KakaoTalk repair",
                                        Err(&err.to_string()),
                                    ),
                                });
                            }
                        }
                    });
                }
                TrayAction::RepairWallpaper => {
                    let manager = manager.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.ensure_active().await {
                            tracing::error!("VM wake before wallpaper repair failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "Wallpaper repair failed".to_string(),
                                body: repair_notification_body(
                                    "Wallpaper repair",
                                    Err(&err.to_string()),
                                ),
                            });
                            return;
                        }
                        match manager.repair_wallpaper().await {
                            Ok(detail) => {
                                tracing::info!("Wallpaper repair requested: {detail}");
                                let _ = action_tx.try_send(TrayAction::Notify {
                                    title: "Wallpaper repair completed".to_string(),
                                    body: repair_notification_body("Wallpaper repair", Ok(&detail)),
                                });
                            }
                            Err(err) => {
                                tracing::error!("Wallpaper repair failed: {err}");
                                let _ = action_tx.try_send(TrayAction::Notify {
                                    title: "Wallpaper repair failed".to_string(),
                                    body: repair_notification_body(
                                        "Wallpaper repair",
                                        Err(&err.to_string()),
                                    ),
                                });
                            }
                        }
                    });
                }
                TrayAction::Pause => {
                    let manager = manager.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.managed_save().await {
                            tracing::error!("VM managed save failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "VM pause failed".to_string(),
                                body: action_notification_body("VM pause", Err(&err.to_string())),
                            });
                        } else {
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "VM pause completed".to_string(),
                                body: action_notification_body("VM pause", Ok("managed-save done")),
                            });
                        }
                    });
                }
                TrayAction::Shutdown => {
                    let manager = manager.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.graceful_shutdown(60).await {
                            tracing::error!("VM shutdown failed: {err}");
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "VM shutdown failed".to_string(),
                                body: action_notification_body(
                                    "VM shutdown",
                                    Err(&err.to_string()),
                                ),
                            });
                        } else {
                            let _ = action_tx.try_send(TrayAction::Notify {
                                title: "VM shutdown completed".to_string(),
                                body: action_notification_body("VM shutdown", Ok("guest stopped")),
                            });
                        }
                    });
                }
                TrayAction::ManagedSaveThenQuit => {
                    let manager = manager.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        handle_managed_save_then_quit(manager, action_tx, "after tray quit").await;
                    });
                }
                TrayAction::Notify { title, body } => {
                    send_desktop_notification(&app, &title, &body);
                }
                TrayAction::Quit => app.quit(),
            }
        }
    });
}

fn main() {
    let cli = cli::Cli::parse();
    init_logging(cli.verbose);
    virt::error::clear_error_callback();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start tokio runtime");

    runtime.block_on(async move {
        match cli.command {
            None => {
                if let Err(err) = run_tray().await {
                    eprintln!("tray 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::Status) => {
                if let Err(err) = run_status().await {
                    eprintln!("status 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::Doctor) => {
                if let Err(err) = run_doctor().await {
                    eprintln!("doctor 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::DiagnosticBundle { output }) => {
                if let Err(err) = run_diagnostic_bundle(output).await {
                    eprintln!("diagnostic-bundle 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::RepairKakao) => {
                if let Err(err) = run_repair_kakao().await {
                    eprintln!("repair-kakao 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::RepairWallpaper) => {
                if let Err(err) = run_repair_wallpaper().await {
                    eprintln!("repair-wallpaper 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::Start { mode, display }) => {
                if let Err(err) = run_start(mode, display).await {
                    eprintln!("start 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::Stop { shutdown }) => {
                if let Err(err) = run_stop(shutdown).await {
                    eprintln!("stop 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::InstallDesktopEntry { exec }) => {
                if let Err(err) = run_install_desktop_entry(exec) {
                    eprintln!("desktop entry 설치 실패: {err}");
                    std::process::exit(1);
                }
            }
            Some(cli::Command::UninstallDesktopEntry) => {
                if let Err(err) = run_uninstall_desktop_entry() {
                    eprintln!("desktop entry 제거 실패: {err}");
                    std::process::exit(1);
                }
            }
        }
    });
}

async fn run_status() -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let manager = vm_manager_from_config(&cfg)?;
    let state = manager.state().await?;

    println!("VM '{}' 상태: {:?}", cfg.vm_name, state);
    println!("lifecycle: {}", lifecycle_summary(cfg.lifecycle));
    println!("rdp: {}", rdp_tcp_status(&cfg.vm_ip).await);
    println!("qemu-ga: {}", qemu_ga_status(&manager, state).await);
    println!("cdrom: {}", cdrom_status(&manager, state).await);
    Ok(())
}

async fn run_doctor() -> error::WinbridgeResult<()> {
    let report = doctor::diagnose_host().await;
    print!("{}", doctor::format_report(&report));
    Ok(())
}

async fn run_repair_kakao() -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    let manager = vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone());

    manager.ensure_active().await?;
    let detail = manager.repair_kakaotalk().await?;
    if detail.is_empty() {
        println!("KakaoTalk repair requested through QEMU guest agent");
    } else {
        println!("{detail}");
    }
    Ok(())
}

async fn run_repair_wallpaper() -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    let manager = vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone());

    manager.ensure_active().await?;
    let detail = manager.repair_wallpaper().await?;
    if detail.is_empty() {
        println!("Wallpaper repair requested through QEMU guest agent");
    } else {
        println!("{detail}");
    }
    Ok(())
}

fn vm_manager_from_config(cfg: &config::WinbridgeConfig) -> error::WinbridgeResult<vm::VmManager> {
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    Ok(vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone()))
}

async fn rdp_tcp_status(vm_ip: &str) -> String {
    match check_tcp_once(vm_ip, RDP_PORT, Duration::from_secs(1)).await {
        Ok(()) => format!("{vm_ip}:{RDP_PORT} reachable"),
        Err(err) => format!("{vm_ip}:{RDP_PORT} unreachable ({err})"),
    }
}

async fn qemu_ga_status(manager: &vm::VmManager, state: vm::VmState) -> String {
    if !state.is_active() {
        return "skipped (VM not active)".to_string();
    }
    match manager.qemu_guest_ping().await {
        Ok(()) => "guest-ping ok".to_string(),
        Err(err) => format!("guest-ping failed ({err})"),
    }
}

async fn cdrom_status(manager: &vm::VmManager, state: vm::VmState) -> String {
    let persistent = match manager.attached_cdroms().await {
        Ok(cdroms) if cdroms.is_empty() => "persistent=none".to_string(),
        Ok(cdroms) => format!("persistent={} attachment(s)", cdroms.len()),
        Err(err) => format!("persistent=inspect failed ({err})"),
    };

    if !state.is_active() {
        return format!("{persistent}, live=skipped (VM not active)");
    }

    let live = match manager.live_attached_cdroms().await {
        Ok(cdroms) if cdroms.is_empty() => "live=none".to_string(),
        Ok(cdroms) => format!("live={} attachment(s)", cdroms.len()),
        Err(err) => format!("live=inspect failed ({err})"),
    };
    format!("{persistent}, {live}")
}

async fn check_tcp_once(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    let address = format!("{host}:{port}");
    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&address)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!("timeout after {}s", timeout.as_secs())),
    }
}

async fn diagnostic_bundle_text() -> String {
    let mut out = String::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let _ = writeln!(out, "winbridge diagnostic bundle");
    let _ = writeln!(out, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "generated-unix-seconds: {now}");
    let _ = writeln!(out);

    let _ = writeln!(out, "[host]");
    append_command_snapshot(&mut out, "uname", "uname", &["-a"]);
    append_command_snapshot(&mut out, "df-home", "df", &["-h", "/home"]);

    match config::WinbridgeConfig::load() {
        Ok(cfg) => {
            let _ = writeln!(out);
            let _ = writeln!(out, "[libvirt]");
            let connect_args = ["--connect", cfg.libvirt_uri.as_str()];
            append_command_snapshot(
                &mut out,
                "virsh-list",
                "virsh",
                &[connect_args[0], connect_args[1], "list", "--all"],
            );
            append_command_snapshot(
                &mut out,
                "virsh-domstate",
                "virsh",
                &[connect_args[0], connect_args[1], "domstate", &cfg.vm_name],
            );
            append_command_snapshot(
                &mut out,
                "virsh-domblklist",
                "virsh",
                &[connect_args[0], connect_args[1], "domblklist", &cfg.vm_name],
            );

            let _ = writeln!(out);
            let _ = writeln!(out, "[status]");
            match vm_manager_from_config(&cfg) {
                Ok(manager) => match manager.state().await {
                    Ok(state) => {
                        let _ = writeln!(out, "vm: {} {:?}", cfg.vm_name, state);
                        let _ = writeln!(out, "lifecycle: {}", lifecycle_summary(cfg.lifecycle));
                        let _ = writeln!(out, "rdp: {}", rdp_tcp_status(&cfg.vm_ip).await);
                        let _ = writeln!(out, "qemu-ga: {}", qemu_ga_status(&manager, state).await);
                        let _ = writeln!(out, "cdrom: {}", cdrom_status(&manager, state).await);
                    }
                    Err(err) => {
                        let _ = writeln!(out, "vm state error: {err}");
                    }
                },
                Err(err) => {
                    let _ = writeln!(out, "manager error: {err}");
                }
            }
        }
        Err(err) => {
            let _ = writeln!(out, "[status]");
            let _ = writeln!(out, "config error: {err}");
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "[doctor]");
    let report = doctor::diagnose_host().await;
    out.push_str(&doctor::format_report(&report));
    out
}

fn append_command_snapshot(out: &mut String, label: &str, program: &str, args: &[&str]) {
    let command = std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let _ = writeln!(out, "## {label}");
    let _ = writeln!(out, "$ {command}");

    match std::process::Command::new(program).args(args).output() {
        Ok(output) => {
            let _ = writeln!(out, "exit: {}", output.status);
            let stdout = truncate_for_bundle(&String::from_utf8_lossy(&output.stdout));
            let stderr = truncate_for_bundle(&String::from_utf8_lossy(&output.stderr));
            if !stdout.trim().is_empty() {
                let _ = writeln!(out, "stdout:");
                let _ = writeln!(out, "{stdout}");
            }
            if !stderr.trim().is_empty() {
                let _ = writeln!(out, "stderr:");
                let _ = writeln!(out, "{stderr}");
            }
        }
        Err(err) => {
            let _ = writeln!(out, "failed to run: {err}");
        }
    }
    let _ = writeln!(out);
}

fn truncate_for_bundle(text: &str) -> String {
    const LIMIT: usize = 12_000;
    if text.len() <= LIMIT {
        return text.to_string();
    }

    let mut end = LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... truncated after {end} bytes ...", &text[..end])
}

fn default_diagnostic_bundle_path() -> error::WinbridgeResult<PathBuf> {
    let dirs = directories::BaseDirs::new().expect("BaseDirs::new must succeed on supported OS");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    Ok(dirs
        .cache_dir()
        .join("winbridge")
        .join("diagnostics")
        .join(format!("winbridge-diagnostics-{timestamp}.txt")))
}

async fn run_diagnostic_bundle(output: Option<PathBuf>) -> error::WinbridgeResult<()> {
    let path = output.unwrap_or(default_diagnostic_bundle_path()?);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = diagnostic_bundle_text().await;
    std::fs::write(&path, text)?;
    println!("Diagnostic bundle written: {}", path.display());
    Ok(())
}

async fn run_tray() -> error::WinbridgeResult<()> {
    let cfg = Arc::new(config::WinbridgeConfig::load()?);
    let backend = Arc::new(vm::libvirt_backend::LibvirtBackendImpl::open(
        &cfg.libvirt_uri,
    )?);
    let manager = Arc::new(vm::VmManager::new(backend, cfg.vm_name.clone()));

    let app = gtk4::Application::builder()
        .application_id(WINBRIDGE_APPLICATION_ID)
        .build();
    app.connect_activate(|_| {});
    let _app_hold = app.hold();
    let handle = tokio::runtime::Handle::current();
    let (action_tx, action_rx) = async_channel::unbounded::<TrayAction>();

    spawn_tray_action_loop(
        app.clone(),
        cfg.clone(),
        manager.clone(),
        handle.clone(),
        action_tx.clone(),
        action_rx,
        Arc::new(|_| cli::DisplayStrategy::StableSlots),
    );

    let open_kakao: Arc<dyn Fn() + Send + Sync> = {
        let action_tx = action_tx.clone();
        Arc::new(move || {
            let _ = action_tx.try_send(TrayAction::Open {
                mode: cli::WindowMode::App,
            });
        })
    };
    let quit_winbridge: Arc<dyn Fn() + Send + Sync> = {
        let action_tx = action_tx.clone();
        Arc::new(move || {
            let _ = action_tx.try_send(tray_quit_action(cfg.lifecycle.quit));
        })
    };
    let repair_kakao: Arc<dyn Fn() + Send + Sync> = {
        let action_tx = action_tx.clone();
        Arc::new(move || {
            let _ = action_tx.try_send(TrayAction::RepairKakao);
        })
    };
    let repair_wallpaper: Arc<dyn Fn() + Send + Sync> = {
        let action_tx = action_tx.clone();
        Arc::new(move || {
            let _ = action_tx.try_send(TrayAction::RepairWallpaper);
        })
    };

    let _kakaotalk_tray_handle = tray::spawn_kakaotalk_tray(tray::KakaoTalkTray {
        on_open: open_kakao.clone(),
        on_repair: repair_kakao.clone(),
        on_quit: quit_winbridge.clone(),
    });

    let _tray_handle = tray::spawn_tray(tray::WinbridgeTray {
        on_open_kakao: open_kakao,
        on_open_desktop: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.try_send(TrayAction::Open {
                    mode: cli::WindowMode::Desktop,
                });
            })
        },
        on_repair_kakao: repair_kakao,
        on_repair_wallpaper: repair_wallpaper,
        on_pause: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.try_send(TrayAction::Pause);
            })
        },
        on_shutdown: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.try_send(TrayAction::Shutdown);
            })
        },
        on_quit: quit_winbridge,
    });

    spawn_process_signal_handler(manager, action_tx);

    app.run_with_args(&["winbridge"]);
    Ok(())
}

async fn run_start(
    mode: cli::WindowMode,
    display: cli::DisplayStrategy,
) -> error::WinbridgeResult<()> {
    let cfg = Arc::new(config::WinbridgeConfig::load()?);
    let backend = Arc::new(vm::libvirt_backend::LibvirtBackendImpl::open(
        &cfg.libvirt_uri,
    )?);
    let manager = Arc::new(vm::VmManager::new(backend, cfg.vm_name.clone()));
    manager.ensure_active().await?;
    wait_for_rdp_ready(&cfg.vm_ip, &cfg.admin_password).await?;

    let app = gtk4::Application::builder()
        .application_id(gtk_application_id(mode))
        .build();
    let handle = tokio::runtime::Handle::current();
    let vm_ip = cfg.vm_ip.clone();
    let password = cfg.admin_password.clone();
    let close_window_policy = cfg.lifecycle.close_window;
    let quit_policy = cfg.lifecycle.quit;
    let start_policy = start_process_policy(mode);
    let _app_hold = match start_policy {
        StartProcessPolicy::HoldTrayAfterWindowClose => Some(app.hold()),
        StartProcessPolicy::ExitWithLastWindow => None,
    };
    let (action_tx, action_rx) = async_channel::unbounded::<TrayAction>();

    spawn_tray_action_loop(
        app.clone(),
        cfg.clone(),
        manager.clone(),
        handle.clone(),
        action_tx.clone(),
        action_rx,
        Arc::new(move |_| display),
    );
    spawn_process_signal_handler(manager, action_tx.clone());

    let activate_action_tx = action_tx.clone();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let _guard = handle.enter();
        let on_close = rdp_window_close_handler(
            rdp_window_close_action(close_window_policy),
            activate_action_tx.clone(),
        );
        let options = rdp_window_options(mode, display);

        if let Err(err) = rdp::RdpWindow::open(app, &vm_ip, &password, options, on_close) {
            tracing::error!("RDP window open failed: {err}");
        } else {
            let _ = activate_action_tx.try_send(TrayAction::WindowOpened);
        }
    });
    let _kakaotalk_tray_handle = if start_policy == StartProcessPolicy::HoldTrayAfterWindowClose {
        let action_tx = action_tx.clone();
        let quit_action_tx = action_tx.clone();
        Some(tray::spawn_kakaotalk_tray(tray::KakaoTalkTray {
            on_open: Arc::new(move || {
                let _ = action_tx.try_send(TrayAction::Open {
                    mode: cli::WindowMode::App,
                });
            }),
            on_repair: {
                let action_tx = quit_action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::RepairKakao);
                })
            },
            on_quit: Arc::new(move || {
                let _ = quit_action_tx.try_send(tray_quit_action(quit_policy));
            }),
        }))
    } else {
        None
    };
    let _tray_handle = if start_policy == StartProcessPolicy::HoldTrayAfterWindowClose {
        Some(tray::spawn_tray(tray::WinbridgeTray {
            on_open_kakao: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::Open {
                        mode: cli::WindowMode::App,
                    });
                })
            },
            on_open_desktop: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::Open {
                        mode: cli::WindowMode::Desktop,
                    });
                })
            },
            on_repair_kakao: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::RepairKakao);
                })
            },
            on_repair_wallpaper: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::RepairWallpaper);
                })
            },
            on_pause: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::Pause);
                })
            },
            on_shutdown: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(TrayAction::Shutdown);
                })
            },
            on_quit: {
                let action_tx = action_tx.clone();
                Arc::new(move || {
                    let _ = action_tx.try_send(tray_quit_action(quit_policy));
                })
            },
        }))
    } else {
        None
    };
    app.run_with_args(&["winbridge"]);

    Ok(())
}

fn gtk_application_id(mode: cli::WindowMode) -> &'static str {
    match mode {
        cli::WindowMode::App => desktop::KAKAOTALK_APPLICATION_ID,
        cli::WindowMode::Desktop => WINBRIDGE_APPLICATION_ID,
    }
}

fn rdp_window_options(
    mode: cli::WindowMode,
    display: cli::DisplayStrategy,
) -> rdp::RdpWindowOptions {
    match mode {
        cli::WindowMode::App => rdp::RdpWindowOptions::kakaotalk_app()
            .with_display_strategy(rdp_display_strategy(display)),
        cli::WindowMode::Desktop => match display {
            cli::DisplayStrategy::StableSlots => rdp::RdpWindowOptions::new("Windows Desktop"),
            cli::DisplayStrategy::ExperimentalMultimon => {
                rdp::RdpWindowOptions::experimental_multimon_desktop()
            }
        },
    }
}

fn rdp_display_strategy(display: cli::DisplayStrategy) -> rdp::RdpDisplayStrategy {
    match display {
        cli::DisplayStrategy::StableSlots => rdp::RdpDisplayStrategy::StableSlots,
        cli::DisplayStrategy::ExperimentalMultimon => rdp::RdpDisplayStrategy::ExperimentalMultimon,
    }
}

async fn wait_for_rdp_ready(vm_ip: &str, password: &str) -> error::WinbridgeResult<()> {
    tracing::info!(vm_ip, port = RDP_PORT, "RDP 포트 준비 대기");
    wait_for_tcp_port_ready(vm_ip, RDP_PORT, RDP_READY_TIMEOUT, RDP_READY_POLL_INTERVAL).await?;

    tracing::info!(vm_ip, port = RDP_PORT, "RDP 핸드셰이크 준비 대기");
    let vm_ip = vm_ip.to_string();
    let password = password.to_string();
    wait_for_ready_operation(
        "RDP 핸드셰이크",
        RDP_READY_TIMEOUT,
        RDP_READY_POLL_INTERVAL,
        move || {
            let vm_ip = vm_ip.clone();
            let password = password.clone();
            async move {
                let username = std::env::var("WINBRIDGE_ADMIN_USER")
                    .unwrap_or_else(|_| "Administrator".to_string());
                let probe = rdp::RdpHeadlessProbe::new(&vm_ip, RDP_PORT, &username, &password)?;
                match tokio::time::timeout(RDP_HANDSHAKE_ATTEMPT_TIMEOUT, probe.probe()).await {
                    Ok(Ok(_result)) => Ok(()),
                    Ok(Err(err)) => Err(err),
                    Err(_) => Err(error::RdpError::Handshake(format!(
                        "RDP probe 시도 시간 초과 ({}s)",
                        RDP_HANDSHAKE_ATTEMPT_TIMEOUT.as_secs()
                    ))
                    .into()),
                }
            }
        },
    )
    .await
}

async fn wait_for_tcp_port_ready(
    host: &str,
    port: u16,
    timeout: Duration,
    interval: Duration,
) -> error::WinbridgeResult<()> {
    let address = format!("{host}:{port}");
    wait_for_ready_operation("TCP 포트", timeout, interval, move || {
        let address = address.clone();
        async move {
            match tokio::time::timeout(interval, tokio::net::TcpStream::connect(&address)).await {
                Ok(Ok(_stream)) => {
                    tracing::info!(%address, "TCP 포트 준비 완료");
                    Ok(())
                }
                Ok(Err(err)) => {
                    Err(error::RdpError::Handshake(format!("TCP 연결 실패: {err}")).into())
                }
                Err(_) => {
                    Err(error::RdpError::Handshake("TCP 연결 시도 시간 초과".to_string()).into())
                }
            }
        }
    })
    .await
}

async fn wait_for_ready_operation<F, Fut>(
    label: &'static str,
    timeout: Duration,
    interval: Duration,
    mut operation: F,
) -> error::WinbridgeResult<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = error::WinbridgeResult<()>>,
{
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match operation().await {
            Ok(()) => return Ok(()),
            Err(err) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(error::RdpError::Handshake(format!(
                        "{label} 준비 시간 초과 (timeout={}s, last_error={err})",
                        timeout.as_secs()
                    ))
                    .into());
                }
                tracing::debug!("{label} 준비 대기 중: {err}");
            }
        }

        tokio::time::sleep(interval).await;
    }
}

fn run_install_desktop_entry(exec: Option<std::path::PathBuf>) -> error::WinbridgeResult<()> {
    let executable = match exec {
        Some(path) => path,
        None => std::env::current_exe()?,
    };
    let installed = desktop::install_kakaotalk_desktop_entry(&executable)?;

    println!(
        "KakaoTalk desktop entry installed:\n  {}\n  {}\n  {}\n  {}",
        installed.desktop_entry_path.display(),
        installed.icon_path.display(),
        installed.command_path.display(),
        installed.autostart_entry_path.display()
    );
    Ok(())
}

fn run_uninstall_desktop_entry() -> error::WinbridgeResult<()> {
    let uninstalled = desktop::uninstall_kakaotalk_desktop_entry()?;

    println!("KakaoTalk desktop entry removed:");
    for path in uninstalled.removed_paths {
        println!("  removed {}", path.display());
    }
    for path in uninstalled.missing_paths {
        println!("  already absent {}", path.display());
    }
    Ok(())
}

async fn run_stop(shutdown: bool) -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    let manager = vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone());

    if shutdown {
        manager.graceful_shutdown(60).await
    } else {
        manager.managed_save().await
    }
}

fn init_logging(verbose: bool) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,winbridge=info"))
    };

    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SignalTestBackend {
        managed_save_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl vm::backend::LibvirtBackend for SignalTestBackend {
        async fn state(&self, _vm_name: &str) -> error::WinbridgeResult<vm::VmState> {
            Ok(vm::VmState::Off)
        }

        async fn start(&self, _vm_name: &str) -> error::WinbridgeResult<()> {
            Ok(())
        }

        async fn resume_from_saved(&self, _vm_name: &str) -> error::WinbridgeResult<()> {
            Ok(())
        }

        async fn managed_save(&self, _vm_name: &str) -> error::WinbridgeResult<()> {
            self.managed_save_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn shutdown(&self, _vm_name: &str) -> error::WinbridgeResult<()> {
            Ok(())
        }

        async fn destroy(&self, _vm_name: &str) -> error::WinbridgeResult<()> {
            Ok(())
        }

        async fn domain_xml(&self, _vm_name: &str) -> error::WinbridgeResult<String> {
            Ok("<domain/>".to_string())
        }

        async fn live_domain_xml(&self, _vm_name: &str) -> error::WinbridgeResult<String> {
            Ok("<domain/>".to_string())
        }

        async fn qemu_agent_command(
            &self,
            _vm_name: &str,
            _command: &str,
            _timeout_secs: i32,
        ) -> error::WinbridgeResult<String> {
            Ok(r#"{"return":{}}"#.to_string())
        }
    }

    #[test]
    fn rdp_window_close_keeps_vm_running_for_mvp_testing() {
        assert_eq!(
            rdp_window_close_action(config::CloseWindowPolicy::KeepRunning),
            RdpWindowCloseAction::CloseWindowOnly
        );
    }

    #[test]
    fn rdp_window_close_can_managed_save_from_config() {
        assert_eq!(
            rdp_window_close_action(config::CloseWindowPolicy::ManagedSave),
            RdpWindowCloseAction::ManagedSave
        );
    }

    #[test]
    fn process_signal_saves_vm_before_quitting_for_host_shutdown() {
        assert_eq!(
            process_signal_action(),
            ProcessSignalAction::ManagedSaveThenQuit
        );
    }

    #[test]
    fn tray_quit_saves_vm_before_quitting() {
        assert_eq!(
            tray_quit_action(config::QuitPolicy::ManagedSave),
            TrayAction::ManagedSaveThenQuit
        );
    }

    #[test]
    fn tray_quit_can_leave_vm_running_from_config() {
        assert_eq!(
            tray_quit_action(config::QuitPolicy::KeepRunning),
            TrayAction::Quit
        );
    }

    #[tokio::test]
    async fn process_signal_handler_managed_saves_then_sends_quit() {
        let managed_save_calls = Arc::new(AtomicUsize::new(0));
        let backend = SignalTestBackend {
            managed_save_calls: managed_save_calls.clone(),
        };
        let manager = Arc::new(vm::VmManager::new(Arc::new(backend), "test-vm"));
        let (action_tx, action_rx) = async_channel::bounded(1);

        handle_process_signal_action(ProcessSignalAction::ManagedSaveThenQuit, manager, action_tx)
            .await;

        assert_eq!(managed_save_calls.load(Ordering::SeqCst), 1);
        assert!(matches!(action_rx.try_recv().unwrap(), TrayAction::Quit));
    }

    #[tokio::test]
    async fn managed_save_then_quit_sends_quit_after_save() {
        let managed_save_calls = Arc::new(AtomicUsize::new(0));
        let backend = SignalTestBackend {
            managed_save_calls: managed_save_calls.clone(),
        };
        let manager = Arc::new(vm::VmManager::new(Arc::new(backend), "test-vm"));
        let (action_tx, action_rx) = async_channel::bounded(1);

        handle_managed_save_then_quit(manager, action_tx, "from test").await;

        assert_eq!(managed_save_calls.load(Ordering::SeqCst), 1);
        assert!(matches!(action_rx.try_recv().unwrap(), TrayAction::Quit));
    }

    #[test]
    fn app_start_keeps_process_alive_as_tray_after_window_close() {
        assert_eq!(
            start_process_policy(cli::WindowMode::App),
            StartProcessPolicy::HoldTrayAfterWindowClose
        );
    }

    #[test]
    fn desktop_start_exits_after_last_window_close() {
        assert_eq!(
            start_process_policy(cli::WindowMode::Desktop),
            StartProcessPolicy::ExitWithLastWindow
        );
    }

    #[test]
    fn lifecycle_idle_timeout_is_disabled_by_default() {
        assert_eq!(
            lifecycle_idle_timeout(config::LifecycleConfig::default()),
            None
        );
    }

    #[test]
    fn lifecycle_idle_timeout_converts_minutes_to_duration() {
        let lifecycle = config::LifecycleConfig {
            idle_timeout_minutes: Some(30),
            ..config::LifecycleConfig::default()
        };

        assert_eq!(
            lifecycle_idle_timeout(lifecycle),
            Some(Duration::from_secs(1800))
        );
    }

    #[test]
    fn status_lifecycle_summary_explains_defaults() {
        assert_eq!(
            lifecycle_summary(config::LifecycleConfig::default()),
            "close-window=keep-running, quit=managed-save, idle-timeout=disabled"
        );
    }

    #[test]
    fn status_lifecycle_summary_explains_configured_timeout() {
        let lifecycle = config::LifecycleConfig {
            close_window: config::CloseWindowPolicy::ManagedSave,
            quit: config::QuitPolicy::KeepRunning,
            idle_timeout_minutes: Some(20),
        };

        assert_eq!(
            lifecycle_summary(lifecycle),
            "close-window=managed-save, quit=keep-running, idle-timeout=20m"
        );
    }

    #[test]
    fn tray_repair_notification_body_includes_next_action() {
        let body = repair_notification_body("KakaoTalk repair", Err("boom"));

        assert!(body.contains("KakaoTalk repair failed"));
        assert!(body.contains("run doctor"));
        assert!(body.contains("open Windows desktop"));
    }

    #[test]
    fn tray_action_notification_body_includes_next_action() {
        let body = action_notification_body("Open KakaoTalk", Err("boom"));

        assert!(body.contains("Open KakaoTalk failed"));
        assert!(body.contains("run doctor"));
        assert!(body.contains("open Windows desktop"));
    }

    #[test]
    fn tray_open_gate_blocks_duplicate_open_until_finished() {
        let mut gate = TrayOpenGate::default();

        assert!(gate.try_begin());
        assert!(!gate.try_begin());
        gate.finish();
        assert!(gate.try_begin());
    }

    #[test]
    fn diagnostic_bundle_snapshot_reports_missing_command() {
        let mut out = String::new();

        append_command_snapshot(
            &mut out,
            "missing",
            "/definitely/missing-winbridge-command",
            &[],
        );

        assert!(out.contains("## missing"));
        assert!(out.contains("failed to run"));
    }

    #[test]
    fn diagnostic_bundle_truncates_large_output_on_char_boundary() {
        let input = format!("{}한글", "a".repeat(12_500));
        let output = truncate_for_bundle(&input);

        assert!(output.contains("truncated after"));
        assert!(std::str::from_utf8(output.as_bytes()).is_ok());
    }

    #[test]
    fn desktop_experimental_multimon_uses_full_two_monitor_viewport() {
        let options = rdp_window_options(
            cli::WindowMode::Desktop,
            cli::DisplayStrategy::ExperimentalMultimon,
        );

        assert_eq!(options.title, "Windows Desktop");
        assert_eq!(
            options.display_strategy,
            rdp::RdpDisplayStrategy::ExperimentalMultimon
        );
        assert_eq!(
            options.virtual_desktop_layout,
            Some(rdp::RdpVirtualDesktopLayout::TwoHorizontalSlots {
                slot_width: 1280,
                slot_height: 720,
            })
        );
        assert_eq!(options.viewport, rdp::RdpViewport::new(0, 0, 2560, 720));
    }

    #[test]
    fn app_mode_uses_kakaotalk_application_identity() {
        assert_eq!(
            gtk_application_id(cli::WindowMode::App),
            desktop::KAKAOTALK_APPLICATION_ID
        );
        assert_eq!(
            gtk_application_id(cli::WindowMode::Desktop),
            "dev.winbridge.Winbridge"
        );
    }

    #[test]
    fn kakaotalk_desktop_entry_launches_app_mode_with_icon_identity() {
        let entry =
            desktop::kakaotalk_desktop_entry(std::path::Path::new("/opt/winbridge/bin/winbridge"));

        assert!(entry.contains("Name=KakaoTalk"));
        assert!(entry.contains("Icon=winbridge-kakaotalk"));
        assert!(entry.contains("StartupWMClass=dev.winbridge.KakaoTalk"));
        assert!(entry.contains(
            "Exec=\"/opt/winbridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[tokio::test]
    async fn tcp_port_wait_returns_when_port_accepts_connections() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        wait_for_tcp_port_ready(
            "127.0.0.1",
            port,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(10),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn tcp_port_wait_times_out_when_port_stays_closed() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = wait_for_tcp_port_ready(
            "127.0.0.1",
            port,
            std::time::Duration::from_millis(30),
            std::time::Duration::from_millis(5),
        )
        .await
        .unwrap_err();

        assert!(format!("{err}").contains("TCP 포트 준비 시간 초과"));
    }

    #[tokio::test]
    async fn readiness_retry_keeps_trying_until_operation_succeeds() {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        wait_for_ready_operation(
            "test readiness",
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(1),
            {
                let attempts = attempts.clone();
                move || {
                    let attempts = attempts.clone();
                    async move {
                        let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if attempt < 2 {
                            Err(error::RdpError::Handshake("not ready yet".to_string()).into())
                        } else {
                            Ok(())
                        }
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}
