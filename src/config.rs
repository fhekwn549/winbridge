use crate::error::{ConfigError, WinbridgeResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WinbridgeConfig {
    pub admin_password: String,
    pub vm_name: String,
    pub vm_ip: String,
    pub libvirt_uri: String,
}

impl WinbridgeConfig {
    pub const DEFAULT_VM_NAME: &'static str = "winbridge-srv2022";
    pub const DEFAULT_VM_IP: &'static str = "192.168.122.50";
    pub const DEFAULT_LIBVIRT_URI: &'static str = "qemu:///system";

    /// `~/.config/winbridge/credentials` (또는 env override) 에서 읽어 들임.
    pub fn load() -> WinbridgeResult<Self> {
        let path = Self::credentials_path();
        Self::load_from(&path)
    }

    pub fn credentials_path() -> PathBuf {
        if let Ok(p) = std::env::var("WINBRIDGE_CREDENTIALS_FILE") {
            return PathBuf::from(p);
        }
        let dirs = directories::BaseDirs::new()
            .expect("BaseDirs::new must succeed on supported OS");
        dirs.config_dir().join("winbridge").join("credentials")
    }

    pub fn load_from(path: &Path) -> WinbridgeResult<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::CredentialsMissing { path: path.display().to_string() }
            } else {
                ConfigError::ParseError {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                }
            }
        })?;

        let mut admin_password: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("WINBRIDGE_ADMIN_PASSWORD=") {
                admin_password = Some(rest.trim().trim_matches('"').to_string());
            }
        }

        let admin_password = admin_password.ok_or_else(|| ConfigError::PasswordMissing {
            path: path.display().to_string(),
        })?;

        Ok(Self {
            admin_password,
            vm_name: std::env::var("WINBRIDGE_VM_NAME")
                .unwrap_or_else(|_| Self::DEFAULT_VM_NAME.into()),
            vm_ip: std::env::var("WINBRIDGE_VM_IP")
                .unwrap_or_else(|_| Self::DEFAULT_VM_IP.into()),
            libvirt_uri: std::env::var("WINBRIDGE_LIBVIRT_URI")
                .unwrap_or_else(|_| Self::DEFAULT_LIBVIRT_URI.into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_password_from_credentials_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "# winbridge credentials").unwrap();
        writeln!(tmp, "WINBRIDGE_ADMIN_PASSWORD=deadbeef1234567890abcdef").unwrap();

        let cfg = WinbridgeConfig::load_from(tmp.path()).unwrap();
        assert_eq!(cfg.admin_password, "deadbeef1234567890abcdef");
        assert_eq!(cfg.vm_name, WinbridgeConfig::DEFAULT_VM_NAME);
    }

    #[test]
    fn missing_file_returns_credentials_missing() {
        let path = Path::new("/nonexistent/winbridge/credentials");
        let err = WinbridgeConfig::load_from(path).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("install.sh"));
    }

    #[test]
    fn missing_password_field_returns_password_missing() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "OTHER_FIELD=foo").unwrap();
        let err = WinbridgeConfig::load_from(tmp.path()).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("WINBRIDGE_ADMIN_PASSWORD"));
    }

    #[test]
    fn quoted_password_is_unquoted() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "WINBRIDGE_ADMIN_PASSWORD=\"abc123\"").unwrap();
        let cfg = WinbridgeConfig::load_from(tmp.path()).unwrap();
        assert_eq!(cfg.admin_password, "abc123");
    }
}
