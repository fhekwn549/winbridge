use crate::error::{RdpError, WinbridgeResult};
use glib::translate::IntoGlib;
use gtk4 as gtk;
use gtk4::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

type RdpTlsStream = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;
type RdpFramed = ironrdp_tokio::TokioFramed<RdpTlsStream>;
const RDP_RECONNECT_ATTEMPTS: usize = 3;
const RDP_RECONNECT_DELAY: Duration = Duration::from_secs(2);

struct RdpSessionConnection {
    connection_result: ironrdp::connector::ConnectionResult,
    framed: RdpFramed,
}

/// Headless RDP handshake probe. The GTK viewer reuses the same connector path.
#[derive(Debug)]
pub struct RdpHeadlessProbe {
    addr: SocketAddr,
    server_name: String,
    username: String,
    password: String,
}

impl RdpHeadlessProbe {
    pub fn new(vm_ip: &str, port: u16, username: &str, password: &str) -> WinbridgeResult<Self> {
        let addr = format!("{vm_ip}:{port}")
            .parse()
            .map_err(|err: std::net::AddrParseError| RdpError::Handshake(err.to_string()))?;

        Ok(Self {
            addr,
            server_name: vm_ip.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    pub async fn probe(&self) -> WinbridgeResult<RdpProbeResult> {
        let session = self
            .connect(
                None,
                RdpDesktopSize::new(1280, 720),
                None,
                RdpDisplayStrategy::StableSlots,
            )
            .await?;

        Ok(RdpProbeResult {
            width: session.connection_result.desktop_size.width,
            height: session.connection_result.desktop_size.height,
            bits_per_pixel: 32,
        })
    }

    fn build_config(&self) -> ironrdp::connector::Config {
        use ironrdp::connector;
        use ironrdp::connector::Credentials;
        use ironrdp::pdu::gcc::KeyboardType;
        use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
        use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
        connector::Config {
            credentials: Credentials::UsernamePassword {
                username: self.username.clone(),
                password: self.password.clone(),
            },
            domain: None,
            enable_tls: false,
            enable_credssp: true,
            keyboard_type: KeyboardType::IbmEnhanced,
            keyboard_subtype: 0,
            keyboard_layout: 0x0000_0409,
            keyboard_functional_keys_count: 12,
            ime_file_name: String::new(),
            dig_product_id: String::new(),
            desktop_size: connector::DesktopSize {
                width: 1280,
                height: 720,
            },
            bitmap: None,
            client_build: 0,
            client_name: "winbridge".to_string(),
            client_dir: "C:\\Windows\\System32\\mstscax.dll".to_string(),
            platform: MajorPlatformType::UNIX,
            enable_server_pointer: false,
            request_data: None,
            autologon: false,
            enable_audio_playback: false,
            pointer_software_rendering: true,
            performance_flags: PerformanceFlags::default(),
            desktop_scale_factor: 0,
            hardware_id: None,
            license_cache: None,
            timezone_info: TimezoneInfo::default(),
        }
    }

    async fn connect(
        &self,
        clipboard_backend: Option<crate::clipboard::TextClipboardBackend>,
        desktop_size: RdpDesktopSize,
        virtual_desktop_layout: Option<RdpVirtualDesktopLayout>,
        display_strategy: RdpDisplayStrategy,
    ) -> WinbridgeResult<RdpSessionConnection> {
        use ironrdp::connector;
        use ironrdp_tokio::reqwest::ReqwestNetworkClient;
        use tokio::net::TcpStream;

        let stream = TcpStream::connect(self.addr)
            .await
            .map_err(|err| RdpError::Handshake(format!("TCP 연결 실패: {err}")))?;
        let client_addr = stream
            .local_addr()
            .map_err(|err| RdpError::Handshake(format!("local address 확인 실패: {err}")))?;

        let mut config = self.build_config();
        config.desktop_size = connector::DesktopSize {
            width: desktop_size.width,
            height: desktop_size.height,
        };
        let mut framed = ironrdp_tokio::TokioFramed::new(stream);
        let mut connector = connector::ClientConnector::new(config, client_addr);
        if let Some(backend) = clipboard_backend {
            connector
                .attach_static_channel(ironrdp::cliprdr::CliprdrClient::new(Box::new(backend)));
        }
        if display_strategy == RdpDisplayStrategy::ExperimentalMultimon {
            if let Some(layout) = virtual_desktop_layout.map(build_display_control_layout) {
                let display_control =
                    ironrdp::displaycontrol::client::DisplayControlClient::new(move |caps| {
                        tracing::info!(?caps, "RDP DisplayControl capabilities received");
                        let pdu: ironrdp::displaycontrol::pdu::DisplayControlPdu =
                            layout.clone().into();
                        tracing::info!("RDP DisplayControl experimental monitor layout requested");
                        Ok(vec![Box::new(pdu)])
                    });
                connector.attach_static_channel(
                    ironrdp::dvc::DrdynvcClient::new().with_dynamic_channel(display_control),
                );
            }
        }

        let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector)
            .await
            .map_err(|err| RdpError::Handshake(format!("RDP negotiation 실패: {err}")))?;
        let stream = framed.into_inner_no_leftover();

        let (tls_stream, server_public_key) = tls_upgrade(stream, &self.server_name).await?;
        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
        let mut framed = ironrdp_tokio::TokioFramed::new(tls_stream);
        let mut network_client = ReqwestNetworkClient::new();

        let result = ironrdp_tokio::connect_finalize(
            upgraded,
            connector,
            &mut framed,
            &mut network_client,
            self.server_name.clone().into(),
            server_public_key,
            None,
        )
        .await
        .map_err(|err| RdpError::Handshake(format!("RDP finalize 실패: {err}")))?;

        Ok(RdpSessionConnection {
            connection_result: result,
            framed,
        })
    }
}

async fn tls_upgrade(
    stream: tokio::net::TcpStream,
    server_name: &str,
) -> WinbridgeResult<(
    tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    Vec<u8>,
)> {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();

    let mut config = tokio_rustls::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth();
    config.key_log = Arc::new(tokio_rustls::rustls::KeyLogFile::new());
    config.resumption = tokio_rustls::rustls::client::Resumption::disabled();

    let server_name = server_name
        .to_string()
        .try_into()
        .map_err(|err| RdpError::Handshake(format!("TLS server name 오류: {err}")))?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tls_stream = connector
        .connect(server_name, stream)
        .await
        .map_err(|err| RdpError::Handshake(format!("TLS handshake 실패: {err}")))?;

    let cert = tls_stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| RdpError::Handshake("TLS peer certificate 없음".to_string()))?;
    let server_public_key = extract_tls_server_public_key(cert.as_ref())?;

    Ok((tls_stream, server_public_key))
}

fn extract_tls_server_public_key(cert: &[u8]) -> WinbridgeResult<Vec<u8>> {
    use x509_cert::der::Decode as _;

    let cert = x509_cert::Certificate::from_der(cert)
        .map_err(|err| RdpError::Handshake(format!("TLS certificate parsing 실패: {err}")))?;
    cert.tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| {
            RdpError::Handshake("TLS public key bit string 정렬 오류".to_string()).into()
        })
}

#[derive(Debug)]
struct NoCertificateVerification;

impl tokio_rustls::rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
        _: &tokio_rustls::rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: tokio_rustls::rustls::pki_types::UnixTime,
    ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error>
    {
        Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        use tokio_rustls::rustls::SignatureScheme;

        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

#[derive(Debug)]
pub struct RdpProbeResult {
    pub width: u16,
    pub height: u16,
    pub bits_per_pixel: u8,
}

pub struct RdpWindow;

#[derive(Clone, Debug)]
pub struct RdpWindowOptions {
    pub title: String,
    pub icon_name: Option<&'static str>,
    pub viewport: RdpViewport,
    pub desktop_size: RdpDesktopSize,
    pub virtual_desktop_layout: Option<RdpVirtualDesktopLayout>,
    pub display_strategy: RdpDisplayStrategy,
}

impl RdpWindowOptions {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            icon_name: None,
            viewport: RdpViewport::new(0, 0, 1280, 720),
            desktop_size: RdpDesktopSize::new(1280, 720),
            virtual_desktop_layout: None,
            display_strategy: RdpDisplayStrategy::StableSlots,
        }
    }

    pub fn kakaotalk_app() -> Self {
        Self {
            title: "winbridge".to_string(),
            icon_name: Some(crate::desktop::WINBRIDGE_ICON_NAME),
            viewport: RdpViewport::new(0, 0, 960, 720),
            desktop_size: RdpDesktopSize::new(960, 720),
            virtual_desktop_layout: None,
            display_strategy: RdpDisplayStrategy::StableSlots,
        }
    }

    pub fn experimental_multimon_desktop() -> Self {
        Self {
            title: "Windows Desktop".to_string(),
            icon_name: None,
            viewport: RdpViewport::new(0, 0, 2560, 720),
            desktop_size: RdpDesktopSize::new(2560, 720),
            virtual_desktop_layout: Some(RdpVirtualDesktopLayout::TwoHorizontalSlots {
                slot_width: 1280,
                slot_height: 720,
            }),
            display_strategy: RdpDisplayStrategy::ExperimentalMultimon,
        }
    }

    pub fn with_display_strategy(mut self, display_strategy: RdpDisplayStrategy) -> Self {
        self.display_strategy = display_strategy;
        self
    }

    pub fn initial_desktop_size(&self) -> (u16, u16) {
        match self.virtual_desktop_layout {
            Some(layout) => initial_desktop_size_for_layout(Some(layout)),
            None => (self.desktop_size.width, self.desktop_size.height),
        }
    }

    pub fn initial_window_size(&self) -> (u16, u16) {
        (self.viewport.width, self.viewport.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdpDisplayStrategy {
    StableSlots,
    ExperimentalMultimon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RdpViewport {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
}

impl RdpViewport {
    pub const fn new(left: u16, top: u16, width: u16, height: u16) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RdpDesktopSize {
    pub width: u16,
    pub height: u16,
}

impl RdpDesktopSize {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdpVirtualDesktopLayout {
    TwoHorizontalSlots { slot_width: u16, slot_height: u16 },
}

fn initial_desktop_size_for_layout(layout: Option<RdpVirtualDesktopLayout>) -> (u16, u16) {
    match layout {
        Some(RdpVirtualDesktopLayout::TwoHorizontalSlots {
            slot_width,
            slot_height,
        }) => (slot_width.saturating_mul(2), slot_height),
        None => (1280, 720),
    }
}

fn build_display_control_layout(
    layout: RdpVirtualDesktopLayout,
) -> ironrdp::displaycontrol::pdu::DisplayControlMonitorLayout {
    match layout {
        RdpVirtualDesktopLayout::TwoHorizontalSlots {
            slot_width,
            slot_height,
        } => {
            let primary = ironrdp::displaycontrol::pdu::MonitorLayoutEntry::new_primary(
                u32::from(slot_width),
                u32::from(slot_height),
            )
            .expect("fixed app monitor dimensions are valid");
            let secondary = ironrdp::displaycontrol::pdu::MonitorLayoutEntry::new_secondary(
                u32::from(slot_width),
                u32::from(slot_height),
            )
            .expect("fixed desktop monitor dimensions are valid")
            .with_position(i32::from(slot_width), 0)
            .expect("secondary monitor position is valid");

            ironrdp::displaycontrol::pdu::DisplayControlMonitorLayout::new(&[primary, secondary])
                .expect("one primary and one secondary monitor is valid")
        }
    }
}

impl RdpWindow {
    pub fn open(
        app: &gtk::Application,
        vm_ip: &str,
        password: &str,
        options: RdpWindowOptions,
        on_close: Arc<dyn Fn() + Send + Sync>,
    ) -> WinbridgeResult<()> {
        let (window_width, window_height) = options.initial_window_size();
        let mut window_builder = gtk::ApplicationWindow::builder()
            .application(app)
            .title(&options.title)
            .default_width(i32::from(window_width))
            .default_height(i32::from(window_height));
        if let Some(icon_name) = options.icon_name {
            window_builder = window_builder.icon_name(icon_name);
        }
        let win = window_builder.build();

        let drawing = gtk::DrawingArea::new();
        drawing.set_hexpand(true);
        drawing.set_vexpand(true);
        win.set_child(Some(&drawing));

        let (tx, rx) = async_channel::unbounded::<RdpFrame>();
        let latest_frame: std::rc::Rc<std::cell::RefCell<Option<RdpFrame>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        {
            let latest_frame = latest_frame.clone();
            let drawing_weak = drawing.downgrade();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(frame) = rx.recv().await {
                    *latest_frame.borrow_mut() = Some(frame);
                    if let Some(drawing) = drawing_weak.upgrade() {
                        drawing.queue_draw();
                    }
                }
            });
        }

        {
            let latest_frame = latest_frame.clone();
            let viewport = options.viewport;
            drawing.set_draw_func(move |_drawing, cr, width, height| {
                if let Some(frame) = latest_frame.borrow().as_ref() {
                    paint_frame_on_cairo(cr, frame, viewport, width, height);
                }
            });
        }

        let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<InputEvent>();
        let (clipboard_backend, clipboard_runtime, clipboard_command_tx, host_clipboard_rx) =
            crate::clipboard::create_text_clipboard_bridge();
        crate::clipboard::install_gtk_text_clipboard_bridge(
            clipboard_command_tx,
            host_clipboard_rx,
        );
        let click_gesture = gtk::GestureClick::new();
        click_gesture.set_button(0);
        let motion_controller = gtk::EventControllerMotion::new();
        let scroll_controller = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );

        install_keyboard_controller(&drawing, input_tx.clone());

        {
            let tx = input_tx.clone();
            let drawing_weak = drawing.downgrade();
            click_gesture.connect_pressed(move |gesture, _n_press, x, y| {
                let (canvas_width, canvas_height) = drawing_weak
                    .upgrade()
                    .map(|drawing| {
                        drawing.grab_focus();
                        (drawing.allocated_width(), drawing.allocated_height())
                    })
                    .unwrap_or((1280, 720));
                let _ = tx.send(InputEvent::MousePress {
                    button: gesture.current_button(),
                    x,
                    y,
                    canvas_width,
                    canvas_height,
                });
            });
        }

        {
            let tx = input_tx.clone();
            let drawing_weak = drawing.downgrade();
            click_gesture.connect_released(move |gesture, _n_press, x, y| {
                let (canvas_width, canvas_height) = drawing_weak
                    .upgrade()
                    .map(|drawing| (drawing.allocated_width(), drawing.allocated_height()))
                    .unwrap_or((1280, 720));
                let _ = tx.send(InputEvent::MouseRelease {
                    button: gesture.current_button(),
                    x,
                    y,
                    canvas_width,
                    canvas_height,
                });
            });
        }

        {
            let tx = input_tx.clone();
            let drawing_weak = drawing.downgrade();
            motion_controller.connect_motion(move |_controller, x, y| {
                let (canvas_width, canvas_height) = drawing_weak
                    .upgrade()
                    .map(|drawing| (drawing.allocated_width(), drawing.allocated_height()))
                    .unwrap_or((1280, 720));
                let _ = tx.send(InputEvent::MouseMove {
                    x,
                    y,
                    canvas_width,
                    canvas_height,
                });
            });
        }

        {
            let tx = input_tx;
            let drawing_weak = drawing.downgrade();
            scroll_controller.connect_scroll(move |controller, _dx, dy| {
                let (canvas_width, canvas_height) = drawing_weak
                    .upgrade()
                    .map(|drawing| {
                        drawing.grab_focus();
                        (drawing.allocated_width(), drawing.allocated_height())
                    })
                    .unwrap_or((1280, 720));
                let (x, y) = controller
                    .current_event()
                    .and_then(|event| event.position())
                    .unwrap_or((0.0, 0.0));
                let _ = tx.send(InputEvent::MouseWheel {
                    delta_y: dy,
                    x,
                    y,
                    canvas_width,
                    canvas_height,
                });
                glib::Propagation::Stop
            });
        }

        drawing.add_controller(click_gesture);
        drawing.add_controller(motion_controller);
        drawing.add_controller(scroll_controller);
        drawing.set_can_focus(true);
        drawing.set_focusable(true);
        drawing.set_focus_on_click(true);

        win.connect_close_request(move |_| {
            (on_close)();
            glib::Propagation::Proceed
        });

        let vm_ip = vm_ip.to_string();
        let password = password.to_string();
        tokio::spawn(async move {
            if let Err(err) = run_rdp_loop_with_retries(
                &vm_ip,
                &password,
                tx,
                input_rx,
                options,
                Some(clipboard_backend),
                Some(clipboard_runtime),
            )
            .await
            {
                tracing::error!("RDP loop exited: {err}");
            }
        });

        win.present();
        gtk::prelude::GtkWindowExt::set_focus(&win, Some(&drawing));
        drawing.grab_focus();
        Ok(())
    }
}

fn should_retry_rdp_loop(attempt: usize, max_attempts: usize) -> bool {
    attempt < max_attempts
}

fn install_keyboard_controller(
    target: &impl IsA<gtk::Widget>,
    input_tx: tokio::sync::mpsc::UnboundedSender<InputEvent>,
) {
    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);

    {
        let tx = input_tx.clone();
        key_controller.connect_key_pressed(move |_controller, keyval, keycode, state| {
            let event = InputEvent::KeyPress {
                keyval: keyval.into_glib(),
                keycode,
                modifiers: state.bits(),
            };
            tracing::debug!(?event, "GTK key press captured");
            let _ = tx.send(event);
            glib::Propagation::Stop
        });
    }

    {
        let tx = input_tx;
        key_controller.connect_key_released(move |_controller, keyval, keycode, state| {
            let event = InputEvent::KeyRelease {
                keyval: keyval.into_glib(),
                keycode,
                modifiers: state.bits(),
            };
            tracing::debug!(?event, "GTK key release captured");
            let _ = tx.send(event);
        });
    }

    target.add_controller(key_controller);
}

#[derive(Clone)]
pub struct RdpFrame {
    pub width: u16,
    pub height: u16,
    pub bgra: Arc<Vec<u8>>,
}

#[derive(Debug)]
pub enum InputEvent {
    KeyPress {
        keyval: u32,
        keycode: u32,
        modifiers: u32,
    },
    KeyRelease {
        keyval: u32,
        keycode: u32,
        modifiers: u32,
    },
    MousePress {
        button: u32,
        x: f64,
        y: f64,
        canvas_width: i32,
        canvas_height: i32,
    },
    MouseRelease {
        button: u32,
        x: f64,
        y: f64,
        canvas_width: i32,
        canvas_height: i32,
    },
    MouseMove {
        x: f64,
        y: f64,
        canvas_width: i32,
        canvas_height: i32,
    },
    MouseWheel {
        delta_y: f64,
        x: f64,
        y: f64,
        canvas_width: i32,
        canvas_height: i32,
    },
}

fn paint_frame_on_cairo(
    cr: &gtk::cairo::Context,
    frame: &RdpFrame,
    viewport: RdpViewport,
    canvas_w: i32,
    canvas_h: i32,
) {
    let stride = i32::from(frame.width) * 4;
    let Ok(surface) = gtk::cairo::ImageSurface::create_for_data(
        (*frame.bgra).clone(),
        gtk::cairo::Format::ARgb32,
        i32::from(frame.width),
        i32::from(frame.height),
        stride,
    ) else {
        return;
    };

    let scale_x = f64::from(canvas_w) / f64::from(viewport.width);
    let scale_y = f64::from(canvas_h) / f64::from(viewport.height);
    let scale = scale_x.min(scale_y).max(0.01);
    let offset_x = (f64::from(canvas_w) - f64::from(viewport.width) * scale) / 2.0;
    let offset_y = (f64::from(canvas_h) - f64::from(viewport.height) * scale) / 2.0;

    let _ = cr.save();
    cr.translate(offset_x, offset_y);
    cr.scale(scale, scale);
    let _ = cr.set_source_surface(
        &surface,
        -f64::from(viewport.left),
        -f64::from(viewport.top),
    );
    cr.rectangle(
        0.0,
        0.0,
        f64::from(viewport.width),
        f64::from(viewport.height),
    );
    cr.clip();
    let _ = cr.paint();
    let _ = cr.restore();
}

async fn run_rdp_loop_with_retries(
    vm_ip: &str,
    password: &str,
    frame_tx: async_channel::Sender<RdpFrame>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<InputEvent>,
    options: RdpWindowOptions,
    clipboard_backend: Option<crate::clipboard::TextClipboardBackend>,
    clipboard_runtime: Option<crate::clipboard::RdpClipboardRuntime>,
) -> WinbridgeResult<()> {
    for attempt in 1..=RDP_RECONNECT_ATTEMPTS {
        match run_rdp_loop(
            vm_ip,
            password,
            frame_tx.clone(),
            &mut input_rx,
            options.clone(),
            clipboard_backend.clone(),
            clipboard_runtime.clone(),
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(err) if should_retry_rdp_loop(attempt, RDP_RECONNECT_ATTEMPTS) => {
                tracing::warn!(
                    attempt,
                    max_attempts = RDP_RECONNECT_ATTEMPTS,
                    "RDP loop exited; retrying after transient failure: {err}"
                );
                tokio::time::sleep(RDP_RECONNECT_DELAY).await;
            }
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

async fn run_rdp_loop(
    vm_ip: &str,
    password: &str,
    frame_tx: async_channel::Sender<RdpFrame>,
    input_rx: &mut tokio::sync::mpsc::UnboundedReceiver<InputEvent>,
    options: RdpWindowOptions,
    clipboard_backend: Option<crate::clipboard::TextClipboardBackend>,
    clipboard_runtime: Option<crate::clipboard::RdpClipboardRuntime>,
) -> WinbridgeResult<()> {
    use ironrdp::session::image::DecodedImage;
    use ironrdp::session::ActiveStage;
    use ironrdp_graphics::image_processing::PixelFormat;

    let username =
        std::env::var("WINBRIDGE_ADMIN_USER").unwrap_or_else(|_| "Administrator".to_string());
    let probe = RdpHeadlessProbe::new(vm_ip, 3389, &username, password)?;
    let (desktop_width, desktop_height) = options.initial_desktop_size();
    let session = probe
        .connect(
            clipboard_backend,
            RdpDesktopSize::new(desktop_width, desktop_height),
            options.virtual_desktop_layout,
            options.display_strategy,
        )
        .await?;
    let width = session.connection_result.desktop_size.width;
    let height = session.connection_result.desktop_size.height;
    let viewport = options.viewport;
    let mut framed = session.framed;
    let mut active_stage = ActiveStage::new(session.connection_result);
    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);
    let mut reactivation: Option<
        ironrdp::connector::connection_activation::ConnectionActivationSequence,
    > = None;

    loop {
        tokio::select! {
            pdu = framed.read_pdu() => {
                let (action, payload) = pdu
                    .map_err(|err| RdpError::Disconnected(format!("RDP PDU read 실패: {err}")))?;
                if let Some(sequence) = reactivation.as_mut() {
                    match try_process_reactivation_pdu(&mut framed, sequence, action, &payload)
                        .await?
                    {
                        ReactivationPduResult::Handled(Some((width, height))) => {
                            tracing::info!(
                                width,
                                height,
                                "RDP deactivation/reactivation sequence completed"
                            );
                            image = DecodedImage::new(PixelFormat::RgbA32, width, height);
                            reactivation = None;
                            continue;
                        }
                        ReactivationPduResult::Handled(None) => {
                            continue;
                        }
                        ReactivationPduResult::Unhandled => {}
                    }
                }

                let outputs = active_stage
                    .process(&mut image, action, &payload)
                    .map_err(|err| RdpError::Protocol(format!("RDP active stage 처리 실패: {err}")))?;
                if let Some(sequence) =
                    handle_active_outputs(&mut framed, &frame_tx, &image, outputs).await?
                {
                    reactivation = Some(sequence);
                }
            }
            maybe_event = input_rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        if reactivation.is_some() {
                            tracing::trace!(?event, "RDP input event ignored during deactivation/reactivation");
                            continue;
                        }
                        let input_events = input_event_to_fastpath_in_viewport(&event, viewport);
                if input_events.is_empty() {
                    tracing::trace!(?event, "RDP input event ignored");
                    continue;
                }
                if matches!(
                    event,
                    InputEvent::KeyPress { .. }
                        | InputEvent::KeyRelease { .. }
                        | InputEvent::MousePress { .. }
                        | InputEvent::MouseRelease { .. }
                        | InputEvent::MouseWheel { .. }
                ) {
                    tracing::debug!(?event, "RDP input event sending");
                }

                        let outputs = active_stage
                            .process_fastpath_input(&mut image, &input_events)
                            .map_err(|err| RdpError::Protocol(format!("RDP input 처리 실패: {err}")))?;
                        if let Some(sequence) =
                            handle_active_outputs(&mut framed, &frame_tx, &image, outputs).await?
                        {
                            reactivation = Some(sequence);
                        }
                    }
                    None => return Ok(()),
                }
            }
            maybe_clipboard_event = recv_clipboard_event(clipboard_runtime.as_ref()) => {
                if reactivation.is_some() {
                    continue;
                }
                let Some(clipboard_event) = maybe_clipboard_event else {
                    continue;
                };
                let outputs = match clipboard_event {
                    ClipboardRuntimeEvent::Command(command) => {
                        let Some(runtime) = clipboard_runtime.as_ref() else {
                            continue;
                        };
                        crate::clipboard::process_clipboard_command(&mut active_stage, runtime, command)?
                    }
                    ClipboardRuntimeEvent::Backend(message) => {
                        crate::clipboard::process_clipboard_message(&mut active_stage, message)?
                    }
                };
                if let Some(sequence) =
                    handle_active_outputs(&mut framed, &frame_tx, &image, outputs).await?
                {
                    reactivation = Some(sequence);
                }
            }
        }
    }
}

enum ClipboardRuntimeEvent {
    Command(crate::clipboard::RdpClipboardCommand),
    Backend(ironrdp::cliprdr::backend::ClipboardMessage),
}

async fn recv_clipboard_event(
    runtime: Option<&crate::clipboard::RdpClipboardRuntime>,
) -> Option<ClipboardRuntimeEvent> {
    let Some(runtime) = runtime else {
        return std::future::pending::<Option<ClipboardRuntimeEvent>>().await;
    };

    tokio::select! {
        command = runtime.command_rx.recv() => command.ok().map(ClipboardRuntimeEvent::Command),
        message = runtime.backend_rx.recv() => message.ok().map(ClipboardRuntimeEvent::Backend),
    }
}

async fn handle_active_outputs(
    framed: &mut RdpFramed,
    frame_tx: &async_channel::Sender<RdpFrame>,
    image: &ironrdp::session::image::DecodedImage,
    outputs: Vec<ironrdp::session::ActiveStageOutput>,
) -> WinbridgeResult<Option<ironrdp::connector::connection_activation::ConnectionActivationSequence>>
{
    use ironrdp::session::ActiveStageOutput;
    use ironrdp_tokio::FramedWrite as _;

    let mut reactivation = None;
    for output in outputs {
        match output {
            ActiveStageOutput::ResponseFrame(frame) => {
                if !frame.is_empty() {
                    framed.write_all(&frame).await.map_err(|err| {
                        RdpError::Disconnected(format!("RDP response write 실패: {err}"))
                    })?;
                }
            }
            ActiveStageOutput::GraphicsUpdate(_rect) => {
                send_decoded_frame(frame_tx, image)?;
            }
            ActiveStageOutput::Terminate(reason) => {
                return Err(
                    RdpError::Disconnected(format!("server terminated session: {reason}")).into(),
                );
            }
            ActiveStageOutput::DeactivateAll(sequence) => {
                tracing::info!("RDP deactivation/reactivation sequence started");
                reactivation = Some(*sequence);
            }
            ActiveStageOutput::PointerDefault
            | ActiveStageOutput::PointerHidden
            | ActiveStageOutput::PointerPosition { .. }
            | ActiveStageOutput::PointerBitmap(_) => {}
        }
    }

    Ok(reactivation)
}

enum ReactivationPduResult {
    Handled(Option<(u16, u16)>),
    Unhandled,
}

async fn try_process_reactivation_pdu(
    framed: &mut RdpFramed,
    sequence: &mut ironrdp::connector::connection_activation::ConnectionActivationSequence,
    action: ironrdp_pdu::Action,
    payload: &[u8],
) -> WinbridgeResult<ReactivationPduResult> {
    use ironrdp::connector::Sequence as _;

    if action != ironrdp_pdu::Action::X224 {
        tracing::debug!(
            ?action,
            "RDP frame ignored during deactivation/reactivation"
        );
        return Ok(ReactivationPduResult::Unhandled);
    }

    if sequence.next_pdu_hint().is_none() {
        if let Some(desktop_size) = drain_reactivation_no_input_frames(framed, sequence).await? {
            return Ok(ReactivationPduResult::Handled(Some(desktop_size)));
        }
    }

    if sequence.next_pdu_hint().is_none() {
        return Ok(ReactivationPduResult::Unhandled);
    }

    let mut next_sequence = sequence.clone();
    let mut output = ironrdp::core::WriteBuf::new();
    if let Err(err) = next_sequence.step(payload, &mut output) {
        tracing::debug!(
            error = %err,
            "RDP frame did not match deactivation/reactivation sequence"
        );
        return Ok(ReactivationPduResult::Unhandled);
    }

    write_reactivation_response(framed, &output).await?;

    *sequence = next_sequence;
    let desktop_size = reactivation_desktop_size(sequence)
        .or(drain_reactivation_no_input_frames(framed, sequence).await?);

    Ok(ReactivationPduResult::Handled(desktop_size))
}

async fn drain_reactivation_no_input_frames(
    framed: &mut RdpFramed,
    sequence: &mut ironrdp::connector::connection_activation::ConnectionActivationSequence,
) -> WinbridgeResult<Option<(u16, u16)>> {
    use ironrdp::connector::Sequence as _;

    while sequence.next_pdu_hint().is_none() {
        if let Some(desktop_size) = reactivation_desktop_size(sequence) {
            return Ok(Some(desktop_size));
        }

        let mut output = ironrdp::core::WriteBuf::new();
        sequence
            .step_no_input(&mut output)
            .map_err(|err| RdpError::Protocol(format!("RDP reactivation 처리 실패: {err}")))?;
        write_reactivation_response(framed, &output).await?;
    }

    Ok(None)
}

fn reactivation_desktop_size(
    sequence: &ironrdp::connector::connection_activation::ConnectionActivationSequence,
) -> Option<(u16, u16)> {
    use ironrdp::connector::connection_activation::ConnectionActivationState;

    match sequence.connection_activation_state() {
        ConnectionActivationState::Finalized { desktop_size, .. } => {
            Some((desktop_size.width, desktop_size.height))
        }
        _ => None,
    }
}

async fn write_reactivation_response(
    framed: &mut RdpFramed,
    output: &ironrdp::core::WriteBuf,
) -> WinbridgeResult<()> {
    use ironrdp_tokio::FramedWrite as _;

    let response = output.filled();
    if !response.is_empty() {
        framed
            .write_all(response)
            .await
            .map_err(|err| RdpError::Disconnected(format!("RDP reactivation write 실패: {err}")))?;
    }

    Ok(())
}

fn send_decoded_frame(
    frame_tx: &async_channel::Sender<RdpFrame>,
    image: &ironrdp::session::image::DecodedImage,
) -> WinbridgeResult<()> {
    let bgra = cairo_bgra_from_decoded(image)?;

    frame_tx
        .try_send(RdpFrame {
            width: image.width(),
            height: image.height(),
            bgra: Arc::new(bgra),
        })
        .map_err(|_| RdpError::Disconnected("GTK frame receiver closed".to_string()).into())
}

fn cairo_bgra_from_decoded(
    image: &ironrdp::session::image::DecodedImage,
) -> WinbridgeResult<Vec<u8>> {
    use ironrdp_graphics::image_processing::PixelFormat;

    let mut out = Vec::with_capacity(image.data().len());
    for pixel in image.data().chunks_exact(4) {
        let (r, g, b, _a) = match image.pixel_format() {
            PixelFormat::RgbA32 => (pixel[0], pixel[1], pixel[2], pixel[3]),
            PixelFormat::RgbX32 => (pixel[0], pixel[1], pixel[2], 0xff),
            PixelFormat::BgrA32 => (pixel[2], pixel[1], pixel[0], pixel[3]),
            PixelFormat::BgrX32 => (pixel[2], pixel[1], pixel[0], 0xff),
            PixelFormat::ARgb32 => (pixel[1], pixel[2], pixel[3], pixel[0]),
            PixelFormat::XRgb32 => (pixel[1], pixel[2], pixel[3], 0xff),
            PixelFormat::ABgr32 => (pixel[3], pixel[2], pixel[1], pixel[0]),
            PixelFormat::XBgr32 => (pixel[3], pixel[2], pixel[1], 0xff),
        };

        out.extend_from_slice(&[b, g, r, 0xff]);
    }

    Ok(out)
}

#[cfg(test)]
fn input_event_to_fastpath(
    event: &InputEvent,
    desktop_width: u16,
    desktop_height: u16,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    input_event_to_fastpath_in_viewport(
        event,
        RdpViewport::new(0, 0, desktop_width, desktop_height),
    )
}

fn input_event_to_fastpath_in_viewport(
    event: &InputEvent,
    viewport: RdpViewport,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    use ironrdp_pdu::input::fast_path::FastPathInputEvent;
    use ironrdp_pdu::input::mouse::{MousePdu, PointerFlags};

    match event {
        InputEvent::KeyPress {
            keyval,
            keycode,
            modifiers,
        } => keyboard_events(*keyval, *keycode, *modifiers, false),
        InputEvent::KeyRelease {
            keyval, keycode, ..
        } => keyboard_events(*keyval, *keycode, 0, true),
        InputEvent::MouseMove {
            x,
            y,
            canvas_width,
            canvas_height,
        } => canvas_to_desktop(
            *x,
            *y,
            *canvas_width,
            *canvas_height,
            viewport.width,
            viewport.height,
        )
        .and_then(|(x, y)| viewport_to_desktop(x, y, viewport))
        .map(|(x_position, y_position)| {
            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags: PointerFlags::MOVE,
                number_of_wheel_rotation_units: 0,
                x_position,
                y_position,
            })]
        })
        .unwrap_or_default(),
        InputEvent::MouseWheel {
            delta_y,
            x,
            y,
            canvas_width,
            canvas_height,
        } => {
            let Some((x_position, y_position)) = canvas_to_desktop(
                *x,
                *y,
                *canvas_width,
                *canvas_height,
                viewport.width,
                viewport.height,
            )
            .and_then(|(x, y)| viewport_to_desktop(x, y, viewport)) else {
                return Vec::new();
            };
            let Some(rotation_units) = wheel_rotation_units(*delta_y) else {
                return Vec::new();
            };

            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags: PointerFlags::VERTICAL_WHEEL,
                number_of_wheel_rotation_units: rotation_units,
                x_position,
                y_position,
            })]
        }
        InputEvent::MousePress {
            button,
            x,
            y,
            canvas_width,
            canvas_height,
        }
        | InputEvent::MouseRelease {
            button,
            x,
            y,
            canvas_width,
            canvas_height,
        } => {
            let Some(button_flag) = mouse_button_flag(*button) else {
                return Vec::new();
            };
            let Some((x_position, y_position)) = canvas_to_desktop(
                *x,
                *y,
                *canvas_width,
                *canvas_height,
                viewport.width,
                viewport.height,
            )
            .and_then(|(x, y)| viewport_to_desktop(x, y, viewport)) else {
                return Vec::new();
            };

            let mut flags = button_flag;
            if matches!(event, InputEvent::MousePress { .. }) {
                flags |= PointerFlags::DOWN;
            }

            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units: 0,
                x_position,
                y_position,
            })]
        }
    }
}

#[cfg(test)]
fn canvas_to_desktop_in_viewport(
    x: f64,
    y: f64,
    canvas_width: i32,
    canvas_height: i32,
    viewport: RdpViewport,
) -> Option<(u16, u16)> {
    canvas_to_desktop(
        x,
        y,
        canvas_width,
        canvas_height,
        viewport.width,
        viewport.height,
    )
    .and_then(|(x, y)| viewport_to_desktop(x, y, viewport))
}

fn viewport_to_desktop(x: u16, y: u16, viewport: RdpViewport) -> Option<(u16, u16)> {
    Some((x.checked_add(viewport.left)?, y.checked_add(viewport.top)?))
}

fn wheel_rotation_units(delta_y: f64) -> Option<i16> {
    if delta_y > 0.0 {
        Some(-120)
    } else if delta_y < 0.0 {
        Some(120)
    } else {
        None
    }
}

fn keyboard_event(
    keyval: u32,
    keycode: u32,
    release: bool,
) -> Option<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    use ironrdp_pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};

    let (scan_code, extended) = keyval_to_scancode(keyval).or_else(|| {
        let scan_code = keycode.checked_sub(8).unwrap_or(keycode);
        u8::try_from(scan_code)
            .ok()
            .map(|scan_code| (scan_code, false))
    })?;

    let mut flags = KeyboardFlags::empty();
    if release {
        flags |= KeyboardFlags::RELEASE;
    }
    if extended {
        flags |= KeyboardFlags::EXTENDED;
    }

    Some(FastPathInputEvent::KeyboardEvent(flags, scan_code))
}

fn keyboard_events(
    keyval: u32,
    keycode: u32,
    modifiers: u32,
    release: bool,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    let mut events = Vec::new();
    if !release && !is_modifier_keyval(keyval) {
        events.extend(modifier_release_events_for_state(modifiers));
    }
    if let Some(event) = keyboard_event(keyval, keycode, release) {
        events.push(event);
    }
    events
}

fn modifier_release_events_for_state(
    modifiers: u32,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    let state = gtk::gdk::ModifierType::from_bits_truncate(modifiers);
    let mut events = Vec::new();

    if !state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        events.extend([
            keyboard_release_event(0x2a, false),
            keyboard_release_event(0x36, false),
        ]);
    }
    if !state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        events.extend([
            keyboard_release_event(0x1d, false),
            keyboard_release_event(0x1d, true),
        ]);
    }
    if !state.contains(gtk::gdk::ModifierType::ALT_MASK) {
        events.extend([
            keyboard_release_event(0x38, false),
            keyboard_release_event(0x38, true),
        ]);
    }
    if !state.contains(gtk::gdk::ModifierType::SUPER_MASK) {
        events.extend([
            keyboard_release_event(0x5b, true),
            keyboard_release_event(0x5c, true),
        ]);
    }

    events
}

fn keyboard_release_event(
    scan_code: u8,
    extended: bool,
) -> ironrdp_pdu::input::fast_path::FastPathInputEvent {
    use ironrdp_pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};

    let mut flags = KeyboardFlags::RELEASE;
    if extended {
        flags |= KeyboardFlags::EXTENDED;
    }

    FastPathInputEvent::KeyboardEvent(flags, scan_code)
}

fn is_modifier_keyval(keyval: u32) -> bool {
    matches!(
        keyval,
        0xffe1 | 0xffe2 | 0xffe3 | 0xffe4 | 0xffe9 | 0xffea | 0xffeb | 0xffec
    )
}

fn keyval_to_scancode(keyval: u32) -> Option<(u8, bool)> {
    if let Some(ch) = char::from_u32(keyval).map(|ch| ch.to_ascii_lowercase()) {
        if let Some(scan_code) = ascii_key_to_scancode(ch) {
            return Some((scan_code, false));
        }
    }

    let scan_code = match keyval {
        0x08 | 0xff08 => 0x0e, // BackSpace
        0x09 | 0xff09 => 0x0f, // Tab
        0x0d | 0xff0d => 0x1c, // Return
        0x1b | 0xff1b => 0x01, // Escape
        0xffe1 => 0x2a,        // Shift_L
        0xffe2 => 0x36,        // Shift_R
        0xffe3 => 0x1d,        // Control_L
        0xffe9 => 0x38,        // Alt_L
        0xffe5 => 0x3a,        // Caps_Lock
        0xffbe..=0xffc7 => 0x3b + u8::try_from(keyval - 0xffbe).ok()?,
        0xffc8 => 0x57, // F11
        0xffc9 => 0x58, // F12
        _ => return extended_keyval_to_scancode(keyval),
    };

    Some((scan_code, false))
}

fn ascii_key_to_scancode(ch: char) -> Option<u8> {
    let scan_code = match ch {
        'a' => 0x1e,
        'b' => 0x30,
        'c' => 0x2e,
        'd' => 0x20,
        'e' => 0x12,
        'f' => 0x21,
        'g' => 0x22,
        'h' => 0x23,
        'i' => 0x17,
        'j' => 0x24,
        'k' => 0x25,
        'l' => 0x26,
        'm' => 0x32,
        'n' => 0x31,
        'o' => 0x18,
        'p' => 0x19,
        'q' => 0x10,
        'r' => 0x13,
        's' => 0x1f,
        't' => 0x14,
        'u' => 0x16,
        'v' => 0x2f,
        'w' => 0x11,
        'x' => 0x2d,
        'y' => 0x15,
        'z' => 0x2c,
        '1' | '!' => 0x02,
        '2' | '@' => 0x03,
        '3' | '#' => 0x04,
        '4' | '$' => 0x05,
        '5' | '%' => 0x06,
        '6' | '^' => 0x07,
        '7' | '&' => 0x08,
        '8' | '*' => 0x09,
        '9' | '(' => 0x0a,
        '0' | ')' => 0x0b,
        '-' | '_' => 0x0c,
        '=' | '+' => 0x0d,
        '[' | '{' => 0x1a,
        ']' | '}' => 0x1b,
        '\\' | '|' => 0x2b,
        ';' | ':' => 0x27,
        '\'' | '"' => 0x28,
        '`' | '~' => 0x29,
        ',' | '<' => 0x33,
        '.' | '>' => 0x34,
        '/' | '?' => 0x35,
        ' ' => 0x39,
        _ => return None,
    };

    Some(scan_code)
}

fn extended_keyval_to_scancode(keyval: u32) -> Option<(u8, bool)> {
    let scan_code = match keyval {
        0xff31..=0xff33 => 0x38, // Hangul / Hangul_Start / Hangul_End -> Right Alt
        0xff34 => 0x1d,          // Hangul_Hanja -> Right Ctrl
        0xff50 => 0x47,          // Home
        0xff51 => 0x4b,          // Left
        0xff52 => 0x48,          // Up
        0xff53 => 0x4d,          // Right
        0xff54 => 0x50,          // Down
        0xff55 => 0x49,          // Page_Up
        0xff56 => 0x51,          // Page_Down
        0xff57 => 0x4f,          // End
        0xff63 => 0x52,          // Insert
        0xffff => 0x53,          // Delete
        0xffe4 => 0x1d,          // Control_R
        0xffea => 0x38,          // Alt_R
        0xffeb => 0x5b,          // Super_L
        0xffec => 0x5c,          // Super_R
        _ => return None,
    };

    Some((scan_code, true))
}

fn mouse_button_flag(button: u32) -> Option<ironrdp_pdu::input::mouse::PointerFlags> {
    use ironrdp_pdu::input::mouse::PointerFlags;

    match button {
        1 => Some(PointerFlags::LEFT_BUTTON),
        2 => Some(PointerFlags::MIDDLE_BUTTON_OR_WHEEL),
        3 => Some(PointerFlags::RIGHT_BUTTON),
        _ => None,
    }
}

fn canvas_to_desktop(
    x: f64,
    y: f64,
    canvas_width: i32,
    canvas_height: i32,
    desktop_width: u16,
    desktop_height: u16,
) -> Option<(u16, u16)> {
    if canvas_width <= 0 || canvas_height <= 0 || desktop_width == 0 || desktop_height == 0 {
        return None;
    }

    let desktop_width_f = f64::from(desktop_width);
    let desktop_height_f = f64::from(desktop_height);
    let scale_x = f64::from(canvas_width) / desktop_width_f;
    let scale_y = f64::from(canvas_height) / desktop_height_f;
    let scale = scale_x.min(scale_y).max(0.01);
    let offset_x = (f64::from(canvas_width) - desktop_width_f * scale) / 2.0;
    let offset_y = (f64::from(canvas_height) - desktop_height_f * scale) / 2.0;

    let desktop_x = ((x - offset_x) / scale).clamp(0.0, desktop_width_f - 1.0);
    let desktop_y = ((y - offset_y) / scale).clamp(0.0, desktop_height_f - 1.0);

    Some((desktop_x.round() as u16, desktop_y.round() as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_invalid_ip_address() {
        let err = RdpHeadlessProbe::new("not an ip", 3389, "Administrator", "secret").unwrap_err();
        assert!(format!("{}", err).contains("RDP"));
    }

    #[test]
    fn new_accepts_ipv4_address_and_port() {
        let probe =
            RdpHeadlessProbe::new("192.168.122.50", 3389, "Administrator", "secret").unwrap();
        assert_eq!(probe.addr.to_string(), "192.168.122.50:3389");
    }

    #[test]
    fn canvas_to_desktop_accounts_for_letterbox_scaling() {
        assert_eq!(
            canvas_to_desktop(640.0, 360.0, 1280, 720, 1280, 720),
            Some((640, 360))
        );
        assert_eq!(
            canvas_to_desktop(320.0, 240.0, 640, 480, 1280, 720),
            Some((640, 360))
        );
        assert_eq!(
            canvas_to_desktop(0.0, 0.0, 640, 480, 1280, 720),
            Some((0, 0))
        );
    }

    #[test]
    fn canvas_to_desktop_can_target_a_virtual_monitor_viewport() {
        let viewport = RdpViewport {
            left: 1280,
            top: 0,
            width: 1280,
            height: 720,
        };

        assert_eq!(
            canvas_to_desktop_in_viewport(320.0, 240.0, 640, 480, viewport),
            Some((1920, 360))
        );
    }

    #[test]
    fn app_mode_uses_file_dialog_sized_single_desktop_for_app_window() {
        let options = RdpWindowOptions::kakaotalk_app();

        assert_eq!(options.title, "winbridge");
        assert_eq!(options.icon_name, Some(crate::desktop::WINBRIDGE_ICON_NAME));
        assert_eq!(options.display_strategy, RdpDisplayStrategy::StableSlots);
        assert_eq!(options.virtual_desktop_layout, None);
        assert_eq!(options.initial_desktop_size(), (960, 720));
        assert_eq!(options.initial_window_size(), (960, 720));
        assert_eq!(
            options.viewport,
            RdpViewport {
                left: 0,
                top: 0,
                width: 960,
                height: 720,
            }
        );
    }

    #[test]
    fn app_mode_can_enable_experimental_multimon_without_changing_viewport() {
        let options = RdpWindowOptions::kakaotalk_app()
            .with_display_strategy(RdpDisplayStrategy::ExperimentalMultimon);

        assert_eq!(
            options.display_strategy,
            RdpDisplayStrategy::ExperimentalMultimon
        );
        assert_eq!(
            options.viewport,
            RdpViewport {
                left: 0,
                top: 0,
                width: 960,
                height: 720,
            }
        );
        assert_eq!(options.virtual_desktop_layout, None);
    }

    #[test]
    fn rdp_loop_retries_until_last_attempt() {
        assert!(should_retry_rdp_loop(1, 3));
        assert!(should_retry_rdp_loop(2, 3));
        assert!(!should_retry_rdp_loop(3, 3));
    }

    #[test]
    fn two_horizontal_slot_layout_uses_wide_initial_desktop() {
        assert_eq!(
            initial_desktop_size_for_layout(Some(RdpVirtualDesktopLayout::TwoHorizontalSlots {
                slot_width: 1280,
                slot_height: 720,
            })),
            (2560, 720)
        );
        assert_eq!(initial_desktop_size_for_layout(None), (1280, 720));
    }

    #[test]
    fn display_control_layout_builds_two_horizontal_monitors() {
        let layout = build_display_control_layout(RdpVirtualDesktopLayout::TwoHorizontalSlots {
            slot_width: 1280,
            slot_height: 720,
        });
        let monitors = layout.monitors();

        assert_eq!(monitors.len(), 2);
        assert!(monitors[0].is_primary());
        assert_eq!(monitors[0].position(), Some((0, 0)));
        assert_eq!(monitors[0].dimensions(), (1280, 720));
        assert!(!monitors[1].is_primary());
        assert_eq!(monitors[1].position(), Some((1280, 0)));
        assert_eq!(monitors[1].dimensions(), (1280, 720));
    }

    #[test]
    fn keyval_to_scancode_maps_ascii_and_extended_keys() {
        assert_eq!(keyval_to_scancode(u32::from('a')), Some((0x1e, false)));
        assert_eq!(keyval_to_scancode(u32::from('A')), Some((0x1e, false)));
        assert_eq!(keyval_to_scancode(u32::from('?')), Some((0x35, false)));
        assert_eq!(keyval_to_scancode(0x08), Some((0x0e, false)));
        assert_eq!(keyval_to_scancode(0xff08), Some((0x0e, false)));
        assert_eq!(keyval_to_scancode(0xff51), Some((0x4b, true)));
        assert_eq!(keyval_to_scancode(0xff31), Some((0x38, true)));
        assert_eq!(keyval_to_scancode(0xff34), Some((0x1d, true)));
    }

    #[test]
    fn keyval_to_scancode_distinguishes_left_and_right_modifiers() {
        assert_eq!(keyval_to_scancode(0xffe1), Some((0x2a, false))); // Shift_L
        assert_eq!(keyval_to_scancode(0xffe2), Some((0x36, false))); // Shift_R
        assert_eq!(keyval_to_scancode(0xffe3), Some((0x1d, false))); // Control_L
        assert_eq!(keyval_to_scancode(0xffe4), Some((0x1d, true))); // Control_R
        assert_eq!(keyval_to_scancode(0xffe9), Some((0x38, false))); // Alt_L
        assert_eq!(keyval_to_scancode(0xffea), Some((0x38, true))); // Alt_R
    }

    #[test]
    fn key_press_releases_stale_modifiers_before_backspace() {
        use ironrdp_pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};

        let events = input_event_to_fastpath(
            &InputEvent::KeyPress {
                keyval: 0xff08,
                keycode: 22,
                modifiers: 0,
            },
            1280,
            720,
        );

        assert!(events.iter().any(|event| {
            matches!(
                event,
                FastPathInputEvent::KeyboardEvent(flags, 0x1d)
                    if flags.contains(KeyboardFlags::RELEASE)
            )
        }));
        assert!(matches!(
            events.last(),
            Some(FastPathInputEvent::KeyboardEvent(flags, 0x0e))
                if !flags.contains(KeyboardFlags::RELEASE)
        ));
    }

    #[test]
    fn key_press_preserves_current_control_modifier() {
        use ironrdp_pdu::input::fast_path::FastPathInputEvent;

        let events = input_event_to_fastpath(
            &InputEvent::KeyPress {
                keyval: u32::from('v'),
                keycode: 55,
                modifiers: gtk::gdk::ModifierType::CONTROL_MASK.bits(),
            },
            1280,
            720,
        );

        assert!(!events
            .iter()
            .any(|event| { matches!(event, FastPathInputEvent::KeyboardEvent(_, 0x1d)) }));
        assert!(matches!(
            events.last(),
            Some(FastPathInputEvent::KeyboardEvent(_, 0x2f))
        ));
    }

    #[test]
    fn mouse_input_event_maps_to_rdp_coordinates_and_flags() {
        use ironrdp_pdu::input::fast_path::FastPathInputEvent;
        use ironrdp_pdu::input::mouse::PointerFlags;

        let events = input_event_to_fastpath(
            &InputEvent::MousePress {
                button: 1,
                x: 320.0,
                y: 240.0,
                canvas_width: 640,
                canvas_height: 480,
            },
            1280,
            720,
        );

        let [FastPathInputEvent::MouseEvent(event)] = events.as_slice() else {
            panic!("expected one mouse event");
        };

        assert!(event.flags.contains(PointerFlags::LEFT_BUTTON));
        assert!(event.flags.contains(PointerFlags::DOWN));
        assert_eq!((event.x_position, event.y_position), (640, 360));
    }

    #[test]
    fn right_mouse_input_event_maps_to_rdp_coordinates_and_flags() {
        use ironrdp_pdu::input::fast_path::FastPathInputEvent;
        use ironrdp_pdu::input::mouse::PointerFlags;

        let events = input_event_to_fastpath(
            &InputEvent::MousePress {
                button: 3,
                x: 320.0,
                y: 240.0,
                canvas_width: 640,
                canvas_height: 480,
            },
            1280,
            720,
        );

        let [FastPathInputEvent::MouseEvent(event)] = events.as_slice() else {
            panic!("expected one right mouse event");
        };

        assert!(event.flags.contains(PointerFlags::RIGHT_BUTTON));
        assert!(event.flags.contains(PointerFlags::DOWN));
        assert_eq!((event.x_position, event.y_position), (640, 360));
    }

    #[test]
    fn mouse_wheel_down_maps_to_negative_vertical_wheel_event() {
        use ironrdp_pdu::input::fast_path::FastPathInputEvent;
        use ironrdp_pdu::input::mouse::PointerFlags;

        let events = input_event_to_fastpath(
            &InputEvent::MouseWheel {
                delta_y: 1.0,
                x: 320.0,
                y: 240.0,
                canvas_width: 640,
                canvas_height: 480,
            },
            1280,
            720,
        );

        let [FastPathInputEvent::MouseEvent(event)] = events.as_slice() else {
            panic!("expected one wheel event");
        };

        assert!(event.flags.contains(PointerFlags::VERTICAL_WHEEL));
        assert_eq!(event.number_of_wheel_rotation_units, -120);
        assert_eq!((event.x_position, event.y_position), (640, 360));
    }

    #[test]
    fn mouse_wheel_up_maps_to_positive_vertical_wheel_event() {
        use ironrdp_pdu::input::fast_path::FastPathInputEvent;
        use ironrdp_pdu::input::mouse::PointerFlags;

        let events = input_event_to_fastpath(
            &InputEvent::MouseWheel {
                delta_y: -1.0,
                x: 320.0,
                y: 240.0,
                canvas_width: 640,
                canvas_height: 480,
            },
            1280,
            720,
        );

        let [FastPathInputEvent::MouseEvent(event)] = events.as_slice() else {
            panic!("expected one wheel event");
        };

        assert!(event.flags.contains(PointerFlags::VERTICAL_WHEEL));
        assert_eq!(event.number_of_wheel_rotation_units, 120);
        assert_eq!((event.x_position, event.y_position), (640, 360));
    }

    #[test]
    fn mouse_wheel_zero_delta_is_ignored() {
        let events = input_event_to_fastpath(
            &InputEvent::MouseWheel {
                delta_y: 0.0,
                x: 320.0,
                y: 240.0,
                canvas_width: 640,
                canvas_height: 480,
            },
            1280,
            720,
        );

        assert!(events.is_empty());
    }
}
