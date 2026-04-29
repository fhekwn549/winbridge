pub mod backend;

use crate::error::WinbridgeResult;
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

impl VmManager {
    pub fn new(backend: Arc<dyn backend::LibvirtBackend>, vm_name: impl Into<String>) -> Self {
        Self { backend, vm_name: vm_name.into() }
    }

    pub async fn state(&self) -> WinbridgeResult<VmState> {
        self.backend.state(&self.vm_name).await
    }
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
}
