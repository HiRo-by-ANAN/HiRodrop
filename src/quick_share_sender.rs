//! Quick Share sender over an existing LAN connection.
//!
//! Discovery uses the official QR rendezvous so a desktop without Bluetooth
//! can still wake the stock Android Quick Share receiver.

use crate::quick_share::{QuickShareAdvertisementData, QuickShareDeviceType, QuickShareEndpointId};
use crate::quick_share_crypto::{QuickShareCryptoError, SecureChannel, Ukey2Initiator};
use crate::quick_share_qr::{QuickShareQrError, QuickShareQrSession};
use crate::quick_share_transport::{read_frame, write_frame, FrameIoError, MAX_WIRE_FRAME_BYTES};
use crate::quick_share_wire::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use prost::Message;
use rand::{rngs::OsRng, RngCore};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, UNIX_EPOCH};

const LAST_CHUNK: i32 = 1;
const FILE_CHUNK_BYTES: usize = 512 * 1024;
const MAX_TOTAL_TRANSFER_BYTES: u64 = 100 * 1024 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum QuickShareSendProgress {
    WaitingForQrScan,
    PhoneFound {
        name: String,
    },
    Connecting,
    Negotiating {
        phase: &'static str,
    },
    VerificationPin {
        pin: String,
    },
    WaitingForAcceptance,
    Finalizing,
    Sending {
        sent_bytes: u64,
        total_bytes: u64,
        file_name: String,
    },
    Complete,
}

#[derive(Debug, thiserror::Error)]
pub enum QuickShareSendError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("wire framing error: {0}")]
    Frame(#[from] FrameIoError),
    #[error("cryptographic protocol error: {0}")]
    Crypto(#[from] QuickShareCryptoError),
    #[error("invalid protobuf: {0}")]
    Protobuf(#[from] prost::DecodeError),
    #[error("QR rendezvous failed: {0}")]
    Qr(#[from] QuickShareQrError),
    #[error("invalid or unreadable file: {0}")]
    InvalidFile(String),
    #[error("the selected files exceed the 100 GiB safety limit")]
    TransferTooLarge,
    #[error("unexpected Quick Share protocol message: {0}")]
    Protocol(&'static str),
    #[error("the phone rejected the transfer")]
    Rejected,
}

impl QuickShareSendError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Io(_) | Self::Frame(_) | Self::Crypto(_) | Self::Protobuf(_) | Self::Protocol(_)
        )
    }
}

#[derive(Debug)]
struct OutgoingFile {
    path: PathBuf,
    name: String,
    parent_folder: Option<String>,
    size: u64,
    payload_id: i64,
    file_type: FileType,
    mime_type: &'static str,
    last_modified_timestamp_millis: i64,
}

/// Discover a phone by QR code, connect, negotiate end-to-end encryption, and
/// send all selected files. The callback is invoked from the calling thread.
pub fn send_files_via_qr<F>(
    paths: Vec<PathBuf>,
    display_name: &str,
    qr: QuickShareQrSession,
    progress: F,
) -> Result<(), QuickShareSendError>
where
    F: FnMut(QuickShareSendProgress),
{
    send_files_via_qr_cancelable(paths, display_name, qr, &AtomicBool::new(false), progress)
}

/// QR sender variant for interactive clients that need to abandon an unscanned
/// code before selecting a different file or LAN target.
pub fn send_files_via_qr_cancelable<F>(
    paths: Vec<PathBuf>,
    display_name: &str,
    qr: QuickShareQrSession,
    cancelled: &AtomicBool,
    mut progress: F,
) -> Result<(), QuickShareSendError>
where
    F: FnMut(QuickShareSendProgress),
{
    let files = prepare_files(paths)?;
    progress(QuickShareSendProgress::WaitingForQrScan);
    let endpoint = qr.discover_receiver_cancelable(Duration::from_secs(120), cancelled)?;
    progress(QuickShareSendProgress::PhoneFound {
        name: endpoint.name,
    });
    send_prepared_files(
        files,
        display_name,
        Some(qr),
        endpoint.address,
        &mut progress,
    )
}

/// Connect directly to a Quick Share receiver discovered on the existing LAN.
/// The receiver shows the normal verification PIN and consent prompt.
pub fn send_files_to_device<F>(
    paths: Vec<PathBuf>,
    display_name: &str,
    address: std::net::SocketAddr,
    mut progress: F,
) -> Result<(), QuickShareSendError>
where
    F: FnMut(QuickShareSendProgress),
{
    let files = prepare_files(paths)?;
    send_prepared_files(files, display_name, None, address, &mut progress)
}

fn send_prepared_files<F>(
    files: Vec<OutgoingFile>,
    display_name: &str,
    qr: Option<QuickShareQrSession>,
    address: std::net::SocketAddr,
    progress: &mut F,
) -> Result<(), QuickShareSendError>
where
    F: FnMut(QuickShareSendProgress),
{
    progress(QuickShareSendProgress::Connecting);
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(15))?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    progress(QuickShareSendProgress::Negotiating {
        phase: "Nearby Connections",
    });
    send_connection_request(&mut stream, display_name)?;
    let (initiator, client_init) = Ukey2Initiator::begin()?;
    write_frame(&mut stream, &client_init)?;
    progress(QuickShareSendProgress::Negotiating { phase: "UKEY2" });
    let server_init = read_frame(&mut stream)?;
    let (client_finish, mut channel, pin, auth_key) = initiator.complete(&server_init)?;
    write_frame(&mut stream, &client_finish)?;
    send_plain_connection_accept(&mut stream)?;
    progress(QuickShareSendProgress::Negotiating {
        phase: "連線確認"
    });
    let server_response = OfflineFrame::decode(read_frame(&mut stream)?.as_slice())?;
    require_offline_type(&server_response, OfflineFrameType::ConnectionResponse)?;
    let peer_response = server_response
        .v1
        .as_ref()
        .and_then(|frame| frame.connection_response.as_ref());
    let accepted = peer_response.and_then(|frame| frame.response)
        == Some(ConnectionResponseStatus::Accept as i32);
    if !accepted {
        return Err(QuickShareSendError::Protocol(
            "phone rejected the LAN connection",
        ));
    }
    let peer_safe_to_disconnect_version = peer_response
        .and_then(|frame| frame.safe_to_disconnect_version)
        .unwrap_or(0);
    progress(QuickShareSendProgress::VerificationPin { pin });

    let mut assembler = BytesAssembler::default();
    progress(QuickShareSendProgress::Negotiating {
        phase: "配對金鑰"
    });
    send_paired_key_encryption(
        &mut stream,
        &mut channel,
        qr.as_ref().map(|session| session.sign_auth_key(&auth_key)),
    )?;
    require_sharing_type(
        &read_next_sharing_frame(&mut stream, &mut channel, &mut assembler)?,
        SharingFrameType::PairedKeyEncryption,
    )?;
    send_paired_key_result(&mut stream, &mut channel)?;
    require_sharing_type(
        &read_next_sharing_frame(&mut stream, &mut channel, &mut assembler)?,
        SharingFrameType::PairedKeyResult,
    )?;
    progress(QuickShareSendProgress::Negotiating {
        phase: "檔案資訊"
    });
    send_introduction(&mut stream, &mut channel, &files)?;
    progress(QuickShareSendProgress::WaitingForAcceptance);
    let response = read_next_sharing_frame(&mut stream, &mut channel, &mut assembler)?;
    require_sharing_type(&response, SharingFrameType::Response)?;
    let status = response
        .v1
        .and_then(|frame| frame.connection_response)
        .and_then(|frame| frame.status);
    if status != Some(SharingResponseStatus::Accept as i32) {
        return Err(QuickShareSendError::Rejected);
    }

    let total_bytes = files.iter().map(|file| file.size).sum();
    let mut sent_bytes = 0_u64;
    for file in files {
        let mut reader = File::open(&file.path)?;
        let mut offset = 0_u64;
        let mut buffer = vec![0_u8; FILE_CHUNK_BYTES];
        loop {
            acknowledge_pending_peer_frames(&mut stream, &mut channel)?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            send_file_chunk(
                &mut stream,
                &mut channel,
                &file,
                offset,
                &buffer[..read],
                false,
            )?;
            offset += read as u64;
            sent_bytes += read as u64;
            progress(QuickShareSendProgress::Sending {
                sent_bytes,
                total_bytes,
                file_name: file.name.clone(),
            });
        }
        send_file_chunk(&mut stream, &mut channel, &file, offset, &[], true)?;
        acknowledge_pending_peer_frames(&mut stream, &mut channel)?;
    }
    progress(QuickShareSendProgress::Finalizing);
    finalize_transfer(&mut stream, &mut channel, peer_safe_to_disconnect_version)?;
    progress(QuickShareSendProgress::Complete);
    Ok(())
}

fn prepare_files(paths: Vec<PathBuf>) -> Result<Vec<OutgoingFile>, QuickShareSendError> {
    if paths.is_empty() {
        return Err(QuickShareSendError::InvalidFile("no files selected".into()));
    }
    let mut candidates = Vec::new();
    for path in paths {
        collect_selected_path(&path, None, &mut candidates)?;
    }
    if candidates.is_empty() {
        return Err(QuickShareSendError::InvalidFile(
            "the selected folders contain no files".into(),
        ));
    }

    let mut ids = HashSet::new();
    let mut total = 0_u64;
    let mut files = Vec::with_capacity(candidates.len());
    for (path, parent_folder) in candidates {
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            QuickShareSendError::InvalidFile(format!("{}: {error}", path.display()))
        })?;
        if !metadata.is_file() {
            return Err(QuickShareSendError::InvalidFile(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        total = total
            .checked_add(metadata.len())
            .ok_or(QuickShareSendError::TransferTooLarge)?;
        if total > MAX_TOTAL_TRANSFER_BYTES {
            return Err(QuickShareSendError::TransferTooLarge);
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| QuickShareSendError::InvalidFile(path.display().to_string()))?
            .to_owned();
        let payload_id = loop {
            // Current Windows Quick Share and One UI use this as both a
            // payload key and attachment UUID. Negative IDs are accepted as
            // TCP payload names but never committed/renamed by Windows.
            let id = (OsRng.next_u64() & i64::MAX as u64) as i64;
            if id != 0 && ids.insert(id) {
                break id;
            }
        };
        let (file_type, mime_type) = classify_file(&path);
        let last_modified_timestamp_millis = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or(0);
        files.push(OutgoingFile {
            path,
            name,
            parent_folder,
            size: metadata.len(),
            payload_id,
            file_type,
            mime_type,
            last_modified_timestamp_millis,
        });
    }
    Ok(files)
}

fn collect_selected_path(
    path: &Path,
    parent_folder: Option<String>,
    files: &mut Vec<(PathBuf, Option<String>)>,
) -> Result<(), QuickShareSendError> {
    if files.len() >= 10_000 {
        return Err(QuickShareSendError::InvalidFile(
            "more than 10,000 files were selected".into(),
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        QuickShareSendError::InvalidFile(format!("{}: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(QuickShareSendError::InvalidFile(format!(
            "{} is a symbolic link",
            path.display()
        )));
    }
    if metadata.is_file() {
        files.push((path.to_owned(), parent_folder));
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(QuickShareSendError::InvalidFile(format!(
            "{} is not a regular file or folder",
            path.display()
        )));
    }

    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| QuickShareSendError::InvalidFile(path.display().to_string()))?;
    let folder = match parent_folder {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    let mut entries = fs::read_dir(path)
        .map_err(|error| QuickShareSendError::InvalidFile(format!("{}: {error}", path.display())))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            QuickShareSendError::InvalidFile(format!("{}: {error}", path.display()))
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        collect_selected_path(&entry.path(), Some(folder.clone()), files)?;
    }
    Ok(())
}

fn classify_file(path: &Path) -> (FileType, &'static str) {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => (FileType::Image, "image/jpeg"),
        "png" => (FileType::Image, "image/png"),
        "gif" => (FileType::Image, "image/gif"),
        "webp" => (FileType::Image, "image/webp"),
        "mp4" => (FileType::Video, "video/mp4"),
        "mov" => (FileType::Video, "video/quicktime"),
        "mp3" => (FileType::Audio, "audio/mpeg"),
        "m4a" => (FileType::Audio, "audio/mp4"),
        "pdf" => (FileType::Document, "application/pdf"),
        "txt" => (FileType::Document, "text/plain"),
        "apk" => (
            FileType::AndroidApp,
            "application/vnd.android.package-archive",
        ),
        _ => (FileType::Unknown, "application/octet-stream"),
    }
}

fn send_connection_request(
    stream: &mut TcpStream,
    display_name: &str,
) -> Result<(), QuickShareSendError> {
    let endpoint_id = QuickShareEndpointId::generate();
    let mut metadata = [0_u8; 16];
    OsRng.fill_bytes(&mut metadata);
    let data = QuickShareAdvertisementData::from_parts(
        endpoint_id,
        metadata,
        QuickShareDeviceType::Laptop,
        display_name,
    )
    .map_err(|_| QuickShareSendError::Protocol("invalid desktop device name"))?;
    let endpoint_info = URL_SAFE_NO_PAD
        .decode(data.endpoint_info)
        .map_err(|_| QuickShareSendError::Protocol("failed to build endpoint info"))?;
    let mut frame = offline_v1(OfflineFrameType::ConnectionRequest);
    frame.v1.as_mut().expect("v1").connection_request = Some(ConnectionRequestFrame {
        endpoint_id: Some(endpoint_id.as_str().to_owned()),
        endpoint_name: Some(display_name.as_bytes().to_vec()),
        mediums: vec![ConnectionMedium::WifiLan as i32],
        endpoint_info: Some(endpoint_info),
        ..Default::default()
    });
    write_frame(stream, &frame.encode_to_vec())?;
    Ok(())
}

fn send_plain_connection_accept(stream: &mut TcpStream) -> Result<(), QuickShareSendError> {
    let os_type = if cfg!(target_os = "macos") {
        OsType::Apple
    } else {
        OsType::Linux
    };
    let mut frame = offline_v1(OfflineFrameType::ConnectionResponse);
    frame.v1.as_mut().expect("v1").connection_response = Some(ConnectionResponseFrame {
        status: Some(0),
        handshake_data: None,
        response: Some(ConnectionResponseStatus::Accept as i32),
        os_info: Some(OsInfo {
            os_type: Some(os_type as i32),
        }),
        multiplex_socket_bitmask: Some(0),
        nearby_connections_version: None,
        safe_to_disconnect_version: Some(1),
        keep_alive_timeout_millis: Some(600_000),
    });
    write_frame(stream, &frame.encode_to_vec())?;
    Ok(())
}

fn send_paired_key_encryption(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    qr_signature: Option<Vec<u8>>,
) -> Result<(), QuickShareSendError> {
    let mut signed_data = vec![0_u8; 72];
    let mut secret_id_hash = vec![0_u8; 6];
    OsRng.fill_bytes(&mut signed_data);
    OsRng.fill_bytes(&mut secret_id_hash);
    let mut frame = sharing_v1(SharingFrameType::PairedKeyEncryption);
    frame.v1.as_mut().expect("v1").paired_key_encryption = Some(SharingPairedKeyEncryption {
        signed_data: Some(signed_data),
        secret_id_hash: Some(secret_id_hash),
        optional_signed_data: None,
        qr_code_handshake_data: qr_signature,
    });
    send_sharing_frame(stream, channel, &frame)
}

fn send_paired_key_result(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
) -> Result<(), QuickShareSendError> {
    let mut frame = sharing_v1(SharingFrameType::PairedKeyResult);
    frame.v1.as_mut().expect("v1").paired_key_result = Some(SharingPairedKeyResult {
        status: Some(PairedKeyStatus::Unable as i32),
        // NearDrop and stock unauthenticated sessions omit this optional
        // field. Mirroring that wire shape avoids advertising an identity we
        // have not actually authenticated.
        os_type: None,
    });
    send_sharing_frame(stream, channel, &frame)
}

fn send_introduction(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    files: &[OutgoingFile],
) -> Result<(), QuickShareSendError> {
    let mut frame = sharing_v1(SharingFrameType::Introduction);
    frame.v1.as_mut().expect("v1").introduction = Some(IntroductionFrame {
        file_metadata: files
            .iter()
            .map(|file| FileMetadata {
                name: Some(file.name.clone()),
                file_type: Some(file.file_type as i32),
                payload_id: Some(file.payload_id),
                size: Some(file.size as i64),
                mime_type: Some(file.mime_type.into()),
                // Stock One UI 8 and current Windows receivers use this
                // attachment ID to associate the FILE payload with its name.
                id: Some(file.payload_id),
                parent_folder: file.parent_folder.clone(),
            })
            .collect(),
        // Stock Quick Share and NearDrop leave this optional field absent for
        // ordinary file transfers. `true` is reserved for newer upgrade flows
        // and causes some Windows/Android receivers to abort the introduction.
        start_transfer: None,
        use_case: Some(SharingUseCase::NearbyShare as i32),
        ..Default::default()
    });
    send_sharing_frame(stream, channel, &frame)
}

fn finalize_transfer(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    peer_safe_to_disconnect_version: i32,
) -> Result<(), QuickShareSendError> {
    if peer_safe_to_disconnect_version >= 1 {
        let mut disconnection = offline_v1(OfflineFrameType::Disconnection);
        disconnection.v1.as_mut().expect("v1").disconnection = Some(DisconnectionFrame {
            request_safe_to_disconnect: Some(true),
            ack_safe_to_disconnect: Some(false),
        });
        send_encrypted_offline(stream, channel, &disconnection)?;
    }

    // Version-0 peers such as Windows finalize and close first. Version-1
    // Android peers acknowledge our safe-disconnect request. In both cases,
    // waiting prevents a TCP close from invalidating payloads still queued in
    // the stock receiver's read pipeline.
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    loop {
        match read_encrypted_offline(stream, channel) {
            Ok(frame) => {
                let Some(v1) = frame.v1.as_ref() else {
                    continue;
                };
                match v1
                    .frame_type
                    .and_then(|value| OfflineFrameType::try_from(value).ok())
                {
                    Some(OfflineFrameType::Disconnection) => {
                        let request = v1
                            .disconnection
                            .as_ref()
                            .and_then(|frame| frame.request_safe_to_disconnect)
                            .unwrap_or(false);
                        let ack = v1
                            .disconnection
                            .as_ref()
                            .and_then(|frame| frame.ack_safe_to_disconnect)
                            .unwrap_or(false);
                        if request && !ack {
                            let mut response = offline_v1(OfflineFrameType::Disconnection);
                            response.v1.as_mut().expect("v1").disconnection =
                                Some(DisconnectionFrame {
                                    request_safe_to_disconnect: Some(false),
                                    ack_safe_to_disconnect: Some(true),
                                });
                            send_encrypted_offline(stream, channel, &response)?;
                        }
                        eprintln!("HiRodrop sender: receiver finalized (ack={ack})");
                        return Ok(());
                    }
                    Some(OfflineFrameType::KeepAlive) => {
                        let keep_alive = v1.keep_alive.as_ref();
                        if !keep_alive.and_then(|frame| frame.ack).unwrap_or(false) {
                            let mut ack = offline_v1(OfflineFrameType::KeepAlive);
                            ack.v1.as_mut().expect("v1").keep_alive = Some(KeepAliveFrame {
                                ack: Some(true),
                                seq_num: keep_alive.and_then(|frame| frame.seq_num),
                            });
                            send_encrypted_offline(stream, channel, &ack)?;
                        }
                    }
                    _ => {}
                }
            }
            Err(QuickShareSendError::Frame(FrameIoError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionAborted
                ) =>
            {
                eprintln!("HiRodrop sender: receiver finalized by closing the connection");
                return Ok(());
            }
            Err(QuickShareSendError::Frame(FrameIoError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(QuickShareSendError::Protocol(
                    "receiver did not confirm that the file was committed",
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn send_file_chunk(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    file: &OutgoingFile,
    offset: u64,
    body: &[u8],
    last: bool,
) -> Result<(), QuickShareSendError> {
    let mut frame = offline_v1(OfflineFrameType::PayloadTransfer);
    frame.v1.as_mut().expect("v1").payload_transfer = Some(PayloadTransferFrame {
        packet_type: Some(PayloadPacketType::Data as i32),
        payload_header: Some(PayloadHeader {
            id: Some(file.payload_id),
            payload_type: Some(PayloadType::File as i32),
            total_size: Some(file.size as i64),
            is_sensitive: Some(false),
            file_name: Some(file.name.clone()),
            parent_folder: Some(file.parent_folder.clone().unwrap_or_default()),
            last_modified_timestamp_millis: Some(file.last_modified_timestamp_millis),
        }),
        payload_chunk: Some(PayloadChunk {
            flags: Some(if last { LAST_CHUNK } else { 0 }),
            offset: Some(offset as i64),
            // Keep the optional bytes field present even on the dedicated
            // zero-byte terminator. Current stock receivers distinguish an
            // empty body from a missing body in their FILE state machine.
            body: Some(body.to_vec()),
            index: None,
        }),
    });
    send_encrypted_offline(stream, channel, &frame)
}

fn acknowledge_pending_peer_frames(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
) -> Result<(), QuickShareSendError> {
    for _ in 0..8 {
        stream.set_nonblocking(true)?;
        let available = loop {
            match stream.peek(&mut [0_u8; 1]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        stream.set_nonblocking(false)?;
        match available {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(0) => {
                return Err(QuickShareSendError::Protocol(
                    "receiver disconnected during transfer",
                ))
            }
            Ok(_) => {}
        }

        let frame = read_encrypted_offline(stream, channel)?;
        let Some(v1) = frame.v1.as_ref() else {
            continue;
        };
        match v1
            .frame_type
            .and_then(|value| OfflineFrameType::try_from(value).ok())
        {
            Some(OfflineFrameType::KeepAlive) => {
                let keep_alive = v1.keep_alive.as_ref();
                if !keep_alive.and_then(|item| item.ack).unwrap_or(false) {
                    let mut ack = offline_v1(OfflineFrameType::KeepAlive);
                    ack.v1.as_mut().expect("v1").keep_alive = Some(KeepAliveFrame {
                        ack: Some(true),
                        seq_num: keep_alive.and_then(|item| item.seq_num),
                    });
                    send_encrypted_offline(stream, channel, &ack)?;
                }
            }
            Some(OfflineFrameType::Disconnection) => {
                return Err(QuickShareSendError::Protocol(
                    "receiver disconnected during transfer",
                ))
            }
            Some(OfflineFrameType::PayloadTransfer) => {
                // Stock peers may acknowledge payload progress while the file
                // stream is still moving. Reading it here also advances the
                // authenticated receive sequence so later keep-alives remain
                // valid.
            }
            _ => {}
        }
    }
    Ok(())
}

fn send_sharing_frame(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    sharing: &SharingFrame,
) -> Result<(), QuickShareSendError> {
    let body = sharing.encode_to_vec();
    let payload_id = OsRng.next_u64() as i64;
    for (offset, chunk, flags) in [
        (0_i64, Some(body.clone()), 0),
        (body.len() as i64, Some(Vec::new()), LAST_CHUNK),
    ] {
        let mut data = offline_v1(OfflineFrameType::PayloadTransfer);
        data.v1.as_mut().expect("v1").payload_transfer = Some(PayloadTransferFrame {
            packet_type: Some(PayloadPacketType::Data as i32),
            payload_header: Some(PayloadHeader {
                id: Some(payload_id),
                payload_type: Some(PayloadType::Bytes as i32),
                total_size: Some(body.len() as i64),
                is_sensitive: Some(false),
                file_name: None,
                parent_folder: None,
                last_modified_timestamp_millis: None,
            }),
            payload_chunk: Some(PayloadChunk {
                flags: Some(flags),
                offset: Some(offset),
                body: chunk,
                index: None,
            }),
        });
        send_encrypted_offline(stream, channel, &data)?;
    }
    Ok(())
}

fn send_encrypted_offline(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    frame: &OfflineFrame,
) -> Result<(), QuickShareSendError> {
    let encrypted = channel.encrypt_offline_frame(frame)?;
    write_frame(stream, &encrypted)?;
    Ok(())
}

fn read_encrypted_offline(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
) -> Result<OfflineFrame, QuickShareSendError> {
    Ok(channel.decrypt_offline_frame(&read_frame(stream)?)?)
}

#[derive(Default)]
struct BytesAssembler {
    buffers: HashMap<i64, Vec<u8>>,
}

impl BytesAssembler {
    fn push(
        &mut self,
        transfer: &PayloadTransferFrame,
    ) -> Result<Option<Vec<u8>>, QuickShareSendError> {
        let header = transfer
            .payload_header
            .as_ref()
            .ok_or(QuickShareSendError::Protocol("missing payload header"))?;
        let chunk = transfer
            .payload_chunk
            .as_ref()
            .ok_or(QuickShareSendError::Protocol("missing payload chunk"))?;
        let id = header
            .id
            .ok_or(QuickShareSendError::Protocol("missing payload ID"))?;
        let total = header
            .total_size
            .ok_or(QuickShareSendError::Protocol("missing payload size"))?;
        if total < 0 || total as usize > MAX_WIRE_FRAME_BYTES {
            return Err(QuickShareSendError::Protocol("byte payload is too large"));
        }
        let buffer = self.buffers.entry(id).or_default();
        if chunk.offset != Some(buffer.len() as i64) {
            self.buffers.remove(&id);
            return Err(QuickShareSendError::Protocol(
                "unexpected byte payload offset",
            ));
        }
        if let Some(body) = &chunk.body {
            buffer.extend_from_slice(body);
        }
        if chunk.flags.unwrap_or(0) & LAST_CHUNK != 0 {
            let complete = self.buffers.remove(&id).expect("payload buffer exists");
            if complete.len() != total as usize {
                return Err(QuickShareSendError::Protocol("incomplete byte payload"));
            }
            Ok(Some(complete))
        } else {
            Ok(None)
        }
    }
}

fn read_next_sharing_frame(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    assembler: &mut BytesAssembler,
) -> Result<SharingFrame, QuickShareSendError> {
    loop {
        let frame = read_encrypted_offline(stream, channel)?;
        let v1 = frame
            .v1
            .as_ref()
            .ok_or(QuickShareSendError::Protocol("missing encrypted v1 frame"))?;
        match v1
            .frame_type
            .and_then(|value| OfflineFrameType::try_from(value).ok())
        {
            Some(OfflineFrameType::KeepAlive) => {
                let keep_alive = v1.keep_alive.as_ref();
                if !keep_alive.and_then(|frame| frame.ack).unwrap_or(false) {
                    let mut ack = offline_v1(OfflineFrameType::KeepAlive);
                    ack.v1.as_mut().expect("v1").keep_alive = Some(KeepAliveFrame {
                        ack: Some(true),
                        seq_num: keep_alive.and_then(|frame| frame.seq_num),
                    });
                    send_encrypted_offline(stream, channel, &ack)?;
                }
            }
            Some(OfflineFrameType::PayloadTransfer) => {
                let transfer = v1
                    .payload_transfer
                    .as_ref()
                    .ok_or(QuickShareSendError::Protocol("missing payload transfer"))?;
                if let Some(bytes) = assembler.push(transfer)? {
                    return Ok(SharingFrame::decode(bytes.as_slice())?);
                }
            }
            Some(OfflineFrameType::Disconnection) => {
                return Err(QuickShareSendError::Protocol("phone disconnected"));
            }
            _ => {}
        }
    }
}

fn require_offline_type(
    frame: &OfflineFrame,
    expected: OfflineFrameType,
) -> Result<(), QuickShareSendError> {
    if frame.v1.as_ref().and_then(|v1| v1.frame_type) == Some(expected as i32) {
        Ok(())
    } else {
        Err(QuickShareSendError::Protocol(
            "unexpected offline frame type",
        ))
    }
}

fn require_sharing_type(
    frame: &SharingFrame,
    expected: SharingFrameType,
) -> Result<(), QuickShareSendError> {
    if frame.v1.as_ref().and_then(|v1| v1.frame_type) == Some(expected as i32) {
        Ok(())
    } else {
        Err(QuickShareSendError::Protocol(
            "unexpected sharing frame type",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConsentMode, QuickShareReceiver, QuickShareReceiverConfig};
    use std::fs;

    #[test]
    fn common_extensions_get_android_friendly_metadata() {
        assert_eq!(
            classify_file(Path::new("photo.JPG")),
            (FileType::Image, "image/jpeg")
        );
        assert_eq!(
            classify_file(Path::new("archive.bin")),
            (FileType::Unknown, "application/octet-stream")
        );
    }

    #[test]
    fn outgoing_attachment_ids_are_positive_and_unique() {
        let root = std::env::temp_dir().join(format!("hirodrop-ids-{}", OsRng.next_u64()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.png");
        let second = root.join("second.png");
        fs::write(&first, b"one").unwrap();
        fs::write(&second, b"two").unwrap();

        let files = prepare_files(vec![first, second]).unwrap();
        assert!(files.iter().all(|file| file.payload_id > 0));
        assert_ne!(files[0].payload_id, files[1].payload_id);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sender_and_receiver_complete_a_two_file_encrypted_transfer() {
        let root = std::env::temp_dir().join(format!("hirodrop-loopback-{}", OsRng.next_u64()));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let first = source.join("第一份.txt");
        let second = source.join("photo.jpg");
        let folder = source.join("資料夾");
        let nested = folder.join("子目錄");
        let nested_file = nested.join("note.txt");
        fs::create_dir_all(&nested).unwrap();
        fs::write(&first, b"hello from HiRodrop").unwrap();
        fs::write(&second, vec![0x5a; FILE_CHUNK_BYTES + 37]).unwrap();
        fs::write(&nested_file, b"folder transfer").unwrap();

        let mut config = QuickShareReceiverConfig::new("receiver", destination.clone());
        config.consent_mode = ConsentMode::AutoAccept;
        let receiver = match QuickShareReceiver::bind(config) {
            Ok(receiver) => receiver,
            Err(crate::QuickShareReceiveError::Io(error))
                if error.kind() == io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(error) => panic!("failed to bind test receiver: {error}"),
        };
        let address = format!("127.0.0.1:{}", receiver.port().unwrap())
            .parse()
            .unwrap();
        let receive_thread = std::thread::spawn(move || receiver.receive_once().unwrap());
        let files = prepare_files(vec![first.clone(), second.clone(), folder]).unwrap();
        assert_eq!(files[2].parent_folder.as_deref(), Some("資料夾/子目錄"));
        let qr = QuickShareQrSession::generate().unwrap();
        let mut progress = Vec::new();
        send_prepared_files(files, "sender", Some(qr), address, &mut |event| {
            progress.push(event)
        })
        .unwrap();
        let summary = receive_thread.join().unwrap();

        assert_eq!(summary.saved_paths.len(), 3);
        assert_eq!(
            fs::read(destination.join("第一份.txt")).unwrap(),
            fs::read(first).unwrap()
        );
        assert_eq!(
            fs::read(destination.join("photo.jpg")).unwrap(),
            fs::read(second).unwrap()
        );
        assert_eq!(
            fs::read(destination.join("資料夾/子目錄/note.txt")).unwrap(),
            fs::read(nested_file).unwrap()
        );
        assert!(matches!(
            progress.last(),
            Some(QuickShareSendProgress::Complete)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_lan_sender_works_without_qr_signature() {
        let root = std::env::temp_dir().join(format!("hirodrop-direct-{}", OsRng.next_u64()));
        let destination = root.join("destination");
        fs::create_dir_all(&destination).unwrap();
        let source = root.join("from-mac.txt");
        fs::write(&source, b"direct LAN Quick Share").unwrap();

        let mut config = QuickShareReceiverConfig::new("windows", destination.clone());
        config.consent_mode = ConsentMode::AutoAccept;
        let receiver = match QuickShareReceiver::bind(config) {
            Ok(receiver) => receiver,
            Err(crate::QuickShareReceiveError::Io(error))
                if error.kind() == io::ErrorKind::PermissionDenied =>
            {
                return;
            }
            Err(error) => panic!("failed to bind test receiver: {error}"),
        };
        let address = format!("127.0.0.1:{}", receiver.port().unwrap())
            .parse()
            .unwrap();
        let (progress_tx, progress_rx) = std::sync::mpsc::channel();
        let receive_thread = std::thread::spawn(move || loop {
            if let Some(summary) = receiver
                .try_receive_once_with_consent_and_progress(
                    |_| true,
                    |progress| {
                        progress_tx.send(progress).unwrap();
                    },
                )
                .unwrap()
            {
                break summary;
            }
            std::thread::sleep(Duration::from_millis(5));
        });

        send_files_to_device(vec![source], "mac", address, |_| {}).unwrap();
        let summary = receive_thread.join().unwrap();
        assert_eq!(summary.saved_paths.len(), 1);
        assert_eq!(
            fs::read(destination.join("from-mac.txt")).unwrap(),
            b"direct LAN Quick Share"
        );
        let progress = progress_rx.try_iter().last().unwrap();
        assert!(matches!(
            progress,
            crate::QuickShareReceiveProgress::Receiving {
                received_bytes: 22,
                total_bytes: 22,
                ..
            }
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
