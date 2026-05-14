use crate::error::{WinbridgeError, WinbridgeResult};
use std::path::{Path, PathBuf};

pub const KAKAOTALK_APPLICATION_ID: &str = "dev.winbridge.KakaoTalk";
pub const KAKAOTALK_AUTOSTART_FILE_NAME: &str = "dev.winbridge.KakaoTalk.desktop";
pub const KAKAOTALK_COMMAND_NAME: &str = "kakaotalk";
pub const KAKAOTALK_DESKTOP_FILE_NAME: &str = "dev.winbridge.KakaoTalk.desktop";
pub const KAKAOTALK_ICON_NAME: &str = "winbridge-kakaotalk";

pub(crate) const KAKAOTALK_ICON_PNG: &[u8] =
    include_bytes!("../assets/icons/winbridge-kakaotalk.png");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDesktopEntry {
    pub desktop_entry_path: PathBuf,
    pub icon_path: PathBuf,
    pub command_path: PathBuf,
    pub autostart_entry_path: PathBuf,
}

pub fn kakaotalk_desktop_entry(winbridge_executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=KakaoTalk\n\
Comment=Open Windows KakaoTalk through winbridge\n\
Exec={} start --mode app --display stable-slots\n\
Icon={KAKAOTALK_ICON_NAME}\n\
Terminal=false\n\
Categories=Network;InstantMessaging;\n\
StartupNotify=true\n\
StartupWMClass={KAKAOTALK_APPLICATION_ID}\n",
        quote_exec_path(winbridge_executable)
    )
}

pub fn kakaotalk_autostart_entry(winbridge_executable: &Path) -> String {
    format!(
        "{}X-GNOME-Autostart-enabled=true\n",
        kakaotalk_desktop_entry(winbridge_executable)
    )
}

pub fn install_kakaotalk_desktop_entry(
    winbridge_executable: &Path,
) -> WinbridgeResult<InstalledDesktopEntry> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Err(WinbridgeError::Other(anyhow::anyhow!(
            "사용자 데이터 디렉터리를 확인할 수 없습니다"
        )));
    };
    install_kakaotalk_desktop_entry_in(
        base_dirs.data_local_dir(),
        base_dirs.config_dir(),
        base_dirs.executable_dir(),
        winbridge_executable,
    )
}

pub fn install_kakaotalk_desktop_entry_in(
    data_local_dir: &Path,
    config_dir: &Path,
    executable_dir: Option<&Path>,
    winbridge_executable: &Path,
) -> WinbridgeResult<InstalledDesktopEntry> {
    let desktop_entry_path = data_local_dir
        .join("applications")
        .join(KAKAOTALK_DESKTOP_FILE_NAME);
    let icon_path = data_local_dir
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join(format!("{KAKAOTALK_ICON_NAME}.png"));
    let Some(executable_dir) = executable_dir else {
        return Err(WinbridgeError::Other(anyhow::anyhow!(
            "사용자 실행 파일 디렉터리를 확인할 수 없습니다"
        )));
    };
    let command_path = executable_dir.join(KAKAOTALK_COMMAND_NAME);
    let autostart_entry_path = config_dir
        .join("autostart")
        .join(KAKAOTALK_AUTOSTART_FILE_NAME);

    if let Some(parent) = desktop_entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = command_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = autostart_entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(
        &desktop_entry_path,
        kakaotalk_desktop_entry(winbridge_executable),
    )?;
    std::fs::write(&icon_path, KAKAOTALK_ICON_PNG)?;
    std::fs::write(&command_path, kakaotalk_command(winbridge_executable))?;
    set_executable(&command_path)?;
    std::fs::write(
        &autostart_entry_path,
        kakaotalk_autostart_entry(winbridge_executable),
    )?;

    Ok(InstalledDesktopEntry {
        desktop_entry_path,
        icon_path,
        command_path,
        autostart_entry_path,
    })
}

pub fn kakaotalk_command(winbridge_executable: &Path) -> String {
    format!(
        "#!/usr/bin/env sh\nexec {} start --mode app --display stable-slots\n",
        shell_quote_path(winbridge_executable)
    )
}

fn quote_exec_path(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

fn shell_quote_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_uses_kakaotalk_identity_and_icon() {
        let entry = kakaotalk_desktop_entry(Path::new("/opt/winbridge/bin/winbridge"));

        assert!(entry.contains("Name=KakaoTalk"));
        assert!(entry.contains("Icon=winbridge-kakaotalk"));
        assert!(entry.contains("StartupWMClass=dev.winbridge.KakaoTalk"));
        assert!(entry.contains(
            "Exec=\"/opt/winbridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn desktop_entry_quotes_executable_paths_with_spaces() {
        let entry = kakaotalk_desktop_entry(Path::new("/opt/win bridge/bin/winbridge"));

        assert!(entry.contains(
            "Exec=\"/opt/win bridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn autostart_entry_launches_kakaotalk_on_login() {
        let entry = kakaotalk_autostart_entry(Path::new("/opt/winbridge/bin/winbridge"));

        assert!(entry.contains("Name=KakaoTalk"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(entry.contains(
            "Exec=\"/opt/winbridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn terminal_command_launches_kakaotalk_app_mode() {
        let command = kakaotalk_command(Path::new("/opt/win bridge/bin/winbridge"));

        assert_eq!(
            command,
            "#!/usr/bin/env sh\nexec '/opt/win bridge/bin/winbridge' start --mode app --display stable-slots\n"
        );
    }

    #[test]
    fn terminal_command_quotes_single_quotes() {
        let command = kakaotalk_command(Path::new("/opt/win'bridge/bin/winbridge"));

        assert!(command.contains("exec '/opt/win'\\''bridge/bin/winbridge' start"));
    }

    #[test]
    fn installer_writes_terminal_command_next_to_user_executables() {
        let tmp = tempfile::tempdir().unwrap();
        let data_local_dir = tmp.path().join("share");
        let config_dir = tmp.path().join("config");
        let executable_dir = tmp.path().join("bin");
        let installed = install_kakaotalk_desktop_entry_in(
            &data_local_dir,
            &config_dir,
            Some(&executable_dir),
            Path::new("/opt/winbridge/bin/winbridge"),
        )
        .unwrap();

        assert_eq!(installed.command_path, executable_dir.join("kakaotalk"));
        let command = std::fs::read_to_string(installed.command_path).unwrap();
        assert!(command.contains("exec '/opt/winbridge/bin/winbridge' start --mode app"));

        assert_eq!(
            installed.autostart_entry_path,
            config_dir
                .join("autostart")
                .join("dev.winbridge.KakaoTalk.desktop")
        );
        let autostart = std::fs::read_to_string(installed.autostart_entry_path).unwrap();
        assert!(autostart.contains("X-GNOME-Autostart-enabled=true"));
    }
}
