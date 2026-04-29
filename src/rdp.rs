use crate::error::{RdpError, WinbridgeResult};
use gtk4 as gtk;
use gtk4::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;

/// Headless RDP handshake probe. Frame handling is added in the GTK phase.
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
        use ironrdp::connector;
        use ironrdp::connector::Credentials;
        use ironrdp::pdu::gcc::KeyboardType;
        use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
        use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
        use ironrdp_tokio::reqwest::ReqwestNetworkClient;
        use tokio::net::TcpStream;

        let stream = TcpStream::connect(self.addr)
            .await
            .map_err(|err| RdpError::Handshake(format!("TCP 연결 실패: {err}")))?;
        let client_addr = stream
            .local_addr()
            .map_err(|err| RdpError::Handshake(format!("local address 확인 실패: {err}")))?;

        let config = connector::Config {
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
        };

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

        Ok(RdpProbeResult {
            width: result.desktop_size.width,
            height: result.desktop_size.height,
            bits_per_pixel: 32,
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
    pub fn open(app: &gtk::Application, vm_ip: &str, password: &str) -> WinbridgeResult<()> {
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

        let vm_ip = vm_ip.to_string();
        let password = password.to_string();
        tokio::spawn(async move {
            if let Err(err) = run_rdp_loop(&vm_ip, &password, tx).await {
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
    tx: glib::Sender<RdpFrame>,
) -> WinbridgeResult<()> {
    let username =
        std::env::var("WINBRIDGE_ADMIN_USER").unwrap_or_else(|_| "Administrator".to_string());
    let probe = RdpHeadlessProbe::new(vm_ip, 3389, &username, password)?;
    let result = probe.probe().await?;
    let bgra = placeholder_frame(result.width, result.height);

    tx.send(RdpFrame {
        width: result.width,
        height: result.height,
        bgra: Arc::new(bgra),
    })
    .map_err(|_| RdpError::Disconnected("GTK frame receiver closed".to_string()))?;

    Ok(())
}

fn placeholder_frame(width: u16, height: u16) -> Vec<u8> {
    let mut bgra = vec![0; usize::from(width) * usize::from(height) * 4];
    for y in 0..height {
        for x in 0..width {
            let idx = (usize::from(y) * usize::from(width) + usize::from(x)) * 4;
            bgra[idx] = 0x30;
            bgra[idx + 1] = 0x2a + ((u32::from(y) * 40 / u32::from(height.max(1))) as u8);
            bgra[idx + 2] = 0x24 + ((u32::from(x) * 40 / u32::from(width.max(1))) as u8);
            bgra[idx + 3] = 0xff;
        }
    }
    bgra
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
}
