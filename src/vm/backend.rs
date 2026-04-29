use crate::error::WinbridgeResult;
use crate::vm::VmState;
use async_trait::async_trait;

/// libvirt API의 추상화. 운영 코드는 `LibvirtBackendImpl`을 사용,
/// 단위 테스트는 mockall이 생성한 mock을 사용.
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LibvirtBackend: Send + Sync + 'static {
    async fn state(&self, vm_name: &str) -> WinbridgeResult<VmState>;
    async fn start(&self, vm_name: &str) -> WinbridgeResult<()>;
    async fn resume_from_saved(&self, vm_name: &str) -> WinbridgeResult<()>;
    async fn managed_save(&self, vm_name: &str) -> WinbridgeResult<()>;
    async fn shutdown(&self, vm_name: &str) -> WinbridgeResult<()>;
    async fn destroy(&self, vm_name: &str) -> WinbridgeResult<()>;
}
