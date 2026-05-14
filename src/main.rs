use clap::Parser;
use gtk4::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use winbridge::{cli, config, desktop, error, rdp, tray, vm};

const RDP_PORT: u16 = 3389;
const RDP_READY_TIMEOUT: Duration = Duration::from_secs(180);
const RDP_READY_POLL_INTERVAL: Duration = Duration::from_secs(2);
const RDP_HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
const WINBRIDGE_APPLICATION_ID: &str = "dev.winbridge.Winbridge";

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
    Pause,
    Shutdown,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RdpWindowCloseAction {
    CloseWindowOnly,
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

fn rdp_window_close_action() -> RdpWindowCloseAction {
    RdpWindowCloseAction::CloseWindowOnly
}

fn process_signal_action() -> ProcessSignalAction {
    ProcessSignalAction::ManagedSaveThenQuit
}

fn start_process_policy(mode: cli::WindowMode) -> StartProcessPolicy {
    match mode {
        cli::WindowMode::App => StartProcessPolicy::HoldTrayAfterWindowClose,
        cli::WindowMode::Desktop => StartProcessPolicy::ExitWithLastWindow,
    }
}

fn rdp_window_close_handler(action: RdpWindowCloseAction) -> Arc<dyn Fn() + Send + Sync> {
    match action {
        RdpWindowCloseAction::CloseWindowOnly => Arc::new(|| {
            tracing::debug!("RDP window closed; VM state left unchanged");
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
            tracing::info!("saving VM before quitting winbridge after process signal");
            if let Err(err) = manager.managed_save().await {
                tracing::error!("VM managed save during process shutdown failed: {err}");
            }
        }
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

        while let Ok(action) = action_rx.recv().await {
            match action {
                TrayAction::Open { mode } => {
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
                            let _ = action_tx.try_send(TrayAction::OpenFinished);
                            return;
                        }
                        if let Err(err) = wait_for_rdp_ready(&cfg.vm_ip, &cfg.admin_password).await
                        {
                            tracing::error!("RDP readiness wait failed: {err}");
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

                    let on_close = rdp_window_close_handler(rdp_window_close_action());
                    let options = rdp_window_options(mode, display_resolver(mode));

                    if let Err(err) =
                        rdp::RdpWindow::open(&app, &vm_ip, &password, options, on_close)
                    {
                        tracing::error!("RDP window open failed: {err}");
                    }
                    open_gate.finish();
                }
                TrayAction::OpenFinished => open_gate.finish(),
                TrayAction::Pause => {
                    let manager = manager.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.managed_save().await {
                            tracing::error!("VM managed save failed: {err}");
                        }
                    });
                }
                TrayAction::Shutdown => {
                    let manager = manager.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.graceful_shutdown(60).await {
                            tracing::error!("VM shutdown failed: {err}");
                        }
                    });
                }
                TrayAction::Quit => app.quit(),
            }
        }
    });
}

fn main() {
    let cli = cli::Cli::parse();
    init_logging(cli.verbose);

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
        }
    });
}

async fn run_status() -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    let manager = vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone());
    let state = manager.state().await?;

    println!("VM '{}' 상태: {:?}", cfg.vm_name, state);
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
            let _ = action_tx.try_send(TrayAction::Quit);
        })
    };

    let _kakaotalk_tray_handle = tray::spawn_kakaotalk_tray(tray::KakaoTalkTray {
        on_open: open_kakao.clone(),
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

    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let _guard = handle.enter();
        let on_close = rdp_window_close_handler(rdp_window_close_action());
        let options = rdp_window_options(mode, display);

        if let Err(err) = rdp::RdpWindow::open(app, &vm_ip, &password, options, on_close) {
            tracing::error!("RDP window open failed: {err}");
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
            on_quit: Arc::new(move || {
                let _ = quit_action_tx.try_send(TrayAction::Quit);
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
                    let _ = action_tx.try_send(TrayAction::Quit);
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
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
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
    }

    #[test]
    fn rdp_window_close_keeps_vm_running_for_mvp_testing() {
        assert_eq!(
            rdp_window_close_action(),
            RdpWindowCloseAction::CloseWindowOnly
        );
    }

    #[test]
    fn process_signal_saves_vm_before_quitting_for_host_shutdown() {
        assert_eq!(
            process_signal_action(),
            ProcessSignalAction::ManagedSaveThenQuit
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
    fn tray_open_gate_blocks_duplicate_open_until_finished() {
        let mut gate = TrayOpenGate::default();

        assert!(gate.try_begin());
        assert!(!gate.try_begin());
        gate.finish();
        assert!(gate.try_begin());
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
