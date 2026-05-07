use thiserror::Error;

pub type WinbridgeResult<T> = std::result::Result<T, WinbridgeError>;

#[derive(Error, Debug)]
pub enum WinbridgeError {
    #[error("VM 작업 실패: {0}")]
    Vm(#[from] VmError),

    #[error("RDP 연결 실패: {0}")]
    Rdp(#[from] RdpError),

    #[error("설정 로드 실패: {0}")]
    Config(#[from] ConfigError),

    #[error("I/O 오류: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Error, Debug)]
pub enum VmError {
    #[error("libvirt 연결 실패: 사용자가 libvirt 그룹에 속해 있는지 확인하세요. (원인: {0})")]
    LibvirtConnect(String),

    #[error("VM 도메인 '{name}'을(를) 찾을 수 없습니다")]
    DomainNotFound { name: String },

    #[error("VM 상태 전환 시간 초과 ({operation}, {timeout_secs}s)")]
    StateTimeout {
        operation: &'static str,
        timeout_secs: u64,
    },

    #[error("libvirt API 오류: {0}")]
    LibvirtApi(String),
}

#[derive(Error, Debug)]
pub enum RdpError {
    #[error("RDP 핸드셰이크 실패: {0}")]
    Handshake(String),

    #[error("RDP 연결 끊김: {0}")]
    Disconnected(String),

    #[error("RDP 데이터 처리 오류: {0}")]
    Protocol(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("자격 증명 파일 없음: {path}. install.sh를 먼저 실행하세요.")]
    CredentialsMissing { path: String },

    #[error("자격 증명 파일에 WINBRIDGE_ADMIN_PASSWORD 항목이 없습니다 ({path})")]
    PasswordMissing { path: String },

    #[error("자격 증명 파일 파싱 오류 ({path}): {reason}")]
    ParseError { path: String, reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_error_displays_korean_guidance_for_libvirt_connect() {
        let err = VmError::LibvirtConnect("권한 거부".into());
        let msg = format!("{}", err);
        assert!(msg.contains("libvirt 그룹"));
        assert!(msg.contains("권한 거부"));
    }

    #[test]
    fn winbridge_error_wraps_vm_error_via_from() {
        let inner = VmError::DomainNotFound {
            name: "winbridge-srv2022".into(),
        };
        let outer: WinbridgeError = inner.into();
        let msg = format!("{}", outer);
        assert!(msg.contains("winbridge-srv2022"));
    }
}
