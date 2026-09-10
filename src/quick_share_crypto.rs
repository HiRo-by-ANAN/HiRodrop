//! UKEY2 responder and SecureMessage channel used by Quick Share.

use crate::quick_share_wire::{
    DeviceToDeviceMessage, EcP256PublicKey, EncryptionScheme, GcmMetadata, GcmMetadataType,
    GenericPublicKey, HeaderAndBody, OfflineFrame, PublicKeyType, SecureHeader, SecureMessage,
    SignatureScheme, Ukey2ClientFinished, Ukey2ClientInit, Ukey2HandshakeCipher, Ukey2Message,
    Ukey2MessageType, Ukey2ServerInit,
};
use aes::Aes256;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use p256::{
    ecdh::diffie_hellman, elliptic_curve::sec1::ToEncodedPoint, EncodedPoint, FieldBytes,
    PublicKey, SecretKey,
};
use prost::Message;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256, Sha512};

type Aes256CbcEncryptor = cbc::Encryptor<Aes256>;
type Aes256CbcDecryptor = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

const NEXT_PROTOCOL: &str = "AES_256_CBC-HMAC_SHA256";
const D2D_SALT: [u8; 32] = [
    0x82, 0xAA, 0x55, 0xA0, 0xD3, 0x97, 0xF8, 0x83, 0x46, 0xCA, 0x1C, 0xEE, 0x8D, 0x39, 0x09, 0xB9,
    0x5F, 0x13, 0xFA, 0x7D, 0xEB, 0x1D, 0x4A, 0xB3, 0x83, 0x76, 0xB8, 0x25, 0x6D, 0xA8, 0x55, 0x10,
];

#[derive(Debug, thiserror::Error)]
pub enum QuickShareCryptoError {
    #[error("invalid protobuf: {0}")]
    Protobuf(#[from] prost::DecodeError),
    #[error("unexpected UKEY2 message type")]
    UnexpectedUkey2Message,
    #[error("unsupported or malformed UKEY2 client init")]
    InvalidClientInit,
    #[error("UKEY2 client-finish commitment did not match")]
    CommitmentMismatch,
    #[error("invalid P-256 public key")]
    InvalidPublicKey,
    #[error("key derivation failed")]
    KeyDerivation,
    #[error("invalid SecureMessage envelope")]
    InvalidSecureMessage,
    #[error("SecureMessage authentication failed")]
    AuthenticationFailed,
    #[error("SecureMessage decryption failed")]
    DecryptionFailed,
    #[error("unexpected encrypted sequence number: expected {expected}, got {actual}")]
    SequenceMismatch { expected: i32, actual: i32 },
}

/// Server half of the three-message UKEY2 P-256 handshake.
pub struct Ukey2Responder {
    commitment: Vec<u8>,
    client_init_raw: Vec<u8>,
    server_init_raw: Vec<u8>,
    secret_key: SecretKey,
}

/// Client half of the UKEY2 P-256 exchange, used by desktop -> Android sends.
pub struct Ukey2Initiator {
    client_init_raw: Vec<u8>,
    client_finish_raw: Vec<u8>,
    secret_key: SecretKey,
}

impl Ukey2Initiator {
    /// Generate the committed ClientInit and retain the matching ClientFinish.
    pub fn begin() -> Result<(Self, Vec<u8>), QuickShareCryptoError> {
        let secret_key = SecretKey::random(&mut OsRng);
        let finish = Ukey2ClientFinished {
            public_key: Some(encode_generic_public_key(&secret_key.public_key()).encode_to_vec()),
        };
        let client_finish_raw = Ukey2Message {
            message_type: Some(Ukey2MessageType::ClientFinish as i32),
            message_data: Some(finish.encode_to_vec()),
        }
        .encode_to_vec();
        let commitment = Sha512::digest(&client_finish_raw).to_vec();
        let mut random = vec![0_u8; 32];
        OsRng.fill_bytes(&mut random);
        let init = Ukey2ClientInit {
            version: Some(1),
            random: Some(random),
            cipher_commitments: vec![crate::quick_share_wire::CipherCommitment {
                handshake_cipher: Some(Ukey2HandshakeCipher::P256Sha512 as i32),
                commitment: Some(commitment),
            }],
            next_protocol: Some(NEXT_PROTOCOL.into()),
        };
        let client_init_raw = Ukey2Message {
            message_type: Some(Ukey2MessageType::ClientInit as i32),
            message_data: Some(init.encode_to_vec()),
        }
        .encode_to_vec();
        Ok((
            Self {
                client_init_raw: client_init_raw.clone(),
                client_finish_raw,
                secret_key,
            },
            client_init_raw,
        ))
    }

    /// Validate ServerInit and return the committed ClientFinish, encrypted
    /// channel, display PIN, and UKEY2 authentication key.
    pub fn complete(
        self,
        server_init_raw: &[u8],
    ) -> Result<(Vec<u8>, SecureChannel, String, [u8; 32]), QuickShareCryptoError> {
        let outer = Ukey2Message::decode(server_init_raw)?;
        if outer.message_type != Some(Ukey2MessageType::ServerInit as i32) {
            return Err(QuickShareCryptoError::UnexpectedUkey2Message);
        }
        let init = Ukey2ServerInit::decode(
            outer
                .message_data
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidClientInit)?,
        )?;
        if init.version != Some(1)
            || init.random.as_ref().map(Vec::len) != Some(32)
            || init.handshake_cipher != Some(Ukey2HandshakeCipher::P256Sha512 as i32)
        {
            return Err(QuickShareCryptoError::InvalidClientInit);
        }
        let generic_key = GenericPublicKey::decode(
            init.public_key
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidPublicKey)?,
        )?;
        let peer = decode_generic_public_key(&generic_key)?;
        let shared = diffie_hellman(self.secret_key.to_nonzero_scalar(), peer.as_affine());
        let derived_secret = Sha256::digest(shared.raw_secret_bytes());
        let mut transcript = self.client_init_raw;
        transcript.extend_from_slice(server_init_raw);
        let (keys, auth_key) = derive_session_keys(&derived_secret, &transcript, false)?;
        let pin = pin_code(&auth_key);
        Ok((
            self.client_finish_raw,
            SecureChannel::new(keys),
            pin,
            auth_key,
        ))
    }
}

impl Ukey2Responder {
    /// Validate ClientInit and return `(responder_state, serialized ServerInit)`.
    pub fn accept_client_init(raw: &[u8]) -> Result<(Self, Vec<u8>), QuickShareCryptoError> {
        let outer = Ukey2Message::decode(raw)?;
        if outer.message_type != Some(Ukey2MessageType::ClientInit as i32) {
            return Err(QuickShareCryptoError::UnexpectedUkey2Message);
        }
        let init = Ukey2ClientInit::decode(
            outer
                .message_data
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidClientInit)?,
        )?;
        if init.version != Some(1)
            || init.random.as_ref().map(Vec::len) != Some(32)
            || init.next_protocol.as_deref() != Some(NEXT_PROTOCOL)
        {
            return Err(QuickShareCryptoError::InvalidClientInit);
        }
        let commitment = init
            .cipher_commitments
            .iter()
            .find(|item| item.handshake_cipher == Some(Ukey2HandshakeCipher::P256Sha512 as i32))
            .and_then(|item| item.commitment.clone())
            .filter(|value| value.len() == 64)
            .ok_or(QuickShareCryptoError::InvalidClientInit)?;

        let secret_key = SecretKey::random(&mut OsRng);
        let public_key = encode_generic_public_key(&secret_key.public_key()).encode_to_vec();
        let mut random = vec![0_u8; 32];
        OsRng.fill_bytes(&mut random);
        let init = Ukey2ServerInit {
            version: Some(1),
            random: Some(random),
            handshake_cipher: Some(Ukey2HandshakeCipher::P256Sha512 as i32),
            public_key: Some(public_key),
        };
        let server_init_raw = Ukey2Message {
            message_type: Some(Ukey2MessageType::ServerInit as i32),
            message_data: Some(init.encode_to_vec()),
        }
        .encode_to_vec();

        Ok((
            Self {
                commitment,
                client_init_raw: raw.to_vec(),
                server_init_raw: server_init_raw.clone(),
                secret_key,
            },
            server_init_raw,
        ))
    }

    /// Validate ClientFinish, perform ECDH, and establish the encrypted channel.
    pub fn complete(
        self,
        client_finish_raw: &[u8],
    ) -> Result<(SecureChannel, String, String), QuickShareCryptoError> {
        let actual_commitment = Sha512::digest(client_finish_raw);
        if actual_commitment.as_slice() != self.commitment {
            return Err(QuickShareCryptoError::CommitmentMismatch);
        }

        let outer = Ukey2Message::decode(client_finish_raw)?;
        if outer.message_type != Some(Ukey2MessageType::ClientFinish as i32) {
            return Err(QuickShareCryptoError::UnexpectedUkey2Message);
        }
        let finish = Ukey2ClientFinished::decode(
            outer
                .message_data
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidPublicKey)?,
        )?;
        let generic_key = GenericPublicKey::decode(
            finish
                .public_key
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidPublicKey)?,
        )?;
        let peer = decode_generic_public_key(&generic_key)?;
        let shared = diffie_hellman(self.secret_key.to_nonzero_scalar(), peer.as_affine());
        let derived_secret = Sha256::digest(shared.raw_secret_bytes());
        let mut transcript = self.client_init_raw;
        transcript.extend_from_slice(&self.server_init_raw);
        let (keys, auth_key) = derive_session_keys(&derived_secret, &transcript, true)?;
        let pin = pin_code(&auth_key);
        let digest = Sha256::digest(peer.to_sec1_bytes().as_ref());
        let peer_fingerprint: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        Ok((SecureChannel::new(keys), pin, peer_fingerprint))
    }
}

#[derive(Clone)]
struct SessionKeys {
    decrypt_key: [u8; 32],
    receive_hmac_key: [u8; 32],
    encrypt_key: [u8; 32],
    send_hmac_key: [u8; 32],
}

/// Stateful AES-256-CBC + HMAC-SHA256 channel with replay protection.
pub struct SecureChannel {
    keys: SessionKeys,
    receive_sequence: i32,
    send_sequence: i32,
}

impl SecureChannel {
    fn new(keys: SessionKeys) -> Self {
        Self {
            keys,
            receive_sequence: 0,
            send_sequence: 0,
        }
    }

    pub fn encrypt_offline_frame(
        &mut self,
        frame: &OfflineFrame,
    ) -> Result<Vec<u8>, QuickShareCryptoError> {
        self.send_sequence = self
            .send_sequence
            .checked_add(1)
            .ok_or(QuickShareCryptoError::InvalidSecureMessage)?;
        let plaintext = DeviceToDeviceMessage {
            message: Some(frame.encode_to_vec()),
            sequence_number: Some(self.send_sequence),
        }
        .encode_to_vec();

        let mut iv = [0_u8; 16];
        OsRng.fill_bytes(&mut iv);
        let body = Aes256CbcEncryptor::new_from_slices(&self.keys.encrypt_key, &iv)
            .map_err(|_| QuickShareCryptoError::KeyDerivation)?
            .encrypt_padded_vec_mut::<Pkcs7>(&plaintext);
        let metadata = GcmMetadata {
            metadata_type: Some(GcmMetadataType::DeviceToDeviceMessage as i32),
            version: Some(1),
        }
        .encode_to_vec();
        let header_and_body = HeaderAndBody {
            header: SecureHeader {
                signature_scheme: Some(SignatureScheme::HmacSha256 as i32),
                encryption_scheme: Some(EncryptionScheme::Aes256Cbc as i32),
                iv: Some(iv.to_vec()),
                public_metadata: Some(metadata),
            },
            body,
        }
        .encode_to_vec();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.keys.send_hmac_key)
            .map_err(|_| QuickShareCryptoError::KeyDerivation)?;
        mac.update(&header_and_body);
        Ok(SecureMessage {
            header_and_body,
            signature: mac.finalize().into_bytes().to_vec(),
        }
        .encode_to_vec())
    }

    pub fn decrypt_offline_frame(
        &mut self,
        secure_message: &[u8],
    ) -> Result<OfflineFrame, QuickShareCryptoError> {
        let secure = SecureMessage::decode(secure_message)?;
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.keys.receive_hmac_key)
            .map_err(|_| QuickShareCryptoError::KeyDerivation)?;
        mac.update(&secure.header_and_body);
        mac.verify_slice(&secure.signature)
            .map_err(|_| QuickShareCryptoError::AuthenticationFailed)?;

        let envelope = HeaderAndBody::decode(secure.header_and_body.as_slice())?;
        if envelope.header.signature_scheme != Some(SignatureScheme::HmacSha256 as i32)
            || envelope.header.encryption_scheme != Some(EncryptionScheme::Aes256Cbc as i32)
        {
            return Err(QuickShareCryptoError::InvalidSecureMessage);
        }
        let iv = envelope
            .header
            .iv
            .as_deref()
            .filter(|value| value.len() == 16)
            .ok_or(QuickShareCryptoError::InvalidSecureMessage)?;
        let plaintext = Aes256CbcDecryptor::new_from_slices(&self.keys.decrypt_key, iv)
            .map_err(|_| QuickShareCryptoError::KeyDerivation)?
            .decrypt_padded_vec_mut::<Pkcs7>(&envelope.body)
            .map_err(|_| QuickShareCryptoError::DecryptionFailed)?;
        let message = DeviceToDeviceMessage::decode(plaintext.as_slice())?;
        let actual = message
            .sequence_number
            .ok_or(QuickShareCryptoError::InvalidSecureMessage)?;
        let expected = self
            .receive_sequence
            .checked_add(1)
            .ok_or(QuickShareCryptoError::InvalidSecureMessage)?;
        if actual != expected {
            return Err(QuickShareCryptoError::SequenceMismatch { expected, actual });
        }
        self.receive_sequence = actual;
        OfflineFrame::decode(
            message
                .message
                .as_deref()
                .ok_or(QuickShareCryptoError::InvalidSecureMessage)?,
        )
        .map_err(Into::into)
    }
}

fn encode_generic_public_key(public_key: &PublicKey) -> GenericPublicKey {
    let encoded = public_key.to_encoded_point(false);
    GenericPublicKey {
        key_type: Some(PublicKeyType::EcP256 as i32),
        ec_p256_public_key: Some(EcP256PublicKey {
            x: signed_coordinate(encoded.x().expect("uncompressed P-256 x")),
            y: signed_coordinate(encoded.y().expect("uncompressed P-256 y")),
        }),
    }
}

fn signed_coordinate(coordinate: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(33);
    if coordinate.first().is_some_and(|byte| byte & 0x80 != 0) {
        output.push(0);
    }
    output.extend_from_slice(coordinate);
    output
}

fn decode_generic_public_key(key: &GenericPublicKey) -> Result<PublicKey, QuickShareCryptoError> {
    if key.key_type != Some(PublicKeyType::EcP256 as i32) {
        return Err(QuickShareCryptoError::InvalidPublicKey);
    }
    let key = key
        .ec_p256_public_key
        .as_ref()
        .ok_or(QuickShareCryptoError::InvalidPublicKey)?;
    let x = unsigned_coordinate(&key.x)?;
    let y = unsigned_coordinate(&key.y)?;
    let point = EncodedPoint::from_affine_coordinates(
        FieldBytes::from_slice(&x),
        FieldBytes::from_slice(&y),
        false,
    );
    PublicKey::from_sec1_bytes(point.as_bytes())
        .map_err(|_| QuickShareCryptoError::InvalidPublicKey)
}

fn unsigned_coordinate(value: &[u8]) -> Result<[u8; 32], QuickShareCryptoError> {
    let value = if value.len() == 33 && value.first() == Some(&0) {
        &value[1..]
    } else {
        value
    };
    if value.is_empty() || value.len() > 32 {
        return Err(QuickShareCryptoError::InvalidPublicKey);
    }
    let mut result = [0_u8; 32];
    result[32 - value.len()..].copy_from_slice(value);
    Ok(result)
}

fn derive_session_keys(
    secret: &[u8],
    transcript: &[u8],
    server: bool,
) -> Result<(SessionKeys, [u8; 32]), QuickShareCryptoError> {
    let auth_key = hkdf(secret, b"UKEY2 v1 auth", transcript)?;
    let next_secret = hkdf(secret, b"UKEY2 v1 next", transcript)?;
    let client_d2d = hkdf(&next_secret, &D2D_SALT, b"client")?;
    let server_d2d = hkdf(&next_secret, &D2D_SALT, b"server")?;
    let secure_message_salt = Sha256::digest(b"SecureMessage");
    let client_encrypt = hkdf(&client_d2d, &secure_message_salt, b"ENC:2")?;
    let client_hmac = hkdf(&client_d2d, &secure_message_salt, b"SIG:1")?;
    let server_encrypt = hkdf(&server_d2d, &secure_message_salt, b"ENC:2")?;
    let server_hmac = hkdf(&server_d2d, &secure_message_salt, b"SIG:1")?;
    let keys = if server {
        SessionKeys {
            decrypt_key: client_encrypt,
            receive_hmac_key: client_hmac,
            encrypt_key: server_encrypt,
            send_hmac_key: server_hmac,
        }
    } else {
        SessionKeys {
            decrypt_key: server_encrypt,
            receive_hmac_key: server_hmac,
            encrypt_key: client_encrypt,
            send_hmac_key: client_hmac,
        }
    };
    Ok((keys, auth_key))
}

fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; 32], QuickShareCryptoError> {
    let mut output = [0_u8; 32];
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, &mut output)
        .map_err(|_| QuickShareCryptoError::KeyDerivation)?;
    Ok(output)
}

pub fn pin_code(auth_key: &[u8]) -> String {
    let mut hash = 0_i32;
    let mut multiplier = 1_i32;
    for byte in auth_key {
        hash = (hash + (*byte as i8 as i32) * multiplier) % 9_973;
        multiplier = (multiplier * 31) % 9_973;
    }
    format!("{:04}", hash.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick_share_wire::{offline_v1, OfflineFrameType};

    #[test]
    fn secure_channels_authenticate_encrypt_and_enforce_sequence() {
        let secret = [0x42; 32];
        let transcript = b"client-init || server-init";
        let (server_keys, server_auth) = derive_session_keys(&secret, transcript, true).unwrap();
        let (client_keys, client_auth) = derive_session_keys(&secret, transcript, false).unwrap();
        assert_eq!(server_auth, client_auth);

        let mut server = SecureChannel::new(server_keys);
        let mut client = SecureChannel::new(client_keys);
        let frame = offline_v1(OfflineFrameType::KeepAlive);
        let encrypted = client.encrypt_offline_frame(&frame).unwrap();
        let decrypted = server.decrypt_offline_frame(&encrypted).unwrap();
        assert_eq!(
            decrypted.v1.unwrap().frame_type,
            Some(OfflineFrameType::KeepAlive as i32)
        );
        assert!(matches!(
            server.decrypt_offline_frame(&encrypted),
            Err(QuickShareCryptoError::SequenceMismatch { .. })
        ));
    }

    #[test]
    fn malformed_p256_coordinates_are_rejected() {
        let key = GenericPublicKey {
            key_type: Some(PublicKeyType::EcP256 as i32),
            ec_p256_public_key: Some(EcP256PublicKey {
                x: vec![1; 34],
                y: vec![2; 32],
            }),
        };
        assert!(matches!(
            decode_generic_public_key(&key),
            Err(QuickShareCryptoError::InvalidPublicKey)
        ));
    }

    #[test]
    fn initiator_and_responder_establish_matching_channels() {
        let (initiator, client_init) = Ukey2Initiator::begin().unwrap();
        let (responder, server_init) = Ukey2Responder::accept_client_init(&client_init).unwrap();
        let (client_finish, mut client, client_pin, client_auth) =
            initiator.complete(&server_init).unwrap();
        let (mut server, server_pin, _fingerprint) = responder.complete(&client_finish).unwrap();
        assert_eq!(client_pin, server_pin);
        assert_eq!(client_pin, pin_code(&client_auth));

        let frame = offline_v1(OfflineFrameType::KeepAlive);
        let encrypted = client.encrypt_offline_frame(&frame).unwrap();
        assert_eq!(
            server
                .decrypt_offline_frame(&encrypted)
                .unwrap()
                .v1
                .unwrap()
                .frame_type,
            Some(OfflineFrameType::KeepAlive as i32)
        );
    }
}
