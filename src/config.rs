use crate::error::{ConfigError, WinbridgeResult};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WinbridgeConfig {
    pub admin_password: String,
    pub vm_name: String,
    pub vm_ip: String,
    pub libvirt_uri: String,
    pub lifecycle: LifecycleConfig,
}

impl WinbridgeConfig {
    pub const DEFAULT_VM_NAME: &'static str = "winbridge-srv2022";
    pub const DEFAULT_VM_IP: &'static str = "192.168.122.50";
    pub const DEFAULT_LIBVIRT_URI: &'static str = "qemu:///system";

    /// `~/.config/winbridge/credentials` (또는 env override) 에서 읽어 들임.
    pub fn load() -> WinbridgeResult<Self> {
        let credentials_path = Self::credentials_path();
        let config_path = Self::config_path();
        Self::load_from_paths(&credentials_path, Some(&config_path))
    }

    pub fn credentials_path() -> PathBuf {
        if let Ok(p) = std::env::var("WINBRIDGE_CREDENTIALS_FILE") {
            return PathBuf::from(p);
        }
        let dirs =
            directories::BaseDirs::new().expect("BaseDirs::new must succeed on supported OS");
        dirs.config_dir().join("winbridge").join("credentials")
    }

    pub fn config_path() -> PathBuf {
        if let Ok(p) = std::env::var("WINBRIDGE_CONFIG_FILE") {
            return PathBuf::from(p);
        }
        let dirs =
            directories::BaseDirs::new().expect("BaseDirs::new must succeed on supported OS");
        dirs.config_dir().join("winbridge").join("config.toml")
    }

    pub fn load_from(path: &Path) -> WinbridgeResult<Self> {
        Self::load_from_paths(path, None)
    }

    pub fn load_from_paths(
        credentials_path: &Path,
        config_path: Option<&Path>,
    ) -> WinbridgeResult<Self> {
        let text = std::fs::read_to_string(credentials_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::CredentialsMissing {
                    path: credentials_path.display().to_string(),
                }
            } else {
                ConfigError::ParseError {
                    path: credentials_path.display().to_string(),
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
            path: credentials_path.display().to_string(),
        })?;
        let lifecycle = match config_path {
            Some(path) => LifecycleConfig::load_from(path)?,
            None => LifecycleConfig::default(),
        };

        Ok(Self {
            admin_password,
            vm_name: std::env::var("WINBRIDGE_VM_NAME")
                .unwrap_or_else(|_| Self::DEFAULT_VM_NAME.into()),
            vm_ip: std::env::var("WINBRIDGE_VM_IP").unwrap_or_else(|_| Self::DEFAULT_VM_IP.into()),
            libvirt_uri: std::env::var("WINBRIDGE_LIBVIRT_URI")
                .unwrap_or_else(|_| Self::DEFAULT_LIBVIRT_URI.into()),
            lifecycle,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleConfig {
    pub close_window: CloseWindowPolicy,
    pub quit: QuitPolicy,
    pub idle_timeout_minutes: Option<u64>,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            close_window: CloseWindowPolicy::KeepRunning,
            quit: QuitPolicy::ManagedSave,
            idle_timeout_minutes: None,
        }
    }
}

impl LifecycleConfig {
    fn load_from(path: &Path) -> WinbridgeResult<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(err) => {
                return Err(ConfigError::ParseError {
                    path: path.display().to_string(),
                    reason: err.to_string(),
                }
                .into());
            }
        };

        let file: FileConfig = toml::from_str(&text).map_err(|err| ConfigError::ParseError {
            path: path.display().to_string(),
            reason: err.to_string(),
        })?;
        let mut lifecycle = Self::default();
        if let Some(config) = file.lifecycle {
            if let Some(close_window) = config.close_window {
                lifecycle.close_window = close_window;
            }
            if let Some(quit) = config.quit {
                lifecycle.quit = quit;
            }
            if config.idle_timeout_minutes.is_some() {
                lifecycle.idle_timeout_minutes = config.idle_timeout_minutes;
            }
        }
        Ok(lifecycle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CloseWindowPolicy {
    KeepRunning,
    ManagedSave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuitPolicy {
    ManagedSave,
    KeepRunning,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    lifecycle: Option<LifecycleFileConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct LifecycleFileConfig {
    close_window: Option<CloseWindowPolicy>,
    quit: Option<QuitPolicy>,
    idle_timeout_minutes: Option<u64>,
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
        assert_eq!(cfg.lifecycle, LifecycleConfig::default());
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

    #[test]
    fn parses_lifecycle_config_from_toml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "WINBRIDGE_ADMIN_PASSWORD=dummy").unwrap();
        let mut app_config = tempfile::NamedTempFile::new().unwrap();
        writeln!(app_config, "[lifecycle]").unwrap();
        writeln!(app_config, "close-window = \"managed-save\"").unwrap();
        writeln!(app_config, "quit = \"keep-running\"").unwrap();
        writeln!(app_config, "idle-timeout-minutes = 20").unwrap();

        let cfg = WinbridgeConfig::load_from_paths(tmp.path(), Some(app_config.path())).unwrap();

        assert_eq!(cfg.lifecycle.close_window, CloseWindowPolicy::ManagedSave);
        assert_eq!(cfg.lifecycle.quit, QuitPolicy::KeepRunning);
        assert_eq!(cfg.lifecycle.idle_timeout_minutes, Some(20));
    }

    #[test]
    fn missing_lifecycle_config_uses_defaults() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "WINBRIDGE_ADMIN_PASSWORD=dummy").unwrap();

        let cfg = WinbridgeConfig::load_from_paths(
            tmp.path(),
            Some(Path::new("/nonexistent/winbridge/config.toml")),
        )
        .unwrap();

        assert_eq!(cfg.lifecycle, LifecycleConfig::default());
    }
}
