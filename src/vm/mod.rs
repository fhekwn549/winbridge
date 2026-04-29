pub mod backend;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_requires_start() {
        assert!(VmState::Off.requires_start());
        assert!(!VmState::Off.requires_resume());
    }

    #[test]
    fn saved_requires_resume() {
        assert!(VmState::Saved.requires_resume());
        assert!(!VmState::Saved.requires_start());
    }

    #[test]
    fn active_needs_no_action() {
        assert!(VmState::Active.is_active());
        assert!(!VmState::Active.requires_start());
        assert!(!VmState::Active.requires_resume());
    }
}
