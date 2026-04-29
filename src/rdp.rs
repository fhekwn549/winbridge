use crate::error::{RdpError, WinbridgeResult};
use std::net::SocketAddr;

/// Headless RDP handshake probe. Frame handling is added in the GTK phase.
#[derive(Debug)]
pub struct RdpHeadlessProbe {
    addr: SocketAddr,
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
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    pub async fn probe(&self) -> WinbridgeResult<RdpProbeResult> {
        let _ = (&self.addr, &self.username, &self.password);
        Err(RdpError::Handshake("IronRDP probe not yet implemented".into()).into())
    }
}

#[derive(Debug)]
pub struct RdpProbeResult {
    pub width: u16,
    pub height: u16,
    pub bits_per_pixel: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_invalid_ip_address() {
        let err = RdpHeadlessProbe::new("not an ip", 3389, "Administrator", "secret")
            .unwrap_err();
        assert!(format!("{}", err).contains("RDP"));
    }

    #[test]
    fn new_accepts_ipv4_address_and_port() {
        let probe =
            RdpHeadlessProbe::new("192.168.122.50", 3389, "Administrator", "secret").unwrap();
        assert_eq!(probe.addr.to_string(), "192.168.122.50:3389");
    }
}
