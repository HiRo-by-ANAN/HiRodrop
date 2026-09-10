//! Quick Share QR handoff used when the sender has no Bluetooth adapter.
//!
//! Android scans this URL and briefly advertises a matching mDNS endpoint on
//! the existing LAN.  The QR private key is intentionally ephemeral and must
//! live only for the duration of one send session.

use aes_gcm::{
    aead::{Aead, Payload},
    Aes128Gcm, KeyInit, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hkdf::Hkdf;
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::sec1::ToEncodedPoint,
    SecretKey,
};
use rand::rngs::OsRng;
use sha2::Sha256;
use std::net::{SocketAddr, SocketAddrV6};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::quick_share::QUICK_SHARE_SERVICE;

const QR_URL_PREFIX: &str = "https://quickshare.google/qrcode#key=";

#[derive(Debug, thiserror::Error)]
pub enum QuickShareQrError {
    #[error("Quick Share QR key derivation failed")]
    KeyDerivation,
    #[error("mDNS operation failed: {0}")]
    Mdns(#[from] mdns_sd::Error),
    #[error("timed out waiting for the phone to scan the Quick Share QR code")]
    TimedOut,
    #[error("the Quick Share QR session was cancelled")]
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickShareQrEndpoint {
    pub name: String,
    pub address: SocketAddr,
}

/// One short-lived QR rendezvous.  Dropping it discards the private key.
pub struct QuickShareQrSession {
    secret_key: SecretKey,
    encoded_public_key: [u8; 35],
    advertising_token: [u8; 16],
    name_encryption_key: [u8; 16],
}

impl QuickShareQrSession {
    pub fn generate() -> Result<Self, QuickShareQrError> {
        Self::from_secret_key(SecretKey::random(&mut OsRng))
    }

    fn from_secret_key(secret_key: SecretKey) -> Result<Self, QuickShareQrError> {
        let point = secret_key.public_key().to_encoded_point(true);
        let compressed = point.as_bytes();
        debug_assert_eq!(compressed.len(), 33);
        let mut encoded_public_key = [0_u8; 35];
        encoded_public_key[2..].copy_from_slice(compressed);
        let advertising_token = derive_16(&encoded_public_key, b"advertisingContext")?;
        let name_encryption_key = derive_16(&encoded_public_key, b"encryptionKey")?;
        Ok(Self {
            secret_key,
            encoded_public_key,
            advertising_token,
            name_encryption_key,
        })
    }

    pub fn url(&self) -> String {
        format!(
            "{QR_URL_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(self.encoded_public_key)
        )
    }

    pub fn advertising_token(&self) -> &[u8; 16] {
        &self.advertising_token
    }

    pub fn name_encryption_key(&self) -> &[u8; 16] {
        &self.name_encryption_key
    }

    /// Sign the UKEY2 authentication key in the IEEE P1363 `r || s` format
    /// expected by Android's QR handoff.
    pub fn sign_auth_key(&self, auth_key: &[u8; 32]) -> Vec<u8> {
        let signing_key = SigningKey::from(&self.secret_key);
        let signature: Signature = signing_key.sign(auth_key);
        signature.to_bytes().to_vec()
    }

    /// Wait for the stock Android Quick Share screen to advertise the token
    /// produced by this QR code.  Only already-connected LAN interfaces are
    /// browsed; this does not enable or scan Bluetooth/Wi-Fi Direct.
    pub fn discover_receiver(
        &self,
        timeout: Duration,
    ) -> Result<QuickShareQrEndpoint, QuickShareQrError> {
        self.discover_receiver_cancelable(timeout, &AtomicBool::new(false))
    }

    /// Like [`Self::discover_receiver`], but checks `cancelled` while waiting so
    /// a GUI can immediately abandon an unscanned QR code and switch targets.
    pub fn discover_receiver_cancelable(
        &self,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<QuickShareQrEndpoint, QuickShareQrError> {
        let daemon = ServiceDaemon::new()?;
        let receiver = daemon.browse(QUICK_SHARE_SERVICE)?;
        let deadline = Instant::now() + timeout;
        let result = 'search: loop {
            if cancelled.load(Ordering::Relaxed) {
                break Err(QuickShareQrError::Cancelled);
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break Err(QuickShareQrError::TimedOut);
            };
            match receiver.recv_timeout(remaining.min(Duration::from_millis(200))) {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    for address in socket_addresses(&service) {
                        if let Some(endpoint) = endpoint_if_token_matches(
                            service.get_property_val_str("n"),
                            address,
                            &self.advertising_token,
                            &self.name_encryption_key,
                        ) {
                            break 'search Ok(endpoint);
                        }
                    }
                }
                Ok(_) => {}
                Err(_) if Instant::now() < deadline => {}
                Err(_) => break Err(QuickShareQrError::TimedOut),
            }
        };
        let _ = daemon.stop_browse(QUICK_SHARE_SERVICE);
        let _ = daemon.shutdown();
        result
    }
}

fn endpoint_if_token_matches(
    encoded_info: Option<&str>,
    address: SocketAddr,
    expected_token: &[u8; 16],
    name_encryption_key: &[u8; 16],
) -> Option<QuickShareQrEndpoint> {
    let info = URL_SAFE_NO_PAD.decode(encoded_info?).ok()?;
    if info.len() < 17 {
        return None;
    }
    if info[0] & 0x10 != 0 {
        return decrypt_hidden_name(&info, address, expected_token, name_encryption_key);
    }
    let (name, records_offset) = parse_visible_name(&info)?;
    let mut offset = records_offset;
    while info.len().saturating_sub(offset) >= 2 {
        let record_type = info[offset];
        let length = info[offset + 1] as usize;
        offset += 2;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= info.len())?;
        if record_type == 1 && &info[offset..end] == expected_token {
            return Some(QuickShareQrEndpoint { name, address });
        }
        offset = end;
    }
    None
}

fn parse_visible_name(info: &[u8]) -> Option<(String, usize)> {
    if info.len() < 18 || info[0] & 0x10 != 0 {
        return None;
    }
    let length = info[17] as usize;
    let end = 18_usize
        .checked_add(length)
        .filter(|end| *end <= info.len())?;
    let name = std::str::from_utf8(&info[18..end]).ok()?.to_owned();
    Some((name, end))
}

fn decrypt_hidden_name(
    info: &[u8],
    address: SocketAddr,
    advertising_token: &[u8; 16],
    name_encryption_key: &[u8; 16],
) -> Option<QuickShareQrEndpoint> {
    let cipher = Aes128Gcm::new_from_slice(name_encryption_key).ok()?;
    let mut offset = 17;
    while info.len().saturating_sub(offset) >= 2 {
        let record_type = info[offset];
        let length = info[offset + 1] as usize;
        offset += 2;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= info.len())?;
        let value = &info[offset..end];
        if record_type == 1 && value.len() >= 28 {
            let (nonce, ciphertext_and_tag) = value.split_at(12);
            if let Ok(plaintext) = cipher.decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext_and_tag,
                    aad: advertising_token,
                },
            ) {
                if let Ok(name) = String::from_utf8(plaintext) {
                    return Some(QuickShareQrEndpoint { name, address });
                }
            }
        }
        offset = end;
    }
    None
}

fn socket_addresses(service: &mdns_sd::ResolvedService) -> Vec<SocketAddr> {
    let port = service.get_port();
    let mut addresses = service
        .get_addresses()
        .iter()
        .filter_map(|address| match address {
            ScopedIp::V4(ip) => Some(SocketAddr::new((*ip.addr()).into(), port)),
            ScopedIp::V6(ip) => Some(SocketAddr::V6(SocketAddrV6::new(
                *ip.addr(),
                port,
                0,
                ip.scope_id().index,
            ))),
            _ => None,
        })
        .collect::<Vec<_>>();
    addresses.sort_by_key(SocketAddr::is_ipv6);
    addresses
}

fn derive_16(ikm: &[u8], info: &[u8]) -> Result<[u8; 16], QuickShareQrError> {
    let mut output = [0_u8; 16];
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, &mut output)
        .map_err(|_| QuickShareQrError::KeyDerivation)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn qr_url_contains_versioned_compressed_p256_key() {
        let secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
        let session = QuickShareQrSession::from_secret_key(secret).unwrap();
        let encoded = session.url();
        let key = encoded.strip_prefix(QR_URL_PREFIX).unwrap();
        let decoded = URL_SAFE_NO_PAD.decode(key).unwrap();
        assert_eq!(decoded.len(), 35);
        assert_eq!(&decoded[..2], &[0, 0]);
        assert!(matches!(decoded[2], 2 | 3));
        assert_eq!(session.advertising_token().len(), 16);
        assert_eq!(session.name_encryption_key().len(), 16);
    }

    #[test]
    fn each_send_session_has_a_different_rendezvous() {
        let first = QuickShareQrSession::generate().unwrap();
        let second = QuickShareQrSession::generate().unwrap();
        assert_ne!(first.url(), second.url());
        assert_ne!(first.advertising_token(), second.advertising_token());
    }

    #[test]
    fn visible_endpoint_matches_only_its_qr_token() {
        let token = [0x5a; 16];
        let name = "測試手機";
        let mut info = vec![2_u8; 17];
        info.push(name.len() as u8);
        info.extend_from_slice(name.as_bytes());
        info.extend_from_slice(&[1, 16]);
        info.extend_from_slice(&token);
        let encoded = URL_SAFE_NO_PAD.encode(info);
        let endpoint = endpoint_if_token_matches(
            Some(&encoded),
            "192.168.1.9:44123".parse().unwrap(),
            &token,
            &[0x77; 16],
        )
        .unwrap();
        assert_eq!(endpoint.name, name);
        assert_eq!(endpoint.address, "192.168.1.9:44123".parse().unwrap());
        assert!(endpoint_if_token_matches(
            Some(&encoded),
            "192.168.1.9:44123".parse().unwrap(),
            &[0; 16],
            &[0x77; 16],
        )
        .is_none());
    }

    #[test]
    fn hidden_endpoint_decrypts_name_using_qr_keys() {
        let token = [0x5a; 16];
        let key = [0x77; 16];
        let nonce = [0x33; 12];
        let name = "測試手機";
        let cipher = Aes128Gcm::new_from_slice(&key).unwrap();
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: name.as_bytes(),
                    aad: &token,
                },
            )
            .unwrap();
        let mut info = vec![0x10; 17];
        let length = nonce.len() + encrypted.len();
        info.extend_from_slice(&[1, length as u8]);
        info.extend_from_slice(&nonce);
        info.extend_from_slice(&encrypted);
        let encoded = URL_SAFE_NO_PAD.encode(info);

        let endpoint = endpoint_if_token_matches(
            Some(&encoded),
            "[fe80::1%3]:44123".parse().unwrap(),
            &token,
            &key,
        )
        .unwrap();
        assert_eq!(endpoint.name, name);
        assert!(endpoint.address.is_ipv6());
        assert!(endpoint_if_token_matches(
            Some(&encoded),
            "192.168.1.9:44123".parse().unwrap(),
            &[0; 16],
            &key,
        )
        .is_none());
    }
}
