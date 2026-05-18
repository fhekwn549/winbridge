use crate::error::{WinbridgeError, WinbridgeResult};
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::png::PngEncoder;
use image::ImageFormat;
use image::{ExtendedColorType, ImageEncoder};
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const WINBRIDGE_APP_APPLICATION_ID: &str = "dev.winbridge.WinbridgeApp";
pub const WINBRIDGE_AUTOSTART_FILE_NAME: &str = "dev.winbridge.WinbridgeApp.desktop";
pub const LEGACY_KAKAOTALK_COMMAND_NAME: &str = "kakaotalk";
pub const WINBRIDGE_DESKTOP_FILE_NAME: &str = "dev.winbridge.WinbridgeApp.desktop";
pub const WINBRIDGE_ICON_NAME: &str = "winbridge";
const LEGACY_WINBRIDGE_AUTOSTART_FILE_NAME: &str = "dev.winbridge.Winbridge.desktop";
const LEGACY_WINBRIDGE_DESKTOP_FILE_NAME: &str = "dev.winbridge.Winbridge.desktop";
const LEGACY_KAKAOTALK_AUTOSTART_FILE_NAME: &str = "dev.winbridge.KakaoTalk.desktop";
const LEGACY_KAKAOTALK_DESKTOP_FILE_NAME: &str = "dev.winbridge.KakaoTalk.desktop";
const LEGACY_KAKAOTALK_ICON_NAME: &str = "winbridge-kakaotalk";

pub(crate) const WINBRIDGE_ICON_PNG: &[u8] =
    include_bytes!("../assets/icons/winbridge.png");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDesktopEntry {
    pub desktop_entry_path: PathBuf,
    pub icon_path: PathBuf,
    pub autostart_entry_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstalledDesktopEntry {
    pub removed_paths: Vec<PathBuf>,
    pub missing_paths: Vec<PathBuf>,
}

pub fn winbridge_desktop_entry(winbridge_executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=winbridge\n\
Comment=Open Winbridge through the Windows VM\n\
Exec={} start --mode app --display stable-slots\n\
Icon={WINBRIDGE_ICON_NAME}\n\
Terminal=false\n\
Categories=Network;InstantMessaging;\n\
StartupNotify=true\n\
StartupWMClass={WINBRIDGE_APP_APPLICATION_ID}\n",
        quote_exec_path(winbridge_executable)
    )
}

pub fn winbridge_autostart_entry(winbridge_executable: &Path) -> String {
    format!(
        "{}X-GNOME-Autostart-enabled=true\n",
        winbridge_desktop_entry(winbridge_executable)
    )
}

pub fn install_winbridge_desktop_entry(
    winbridge_executable: &Path,
) -> WinbridgeResult<InstalledDesktopEntry> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Err(WinbridgeError::Other(anyhow::anyhow!(
            "사용자 데이터 디렉터리를 확인할 수 없습니다"
        )));
    };
    install_winbridge_desktop_entry_in(
        base_dirs.data_local_dir(),
        base_dirs.config_dir(),
        base_dirs.executable_dir(),
        winbridge_executable,
    )
}

pub fn uninstall_winbridge_desktop_entry() -> WinbridgeResult<UninstalledDesktopEntry> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Err(WinbridgeError::Other(anyhow::anyhow!(
            "사용자 데이터 디렉터리를 확인할 수 없습니다"
        )));
    };
    uninstall_winbridge_desktop_entry_in(
        base_dirs.data_local_dir(),
        base_dirs.config_dir(),
        base_dirs.executable_dir(),
    )
}

pub fn install_winbridge_desktop_entry_in(
    data_local_dir: &Path,
    config_dir: &Path,
    executable_dir: Option<&Path>,
    winbridge_executable: &Path,
) -> WinbridgeResult<InstalledDesktopEntry> {
    let desktop_entry_path = data_local_dir
        .join("applications")
        .join(WINBRIDGE_DESKTOP_FILE_NAME);
    let icon_path = data_local_dir
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join(format!("{WINBRIDGE_ICON_NAME}.png"));
    let autostart_entry_path = config_dir
        .join("autostart")
        .join(WINBRIDGE_AUTOSTART_FILE_NAME);

    if let Some(parent) = desktop_entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = autostart_entry_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    remove_legacy_desktop_entry_outputs(data_local_dir, config_dir)?;
    if let Some(executable_dir) = executable_dir {
        remove_if_exists(&executable_dir.join(LEGACY_KAKAOTALK_COMMAND_NAME))?;
    }

    std::fs::write(
        &desktop_entry_path,
        winbridge_desktop_entry(winbridge_executable),
    )?;
    std::fs::write(&icon_path, installed_winbridge_icon_png()?)?;
    std::fs::write(
        &autostart_entry_path,
        winbridge_autostart_entry(winbridge_executable),
    )?;

    Ok(InstalledDesktopEntry {
        desktop_entry_path,
        icon_path,
        autostart_entry_path,
    })
}

pub fn uninstall_winbridge_desktop_entry_in(
    data_local_dir: &Path,
    config_dir: &Path,
    executable_dir: Option<&Path>,
) -> WinbridgeResult<UninstalledDesktopEntry> {
    let mut paths = vec![
        data_local_dir
            .join("applications")
            .join(WINBRIDGE_DESKTOP_FILE_NAME),
        data_local_dir
            .join("applications")
            .join(LEGACY_WINBRIDGE_DESKTOP_FILE_NAME),
        data_local_dir
            .join("applications")
            .join(LEGACY_KAKAOTALK_DESKTOP_FILE_NAME),
        data_local_dir
            .join("icons")
            .join("hicolor")
            .join("256x256")
            .join("apps")
            .join(format!("{WINBRIDGE_ICON_NAME}.png")),
        data_local_dir
            .join("icons")
            .join("hicolor")
            .join("256x256")
            .join("apps")
            .join(format!("{LEGACY_KAKAOTALK_ICON_NAME}.png")),
        config_dir
            .join("autostart")
            .join(WINBRIDGE_AUTOSTART_FILE_NAME),
        config_dir
            .join("autostart")
            .join(LEGACY_WINBRIDGE_AUTOSTART_FILE_NAME),
        config_dir
            .join("autostart")
            .join(LEGACY_KAKAOTALK_AUTOSTART_FILE_NAME),
    ];
    if let Some(executable_dir) = executable_dir {
        paths.push(executable_dir.join(LEGACY_KAKAOTALK_COMMAND_NAME));
    }

    let mut removed_paths = Vec::new();
    let mut missing_paths = Vec::new();
    for path in paths {
        if path.exists() {
            std::fs::remove_file(&path)?;
            removed_paths.push(path);
        } else {
            missing_paths.push(path);
        }
    }

    Ok(UninstalledDesktopEntry {
        removed_paths,
        missing_paths,
    })
}

fn remove_legacy_desktop_entry_outputs(
    data_local_dir: &Path,
    config_dir: &Path,
) -> std::io::Result<()> {
    for path in [
        data_local_dir
            .join("applications")
            .join(LEGACY_KAKAOTALK_DESKTOP_FILE_NAME),
        data_local_dir
            .join("applications")
            .join(LEGACY_WINBRIDGE_DESKTOP_FILE_NAME),
        data_local_dir
            .join("icons")
            .join("hicolor")
            .join("256x256")
            .join("apps")
            .join(format!("{LEGACY_KAKAOTALK_ICON_NAME}.png")),
        config_dir
            .join("autostart")
            .join(LEGACY_KAKAOTALK_AUTOSTART_FILE_NAME),
        config_dir
            .join("autostart")
            .join(LEGACY_WINBRIDGE_AUTOSTART_FILE_NAME),
    ] {
        remove_if_exists(&path)?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
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

pub(crate) fn installed_winbridge_icon_png() -> WinbridgeResult<Vec<u8>> {
    let image = image::load_from_memory(WINBRIDGE_ICON_PNG)
        .map_err(|err| WinbridgeError::Other(anyhow::anyhow!("아이콘 PNG 로드 실패: {err}")))?
        .resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|err| WinbridgeError::Other(anyhow::anyhow!("아이콘 PNG 변환 실패: {err}")))?;
    Ok(bytes.into_inner())
}

pub fn installed_winbridge_icon_ico() -> WinbridgeResult<Vec<u8>> {
    let source = image::load_from_memory(WINBRIDGE_ICON_PNG)
        .map_err(|err| WinbridgeError::Other(anyhow::anyhow!("아이콘 PNG 로드 실패: {err}")))?;
    let mut frames = Vec::new();

    for size in [16_u32, 32, 48, 64, 128, 256] {
        let resized = source
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&resized, size, size, ExtendedColorType::Rgba8)
            .map_err(|err| WinbridgeError::Other(anyhow::anyhow!("아이콘 PNG 변환 실패: {err}")))?;
        frames.push(
            IcoFrame::with_encoded(png, size, size, ExtendedColorType::Rgba8).map_err(|err| {
                WinbridgeError::Other(anyhow::anyhow!("아이콘 ICO 프레임 생성 실패: {err}"))
            })?,
        );
    }

    let mut bytes = Vec::new();
    IcoEncoder::new(&mut bytes)
        .encode_images(&frames)
        .map_err(|err| WinbridgeError::Other(anyhow::anyhow!("아이콘 ICO 변환 실패: {err}")))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_uses_winbridge_identity_and_icon() {
        let entry = winbridge_desktop_entry(Path::new("/opt/winbridge/bin/winbridge"));

        assert!(entry.contains("Name=winbridge"));
        assert!(entry.contains("Icon=winbridge"));
        assert!(entry.contains("StartupWMClass=dev.winbridge.WinbridgeApp"));
        assert!(entry.contains(
            "Exec=\"/opt/winbridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn desktop_entry_quotes_executable_paths_with_spaces() {
        let entry = winbridge_desktop_entry(Path::new("/opt/win bridge/bin/winbridge"));

        assert!(entry.contains(
            "Exec=\"/opt/win bridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn autostart_entry_launches_winbridge_on_login() {
        let entry = winbridge_autostart_entry(Path::new("/opt/winbridge/bin/winbridge"));

        assert!(entry.contains("Name=winbridge"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(entry.contains(
            "Exec=\"/opt/winbridge/bin/winbridge\" start --mode app --display stable-slots"
        ));
    }

    #[test]
    fn installer_removes_legacy_terminal_command() {
        let tmp = tempfile::tempdir().unwrap();
        let data_local_dir = tmp.path().join("share");
        let config_dir = tmp.path().join("config");
        let executable_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let legacy_command = executable_dir.join("kakaotalk");
        std::fs::write(&legacy_command, "old").unwrap();
        let installed = install_winbridge_desktop_entry_in(
            &data_local_dir,
            &config_dir,
            Some(&executable_dir),
            Path::new("/opt/winbridge/bin/winbridge"),
        )
        .unwrap();

        assert!(!legacy_command.exists());

        assert_eq!(
            installed.autostart_entry_path,
            config_dir
                .join("autostart")
                .join("dev.winbridge.WinbridgeApp.desktop")
        );
        let autostart = std::fs::read_to_string(installed.autostart_entry_path).unwrap();
        assert!(autostart.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn installer_writes_real_256px_icon_for_hicolor_theme() {
        let tmp = tempfile::tempdir().unwrap();
        let data_local_dir = tmp.path().join("share");
        let config_dir = tmp.path().join("config");
        let executable_dir = tmp.path().join("bin");
        let installed = install_winbridge_desktop_entry_in(
            &data_local_dir,
            &config_dir,
            Some(&executable_dir),
            Path::new("/opt/winbridge/bin/winbridge"),
        )
        .unwrap();

        let icon = image::open(installed.icon_path).unwrap();
        assert_eq!(icon.width(), 256);
        assert_eq!(icon.height(), 256);
    }

    #[test]
    fn installer_can_write_windows_ico_for_forwarder() {
        let ico = installed_winbridge_icon_ico().unwrap();
        let format = image::guess_format(&ico).unwrap();

        assert_eq!(format, ImageFormat::Ico);
        assert!(ico.len() > 1024);
    }

    #[test]
    fn uninstaller_removes_desktop_entry_outputs() {
        let tmp = tempfile::tempdir().unwrap();
        let data_local_dir = tmp.path().join("share");
        let config_dir = tmp.path().join("config");
        let executable_dir = tmp.path().join("bin");
        let installed = install_winbridge_desktop_entry_in(
            &data_local_dir,
            &config_dir,
            Some(&executable_dir),
            Path::new("/opt/winbridge/bin/winbridge"),
        )
        .unwrap();

        let uninstalled = uninstall_winbridge_desktop_entry_in(
            &data_local_dir,
            &config_dir,
            Some(&executable_dir),
        )
        .unwrap();

        assert_eq!(uninstalled.removed_paths.len(), 3);
        assert_eq!(uninstalled.missing_paths.len(), 6);
        assert!(!installed.desktop_entry_path.exists());
        assert!(!installed.icon_path.exists());
        assert!(!installed.autostart_entry_path.exists());
    }

    #[test]
    fn uninstaller_reports_missing_outputs_without_failing() {
        let tmp = tempfile::tempdir().unwrap();
        let uninstalled = uninstall_winbridge_desktop_entry_in(
            &tmp.path().join("share"),
            &tmp.path().join("config"),
            Some(&tmp.path().join("bin")),
        )
        .unwrap();

        assert!(uninstalled.removed_paths.is_empty());
        assert_eq!(uninstalled.missing_paths.len(), 9);
    }
}
