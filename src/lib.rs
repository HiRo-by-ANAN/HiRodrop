//! HiRodrop's Quick Share protocol and local settings implementation.
//!
//! The library covers LAN advertisement and discovery, QR rendezvous,
//! authenticated encryption, user-consented receiving, and file sending. It
//! does not start Bluetooth, Wi-Fi Direct, hotspots, or cloud relays.

pub mod quick_share;
pub mod quick_share_crypto;
pub mod quick_share_discovery;
pub mod quick_share_qr;
pub mod quick_share_receiver;
pub mod quick_share_sender;
pub mod quick_share_transport;
pub mod quick_share_wire;
pub mod settings;
pub use quick_share::{
    QuickShareAdvertisement, QuickShareAdvertisementData, QuickShareDeviceType,
    QuickShareEndpointId, QuickShareError, QUICK_SHARE_SERVICE,
};
pub use quick_share_discovery::{
    NearbyQuickShareDevice, QuickShareBrowser, QuickShareDiscoveryEvent,
};
pub use quick_share_qr::{QuickShareQrEndpoint, QuickShareQrError, QuickShareQrSession};
pub use quick_share_receiver::{
    default_device_name, default_download_directory, ConsentMode, IncomingPeer,
    QuickShareIncomingConnection, QuickShareReceiveError, QuickShareReceiveProgress,
    QuickShareReceiver, QuickShareReceiverConfig, TransferOffer, TransferOfferItem,
    TransferSummary,
};
pub use quick_share_sender::{
    send_files_to_device, send_files_via_qr, send_files_via_qr_cancelable, QuickShareSendError,
    QuickShareSendProgress,
};
pub use settings::{settings_path, HirodropSettings, InterfaceLanguage};
