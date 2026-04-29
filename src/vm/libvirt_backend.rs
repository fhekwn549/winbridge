use crate::error::{VmError, WinbridgeResult};
use crate::vm::backend::LibvirtBackend;
use crate::vm::VmState;
use async_trait::async_trait;
use std::sync::Mutex;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::sys;

/// 운영용 libvirt backend.
///
/// libvirt 호출은 blocking API이지만 MVP 단계에서는 호출 범위를 짧게 유지하고,
/// UI 통합 시 응답성 문제가 보이면 spawn_blocking으로 분리한다.
pub struct LibvirtBackendImpl {
    connection: Mutex<Connect>,
}

impl LibvirtBackendImpl {
    pub fn open(uri: &str) -> WinbridgeResult<Self> {
        let connection =
            Connect::open(Some(uri)).map_err(|e| VmError::LibvirtConnect(e.to_string()))?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lookup(&self, name: &str) -> WinbridgeResult<Domain> {
        let connection = self
            .connection
            .lock()
            .expect("libvirt connection mutex poisoned");
        Domain::lookup_by_name(&connection, name).map_err(|_| {
            VmError::DomainNotFound {
                name: name.to_string(),
            }
            .into()
        })
    }

    fn classify_state(state: sys::virDomainState, has_managed_save: bool) -> VmState {
        match state {
            sys::VIR_DOMAIN_RUNNING => VmState::Active,
            sys::VIR_DOMAIN_SHUTOFF if has_managed_save => VmState::Saved,
            sys::VIR_DOMAIN_SHUTOFF => VmState::Off,
            _ => VmState::Other,
        }
    }
}

#[async_trait]
impl LibvirtBackend for LibvirtBackendImpl {
    async fn state(&self, vm_name: &str) -> WinbridgeResult<VmState> {
        let domain = self.lookup(vm_name)?;
        let (state, _reason) = domain
            .get_state()
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        let has_managed_save = if state == sys::VIR_DOMAIN_SHUTOFF {
            domain
                .has_managed_save(0)
                .map_err(|e| VmError::LibvirtApi(e.to_string()))?
        } else {
            false
        };

        Ok(Self::classify_state(state, has_managed_save))
    }

    async fn start(&self, vm_name: &str) -> WinbridgeResult<()> {
        let domain = self.lookup(vm_name)?;
        domain
            .create()
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        Ok(())
    }

    async fn resume_from_saved(&self, vm_name: &str) -> WinbridgeResult<()> {
        let domain = self.lookup(vm_name)?;
        domain
            .create()
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        Ok(())
    }

    async fn managed_save(&self, vm_name: &str) -> WinbridgeResult<()> {
        let domain = self.lookup(vm_name)?;
        domain
            .managed_save(0)
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        Ok(())
    }

    async fn shutdown(&self, vm_name: &str) -> WinbridgeResult<()> {
        let domain = self.lookup(vm_name)?;
        domain
            .shutdown()
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        Ok(())
    }

    async fn destroy(&self, vm_name: &str) -> WinbridgeResult<()> {
        let domain = self.lookup(vm_name)?;
        domain
            .destroy()
            .map_err(|e| VmError::LibvirtApi(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_running_as_active() {
        assert_eq!(
            LibvirtBackendImpl::classify_state(sys::VIR_DOMAIN_RUNNING, false),
            VmState::Active
        );
    }

    #[test]
    fn classify_shutoff_with_managed_save_as_saved() {
        assert_eq!(
            LibvirtBackendImpl::classify_state(sys::VIR_DOMAIN_SHUTOFF, true),
            VmState::Saved
        );
    }

    #[test]
    fn classify_shutoff_without_managed_save_as_off() {
        assert_eq!(
            LibvirtBackendImpl::classify_state(sys::VIR_DOMAIN_SHUTOFF, false),
            VmState::Off
        );
    }
}
