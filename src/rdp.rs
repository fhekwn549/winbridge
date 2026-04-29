use crate::error::{RdpError, WinbridgeResult};
use glib::translate::IntoGlib;
use gtk4 as gtk;
use gtk4::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;

type RdpTlsStream = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;
type RdpFramed = ironrdp_tokio::TokioFramed<RdpTlsStream>;

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
        let session = self.connect().await?;

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

    async fn connect(&self) -> WinbridgeResult<RdpSessionConnection> {
        use ironrdp::connector;
        use ironrdp_tokio::reqwest::ReqwestNetworkClient;
        use tokio::net::TcpStream;

        let stream = TcpStream::connect(self.addr)
            .await
            .map_err(|err| RdpError::Handshake(format!("TCP 연결 실패: {err}")))?;
        let client_addr = stream
            .local_addr()
            .map_err(|err| RdpError::Handshake(format!("local address 확인 실패: {err}")))?;

        let config = self.build_config();
        let mut framed = ironrdp_tokio::TokioFramed::new(stream);
        let mut connector = connector::ClientConnector::new(config, client_addr);

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

impl RdpWindow {
    pub fn open(
        app: &gtk::Application,
        vm_ip: &str,
        password: &str,
        on_close: Arc<dyn Fn() + Send + Sync>,
    ) -> WinbridgeResult<()> {
        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .title("KakaoTalk")
            .default_width(1280)
            .default_height(720)
            .build();

        let drawing = gtk::DrawingArea::new();
        drawing.set_hexpand(true);
        drawing.set_vexpand(true);
        win.set_child(Some(&drawing));

        let (tx, rx) = glib::MainContext::channel::<RdpFrame>(glib::Priority::default());
        let latest_frame: std::rc::Rc<std::cell::RefCell<Option<RdpFrame>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        {
            let latest_frame = latest_frame.clone();
            let drawing_weak = drawing.downgrade();
            rx.attach(None, move |frame| {
                *latest_frame.borrow_mut() = Some(frame);
                if let Some(drawing) = drawing_weak.upgrade() {
                    drawing.queue_draw();
                }
                glib::ControlFlow::Continue
            });
        }

        {
            let latest_frame = latest_frame.clone();
            drawing.set_draw_func(move |_drawing, cr, width, height| {
                if let Some(frame) = latest_frame.borrow().as_ref() {
                    paint_frame_on_cairo(cr, frame, width, height);
                }
            });
        }

        let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<InputEvent>();
        let key_controller = gtk::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let click_gesture = gtk::GestureClick::new();
        let motion_controller = gtk::EventControllerMotion::new();

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
            let tx = input_tx.clone();
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
            let tx = input_tx;
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

        drawing.add_controller(click_gesture);
        drawing.add_controller(motion_controller);
        drawing.set_can_focus(true);
        drawing.grab_focus();
        win.add_controller(key_controller);

        win.connect_close_request(move |_| {
            (on_close)();
            glib::Propagation::Proceed
        });

        let vm_ip = vm_ip.to_string();
        let password = password.to_string();
        tokio::spawn(async move {
            if let Err(err) = run_rdp_loop(&vm_ip, &password, tx, input_rx).await {
                tracing::error!("RDP loop exited: {err}");
            }
        });

        win.present();
        Ok(())
    }
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
}

fn paint_frame_on_cairo(cr: &gtk::cairo::Context, frame: &RdpFrame, canvas_w: i32, canvas_h: i32) {
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

    let scale_x = f64::from(canvas_w) / f64::from(frame.width);
    let scale_y = f64::from(canvas_h) / f64::from(frame.height);
    let scale = scale_x.min(scale_y).max(0.01);
    let offset_x = (f64::from(canvas_w) - f64::from(frame.width) * scale) / 2.0;
    let offset_y = (f64::from(canvas_h) - f64::from(frame.height) * scale) / 2.0;

    let _ = cr.save();
    cr.translate(offset_x, offset_y);
    cr.scale(scale, scale);
    let _ = cr.set_source_surface(&surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

async fn run_rdp_loop(
    vm_ip: &str,
    password: &str,
    frame_tx: glib::Sender<RdpFrame>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<InputEvent>,
) -> WinbridgeResult<()> {
    use ironrdp::session::image::DecodedImage;
    use ironrdp::session::ActiveStage;
    use ironrdp_graphics::image_processing::PixelFormat;

    let username =
        std::env::var("WINBRIDGE_ADMIN_USER").unwrap_or_else(|_| "Administrator".to_string());
    let probe = RdpHeadlessProbe::new(vm_ip, 3389, &username, password)?;
    let session = probe.connect().await?;
    let width = session.connection_result.desktop_size.width;
    let height = session.connection_result.desktop_size.height;
    let mut framed = session.framed;
    let mut active_stage = ActiveStage::new(session.connection_result);
    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);

    send_decoded_frame(&frame_tx, &image)?;

    loop {
        tokio::select! {
            pdu = framed.read_pdu() => {
                let (action, payload) = pdu
                    .map_err(|err| RdpError::Disconnected(format!("RDP PDU read 실패: {err}")))?;
                let outputs = active_stage
                    .process(&mut image, action, &payload)
                    .map_err(|err| RdpError::Protocol(format!("RDP active stage 처리 실패: {err}")))?;
                handle_active_outputs(&mut framed, &frame_tx, &image, outputs).await?;
            }
            maybe_event = input_rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        let input_events = input_event_to_fastpath(&event, width, height);
                        if input_events.is_empty() {
                            tracing::trace!(?event, "RDP input event ignored");
                            continue;
                        }
                        if matches!(event, InputEvent::KeyPress { .. } | InputEvent::KeyRelease { .. }) {
                            tracing::debug!(?event, "RDP key event sending");
                        }

                        let outputs = active_stage
                            .process_fastpath_input(&mut image, &input_events)
                            .map_err(|err| RdpError::Protocol(format!("RDP input 처리 실패: {err}")))?;
                        handle_active_outputs(&mut framed, &frame_tx, &image, outputs).await?;
                    }
                    None => return Ok(()),
                }
            }
        }
    }
}

async fn handle_active_outputs(
    framed: &mut RdpFramed,
    frame_tx: &glib::Sender<RdpFrame>,
    image: &ironrdp::session::image::DecodedImage,
    outputs: Vec<ironrdp::session::ActiveStageOutput>,
) -> WinbridgeResult<()> {
    use ironrdp::session::ActiveStageOutput;
    use ironrdp_tokio::FramedWrite as _;

    for output in outputs {
        match output {
            ActiveStageOutput::ResponseFrame(frame) => {
                if !frame.is_empty() {
                    framed
                        .write_all(&frame)
                        .await
                        .map_err(|err| RdpError::Disconnected(format!("RDP response write 실패: {err}")))?;
                }
            }
            ActiveStageOutput::GraphicsUpdate(_rect) => {
                send_decoded_frame(frame_tx, image)?;
            }
            ActiveStageOutput::Terminate(reason) => {
                return Err(RdpError::Disconnected(format!(
                    "server terminated session: {reason}"
                ))
                .into());
            }
            ActiveStageOutput::PointerDefault
            | ActiveStageOutput::PointerHidden
            | ActiveStageOutput::PointerPosition { .. }
            | ActiveStageOutput::PointerBitmap(_)
            | ActiveStageOutput::DeactivateAll(_) => {}
        }
    }

    Ok(())
}

fn send_decoded_frame(
    frame_tx: &glib::Sender<RdpFrame>,
    image: &ironrdp::session::image::DecodedImage,
) -> WinbridgeResult<()> {
    let bgra = cairo_bgra_from_decoded(image)?;

    frame_tx
        .send(RdpFrame {
            width: image.width(),
            height: image.height(),
            bgra: Arc::new(bgra),
        })
        .map_err(|_| RdpError::Disconnected("GTK frame receiver closed".to_string()).into())
}

fn cairo_bgra_from_decoded(image: &ironrdp::session::image::DecodedImage) -> WinbridgeResult<Vec<u8>> {
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

fn input_event_to_fastpath(
    event: &InputEvent,
    desktop_width: u16,
    desktop_height: u16,
) -> Vec<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    use ironrdp_pdu::input::fast_path::FastPathInputEvent;
    use ironrdp_pdu::input::mouse::{MousePdu, PointerFlags};

    match event {
        InputEvent::KeyPress {
            keyval, keycode, ..
        } => keyboard_event(*keyval, *keycode, false).into_iter().collect(),
        InputEvent::KeyRelease {
            keyval, keycode, ..
        } => keyboard_event(*keyval, *keycode, true).into_iter().collect(),
        InputEvent::MouseMove {
            x,
            y,
            canvas_width,
            canvas_height,
        } => canvas_to_desktop(*x, *y, *canvas_width, *canvas_height, desktop_width, desktop_height)
            .map(|(x_position, y_position)| {
                vec![FastPathInputEvent::MouseEvent(MousePdu {
                    flags: PointerFlags::MOVE,
                    number_of_wheel_rotation_units: 0,
                    x_position,
                    y_position,
                })]
            })
            .unwrap_or_default(),
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
            let Some((x_position, y_position)) =
                canvas_to_desktop(*x, *y, *canvas_width, *canvas_height, desktop_width, desktop_height)
            else {
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

fn keyboard_event(
    keyval: u32,
    keycode: u32,
    release: bool,
) -> Option<ironrdp_pdu::input::fast_path::FastPathInputEvent> {
    use ironrdp_pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};

    let (scan_code, extended) = keyval_to_scancode(keyval).or_else(|| {
        let scan_code = keycode.checked_sub(8).unwrap_or(keycode);
        u8::try_from(scan_code).ok().map(|scan_code| (scan_code, false))
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

fn keyval_to_scancode(keyval: u32) -> Option<(u8, bool)> {
    if let Some(ch) = char::from_u32(keyval).map(|ch| ch.to_ascii_lowercase()) {
        if let Some(scan_code) = ascii_key_to_scancode(ch) {
            return Some((scan_code, false));
        }
    }

    let scan_code = match keyval {
        0xff08 => 0x0e, // BackSpace
        0xff09 => 0x0f, // Tab
        0xff0d => 0x1c, // Return
        0xff1b => 0x01, // Escape
        0xffe1 => 0x2a, // Shift_L
        0xffe2 => 0x36, // Shift_R
        0xffe5 => 0x3a, // Caps_Lock
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
        0xff31 | 0xff32 | 0xff33 => 0x38, // Hangul / Hangul_Start / Hangul_End -> Right Alt
        0xff34 => 0x1d, // Hangul_Hanja -> Right Ctrl
        0xff50 => 0x47, // Home
        0xff51 => 0x4b, // Left
        0xff52 => 0x48, // Up
        0xff53 => 0x4d, // Right
        0xff54 => 0x50, // Down
        0xff55 => 0x49, // Page_Up
        0xff56 => 0x51, // Page_Down
        0xff57 => 0x4f, // End
        0xff63 => 0x52, // Insert
        0xffff => 0x53, // Delete
        0xffe4 => 0x1d, // Control_R
        0xffea => 0x38, // Alt_R
        0xffeb => 0x5b, // Super_L
        0xffec => 0x5c, // Super_R
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
        assert_eq!(canvas_to_desktop(640.0, 360.0, 1280, 720, 1280, 720), Some((640, 360)));
        assert_eq!(canvas_to_desktop(320.0, 240.0, 640, 480, 1280, 720), Some((640, 360)));
        assert_eq!(canvas_to_desktop(0.0, 0.0, 640, 480, 1280, 720), Some((0, 0)));
    }

    #[test]
    fn keyval_to_scancode_maps_ascii_and_extended_keys() {
        assert_eq!(keyval_to_scancode(u32::from('a')), Some((0x1e, false)));
        assert_eq!(keyval_to_scancode(u32::from('A')), Some((0x1e, false)));
        assert_eq!(keyval_to_scancode(u32::from('?')), Some((0x35, false)));
        assert_eq!(keyval_to_scancode(0xff51), Some((0x4b, true)));
        assert_eq!(keyval_to_scancode(0xff31), Some((0x38, true)));
        assert_eq!(keyval_to_scancode(0xff34), Some((0x1d, true)));
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
}
