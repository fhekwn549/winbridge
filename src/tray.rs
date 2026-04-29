use ksni::menu::{MenuItem, StandardItem};
use ksni::Tray;
use std::sync::Arc;

pub struct WinbridgeTray {
    pub on_open_kakao: Arc<dyn Fn() + Send + Sync>,
    pub on_pause: Arc<dyn Fn() + Send + Sync>,
    pub on_shutdown: Arc<dyn Fn() + Send + Sync>,
    pub on_quit: Arc<dyn Fn() + Send + Sync>,
}

impl Tray for WinbridgeTray {
    fn icon_name(&self) -> String {
        "winbridge".to_string()
    }

    fn title(&self) -> String {
        "winbridge - KakaoTalk".to_string()
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

pub fn spawn_tray(tray: WinbridgeTray) -> ksni::Handle<WinbridgeTray> {
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    handle
}
