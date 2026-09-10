//! Minimal protobuf wire models needed by the Quick Share receiver.
//!
//! The field numbers and enum values mirror the Apache-2.0/BSD protocol
//! definitions published by Google and Chromium. Unknown fields are ignored,
//! so newer senders remain forward-compatible.

use prost::{Enumeration, Message};

#[derive(Clone, PartialEq, Message)]
pub struct OfflineFrame {
    #[prost(enumeration = "OfflineVersion", optional, tag = "1")]
    pub version: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub v1: Option<OfflineV1Frame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum OfflineVersion {
    Unknown = 0,
    V1 = 1,
}

#[derive(Clone, PartialEq, Message)]
pub struct OfflineV1Frame {
    #[prost(enumeration = "OfflineFrameType", optional, tag = "1")]
    pub frame_type: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub connection_request: Option<ConnectionRequestFrame>,
    #[prost(message, optional, tag = "3")]
    pub connection_response: Option<ConnectionResponseFrame>,
    #[prost(message, optional, tag = "4")]
    pub payload_transfer: Option<PayloadTransferFrame>,
    #[prost(message, optional, tag = "6")]
    pub keep_alive: Option<KeepAliveFrame>,
    #[prost(message, optional, tag = "7")]
    pub disconnection: Option<DisconnectionFrame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum OfflineFrameType {
    Unknown = 0,
    ConnectionRequest = 1,
    ConnectionResponse = 2,
    PayloadTransfer = 3,
    BandwidthUpgradeNegotiation = 4,
    KeepAlive = 5,
    Disconnection = 6,
    PairedKeyEncryption = 7,
    AuthenticationMessage = 8,
    AuthenticationResult = 9,
    AutoResume = 10,
    AutoReconnect = 11,
    BandwidthUpgradeRetry = 12,
}

#[derive(Clone, PartialEq, Message)]
pub struct ConnectionRequestFrame {
    #[prost(string, optional, tag = "1")]
    pub endpoint_id: Option<String>,
    // The original Nearby Connections schema called this field a string, but
    // current Windows Quick Share builds may place opaque bytes here.  We do
    // not use it (endpoint_info is the authenticated display data), so decode
    // the identical length-delimited wire value as bytes for compatibility.
    #[prost(bytes = "vec", optional, tag = "2")]
    pub endpoint_name: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub handshake_data: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "4")]
    pub nonce: Option<i32>,
    #[prost(enumeration = "ConnectionMedium", repeated, tag = "5")]
    pub mediums: Vec<i32>,
    #[prost(bytes = "vec", optional, tag = "6")]
    pub endpoint_info: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "8")]
    pub keep_alive_interval_millis: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    pub keep_alive_timeout_millis: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum ConnectionMedium {
    Unknown = 0,
    Mdns = 1,
    Bluetooth = 2,
    WifiHotspot = 3,
    Ble = 4,
    WifiLan = 5,
    WifiAware = 6,
    Nfc = 7,
    WifiDirect = 8,
    WebRtc = 9,
    BleL2cap = 10,
    Usb = 11,
    WebRtcNonCellular = 12,
    Awdl = 13,
}

#[derive(Clone, PartialEq, Message)]
pub struct ConnectionResponseFrame {
    #[prost(int32, optional, tag = "1")]
    pub status: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub handshake_data: Option<Vec<u8>>,
    #[prost(enumeration = "ConnectionResponseStatus", optional, tag = "3")]
    pub response: Option<i32>,
    #[prost(message, optional, tag = "4")]
    pub os_info: Option<OsInfo>,
    #[prost(int32, optional, tag = "5")]
    pub multiplex_socket_bitmask: Option<i32>,
    #[prost(int32, optional, tag = "6")]
    pub nearby_connections_version: Option<i32>,
    #[prost(int32, optional, tag = "7")]
    pub safe_to_disconnect_version: Option<i32>,
    #[prost(int32, optional, tag = "9")]
    pub keep_alive_timeout_millis: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum ConnectionResponseStatus {
    Unknown = 0,
    Accept = 1,
    Reject = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct OsInfo {
    #[prost(enumeration = "OsType", optional, tag = "1")]
    pub os_type: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum OsType {
    Unknown = 0,
    Android = 1,
    ChromeOs = 2,
    Windows = 3,
    Apple = 4,
    Linux = 100,
}

#[derive(Clone, PartialEq, Message)]
pub struct PayloadTransferFrame {
    #[prost(enumeration = "PayloadPacketType", optional, tag = "1")]
    pub packet_type: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub payload_header: Option<PayloadHeader>,
    #[prost(message, optional, tag = "3")]
    pub payload_chunk: Option<PayloadChunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum PayloadPacketType {
    Unknown = 0,
    Data = 1,
    Control = 2,
    PayloadAck = 3,
}

#[derive(Clone, PartialEq, Message)]
pub struct PayloadHeader {
    #[prost(int64, optional, tag = "1")]
    pub id: Option<i64>,
    #[prost(enumeration = "PayloadType", optional, tag = "2")]
    pub payload_type: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    pub total_size: Option<i64>,
    #[prost(bool, optional, tag = "4")]
    pub is_sensitive: Option<bool>,
    #[prost(string, optional, tag = "5")]
    pub file_name: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub parent_folder: Option<String>,
    #[prost(int64, optional, tag = "7")]
    pub last_modified_timestamp_millis: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum PayloadType {
    Unknown = 0,
    Bytes = 1,
    File = 2,
    Stream = 3,
}

#[derive(Clone, PartialEq, Message)]
pub struct PayloadChunk {
    #[prost(int32, optional, tag = "1")]
    pub flags: Option<i32>,
    #[prost(int64, optional, tag = "2")]
    pub offset: Option<i64>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub body: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "4")]
    pub index: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct KeepAliveFrame {
    #[prost(bool, optional, tag = "1")]
    pub ack: Option<bool>,
    #[prost(uint32, optional, tag = "2")]
    pub seq_num: Option<u32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct DisconnectionFrame {
    #[prost(bool, optional, tag = "1")]
    pub request_safe_to_disconnect: Option<bool>,
    #[prost(bool, optional, tag = "2")]
    pub ack_safe_to_disconnect: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ukey2Message {
    #[prost(enumeration = "Ukey2MessageType", optional, tag = "1")]
    pub message_type: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub message_data: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum Ukey2MessageType {
    Unknown = 0,
    Alert = 1,
    ClientInit = 2,
    ServerInit = 3,
    ClientFinish = 4,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ukey2ClientInit {
    #[prost(int32, optional, tag = "1")]
    pub version: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub random: Option<Vec<u8>>,
    #[prost(message, repeated, tag = "3")]
    pub cipher_commitments: Vec<CipherCommitment>,
    #[prost(string, optional, tag = "4")]
    pub next_protocol: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CipherCommitment {
    #[prost(enumeration = "Ukey2HandshakeCipher", optional, tag = "1")]
    pub handshake_cipher: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub commitment: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum Ukey2HandshakeCipher {
    Reserved = 0,
    P256Sha512 = 100,
    Curve25519Sha512 = 200,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ukey2ServerInit {
    #[prost(int32, optional, tag = "1")]
    pub version: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub random: Option<Vec<u8>>,
    #[prost(enumeration = "Ukey2HandshakeCipher", optional, tag = "3")]
    pub handshake_cipher: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "4")]
    pub public_key: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Ukey2ClientFinished {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub public_key: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GenericPublicKey {
    #[prost(enumeration = "PublicKeyType", optional, tag = "1")]
    pub key_type: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub ec_p256_public_key: Option<EcP256PublicKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum PublicKeyType {
    EcP256 = 1,
    Rsa2048 = 2,
    Dh2048Modp = 3,
}

#[derive(Clone, PartialEq, Message)]
pub struct EcP256PublicKey {
    #[prost(bytes = "vec", required, tag = "1")]
    pub x: Vec<u8>,
    #[prost(bytes = "vec", required, tag = "2")]
    pub y: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SecureMessage {
    #[prost(bytes = "vec", required, tag = "1")]
    pub header_and_body: Vec<u8>,
    #[prost(bytes = "vec", required, tag = "2")]
    pub signature: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SecureHeader {
    #[prost(enumeration = "SignatureScheme", optional, tag = "1")]
    pub signature_scheme: Option<i32>,
    #[prost(enumeration = "EncryptionScheme", optional, tag = "2")]
    pub encryption_scheme: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "5")]
    pub iv: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "6")]
    pub public_metadata: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SignatureScheme {
    HmacSha256 = 1,
    EcdsaP256Sha256 = 2,
    Rsa2048Sha256 = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum EncryptionScheme {
    None = 1,
    Aes256Cbc = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct HeaderAndBody {
    #[prost(message, required, tag = "1")]
    pub header: SecureHeader,
    #[prost(bytes = "vec", required, tag = "2")]
    pub body: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GcmMetadata {
    #[prost(enumeration = "GcmMetadataType", optional, tag = "1")]
    pub metadata_type: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    pub version: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum GcmMetadataType {
    DeviceToDeviceResponderHelloPayload = 12,
    DeviceToDeviceMessage = 13,
}

#[derive(Clone, PartialEq, Message)]
pub struct DeviceToDeviceMessage {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub message: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "2")]
    pub sequence_number: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SharingFrame {
    #[prost(enumeration = "SharingVersion", optional, tag = "1")]
    pub version: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub v1: Option<SharingV1Frame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SharingVersion {
    Unknown = 0,
    V1 = 1,
}

#[derive(Clone, PartialEq, Message)]
pub struct SharingV1Frame {
    #[prost(enumeration = "SharingFrameType", optional, tag = "1")]
    pub frame_type: Option<i32>,
    #[prost(message, optional, tag = "2")]
    pub introduction: Option<IntroductionFrame>,
    #[prost(message, optional, tag = "3")]
    pub connection_response: Option<SharingConnectionResponse>,
    #[prost(message, optional, tag = "4")]
    pub paired_key_encryption: Option<SharingPairedKeyEncryption>,
    #[prost(message, optional, tag = "5")]
    pub paired_key_result: Option<SharingPairedKeyResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SharingFrameType {
    Unknown = 0,
    Introduction = 1,
    Response = 2,
    PairedKeyEncryption = 3,
    PairedKeyResult = 4,
    CertificateInfo = 5,
    Cancel = 6,
    ProgressUpdate = 7,
}

#[derive(Clone, PartialEq, Message)]
pub struct IntroductionFrame {
    #[prost(message, repeated, tag = "1")]
    pub file_metadata: Vec<FileMetadata>,
    #[prost(message, repeated, tag = "2")]
    pub text_metadata: Vec<TextMetadata>,
    #[prost(string, optional, tag = "3")]
    pub required_package: Option<String>,
    #[prost(bool, optional, tag = "6")]
    pub start_transfer: Option<bool>,
    #[prost(enumeration = "SharingUseCase", optional, tag = "8")]
    pub use_case: Option<i32>,
    #[prost(int64, repeated, packed = "true", tag = "9")]
    pub preview_payload_ids: Vec<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SharingUseCase {
    Unknown = 0,
    NearbyShare = 1,
    RemoteCopy = 2,
}

#[derive(Clone, PartialEq, Message)]
pub struct FileMetadata {
    #[prost(string, optional, tag = "1")]
    pub name: Option<String>,
    #[prost(enumeration = "FileType", optional, tag = "2")]
    pub file_type: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    pub payload_id: Option<i64>,
    #[prost(int64, optional, tag = "4")]
    pub size: Option<i64>,
    #[prost(string, optional, tag = "5")]
    pub mime_type: Option<String>,
    #[prost(int64, optional, tag = "6")]
    pub id: Option<i64>,
    #[prost(string, optional, tag = "7")]
    pub parent_folder: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum FileType {
    Unknown = 0,
    Image = 1,
    Video = 2,
    AndroidApp = 3,
    Audio = 4,
    Document = 5,
    ContactCard = 6,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMetadata {
    #[prost(string, optional, tag = "2")]
    pub text_title: Option<String>,
    #[prost(enumeration = "TextType", optional, tag = "3")]
    pub text_type: Option<i32>,
    #[prost(int64, optional, tag = "4")]
    pub payload_id: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub size: Option<i64>,
    #[prost(int64, optional, tag = "6")]
    pub id: Option<i64>,
    #[prost(bool, optional, tag = "7")]
    pub is_sensitive_text: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum TextType {
    Unknown = 0,
    Text = 1,
    Url = 2,
    Address = 3,
    PhoneNumber = 4,
}

#[derive(Clone, PartialEq, Message)]
pub struct SharingConnectionResponse {
    #[prost(enumeration = "SharingResponseStatus", optional, tag = "1")]
    pub status: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SharingResponseStatus {
    Unknown = 0,
    Accept = 1,
    Reject = 2,
    NotEnoughSpace = 3,
    UnsupportedAttachmentType = 4,
    TimedOut = 5,
}

#[derive(Clone, PartialEq, Message)]
pub struct SharingPairedKeyEncryption {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub signed_data: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub secret_id_hash: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub optional_signed_data: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "4")]
    pub qr_code_handshake_data: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SharingPairedKeyResult {
    #[prost(enumeration = "PairedKeyStatus", optional, tag = "1")]
    pub status: Option<i32>,
    #[prost(enumeration = "SharingOsType", optional, tag = "2")]
    pub os_type: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum PairedKeyStatus {
    Unknown = 0,
    Success = 1,
    Fail = 2,
    Unable = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum SharingOsType {
    Unknown = 0,
    Android = 1,
    ChromeOs = 2,
    Ios = 3,
    Windows = 4,
    Macos = 5,
}

pub fn offline_v1(frame_type: OfflineFrameType) -> OfflineFrame {
    OfflineFrame {
        version: Some(OfflineVersion::V1 as i32),
        v1: Some(OfflineV1Frame {
            frame_type: Some(frame_type as i32),
            ..Default::default()
        }),
    }
}

pub fn sharing_v1(frame_type: SharingFrameType) -> SharingFrame {
    SharingFrame {
        version: Some(SharingVersion::V1 as i32),
        v1: Some(SharingV1Frame {
            frame_type: Some(frame_type as i32),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_round_trip_preserves_quick_share_file_metadata() {
        let original = FileMetadata {
            name: Some("報告.pdf".into()),
            payload_id: Some(42),
            size: Some(1234),
            mime_type: Some("application/pdf".into()),
            ..Default::default()
        };
        let encoded = original.encode_to_vec();
        assert_eq!(FileMetadata::decode(encoded.as_slice()).unwrap(), original);
    }
}
