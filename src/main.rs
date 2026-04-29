use clap::Parser;
use gtk4::prelude::*;
use std::sync::Arc;
use winbridge::{cli, config, error, rdp, tray, vm};

enum TrayAction {
    OpenKakao,
    OpenReady { vm_ip: String, password: String },
    Pause,
    Shutdown,
    Quit,
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
            Some(cli::Command::Start) => {
                if let Err(err) = run_start().await {
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
        .application_id("dev.winbridge.Winbridge")
        .build();
    let handle = tokio::runtime::Handle::current();
    let (action_tx, action_rx) =
        glib::MainContext::channel::<TrayAction>(glib::Priority::default());

    {
        let app = app.clone();
        let cfg = cfg.clone();
        let manager = manager.clone();
        let handle = handle.clone();
        let action_tx = action_tx.clone();
        action_rx.attach(None, move |action| {
            match action {
                TrayAction::OpenKakao => {
                    let cfg = cfg.clone();
                    let manager = manager.clone();
                    let handle = handle.clone();
                    let action_tx = action_tx.clone();
                    handle.spawn(async move {
                        if let Err(err) = manager.ensure_active().await {
                            tracing::error!("VM wake failed: {err}");
                            return;
                        }

                        let _ = action_tx.send(TrayAction::OpenReady {
                            vm_ip: cfg.vm_ip.clone(),
                            password: cfg.admin_password.clone(),
                        });
                    });
                }
                TrayAction::OpenReady { vm_ip, password } => {
                    if let Err(err) = rdp::RdpWindow::open(&app, &vm_ip, &password) {
                        tracing::error!("RDP window open failed: {err}");
                    }
                }
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
            glib::ControlFlow::Continue
        });
    }

    let _tray_handle = tray::spawn_tray(tray::WinbridgeTray {
        on_open_kakao: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.send(TrayAction::OpenKakao);
            })
        },
        on_pause: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.send(TrayAction::Pause);
            })
        },
        on_shutdown: {
            let action_tx = action_tx.clone();
            Arc::new(move || {
                let _ = action_tx.send(TrayAction::Shutdown);
            })
        },
        on_quit: Arc::new(move || {
            let _ = action_tx.send(TrayAction::Quit);
        }),
    });

    app.run();
    Ok(())
}

async fn run_start() -> error::WinbridgeResult<()> {
    let cfg = config::WinbridgeConfig::load()?;
    let backend = vm::libvirt_backend::LibvirtBackendImpl::open(&cfg.libvirt_uri)?;
    let manager = vm::VmManager::new(Arc::new(backend), cfg.vm_name.clone());
    manager.ensure_active().await?;

    let app = gtk4::Application::builder()
        .application_id("dev.winbridge.Winbridge")
        .build();
    let handle = tokio::runtime::Handle::current();
    let vm_ip = cfg.vm_ip.clone();
    let password = cfg.admin_password.clone();

    app.connect_activate(move |app| {
        let _guard = handle.enter();
        if let Err(err) = rdp::RdpWindow::open(app, &vm_ip, &password) {
            tracing::error!("RDP window open failed: {err}");
        }
    });
    app.run();

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
