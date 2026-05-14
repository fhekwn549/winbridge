use crate::desktop;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Category, Icon, Tray};
use std::sync::Arc;

pub struct WinbridgeTray {
    pub on_open_kakao: Arc<dyn Fn() + Send + Sync>,
    pub on_open_desktop: Arc<dyn Fn() + Send + Sync>,
    pub on_pause: Arc<dyn Fn() + Send + Sync>,
    pub on_shutdown: Arc<dyn Fn() + Send + Sync>,
    pub on_quit: Arc<dyn Fn() + Send + Sync>,
}

pub struct KakaoTalkTray {
    pub on_open: Arc<dyn Fn() + Send + Sync>,
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
                label: "Open KakaoTalk".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open_kakao)()),
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
                label: "Quit winbridge".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl Tray for KakaoTalkTray {
    fn category(&self) -> Category {
        Category::Communications
    }

    fn id(&self) -> String {
        desktop::KAKAOTALK_ICON_NAME.to_string()
    }

    fn icon_name(&self) -> String {
        desktop::KAKAOTALK_ICON_NAME.to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        kakaotalk_icon_pixmap().into_iter().collect()
    }

    fn title(&self) -> String {
        "KakaoTalk".to_string()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.on_open)();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open KakaoTalk".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.on_open)()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit winbridge".to_string(),
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

pub fn spawn_kakaotalk_tray(tray: KakaoTalkTray) -> ksni::Handle<KakaoTalkTray> {
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    handle
}

fn kakaotalk_icon_pixmap() -> Option<Icon> {
    let image = image::load_from_memory(desktop::KAKAOTALK_ICON_PNG)
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
    fn kakaotalk_tray_uses_dedicated_icon_and_title() {
        let tray = KakaoTalkTray {
            on_open: Arc::new(|| {}),
            on_quit: Arc::new(|| {}),
        };

        assert_eq!(tray.icon_name(), "winbridge-kakaotalk");
        assert_eq!(tray.title(), "KakaoTalk");
        assert_eq!(tray.id(), "winbridge-kakaotalk");
        assert_eq!(tray.category(), ksni::Category::Communications);
        assert!(!tray.icon_pixmap().is_empty());
    }

    #[test]
    fn kakaotalk_tray_menu_can_quit_winbridge() {
        let tray = KakaoTalkTray {
            on_open: Arc::new(|| {}),
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

        assert_eq!(labels, vec!["Open KakaoTalk", "Quit winbridge"]);
    }
}
