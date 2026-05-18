use crate::desktop;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Category, Icon, Tray};
use std::sync::Arc;

pub struct WinbridgeTray {
    pub on_open_winbridge: Arc<dyn Fn() + Send + Sync>,
    pub on_open_desktop: Arc<dyn Fn() + Send + Sync>,
    pub on_repair_winbridge: Arc<dyn Fn() + Send + Sync>,
    pub on_repair_wallpaper: Arc<dyn Fn() + Send + Sync>,
    pub on_pause: Arc<dyn Fn() + Send + Sync>,
    pub on_shutdown: Arc<dyn Fn() + Send + Sync>,
    pub on_quit: Arc<dyn Fn() + Send + Sync>,
}

pub struct WinbridgeAppTray {
    pub on_open: Arc<dyn Fn() + Send + Sync>,
    pub on_repair: Arc<dyn Fn() + Send + Sync>,
    pub on_quit: Arc<dyn Fn() + Send + Sync>,
}

impl Tray for WinbridgeTray {
    fn icon_name(&self) -> String {
        "computer".to_string()
    }

    fn title(&self) -> String {
        "winbridge - Windows VM".to_string()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open_winbridge)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open Windows Desktop".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open_desktop)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Repair Winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_repair_winbridge)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Repair Wallpaper".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_repair_wallpaper)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Pause VM (managed save)".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_pause)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Shutdown VM".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_shutdown)()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Pause VM and Quit winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl Tray for WinbridgeAppTray {
    fn category(&self) -> Category {
        Category::Communications
    }

    fn id(&self) -> String {
        desktop::WINBRIDGE_ICON_NAME.to_string()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        winbridge_icon_pixmap().into_iter().collect()
    }

    fn title(&self) -> String {
        "winbridge".to_string()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.on_open)();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Repair Winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_repair)()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Pause VM and Quit winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn spawn_tray(tray: WinbridgeTray) -> ksni::Handle<WinbridgeTray> {
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    handle
}

pub fn spawn_winbridge_app_tray(tray: WinbridgeAppTray) -> ksni::Handle<WinbridgeAppTray> {
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    handle
}

fn winbridge_icon_pixmap() -> Option<Icon> {
    let image = image::load_from_memory(desktop::WINBRIDGE_ICON_PNG)
        .ok()?
        .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut data = Vec::with_capacity(64 * 64 * 4);
    for pixel in image.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        data.extend_from_slice(&[alpha, red, green, blue]);
    }

    Some(Icon {
        width: 64,
        height: 64,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_tray_uses_winbridge_icon_and_title() {
        let tray = WinbridgeAppTray {
            on_open: Arc::new(|| {}),
            on_repair: Arc::new(|| {}),
            on_quit: Arc::new(|| {}),
        };

        assert_eq!(tray.icon_name(), "");
        assert_eq!(tray.title(), "winbridge");
        assert_eq!(tray.id(), "winbridge");
        assert_eq!(tray.category(), ksni::Category::Communications);
        assert!(!tray.icon_pixmap().is_empty());
    }

    #[test]
    fn winbridge_app_tray_menu_can_quit_winbridge() {
        let tray = WinbridgeAppTray {
            on_open: Arc::new(|| {}),
            on_repair: Arc::new(|| {}),
            on_quit: Arc::new(|| {}),
        };
        let labels: Vec<_> = tray
            .menu()
            .into_iter()
            .filter_map(|item| match item {
                MenuItem::Standard(item) => Some(item.label),
                _ => None,
            })
            .collect();

        assert_eq!(
            labels,
            vec![
                "Open Winbridge",
                "Repair Winbridge",
                "Pause VM and Quit winbridge"
            ]
        );
    }

    #[test]
    fn winbridge_tray_menu_can_repair_winbridge() {
        let tray = WinbridgeTray {
            on_open_winbridge: Arc::new(|| {}),
            on_open_desktop: Arc::new(|| {}),
            on_repair_winbridge: Arc::new(|| {}),
            on_repair_wallpaper: Arc::new(|| {}),
            on_pause: Arc::new(|| {}),
            on_shutdown: Arc::new(|| {}),
            on_quit: Arc::new(|| {}),
        };
        let labels: Vec<_> = tray
            .menu()
            .into_iter()
            .filter_map(|item| match item {
                MenuItem::Standard(item) => Some(item.label),
                _ => None,
            })
            .collect();

        assert!(labels.contains(&"Repair Winbridge".to_string()));
        assert!(labels.contains(&"Repair Wallpaper".to_string()));
    }
}
