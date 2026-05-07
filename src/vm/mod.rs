pub mod backend;
pub mod libvirt_backend;

use crate::error::{VmError, WinbridgeResult};
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
}
