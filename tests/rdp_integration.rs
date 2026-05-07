//! Run only on a host with the Winbridge VM configured and RDP reachable:
//! `cargo test --features integration --test rdp_integration`

#![cfg(feature = "integration")]

use std::sync::Arc;
use winbridge::config::WinbridgeConfig;
use winbridge::rdp::RdpHeadlessProbe;
use winbridge::vm::libvirt_backend::LibvirtBackendImpl;
use winbridge::vm::VmManager;

#[tokio::test]
async fn rdp_probe_connects_and_reports_desktop_dimensions() {
    let cfg = WinbridgeConfig::load().expect("failed to load winbridge config");
    let backend =
        LibvirtBackendImpl::open(&cfg.libvirt_uri).expect("failed to open libvirt backend");
    let manager = VmManager::new(Arc::new(backend), cfg.vm_name.clone());
    manager
        .ensure_active()
        .await
        .expect("failed to ensure VM is active");

    let username =
        std::env::var("WINBRIDGE_ADMIN_USER").unwrap_or_else(|_| "Administrator".to_string());
    let probe = RdpHeadlessProbe::new(&cfg.vm_ip, 3389, &username, &cfg.admin_password)
        .expect("failed to create RDP probe");
    let result = probe.probe().await.expect("RDP probe failed");

    assert!(result.width > 0, "width: {}", result.width);
    assert!(result.height > 0, "height: {}", result.height);
    assert!(result.bits_per_pixel > 0, "bpp: {}", result.bits_per_pixel);
}
