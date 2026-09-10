//! Quick Share receiver state machine for Android -> HiRodrop transfers.

use crate::quick_share_crypto::{QuickShareCryptoError, SecureChannel, Ukey2Responder};
use crate::quick_share_transport::{read_frame, write_frame, FrameIoError, MAX_WIRE_FRAME_BYTES};
use crate::quick_share_wire::*;
use prost::Message;
use rand::{rngs::OsRng, RngCore};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LAST_CHUNK: i32 = 1;
const MAX_TOTAL_TRANSFER_BYTES: u64 = 100 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsentMode {
    Prompt,
    AutoAccept,
}

#[derive(Clone, Debug)]
pub struct QuickShareReceiverConfig {
    pub display_name: String,
    pub download_directory: PathBuf,
    pub consent_mode: ConsentMode,
}

impl QuickShareReceiverConfig {
    pub fn new(display_name: impl Into<String>, download_directory: PathBuf) -> Self {
        Self {
            display_name: display_name.into(),
            download_directory,
            consent_mode: ConsentMode::Prompt,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncomingPeer {
    pub name: String,
    pub device_type: u8,
    pub address: SocketAddr,
}

#[derive(Clone, Debug)]
pub struct TransferSummary {
    pub peer: IncomingPeer,
    pub saved_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct TransferOffer {
    pub peer: IncomingPeer,
    pub pin: String,
    pub peer_fingerprint: String,
    pub items: Vec<TransferOfferItem>,
}

#[derive(Clone, Debug)]
pub struct TransferOfferItem {
    pub name: String,
    pub size: u64,
    pub is_text: bool,
}

#[derive(Clone, Debug)]
pub enum QuickShareReceiveProgress {
    Receiving {
        received_bytes: u64,
        total_bytes: u64,
        file_name: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum QuickShareReceiveError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("wire framing error: {0}")]
    Frame(#[from] FrameIoError),
    #[error("cryptographic protocol error: {0}")]
    Crypto(#[from] QuickShareCryptoError),
    #[error("invalid protobuf: {0}")]
    Protobuf(#[from] prost::DecodeError),
    #[error("unexpected Quick Share protocol message: {0}")]
    Protocol(&'static str),
    #[error("the sender canceled or disconnected")]
    Disconnected,
    #[error("the transfer was rejected")]
    Rejected,
    #[error("unsafe or unsupported file metadata")]
    UnsafeMetadata,
    #[error("declared transfer size exceeds the 100 GiB safety limit")]
    TransferTooLarge,
}

impl QuickShareReceiveError {
    /// Discovery clients (notably the Windows Quick Share app) sometimes open
    /// the advertised TCP port only to close it immediately.  Keep this out of
    /// user-facing failure logs while retaining real protocol errors.
    pub fn is_benign_probe_disconnect(&self) -> bool {
        match self {
            Self::Frame(error) => error.is_peer_disconnect(),
            Self::Io(error) => matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            ),
            Self::Disconnected => true,
            _ => false,
        }
    }
}

/// A dual-stack listener. Binding the unspecified address means every existing
/// Ethernet/Wi-Fi adapter can accept the same advertised TCP port; no adapter
/// is reconfigured and the OS chooses the route.
pub struct QuickShareReceiver {
    listener: TcpListener,
    config: QuickShareReceiverConfig,
}

/// One accepted TCP connection, detached from the listener so GUI services can
/// process multiple Quick Share probes/transfers concurrently.
pub struct QuickShareIncomingConnection {
    stream: TcpStream,
    address: SocketAddr,
    config: QuickShareReceiverConfig,
}

impl QuickShareReceiver {
    pub fn bind(config: QuickShareReceiverConfig) -> Result<Self, QuickShareReceiveError> {
        let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_only_v6(false)?;
        socket.set_reuse_address(true)?;
        socket.bind(
            &"[::]:0"
                .parse::<SocketAddr>()
                .expect("valid bind address")
                .into(),
        )?;
        socket.listen(16)?;
        let listener: TcpListener = socket.into();
        Ok(Self { listener, config })
    }

    pub fn port(&self) -> Result<u16, QuickShareReceiveError> {
        Ok(self.listener.local_addr()?.port())
    }

    pub fn receive_once(&self) -> Result<TransferSummary, QuickShareReceiveError> {
        let mode = self.config.consent_mode;
        self.receive_once_with_consent(move |offer| {
            println!(
                "Incoming Quick Share request from {} ({})",
                offer.peer.name, offer.peer.address
            );
            println!("Verification PIN: {}", offer.pin);
            for item in &offer.items {
                let kind = if item.is_text { "Text" } else { "File" };
                println!("  {kind}: {} ({} bytes)", item.name, item.size);
            }
            obtain_consent(mode).unwrap_or(false)
        })
    }

    pub fn receive_once_with_consent<F>(
        &self,
        consent: F,
    ) -> Result<TransferSummary, QuickShareReceiveError>
    where
        F: FnMut(&TransferOffer) -> bool,
    {
        let (mut stream, address) = self.listener.accept()?;
        self.receive_accepted(&mut stream, address, consent, |_| {})
    }

    /// Poll once without blocking. Intended for a tray/GUI service loop.
    pub fn try_receive_once_with_consent<F>(
        &self,
        consent: F,
    ) -> Result<Option<TransferSummary>, QuickShareReceiveError>
    where
        F: FnMut(&TransferOffer) -> bool,
    {
        self.try_receive_once_with_consent_and_progress(consent, |_| {})
    }

    pub fn try_receive_once_with_consent_and_progress<F, P>(
        &self,
        mut consent: F,
        progress: P,
    ) -> Result<Option<TransferSummary>, QuickShareReceiveError>
    where
        F: FnMut(&TransferOffer) -> bool,
        P: FnMut(QuickShareReceiveProgress),
    {
        let Some(connection) = self.try_accept()? else {
            return Ok(None);
        };
        connection
            .receive_with_consent_and_progress(|offer| Some(consent(offer)), progress)
            .map(Some)
    }

    pub fn try_accept(
        &self,
    ) -> Result<Option<QuickShareIncomingConnection>, QuickShareReceiveError> {
        self.listener.set_nonblocking(true)?;
        let accepted = self.listener.accept();
        self.listener.set_nonblocking(false)?;
        match accepted {
            Ok((stream, address)) => Ok(Some(QuickShareIncomingConnection {
                stream,
                address,
                config: self.config.clone(),
            })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn receive_accepted<F, P>(
        &self,
        stream: &mut TcpStream,
        address: SocketAddr,
        mut consent: F,
        progress: P,
    ) -> Result<TransferSummary, QuickShareReceiveError>
    where
        F: FnMut(&TransferOffer) -> bool,
        P: FnMut(QuickShareReceiveProgress),
    {
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        stream.set_nodelay(true)?;
        receive_connection(
            stream,
            address,
            &self.config,
            |offer| Some(consent(offer)),
            progress,
        )
    }
}

impl QuickShareIncomingConnection {
    pub fn receive_with_consent_and_progress<F, P>(
        mut self,
        consent: F,
        progress: P,
    ) -> Result<TransferSummary, QuickShareReceiveError>
    where
        F: FnMut(&TransferOffer) -> Option<bool>,
        P: FnMut(QuickShareReceiveProgress),
    {
        self.stream.set_nonblocking(false)?;
        self.stream
            .set_read_timeout(Some(Duration::from_secs(20)))?;
        self.stream
            .set_write_timeout(Some(Duration::from_secs(20)))?;
        self.stream.set_nodelay(true)?;
        receive_connection(
            &mut self.stream,
            self.address,
            &self.config,
            consent,
            progress,
        )
    }
}

fn receive_connection<F, P>(
    stream: &mut TcpStream,
    address: SocketAddr,
    config: &QuickShareReceiverConfig,
    mut consent: F,
    progress: P,
) -> Result<TransferSummary, QuickShareReceiveError>
where
    F: FnMut(&TransferOffer) -> Option<bool>,
    P: FnMut(QuickShareReceiveProgress),
{
    let request_raw = read_frame(stream)?;
    let peer = parse_connection_request(&request_raw, address)?;

    let client_init = read_frame(stream)?;
    let (ukey2, server_init) = Ukey2Responder::accept_client_init(&client_init)?;
    write_frame(stream, &server_init)?;
    let client_finish = read_frame(stream)?;
    let (mut channel, pin, fingerprint) = ukey2.complete(&client_finish)?;

    let client_response = OfflineFrame::decode(read_frame(stream)?.as_slice())?;
    require_offline_type(&client_response, OfflineFrameType::ConnectionResponse)?;
    send_plain_connection_accept(stream)?;

    send_paired_key_encryption(stream, &mut channel)?;
    let mut assembler = BytesAssembler::default();
    let paired_encryption = read_next_sharing_frame(stream, &mut channel, &mut assembler)?;
    require_sharing_type(&paired_encryption, SharingFrameType::PairedKeyEncryption)?;
    send_paired_key_result(stream, &mut channel)?;
    let paired_result = read_next_sharing_frame(stream, &mut channel, &mut assembler)?;
    require_sharing_type(&paired_result, SharingFrameType::PairedKeyResult)?;
    let introduction = read_next_sharing_frame(stream, &mut channel, &mut assembler)?;
    require_sharing_type(&introduction, SharingFrameType::Introduction)?;
    let introduction = introduction
        .v1
        .and_then(|v1| v1.introduction)
        .ok_or(QuickShareReceiveError::Protocol("missing introduction"))?;

    let offer = transfer_offer(peer.clone(), pin, fingerprint, &introduction)?;
    let mut last_keep_alive = std::time::Instant::now();
    let accepted = loop {
        if let Some(decision) = consent(&offer) {
            break decision;
        }
        if last_keep_alive.elapsed() > std::time::Duration::from_secs(5) {
            let mut keep_alive = offline_v1(OfflineFrameType::KeepAlive);
            keep_alive.v1.as_mut().expect("v1").keep_alive = Some(KeepAliveFrame {
                ack: Some(false),
                seq_num: None,
            });
            send_encrypted_offline(stream, &mut channel, &keep_alive)?;
            last_keep_alive = std::time::Instant::now();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Also acknowledge any incoming keep-alives while waiting
        stream.set_nonblocking(true)?;
        let mut peek_buf = [0_u8; 1];
        let available = match stream.peek(&mut peek_buf) {
            Ok(0) => Err(QuickShareReceiveError::Protocol(
                "disconnected while waiting for consent",
            )),
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(QuickShareReceiveError::Io(e)),
        }?;
        stream.set_nonblocking(false)?;
        if available {
            let frame = read_encrypted_offline(stream, &mut channel)?;
            if let Some(v1) = frame.v1 {
                if v1.frame_type == Some(OfflineFrameType::KeepAlive as i32) {
                    let ka = v1.keep_alive.unwrap_or_default();
                    if !ka.ack.unwrap_or(false) {
                        send_keep_alive_ack(stream, &mut channel, ka.seq_num)?;
                    }
                }
            }
        }
    };

    if !accepted {
        send_sharing_response(stream, &mut channel, SharingResponseStatus::Reject)?;
        return Err(QuickShareReceiveError::Rejected);
    }
    fs::create_dir_all(&config.download_directory)?;
    let mut pending = PendingTransfer::prepare(&introduction, &config.download_directory)?;
    send_sharing_response(stream, &mut channel, SharingResponseStatus::Accept)?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;

    let receive_result = pending.receive_all(stream, &mut channel, &mut assembler, progress);
    if receive_result.is_err() {
        pending.cleanup_parts();
    }
    let saved_paths = receive_result?;
    let disconnection = offline_v1(OfflineFrameType::Disconnection);
    send_encrypted_offline(stream, &mut channel, &disconnection)?;
    Ok(TransferSummary { peer, saved_paths })
}

fn parse_connection_request(
    raw: &[u8],
    address: SocketAddr,
) -> Result<IncomingPeer, QuickShareReceiveError> {
    let frame = OfflineFrame::decode(raw)?;
    require_offline_type(&frame, OfflineFrameType::ConnectionRequest)?;
    let request =
        frame
            .v1
            .and_then(|v1| v1.connection_request)
            .ok_or(QuickShareReceiveError::Protocol(
                "missing connection request",
            ))?;
    let info = request
        .endpoint_info
        .ok_or(QuickShareReceiveError::Protocol("missing endpoint info"))?;
    if info.len() < 18 {
        return Err(QuickShareReceiveError::Protocol(
            "endpoint info is too short",
        ));
    }
    let name_length = info[17] as usize;
    let end = 18_usize
        .checked_add(name_length)
        .filter(|end| *end <= info.len())
        .ok_or(QuickShareReceiveError::Protocol(
            "invalid endpoint name length",
        ))?;
    let name = std::str::from_utf8(&info[18..end])
        .map_err(|_| QuickShareReceiveError::Protocol("endpoint name is not UTF-8"))?
        .to_owned();
    Ok(IncomingPeer {
        name,
        device_type: (info[0] >> 1) & 0x07,
        address,
    })
}

fn send_plain_connection_accept(stream: &mut TcpStream) -> Result<(), QuickShareReceiveError> {
    let mut frame = offline_v1(OfflineFrameType::ConnectionResponse);
    let os_type = if cfg!(target_os = "macos") {
        OsType::Apple
    } else {
        OsType::Linux
    };
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
) -> Result<(), QuickShareReceiveError> {
    let mut signed_data = vec![0_u8; 72];
    let mut secret_id_hash = vec![0_u8; 6];
    OsRng.fill_bytes(&mut signed_data);
    OsRng.fill_bytes(&mut secret_id_hash);
    let mut frame = sharing_v1(SharingFrameType::PairedKeyEncryption);
    frame.v1.as_mut().expect("v1").paired_key_encryption = Some(SharingPairedKeyEncryption {
        signed_data: Some(signed_data),
        secret_id_hash: Some(secret_id_hash),
        optional_signed_data: None,
        qr_code_handshake_data: None,
    });
    send_sharing_frame(stream, channel, &frame)
}

fn send_paired_key_result(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
) -> Result<(), QuickShareReceiveError> {
    let mut frame = sharing_v1(SharingFrameType::PairedKeyResult);
    frame.v1.as_mut().expect("v1").paired_key_result = Some(SharingPairedKeyResult {
        status: Some(PairedKeyStatus::Unable as i32),
        os_type: Some(SharingOsType::Macos as i32),
    });
    send_sharing_frame(stream, channel, &frame)
}

fn send_sharing_response(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    status: SharingResponseStatus,
) -> Result<(), QuickShareReceiveError> {
    let mut frame = sharing_v1(SharingFrameType::Response);
    frame.v1.as_mut().expect("v1").connection_response = Some(SharingConnectionResponse {
        status: Some(status as i32),
    });
    send_sharing_frame(stream, channel, &frame)
}

fn send_sharing_frame(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    sharing: &SharingFrame,
) -> Result<(), QuickShareReceiveError> {
    let body = sharing.encode_to_vec();
    let payload_id = OsRng.next_u64() as i64;
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
            flags: Some(0),
            offset: Some(0),
            body: Some(body.clone()),
            index: None,
        }),
    });
    send_encrypted_offline(stream, channel, &data)?;
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
            flags: Some(LAST_CHUNK),
            offset: Some(body.len() as i64),
            body: None,
            index: None,
        }),
    });
    send_encrypted_offline(stream, channel, &data)
}

fn send_keep_alive_ack(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    sequence: Option<u32>,
) -> Result<(), QuickShareReceiveError> {
    let mut frame = offline_v1(OfflineFrameType::KeepAlive);
    frame.v1.as_mut().expect("v1").keep_alive = Some(KeepAliveFrame {
        ack: Some(true),
        seq_num: sequence,
    });
    send_encrypted_offline(stream, channel, &frame)
}

fn send_encrypted_offline(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    frame: &OfflineFrame,
) -> Result<(), QuickShareReceiveError> {
    let encrypted = channel.encrypt_offline_frame(frame)?;
    write_frame(stream, &encrypted)?;
    Ok(())
}

fn read_encrypted_offline(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
) -> Result<OfflineFrame, QuickShareReceiveError> {
    let encrypted = read_frame(stream)?;
    Ok(channel.decrypt_offline_frame(&encrypted)?)
}

#[derive(Default)]
struct BytesAssembler {
    buffers: HashMap<i64, Vec<u8>>,
}

impl BytesAssembler {
    fn push(
        &mut self,
        transfer: &PayloadTransferFrame,
    ) -> Result<Option<(i64, Vec<u8>)>, QuickShareReceiveError> {
        let header = transfer
            .payload_header
            .as_ref()
            .ok_or(QuickShareReceiveError::Protocol("missing payload header"))?;
        let chunk = transfer
            .payload_chunk
            .as_ref()
            .ok_or(QuickShareReceiveError::Protocol("missing payload chunk"))?;
        let id = header
            .id
            .ok_or(QuickShareReceiveError::Protocol("missing payload ID"))?;
        let total = header
            .total_size
            .ok_or(QuickShareReceiveError::Protocol("missing payload size"))?;
        if total < 0 || total as usize > MAX_WIRE_FRAME_BYTES {
            return Err(QuickShareReceiveError::Protocol(
                "byte payload is too large",
            ));
        }
        let buffer = self.buffers.entry(id).or_default();
        if chunk.offset != Some(buffer.len() as i64) {
            self.buffers.remove(&id);
            return Err(QuickShareReceiveError::Protocol(
                "unexpected byte-payload offset",
            ));
        }
        if let Some(body) = &chunk.body {
            if buffer.len() + body.len() > total as usize {
                return Err(QuickShareReceiveError::Protocol(
                    "byte payload exceeds declared size",
                ));
            }
            buffer.extend_from_slice(body);
        }
        if chunk.flags.unwrap_or(0) & LAST_CHUNK != 0 {
            let complete = self.buffers.remove(&id).expect("payload buffer exists");
            if complete.len() != total as usize {
                return Err(QuickShareReceiveError::Protocol("incomplete byte payload"));
            }
            return Ok(Some((id, complete)));
        }
        Ok(None)
    }
}

fn read_next_sharing_frame(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    assembler: &mut BytesAssembler,
) -> Result<SharingFrame, QuickShareReceiveError> {
    loop {
        let frame = read_encrypted_offline(stream, channel)?;
        let v1 = frame.v1.as_ref().ok_or(QuickShareReceiveError::Protocol(
            "missing encrypted v1 frame",
        ))?;
        match v1
            .frame_type
            .and_then(|value| OfflineFrameType::try_from(value).ok())
        {
            Some(OfflineFrameType::KeepAlive) => {
                let keep_alive = v1.keep_alive.as_ref();
                if !keep_alive.and_then(|frame| frame.ack).unwrap_or(false) {
                    send_keep_alive_ack(
                        stream,
                        channel,
                        keep_alive.and_then(|frame| frame.seq_num),
                    )?;
                }
            }
            Some(OfflineFrameType::PayloadTransfer) => {
                let transfer = v1
                    .payload_transfer
                    .as_ref()
                    .ok_or(QuickShareReceiveError::Protocol("missing payload transfer"))?;
                if transfer
                    .payload_header
                    .as_ref()
                    .and_then(|header| header.payload_type)
                    == Some(PayloadType::Bytes as i32)
                {
                    if let Some((_, bytes)) = assembler.push(transfer)? {
                        return Ok(SharingFrame::decode(bytes.as_slice())?);
                    }
                }
            }
            Some(OfflineFrameType::Disconnection) => {
                return Err(QuickShareReceiveError::Disconnected)
            }
            _ => {}
        }
    }
}

fn require_offline_type(
    frame: &OfflineFrame,
    expected: OfflineFrameType,
) -> Result<(), QuickShareReceiveError> {
    if frame.v1.as_ref().and_then(|v1| v1.frame_type) == Some(expected as i32) {
        Ok(())
    } else {
        Err(QuickShareReceiveError::Protocol(
            "unexpected offline frame type",
        ))
    }
}

fn require_sharing_type(
    frame: &SharingFrame,
    expected: SharingFrameType,
) -> Result<(), QuickShareReceiveError> {
    if frame.v1.as_ref().and_then(|v1| v1.frame_type) == Some(expected as i32) {
        Ok(())
    } else {
        Err(QuickShareReceiveError::Protocol(
            "unexpected sharing frame type",
        ))
    }
}

fn transfer_offer(
    peer: IncomingPeer,
    pin: String,
    peer_fingerprint: String,
    introduction: &IntroductionFrame,
) -> Result<TransferOffer, QuickShareReceiveError> {
    let mut items = Vec::new();
    for file in &introduction.file_metadata {
        items.push(TransferOfferItem {
            name: file
                .name
                .clone()
                .ok_or(QuickShareReceiveError::UnsafeMetadata)?,
            size: u64::try_from(file.size.ok_or(QuickShareReceiveError::UnsafeMetadata)?)
                .map_err(|_| QuickShareReceiveError::UnsafeMetadata)?,
            is_text: false,
        });
    }
    for text in &introduction.text_metadata {
        items.push(TransferOfferItem {
            name: text
                .text_title
                .clone()
                .unwrap_or_else(|| "Shared text".into()),
            size: u64::try_from(text.size.ok_or(QuickShareReceiveError::UnsafeMetadata)?)
                .map_err(|_| QuickShareReceiveError::UnsafeMetadata)?,
            is_text: true,
        });
    }
    Ok(TransferOffer {
        peer,
        pin,
        peer_fingerprint,
        items,
    })
}

fn obtain_consent(mode: ConsentMode) -> Result<bool, QuickShareReceiveError> {
    if mode == ConsentMode::AutoAccept {
        println!("Transfer auto-accepted by explicit --accept option.");
        return Ok(true);
    }
    print!("Accept this transfer? [y/N] ");
    io::stdout().flush()?;
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

struct PendingFile {
    expected_size: u64,
    received: u64,
    final_path: PathBuf,
    part_path: PathBuf,
    file: File,
}

struct PendingText {
    expected_size: usize,
    text_type: TextType,
}

struct PendingTransfer {
    files: HashMap<i64, PendingFile>,
    texts: HashMap<i64, PendingText>,
    preview_payload_ids: HashSet<i64>,
    download_directory: PathBuf,
    completed: Vec<PathBuf>,
    total_bytes: u64,
    received_bytes: u64,
}

impl PendingTransfer {
    fn prepare(
        introduction: &IntroductionFrame,
        download_directory: &Path,
    ) -> Result<Self, QuickShareReceiveError> {
        if introduction.file_metadata.is_empty() && introduction.text_metadata.is_empty() {
            return Err(QuickShareReceiveError::UnsafeMetadata);
        }
        let mut files = HashMap::new();
        let mut texts = HashMap::new();
        let mut total_size = 0_u64;
        for metadata in &introduction.file_metadata {
            let id = metadata
                .payload_id
                .ok_or(QuickShareReceiveError::UnsafeMetadata)?;
            let size = u64::try_from(
                metadata
                    .size
                    .ok_or(QuickShareReceiveError::UnsafeMetadata)?,
            )
            .map_err(|_| QuickShareReceiveError::UnsafeMetadata)?;
            total_size = total_size
                .checked_add(size)
                .ok_or(QuickShareReceiveError::TransferTooLarge)?;
            let safe_name = sanitize_file_name(
                metadata
                    .name
                    .as_deref()
                    .ok_or(QuickShareReceiveError::UnsafeMetadata)?,
            )?;
            let parent_directory =
                safe_parent_directory(download_directory, metadata.parent_folder.as_deref())?;
            let final_path = unique_destination(&parent_directory, &safe_name);
            let part_path = download_directory.join(format!(
                ".hirodrop-{}-{}.part",
                std::process::id(),
                OsRng.next_u64()
            ));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&part_path)?;
            if files
                .insert(
                    id,
                    PendingFile {
                        expected_size: size,
                        received: 0,
                        final_path,
                        part_path,
                        file,
                    },
                )
                .is_some()
            {
                return Err(QuickShareReceiveError::UnsafeMetadata);
            }
        }
        for metadata in &introduction.text_metadata {
            let id = metadata
                .payload_id
                .ok_or(QuickShareReceiveError::UnsafeMetadata)?;
            let size = usize::try_from(
                metadata
                    .size
                    .ok_or(QuickShareReceiveError::UnsafeMetadata)?,
            )
            .map_err(|_| QuickShareReceiveError::UnsafeMetadata)?;
            if size > MAX_WIRE_FRAME_BYTES {
                return Err(QuickShareReceiveError::UnsafeMetadata);
            }
            total_size = total_size
                .checked_add(size as u64)
                .ok_or(QuickShareReceiveError::TransferTooLarge)?;
            let text_type = metadata
                .text_type
                .and_then(|value| TextType::try_from(value).ok())
                .unwrap_or(TextType::Unknown);
            if texts
                .insert(
                    id,
                    PendingText {
                        expected_size: size,
                        text_type,
                    },
                )
                .is_some()
                || files.contains_key(&id)
            {
                return Err(QuickShareReceiveError::UnsafeMetadata);
            }
        }
        if total_size > MAX_TOTAL_TRANSFER_BYTES {
            return Err(QuickShareReceiveError::TransferTooLarge);
        }
        Ok(Self {
            files,
            texts,
            preview_payload_ids: introduction.preview_payload_ids.iter().copied().collect(),
            download_directory: download_directory.to_owned(),
            completed: Vec::new(),
            total_bytes: total_size,
            received_bytes: 0,
        })
    }

    fn receive_all<P>(
        &mut self,
        stream: &mut TcpStream,
        channel: &mut SecureChannel,
        assembler: &mut BytesAssembler,
        mut progress: P,
    ) -> Result<Vec<PathBuf>, QuickShareReceiveError>
    where
        P: FnMut(QuickShareReceiveProgress),
    {
        while !self.files.is_empty() || !self.texts.is_empty() {
            let frame = read_encrypted_offline(stream, channel)?;
            let v1 = frame.v1.as_ref().ok_or(QuickShareReceiveError::Protocol(
                "missing encrypted v1 frame",
            ))?;
            match v1
                .frame_type
                .and_then(|value| OfflineFrameType::try_from(value).ok())
            {
                Some(OfflineFrameType::KeepAlive) => {
                    let keep_alive = v1.keep_alive.as_ref();
                    if !keep_alive.and_then(|item| item.ack).unwrap_or(false) {
                        send_keep_alive_ack(
                            stream,
                            channel,
                            keep_alive.and_then(|item| item.seq_num),
                        )?;
                    }
                }
                Some(OfflineFrameType::PayloadTransfer) => {
                    let transfer = v1
                        .payload_transfer
                        .as_ref()
                        .ok_or(QuickShareReceiveError::Protocol("missing payload transfer"))?;
                    let payload_type = transfer
                        .payload_header
                        .as_ref()
                        .and_then(|header| header.payload_type)
                        .and_then(|value| PayloadType::try_from(value).ok());
                    match payload_type {
                        Some(PayloadType::File) => {
                            if let Some(file_name) = self.receive_file_chunk(transfer)? {
                                progress(QuickShareReceiveProgress::Receiving {
                                    received_bytes: self.received_bytes,
                                    total_bytes: self.total_bytes,
                                    file_name,
                                });
                            }
                        }
                        Some(PayloadType::Bytes) => {
                            if let Some((id, bytes)) = assembler.push(transfer)? {
                                if self.files.contains_key(&id) {
                                    let file_name = self.receive_file_bytes(id, bytes)?;
                                    progress(QuickShareReceiveProgress::Receiving {
                                        received_bytes: self.received_bytes,
                                        total_bytes: self.total_bytes,
                                        file_name,
                                    });
                                } else if self.texts.contains_key(&id) {
                                    let file_name = self.receive_text(id, bytes)?;
                                    progress(QuickShareReceiveProgress::Receiving {
                                        received_bytes: self.received_bytes,
                                        total_bytes: self.total_bytes,
                                        file_name,
                                    });
                                } else if self.preview_payload_ids.remove(&id) {
                                    println!(
                                        "Received and discarded Quick Share preview ({}) bytes.",
                                        bytes.len()
                                    );
                                } else {
                                    // Newer Android builds may send an authenticated
                                    // optional preview without listing its ID in the
                                    // introduction. The assembler has already bounded
                                    // it to 5 MiB. It is never opened or written.
                                    println!(
                                        "Discarded optional authenticated byte payload {id} ({} bytes).",
                                        bytes.len()
                                    );
                                }
                            }
                        }
                        _ => {
                            return Err(QuickShareReceiveError::Protocol(
                                "unsupported payload type",
                            ))
                        }
                    }
                }
                Some(OfflineFrameType::Disconnection) => {
                    return Err(QuickShareReceiveError::Disconnected)
                }
                _ => {}
            }
        }
        Ok(std::mem::take(&mut self.completed))
    }

    fn receive_file_chunk(
        &mut self,
        transfer: &PayloadTransferFrame,
    ) -> Result<Option<String>, QuickShareReceiveError> {
        let header = transfer
            .payload_header
            .as_ref()
            .ok_or(QuickShareReceiveError::Protocol(
                "missing file payload header",
            ))?;
        let id = header
            .id
            .ok_or(QuickShareReceiveError::Protocol("missing file payload ID"))?;
        let chunk = transfer
            .payload_chunk
            .as_ref()
            .ok_or(QuickShareReceiveError::Protocol(
                "missing file payload chunk",
            ))?;
        let pending = self
            .files
            .get_mut(&id)
            .ok_or(QuickShareReceiveError::Protocol("unknown file payload ID"))?;
        if chunk.offset != Some(pending.received as i64) {
            return Err(QuickShareReceiveError::Protocol("unexpected file offset"));
        }
        let mut progress_name = None;
        if let Some(body) = &chunk.body {
            let next = pending
                .received
                .checked_add(body.len() as u64)
                .ok_or(QuickShareReceiveError::Protocol("file size overflow"))?;
            if next > pending.expected_size {
                return Err(QuickShareReceiveError::Protocol(
                    "file exceeds declared size",
                ));
            }
            pending.file.write_all(body)?;
            pending.received = next;
            self.received_bytes = self.received_bytes.saturating_add(body.len() as u64);
            progress_name = Some(
                pending
                    .final_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("檔案")
                    .to_owned(),
            );
        }
        if chunk.flags.unwrap_or(0) & LAST_CHUNK != 0 {
            if pending.received != pending.expected_size {
                return Err(QuickShareReceiveError::Protocol("incomplete file payload"));
            }
            let pending = self.files.remove(&id).expect("pending file exists");
            pending.file.sync_all()?;
            drop(pending.file);
            fs::rename(&pending.part_path, &pending.final_path)?;
            println!("Saved: {}", pending.final_path.display());
            self.completed.push(pending.final_path);
        }
        Ok(progress_name)
    }

    fn receive_file_bytes(
        &mut self,
        id: i64,
        bytes: Vec<u8>,
    ) -> Result<String, QuickShareReceiveError> {
        let mut pending = self
            .files
            .remove(&id)
            .ok_or(QuickShareReceiveError::Protocol("unknown file payload ID"))?;
        if bytes.len() as u64 != pending.expected_size {
            return Err(QuickShareReceiveError::Protocol(
                "file payload size mismatch",
            ));
        }
        self.received_bytes = self.received_bytes.saturating_add(bytes.len() as u64);
        let file_name = pending
            .final_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("檔案")
            .to_owned();
        pending.file.write_all(&bytes)?;
        pending.file.sync_all()?;
        drop(pending.file);
        fs::rename(&pending.part_path, &pending.final_path)?;
        println!("Saved: {}", pending.final_path.display());
        self.completed.push(pending.final_path);
        Ok(file_name)
    }

    fn receive_text(&mut self, id: i64, bytes: Vec<u8>) -> Result<String, QuickShareReceiveError> {
        let pending = self
            .texts
            .remove(&id)
            .ok_or(QuickShareReceiveError::Protocol("unknown text payload ID"))?;
        if bytes.len() != pending.expected_size {
            return Err(QuickShareReceiveError::Protocol(
                "text payload size mismatch",
            ));
        }
        self.received_bytes = self.received_bytes.saturating_add(bytes.len() as u64);
        let suffix = if pending.text_type == TextType::Url {
            "url.txt"
        } else {
            "txt"
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let path = unique_destination(
            &self.download_directory,
            &format!("Quick Share {timestamp}.{suffix}"),
        );
        fs::write(&path, bytes)?;
        println!("Saved: {}", path.display());
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("分享的文字")
            .to_owned();
        self.completed.push(path);
        Ok(file_name)
    }

    fn cleanup_parts(&self) {
        for pending in self.files.values() {
            let _ = fs::remove_file(&pending.part_path);
        }
    }
}

fn sanitize_file_name(name: &str) -> Result<String, QuickShareReceiveError> {
    let leaf = name
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .ok_or(QuickShareReceiveError::UnsafeMetadata)?;
    sanitize_path_component(leaf)
}

fn sanitize_path_component(component: &str) -> Result<String, QuickShareReceiveError> {
    if component.is_empty() || component == "." || component == ".." {
        return Err(QuickShareReceiveError::UnsafeMetadata);
    }
    let sanitized: String = component
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    if sanitized.trim().is_empty() {
        Err(QuickShareReceiveError::UnsafeMetadata)
    } else {
        Ok(sanitized)
    }
}

fn safe_parent_directory(
    download_directory: &Path,
    parent_folder: Option<&str>,
) -> Result<PathBuf, QuickShareReceiveError> {
    let Some(parent_folder) = parent_folder.filter(|value| !value.is_empty()) else {
        return Ok(download_directory.to_owned());
    };
    let mut directory = download_directory.to_owned();
    for component in parent_folder.split(['/', '\\']) {
        let component = sanitize_path_component(component)?;
        directory.push(component);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(QuickShareReceiveError::UnsafeMetadata)
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&directory)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(directory)
}

fn unique_destination(directory: &Path, name: &str) -> PathBuf {
    let initial = directory.join(name);
    if !initial.exists() {
        return initial;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..u32::MAX {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("hirodrop-{}", OsRng.next_u64()))
}

pub fn default_device_name() -> String {
    std::env::var("HIRODROP_NAME")
        .ok()
        .and_then(valid_device_name)
        .or_else(|| {
            if cfg!(target_os = "macos") {
                command_device_name("/usr/sbin/scutil", &["--get", "ComputerName"])
            } else {
                None
            }
        })
        .or_else(|| std::env::var("HOSTNAME").ok().and_then(valid_device_name))
        .or_else(|| command_device_name("hostname", &[]))
        .unwrap_or_else(|| "HiRodrop".into())
}

fn command_device_name(program: &str, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
        .and_then(valid_device_name)
}

fn valid_device_name(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 255).then(|| value.to_owned())
}

pub fn default_download_directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Downloads").join("HiRodrop"))
        .unwrap_or_else(|| PathBuf::from("HiRodrop Downloads"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, TcpStream};

    #[test]
    fn sender_cannot_escape_the_download_directory() {
        assert_eq!(
            sanitize_file_name("../../secret.txt").unwrap(),
            "secret.txt"
        );
        assert_eq!(
            sanitize_file_name("C:\\work\\report?.pdf").unwrap(),
            "report_.pdf"
        );
        assert!(sanitize_file_name("..").is_err());
        let root = std::env::temp_dir().join(format!("hirodrop-safe-path-{}", OsRng.next_u64()));
        fs::create_dir_all(&root).unwrap();
        assert!(safe_parent_directory(&root, Some("../escape")).is_err());
        assert!(safe_parent_directory(&root, Some("folder/./escape")).is_err());
        assert_eq!(
            safe_parent_directory(&root, Some("資料夾/子目錄")).unwrap(),
            root.join("資料夾/子目錄")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn endpoint_info_parser_reads_utf8_name_and_device_type() {
        let name = "Galaxy 測試機";
        let mut info = vec![2_u8; 17];
        info[0] = 2;
        info.push(name.len() as u8);
        info.extend_from_slice(name.as_bytes());
        let mut frame = offline_v1(OfflineFrameType::ConnectionRequest);
        frame.v1.as_mut().unwrap().connection_request = Some(ConnectionRequestFrame {
            endpoint_info: Some(info),
            ..Default::default()
        });
        let peer =
            parse_connection_request(&frame.encode_to_vec(), "127.0.0.1:1234".parse().unwrap())
                .unwrap();
        assert_eq!(peer.name, name);
        assert_eq!(peer.device_type, 1);
    }

    #[test]
    fn windows_binary_endpoint_name_does_not_break_connection_request() {
        let name = "Windows Quick Share";
        let mut info = vec![6_u8; 17];
        info.push(name.len() as u8);
        info.extend_from_slice(name.as_bytes());
        let mut frame = offline_v1(OfflineFrameType::ConnectionRequest);
        frame.v1.as_mut().unwrap().connection_request = Some(ConnectionRequestFrame {
            endpoint_name: Some(vec![0xff, 0xfe, 0x00, 0x81]),
            endpoint_info: Some(info),
            ..Default::default()
        });

        let peer =
            parse_connection_request(&frame.encode_to_vec(), "127.0.0.1:4321".parse().unwrap())
                .unwrap();
        assert_eq!(peer.name, name);
        assert_eq!(peer.device_type, 3);
    }

    #[test]
    fn listener_accepts_ipv4_and_ipv6_on_one_port() {
        let receiver = match QuickShareReceiver::bind(QuickShareReceiverConfig::new(
            "test",
            PathBuf::from("unused"),
        )) {
            Ok(receiver) => receiver,
            Err(QuickShareReceiveError::Io(error))
                if error.kind() == io::ErrorKind::PermissionDenied =>
            {
                // Some CI/sandbox environments forbid even loopback sockets.
                return;
            }
            Err(error) => panic!("failed to bind dual-stack listener: {error}"),
        };
        let port = receiver.port().unwrap();

        let ipv4 = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (accepted_ipv4, _) = receiver.listener.accept().unwrap();
        match accepted_ipv4.peer_addr().unwrap().ip() {
            IpAddr::V4(address) => assert_eq!(address, Ipv4Addr::LOCALHOST),
            IpAddr::V6(address) => {
                assert_eq!(address.to_ipv4_mapped(), Some(Ipv4Addr::LOCALHOST))
            }
        }
        drop((ipv4, accepted_ipv4));

        let ipv6 = TcpStream::connect(("::1", port)).unwrap();
        let (accepted_ipv6, _) = receiver.listener.accept().unwrap();
        assert!(accepted_ipv6.peer_addr().unwrap().ip().is_ipv6());
        drop((ipv6, accepted_ipv6));
    }
}
