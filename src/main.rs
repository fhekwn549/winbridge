use clap::Parser;
use gtk4::prelude::*;
use std::sync::Arc;
use winbridge::{cli, config, error, rdp, vm};

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
                tracing::info!("stub tray entry point");
                println!("winbridge tray placeholder - see Phase 5");
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
                println!("stop placeholder (shutdown={shutdown}) - implemented in Phase 5");
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
