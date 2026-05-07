use crate::error::{WinbridgeError, WinbridgeResult};
use std::path::{Path, PathBuf};

pub const KAKAOTALK_APPLICATION_ID: &str = "dev.winbridge.KakaoTalk";
pub const KAKAOTALK_DESKTOP_FILE_NAME: &str = "dev.winbridge.KakaoTalk.desktop";
pub const KAKAOTALK_ICON_NAME: &str = "winbridge-kakaotalk";

pub(crate) const KAKAOTALK_ICON_PNG: &[u8] =
    include_bytes!("../assets/icons/winbridge-kakaotalk.png");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDesktopEntry {
    pub desktop_entry_path: PathBuf,
    pub icon_path: PathBuf,
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

pub fn install_kakaotalk_desktop_entry(
    winbridge_executable: &Path,
) -> WinbridgeResult<InstalledDesktopEntry> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Err(WinbridgeError::Other(anyhow::anyhow!(
            "사용자 데이터 디렉터리를 확인할 수 없습니다"
        )));
    };
    install_kakaotalk_desktop_entry_in(base_dirs.data_local_dir(), winbridge_executable)
}

pub fn install_kakaotalk_desktop_entry_in(
    data_local_dir: &Path,
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

    if let Some(parent) = desktop_entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(
        &desktop_entry_path,
        kakaotalk_desktop_entry(winbridge_executable),
    )?;
    std::fs::write(&icon_path, KAKAOTALK_ICON_PNG)?;

    Ok(InstalledDesktopEntry {
        desktop_entry_path,
        icon_path,
    })
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
}
