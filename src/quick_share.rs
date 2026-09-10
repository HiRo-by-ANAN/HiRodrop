//! Quick Share-compatible LAN discovery advertisement.
//!
//! This is intentionally limited to the discovery packet that Android's
//! built-in Quick Share UI understands. It uses the host's existing LAN
//! interfaces and does not enable Bluetooth, Wi-Fi Direct, or a hotspot.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use mdns_sd::{ServiceDaemon, ServiceInfo, UnregisterStatus};
use rand::{rngs::OsRng, RngCore};
use std::time::Duration;

/// Service type used by Nearby Connections / Quick Share for LAN discovery.
pub const QUICK_SHARE_SERVICE: &str = "_FC9F5ED42C8A._tcp.local.";

const PCP_POINT_TO_POINT: u8 = 0x23;
const QUICK_SHARE_SERVICE_ID: [u8; 3] = [0xFC, 0x9F, 0x5E];
const ENDPOINT_ID_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Debug, thiserror::Error)]
pub enum QuickShareError {
    #[error("Quick Share device name must contain 1 to 255 UTF-8 bytes")]
    InvalidDeviceName,
    #[error("Quick Share endpoint ID must contain exactly four ASCII letters or digits")]
    InvalidEndpointId,
    #[error("mDNS operation failed: {0}")]
    Mdns(#[from] mdns_sd::Error),
    #[error("mDNS service did not stop within the requested timeout")]
    ShutdownTimedOut,
}

/// Four-character ephemeral identifier expected by Nearby Connections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuickShareEndpointId([u8; 4]);

impl QuickShareEndpointId {
    pub fn generate() -> Self {
        let mut entropy = [0_u8; 4];
        OsRng.fill_bytes(&mut entropy);
        Self(entropy.map(|byte| ENDPOINT_ID_ALPHABET[byte as usize % ENDPOINT_ID_ALPHABET.len()]))
    }

    pub fn new(value: [u8; 4]) -> Result<Self, QuickShareError> {
        if value.iter().all(u8::is_ascii_alphanumeric) {
            Ok(Self(value))
        } else {
            Err(QuickShareError::InvalidEndpointId)
        }
    }

    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        // Construction guarantees that all four bytes are ASCII.
        std::str::from_utf8(&self.0).expect("validated ASCII endpoint ID")
    }
}

/// The wire values used by Quick Share's endpoint-info field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum QuickShareDeviceType {
    Unknown = 0,
    Phone = 1,
    Tablet = 2,
    Laptop = 3,
}

/// Exact values placed in the DNS-SD instance label and `n` TXT record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickShareAdvertisementData {
    pub endpoint_id: QuickShareEndpointId,
    pub instance_name: String,
    pub endpoint_info: String,
}

impl QuickShareAdvertisementData {
    pub fn generate(display_name: &str) -> Result<Self, QuickShareError> {
        let endpoint_id = QuickShareEndpointId::generate();
        let mut metadata = [0_u8; 16];
        OsRng.fill_bytes(&mut metadata);
        Self::from_parts(
            endpoint_id,
            metadata,
            QuickShareDeviceType::Laptop,
            display_name,
        )
    }

    pub fn from_parts(
        endpoint_id: QuickShareEndpointId,
        metadata: [u8; 16],
        device_type: QuickShareDeviceType,
        display_name: &str,
    ) -> Result<Self, QuickShareError> {
        let name = display_name.as_bytes();
        if name.is_empty() || name.len() > u8::MAX as usize {
            return Err(QuickShareError::InvalidDeviceName);
        }

        let mut instance_bytes = Vec::with_capacity(10);
        instance_bytes.push(PCP_POINT_TO_POINT);
        instance_bytes.extend_from_slice(endpoint_id.as_bytes());
        instance_bytes.extend_from_slice(&QUICK_SHARE_SERVICE_ID);
        instance_bytes.extend_from_slice(&[0, 0]);

        let mut endpoint_info = Vec::with_capacity(18 + name.len());
        endpoint_info.push((device_type as u8) << 1);
        endpoint_info.extend_from_slice(&metadata);
        endpoint_info.push(name.len() as u8);
        endpoint_info.extend_from_slice(name);

        Ok(Self {
            endpoint_id,
            instance_name: URL_SAFE_NO_PAD.encode(instance_bytes),
            endpoint_info: URL_SAFE_NO_PAD.encode(endpoint_info),
        })
    }
}

/// Owns one Quick Share-compatible DNS-SD registration.
pub struct QuickShareAdvertisement {
    daemon: ServiceDaemon,
    fullname: String,
    data: QuickShareAdvertisementData,
}

impl QuickShareAdvertisement {
    /// Advertise a laptop receiver over already-connected LAN interfaces.
    ///
    /// The caller owns `listener_port`; the GUI and daemon pair this record
    /// with the encrypted Quick Share receiver.
    pub fn start(display_name: &str, listener_port: u16) -> Result<Self, QuickShareError> {
        let data = QuickShareAdvertisementData::generate(display_name)?;
        let daemon = ServiceDaemon::new()?;
        let host_name = format!(
            "hiro-{}.local.",
            data.endpoint_id.as_str().to_ascii_lowercase()
        );
        let properties = [("n", data.endpoint_info.as_str())];
        let service = ServiceInfo::new(
            QUICK_SHARE_SERVICE,
            &data.instance_name,
            &host_name,
            "",
            listener_port,
            &properties[..],
        )?
        .enable_addr_auto();
        let fullname = service.get_fullname().to_owned();
        daemon.register(service)?;

        Ok(Self {
            daemon,
            fullname,
            data,
        })
    }

    pub fn stop(self, timeout: Duration) -> Result<(), QuickShareError> {
        let status = self.daemon.unregister(&self.fullname)?;
        match status.recv_timeout(timeout) {
            Ok(UnregisterStatus::OK) => {}
            Ok(_) | Err(_) => return Err(QuickShareError::ShutdownTimedOut),
        }
        let _ = self.daemon.shutdown()?;
        Ok(())
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    pub fn data(&self) -> &QuickShareAdvertisementData {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_label_has_the_expected_nearby_connections_bytes() {
        let endpoint_id = QuickShareEndpointId::new(*b"Ab1Z").unwrap();
        let data = QuickShareAdvertisementData::from_parts(
            endpoint_id,
            [0x11; 16],
            QuickShareDeviceType::Laptop,
            "HiRodrop",
        )
        .unwrap();

        let decoded = URL_SAFE_NO_PAD.decode(data.instance_name).unwrap();
        assert_eq!(
            decoded,
            [0x23, b'A', b'b', b'1', b'Z', 0xFC, 0x9F, 0x5E, 0, 0]
        );
    }

    #[test]
    fn endpoint_info_identifies_a_laptop_and_preserves_utf8_name() {
        let endpoint_id = QuickShareEndpointId::new(*b"aB19").unwrap();
        let metadata = [0xA5; 16];
        let name = "HiRodrop 工作站";
        let data = QuickShareAdvertisementData::from_parts(
            endpoint_id,
            metadata,
            QuickShareDeviceType::Laptop,
            name,
        )
        .unwrap();

        let decoded = URL_SAFE_NO_PAD.decode(data.endpoint_info).unwrap();
        assert_eq!(decoded[0], 6);
        assert_eq!(&decoded[1..17], &metadata);
        assert_eq!(decoded[17] as usize, name.len());
        assert_eq!(&decoded[18..], name.as_bytes());
    }

    #[test]
    fn rejects_names_that_cannot_fit_in_the_wire_length_byte() {
        let endpoint_id = QuickShareEndpointId::new(*b"Ab1Z").unwrap();
        let oversized = "界".repeat(86);
        let result = QuickShareAdvertisementData::from_parts(
            endpoint_id,
            [0; 16],
            QuickShareDeviceType::Laptop,
            &oversized,
        );

        assert!(matches!(result, Err(QuickShareError::InvalidDeviceName)));
    }

    #[test]
    fn endpoint_id_rejects_punctuation() {
        assert!(matches!(
            QuickShareEndpointId::new(*b"A-1Z"),
            Err(QuickShareError::InvalidEndpointId)
        ));
    }
}
