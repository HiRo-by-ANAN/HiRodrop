use eframe::egui;
use hirodrop_core::{
    send_files_to_device, send_files_via_qr_cancelable, ConsentMode, HirodropSettings,
    InterfaceLanguage, NearbyQuickShareDevice, QuickShareAdvertisement, QuickShareBrowser,
    QuickShareDiscoveryEvent, QuickShareQrError, QuickShareQrSession, QuickShareReceiveError,
    QuickShareReceiveProgress, QuickShareReceiver, QuickShareReceiverConfig, QuickShareSendError,
    QuickShareSendProgress, TransferOffer, TransferOfferItem,
};
use qrcode::{Color, QrCode};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "hirodrop-gui/macos_service.rs"]
mod macos_service;

#[cfg(target_os = "macos")]
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

const BG: egui::Color32 = egui::Color32::from_rgb(19, 20, 22);
const CARD: egui::Color32 = egui::Color32::from_rgb(46, 48, 52);
const CARD_DARK: egui::Color32 = egui::Color32::from_rgb(31, 33, 37);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(151, 187, 255);
const MUTED: egui::Color32 = egui::Color32::from_rgb(174, 177, 184);
const BUTTON_HEIGHT: f32 = 46.0;
const PORTRAIT_WIDTH: f32 = 376.0;
const APP_VERSION_LABEL: &str = "HiRodrop 0.1 Beta";
const OPEN_SOURCE_ACKNOWLEDGEMENTS: &[(&str, &str)] = &[
    ("eframe / egui", "MIT OR Apache-2.0"),
    ("egui default fonts", "OFL-1.1 AND Ubuntu-font-1.0"),
    (
        "RustCrypto (AES, AES-GCM, CBC, HKDF, HMAC, SHA-2, P-256)",
        "MIT OR Apache-2.0",
    ),
    ("base64", "MIT OR Apache-2.0"),
    ("image", "MIT OR Apache-2.0"),
    ("mdns-sd", "MIT OR Apache-2.0"),
    ("notify-rust", "MIT OR Apache-2.0"),
    ("prost", "Apache-2.0"),
    ("qrcode", "MIT OR Apache-2.0"),
    ("rand", "MIT OR Apache-2.0"),
    ("rfd", "MIT"),
    ("serde / serde_json", "MIT OR Apache-2.0"),
    ("socket2", "MIT OR Apache-2.0"),
    ("thiserror", "MIT OR Apache-2.0"),
    ("tray-icon", "MIT OR Apache-2.0"),
];

fn tr(language: InterfaceLanguage, text: &'static str) -> &'static str {
    match language {
        InterfaceLanguage::TraditionalChinese => text,
        InterfaceLanguage::Japanese => match text {
            "設定" => "設定",
            "接收" => "受信",
            "傳送" => "送信",
            "可接收" => "受信可能",
            "未接收" => "受信停止",
            "收到傳送要求" => "共有リクエストを受信",
            "請在兩台裝置核對 PIN" => "両方の端末で PIN を確認してください",
            "接受" => "受け入れる",
            "拒絕" => "拒否",
            "接收準備中…" => "受信準備中…",
            "目前不接收" => "現在は受信していません",
            "附近的 Quick Share 裝置可以找到這台電腦" => {
                "近くの Quick Share 端末からこのパソコンを検出できます"
            }
            "按下方按鈕即可重新進入接收狀態" => "ボタンを押すと受信を再開します",
            "開始接收" => "受信を開始",
            "選擇要分享的內容" => "共有する内容を選択",
            "也可以把檔案或資料夾拖進視窗" => "ファイルやフォルダをここにドロップできます",
            "＋  選取檔案" => "＋  ファイル",
            "＋  選取資料夾" => "＋  フォルダ",
            "編輯" => "編集",
            "清除" => "クリア",
            "與附近裝置分享" => "近くのデバイスと共有",
            "透過現有區域網路自動尋找，不需要 Wi‑Fi 或藍牙介面卡" => {
                "現在の LAN で自動検索します。Wi‑Fi／Bluetooth アダプターは不要です"
            }
            "正在搜尋 Quick Share 裝置…" => "Quick Share デバイスを検索中…",
            "請在 Windows 開啟 Quick Share 接收畫面" => {
                "Windows で Quick Share の受信画面を開いてください"
            }
            "按一下直接傳送，並核對兩台裝置的 PIN" => {
                "クリックして直接送信し、両端末の PIN を確認します"
            }
            "或使用 QR code" => "または QR コードを使用",
            "用 Android 原生 Quick Share 掃描" => "Android の Quick Share でスキャン",
            "取消 QR／改用其他方式" => "QR を中止／別の方法を使う",
            "手機不必安裝 App" => "スマートフォンへのアプリ導入は不要です",
            "找不到區網裝置時會自動顯示 QR" => {
                "LAN デバイスがない場合は QR を自動表示します"
            }
            "掃描後由 Android 原生 Quick Share 接收" => {
                "スキャン後、Android の Quick Share で受信します"
            }
            "顯示 QR" => "QR を表示",
            "裝置名稱" => "デバイス名",
            "裝置分享設定" => "デバイスの共有設定",
            "附近的所有人都能看到分享要求" => "近くにいる全員が共有リクエストを表示できます",
            "目前依 Quick Share 相容模式固定，不能改成聯絡人限定" => {
                "Quick Share 互換モードのため、連絡先限定には変更できません"
            }
            "接收檔案儲存位置" => "受信ファイルの保存先",
            "選擇資料夾" => "フォルダを選択",
            "接收與提醒" => "受信と通知",
            "開啟 HiRodrop 時自動進入接收狀態" => "HiRodrop 起動時に自動で受信を開始",
            "收到要求與完成時通知我" => "リクエスト受信時と完了時に通知",
            "傳送選項" => "送信オプション",
            "顯示速度、容量與傳送進度" => "速度、サイズ、進行状況を表示",
            "找不到區網裝置時自動顯示 QR" => "LAN デバイスがない場合に QR を自動表示",
            "區網傳送失敗時自動重試一次" => "LAN 送信失敗時に一度だけ再試行",
            "語言" => "言語",
            "安全性" => "セキュリティ",
            "開源軟體致謝" => "オープンソース謝辞",
            "HiRodrop 以 GPLv3 或更新版本開源，感謝以下專案：" => {
                "HiRodrop は GPLv3 以降で公開されています。以下のプロジェクトに感謝します。"
            }
            "每次連線皆使用端對端加密，並以 PIN 核對裝置。" => {
                "すべての接続をエンドツーエンドで暗号化し、PIN で端末を確認します。"
            }
            "可信裝置將以密碼學身分實作；目前不會用名稱或 IP 自動放行。" => {
                "信頼済み端末は暗号学的 ID で実装予定です。名前や IP だけでは自動許可しません。"
            }
            "Galaxy 相容性" => "Galaxy 互換性",
            "若啟用三星的 AirDrop 相容模式後 Wi‑Fi 會斷線，請先將它關閉。" => {
                "Samsung の AirDrop 互換モードで Wi‑Fi が切れる場合は、先に無効にしてください。"
            }
            "HiRodrop 不會自行更改手機的 Wi‑Fi 或分享設定。" => {
                "HiRodrop がスマートフォンの Wi‑Fi や共有設定を変更することはありません。"
            }
            "Galaxy 提示：若開啟 AirDrop 相容模式會中斷 Wi‑Fi，請先關閉該模式再使用 LAN／QR 傳送。" => {
                "Galaxy：AirDrop 互換モードで Wi‑Fi が切れる場合は、無効にしてから LAN／QR 送信を使用してください。"
            }
            "儲存設定" => "設定を保存",
            "目前沒有紀錄" => "履歴はありません",
            "活動紀錄" => "アクティビティ",
            "停止接收" => "受信を停止",
            "選取檔案後即可傳送" => "ファイルを選択すると送信できます",
            "請用手機相機或 Quick Share 掃描 QR code" => {
                "スマートフォンのカメラまたは Quick Share で QR をスキャンしてください"
            }
            "已取消 QR，可修改檔案或選擇其他裝置" => {
                "QR を中止しました。ファイルまたは送信先を変更できます"
            }
            "裝置沒有可用的內網位址" => "デバイスに利用可能な LAN アドレスがありません",
            "等待手機掃描 QR code…" => "スマートフォンの QR スキャンを待機中…",
            "正在建立端對端加密連線…" => "エンドツーエンド暗号化接続を確立中…",
            "等待手機接受傳送…" => "スマートフォンの受け入れを待機中…",
            "資料已送達，等待接收端完成寫入…" => "送信済み。受信側の保存完了を待機中…",
            "傳送完成" => "送信完了",
            "收到 Quick Share 傳送要求" => "Quick Share の共有リクエスト",
            "HiRodrop 接收完成" => "HiRodrop 受信完了",
            "檔案已安全儲存，接收服務繼續待命。" => {
                "ファイルを保存しました。引き続き受信を待機します。"
            }
            "HiRodrop 傳送完成" => "HiRodrop 送信完了",
            "所有選取的項目都已送達。" => "選択した項目をすべて送信しました。",
            "HiRodrop 傳送失敗" => "HiRodrop 送信失敗",
            "顯示 HiRodrop" => "HiRodrop を表示",
            "傳送檔案…" => "ファイルを送信…",
            "結束 HiRodrop" => "HiRodrop を終了",
            "HiRodrop · Quick Share 接收中" => "HiRodrop · Quick Share 受信中",
            "接收服務尚未啟動" => "受信サービスはまだ開始されていません",
            "裝置名稱必須是 1–255 UTF-8 bytes" => "デバイス名は 1～255 UTF-8 バイトで入力してください",
            "請選擇接收資料夾" => "受信フォルダを選択してください",
            "正在啟動接收服務…" => "受信サービスを開始中…",
            "正在停止…" => "停止中…",
            "可被 Android 原生 Quick Share 找到" => "Android の Quick Share から検出可能です",
            "接收完成，繼續待命" => "受信完了。引き続き待機中",
            "上一筆連線失敗，仍在待命" => "直前の接続に失敗しました。引き続き待機中",
            "接收服務已停止" => "受信サービスを停止しました",
            "正在接收檔案…" => "ファイルを受信中…",
            "已拒絕，繼續待命" => "拒否しました。引き続き待機中",
            "設定已儲存" => "設定を保存しました",
            "檔案" => "ファイル",
            "Android（QR）" => "Android（QR）",
            _ => text,
        },
        InterfaceLanguage::English => match text {
            "設定" => "Settings",
            "接收" => "Receive",
            "傳送" => "Send",
            "可接收" => "Available",
            "未接收" => "Not receiving",
            "收到傳送要求" => "Incoming share request",
            "請在兩台裝置核對 PIN" => "Verify the PIN on both devices",
            "接受" => "Accept",
            "拒絕" => "Decline",
            "接收準備中…" => "Ready to receive…",
            "目前不接收" => "Not receiving",
            "附近的 Quick Share 裝置可以找到這台電腦" => {
                "Nearby Quick Share devices can find this computer"
            }
            "按下方按鈕即可重新進入接收狀態" => "Use the button below to start receiving",
            "開始接收" => "Start receiving",
            "選擇要分享的內容" => "Choose what to share",
            "也可以把檔案或資料夾拖進視窗" => "You can also drop files or folders here",
            "＋  選取檔案" => "+  Files",
            "＋  選取資料夾" => "+  Folder",
            "編輯" => "Edit",
            "清除" => "Clear",
            "與附近裝置分享" => "Share with nearby devices",
            "透過現有區域網路自動尋找，不需要 Wi‑Fi 或藍牙介面卡" => {
                "Searches your current LAN; no Wi-Fi or Bluetooth adapter required"
            }
            "正在搜尋 Quick Share 裝置…" => "Searching for Quick Share devices…",
            "請在 Windows 開啟 Quick Share 接收畫面" => {
                "Open the Quick Share receive screen on Windows"
            }
            "按一下直接傳送，並核對兩台裝置的 PIN" => {
                "Click to send directly, then verify the PIN on both devices"
            }
            "或使用 QR code" => "Or use a QR code",
            "用 Android 原生 Quick Share 掃描" => "Scan with Android Quick Share",
            "取消 QR／改用其他方式" => "Cancel QR / use another method",
            "手機不必安裝 App" => "No phone app required",
            "找不到區網裝置時會自動顯示 QR" => {
                "QR appears automatically when no LAN device is found"
            }
            "掃描後由 Android 原生 Quick Share 接收" => {
                "Scan to receive with Android Quick Share"
            }
            "顯示 QR" => "Show QR",
            "裝置名稱" => "Device name",
            "裝置分享設定" => "Device visibility",
            "附近的所有人都能看到分享要求" => "Everyone nearby can see share requests",
            "目前依 Quick Share 相容模式固定，不能改成聯絡人限定" => {
                "Quick Share compatibility currently requires this fixed visibility"
            }
            "接收檔案儲存位置" => "Save received files to",
            "選擇資料夾" => "Choose folder",
            "接收與提醒" => "Receiving and notifications",
            "開啟 HiRodrop 時自動進入接收狀態" => "Start receiving when HiRodrop opens",
            "收到要求與完成時通知我" => "Notify me for requests and completed transfers",
            "傳送選項" => "Sending options",
            "顯示速度、容量與傳送進度" => "Show speed, size, and transfer progress",
            "找不到區網裝置時自動顯示 QR" => "Show QR when no LAN device is found",
            "區網傳送失敗時自動重試一次" => "Retry a failed LAN transfer once",
            "語言" => "Language",
            "安全性" => "Security",
            "開源軟體致謝" => "Open-source acknowledgements",
            "HiRodrop 以 GPLv3 或更新版本開源，感謝以下專案：" => {
                "HiRodrop is released under GPLv3 or later. Thanks to these projects:"
            }
            "每次連線皆使用端對端加密，並以 PIN 核對裝置。" => {
                "Every connection is end-to-end encrypted and verified with a PIN."
            }
            "可信裝置將以密碼學身分實作；目前不會用名稱或 IP 自動放行。" => {
                "Trusted devices will use cryptographic identity; names and IPs are never auto-approved."
            }
            "Galaxy 相容性" => "Galaxy compatibility",
            "若啟用三星的 AirDrop 相容模式後 Wi‑Fi 會斷線，請先將它關閉。" => {
                "If Samsung AirDrop compatibility disconnects Wi-Fi, turn it off before using HiRodrop."
            }
            "HiRodrop 不會自行更改手機的 Wi‑Fi 或分享設定。" => {
                "HiRodrop never changes phone Wi-Fi or sharing settings."
            }
            "Galaxy 提示：若開啟 AirDrop 相容模式會中斷 Wi‑Fi，請先關閉該模式再使用 LAN／QR 傳送。" => {
                "Galaxy: if AirDrop compatibility disconnects Wi-Fi, turn it off before LAN or QR transfers."
            }
            "儲存設定" => "Save settings",
            "目前沒有紀錄" => "No activity yet",
            "活動紀錄" => "Activity",
            "停止接收" => "Stop receiving",
            "選取檔案後即可傳送" => "Choose files to begin sending",
            "請用手機相機或 Quick Share 掃描 QR code" => {
                "Scan the QR code with your phone camera or Quick Share"
            }
            "已取消 QR，可修改檔案或選擇其他裝置" => {
                "QR cancelled. You can edit files or choose another device"
            }
            "裝置沒有可用的內網位址" => "The device has no available LAN address",
            "等待手機掃描 QR code…" => "Waiting for the phone to scan the QR code…",
            "正在建立端對端加密連線…" => "Establishing an end-to-end encrypted connection…",
            "等待手機接受傳送…" => "Waiting for the phone to accept…",
            "資料已送達，等待接收端完成寫入…" => {
                "Data delivered; waiting for the receiver to finish saving…"
            }
            "傳送完成" => "Transfer complete",
            "收到 Quick Share 傳送要求" => "Incoming Quick Share request",
            "HiRodrop 接收完成" => "HiRodrop receive complete",
            "檔案已安全儲存，接收服務繼續待命。" => {
                "Files saved safely. HiRodrop is still ready to receive."
            }
            "HiRodrop 傳送完成" => "HiRodrop transfer complete",
            "所有選取的項目都已送達。" => "All selected items were delivered.",
            "HiRodrop 傳送失敗" => "HiRodrop transfer failed",
            "顯示 HiRodrop" => "Show HiRodrop",
            "傳送檔案…" => "Send files…",
            "結束 HiRodrop" => "Quit HiRodrop",
            "HiRodrop · Quick Share 接收中" => "HiRodrop · Quick Share receiving",
            "接收服務尚未啟動" => "The receiving service has not started",
            "裝置名稱必須是 1–255 UTF-8 bytes" => "Device name must be 1–255 UTF-8 bytes",
            "請選擇接收資料夾" => "Choose a folder for received files",
            "正在啟動接收服務…" => "Starting the receiving service…",
            "正在停止…" => "Stopping…",
            "可被 Android 原生 Quick Share 找到" => "Visible to Android Quick Share",
            "接收完成，繼續待命" => "Receive complete; still available",
            "上一筆連線失敗，仍在待命" => "Last connection failed; still available",
            "接收服務已停止" => "Receiving service stopped",
            "正在接收檔案…" => "Receiving files…",
            "已拒絕，繼續待命" => "Declined; still available",
            "設定已儲存" => "Settings saved",
            "檔案" => "File",
            "Android（QR）" => "Android (QR)",
            _ => text,
        },
    }
}

fn main() -> eframe::Result {
    let launch_files = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([PORTRAIT_WIDTH, 780.0])
            .with_min_inner_size([350.0, 650.0])
            .with_drag_and_drop(true)
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "HiRodrop",
        options,
        Box::new(move |creation| {
            configure_native_notification_identity();
            configure_system_font(&creation.egui_ctx);
            configure_theme(&creation.egui_ctx);
            #[cfg(target_os = "macos")]
            let finder_files = macos_service::install();
            Ok(Box::new(HirodropApp::new(
                launch_files,
                #[cfg(target_os = "macos")]
                finder_files,
            )))
        }),
    )
}

fn configure_native_notification_identity() {
    #[cfg(target_os = "macos")]
    {
        // mac-notification-sys otherwise asks AppleScript to locate an app
        // literally named `use_default`, which opens the macOS application
        // chooser. Pin notifications to our bundle before the first alert.
        let _ = notify_rust::set_application("com.hirodrop.desktop");
    }
}

fn configure_system_font(context: &egui::Context) {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
        ]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ]
    };
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let font_name = "hirodrop-cjk".to_owned();
    fonts
        .font_data
        .insert(font_name.clone(), egui::FontData::from_owned(bytes).into());
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push(font_name.clone());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(font_name);
    context.set_fonts(fonts);
}

fn configure_theme(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(59, 62, 68);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(73, 79, 90);
    visuals.selection.bg_fill = egui::Color32::from_rgb(62, 91, 145);
    visuals.hyperlink_color = ACCENT;
    context.set_visuals(visuals);
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Page {
    Receive,
    Send,
    Settings,
}

enum ServiceEvent {
    Listening(u16),
    Offer(TransferOffer, Sender<bool>),
    Receiving {
        received_bytes: u64,
        total_bytes: u64,
        file_name: String,
    },
    Complete {
        peer_name: String,
        paths: String,
    },
    Error(String),
    Stopped,
}

enum SenderEvent {
    Progress(QuickShareSendProgress),
    Failed(QuickShareSendError),
}

struct PendingConsent {
    offer: TransferOffer,
    reply: Sender<bool>,
}

struct HirodropApp {
    display_name: String,
    download_directory: String,
    start_receiving_on_launch: bool,
    show_notifications: bool,
    show_transfer_details: bool,
    auto_show_qr: bool,
    retry_failed_once: bool,
    language: InterfaceLanguage,
    trusted_fingerprints: Vec<String>,
    auto_start_pending: bool,
    running: bool,
    status: String,
    activity: Vec<String>,
    event_rx: Option<Receiver<ServiceEvent>>,
    stop: Option<Arc<AtomicBool>>,
    pending: Option<PendingConsent>,
    page: Page,
    selected_files: Vec<PathBuf>,
    scroll_send_to_top: bool,
    send_running: bool,
    send_failed: bool,
    send_status: String,
    send_progress: f32,
    receive_progress: f32,
    send_bytes: u64,
    send_total: u64,
    receive_bytes: u64,
    receive_total: u64,
    transfer_started_at: Option<Instant>,
    files_selected_at: Option<Instant>,
    send_pin: Option<String>,
    selected_device_name: Option<String>,
    qr_url: Option<String>,
    qr_code: Option<QrCode>,
    sender_rx: Option<Receiver<SenderEvent>>,
    send_cancel: Option<Arc<AtomicBool>>,
    qr_waiting_for_scan: bool,
    browser: Option<QuickShareBrowser>,
    nearby: BTreeMap<String, NearbyQuickShareDevice>,
    toast: Option<(String, String, Instant)>,
    #[cfg(target_os = "macos")]
    tray: Option<AppTray>,
    #[cfg(target_os = "macos")]
    finder_files: Receiver<Vec<PathBuf>>,
    #[cfg(target_os = "macos")]
    queued_finder_files: Vec<PathBuf>,
    quitting: bool,
}

impl HirodropApp {
    fn new(
        launch_files: Vec<PathBuf>,
        #[cfg(target_os = "macos")] finder_files: Receiver<Vec<PathBuf>>,
    ) -> Self {
        let settings = HirodropSettings::load();
        let has_launch_files = !launch_files.is_empty();
        Self {
            display_name: settings.display_name,
            download_directory: settings.download_directory.display().to_string(),
            start_receiving_on_launch: settings.start_receiving_on_launch,
            show_notifications: settings.show_notifications,
            show_transfer_details: settings.show_transfer_details,
            auto_show_qr: settings.auto_show_qr,
            retry_failed_once: settings.retry_failed_once,
            language: settings.language,
            trusted_fingerprints: settings.trusted_fingerprints,
            auto_start_pending: settings.start_receiving_on_launch,
            running: false,
            status: tr(settings.language, "接收服務尚未啟動").into(),
            activity: Vec::new(),
            event_rx: None,
            stop: None,
            pending: None,
            page: if has_launch_files {
                Page::Send
            } else {
                Page::Receive
            },
            selected_files: launch_files,
            scroll_send_to_top: has_launch_files,
            send_running: false,
            send_failed: false,
            send_status: tr(settings.language, "選取檔案後即可傳送").into(),
            send_progress: 0.0,
            receive_progress: 0.0,
            send_bytes: 0,
            send_total: 0,
            receive_bytes: 0,
            receive_total: 0,
            transfer_started_at: None,
            files_selected_at: has_launch_files.then(Instant::now),
            send_pin: None,
            selected_device_name: None,
            qr_url: None,
            qr_code: None,
            sender_rx: None,
            send_cancel: None,
            qr_waiting_for_scan: false,
            browser: QuickShareBrowser::start().ok(),
            nearby: BTreeMap::new(),
            toast: None,
            #[cfg(target_os = "macos")]
            tray: AppTray::new(&app_icon(), settings.language),
            #[cfg(target_os = "macos")]
            finder_files,
            #[cfg(target_os = "macos")]
            queued_finder_files: Vec::new(),
            quitting: false,
        }
    }

    fn settings(&self) -> HirodropSettings {
        HirodropSettings {
            display_name: self.display_name.trim().to_owned(),
            download_directory: PathBuf::from(self.download_directory.trim()),
            start_receiving_on_launch: self.start_receiving_on_launch,
            show_notifications: self.show_notifications,
            show_transfer_details: self.show_transfer_details,
            auto_show_qr: self.auto_show_qr,
            retry_failed_once: self.retry_failed_once,
            language: self.language,
            trusted_fingerprints: self.trusted_fingerprints.clone(),
        }
    }

    fn save_settings(&mut self) {
        if let Err(error) = self.settings().save() {
            self.activity
                .push(settings_save_failed(self.language, &error.to_string()));
        }
        #[cfg(target_os = "macos")]
        {
            self.tray = AppTray::new(&app_icon(), self.language);
        }
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        let display_name = self.display_name.trim().to_owned();
        if display_name.is_empty() || display_name.len() > 255 {
            self.status = tr(self.language, "裝置名稱必須是 1–255 UTF-8 bytes").into();
            return;
        }
        let download_directory = PathBuf::from(self.download_directory.trim());
        if download_directory.as_os_str().is_empty() {
            self.status = tr(self.language, "請選擇接收資料夾").into();
            return;
        }
        self.save_settings();
        let (event_tx, event_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        thread::spawn(move || {
            receive_service(display_name, download_directory, worker_stop, event_tx)
        });
        self.event_rx = Some(event_rx);
        self.stop = Some(stop);
        self.running = true;
        self.status = tr(self.language, "正在啟動接收服務…").into();
    }

    fn stop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(pending) = self.pending.take() {
            let _ = pending.reply.send(false);
        }
        self.status = tr(self.language, "正在停止…").into();
    }

    fn prepare_to_quit(&mut self) {
        self.quitting = true;
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(pending) = self.pending.take() {
            let _ = pending.reply.send(false);
        }
        if let Some(cancel) = self.send_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn add_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if path.is_file() && !self.selected_files.contains(&path) {
                self.selected_files.push(path.clone());
            } else if path.is_dir() && !self.selected_files.contains(&path) {
                // The backend protocol handles folders natively, but we also want the GUI
                // to accept the folder itself as a valid selection so it can be passed to the core logic.
                self.selected_files.push(path);
            }
        }
        if !self.selected_files.is_empty() && !self.send_running {
            self.send_failed = false;
            self.send_status = selected_status(self.language, self.selected_files.len());
            self.files_selected_at = Some(Instant::now());
            self.send_pin = None;
            self.selected_device_name = None;
        }
    }

    #[cfg(target_os = "macos")]
    fn poll_finder_files(&mut self, context: &egui::Context) {
        while let Ok(files) = self.finder_files.try_recv() {
            for file in files {
                if !self.queued_finder_files.contains(&file) {
                    self.queued_finder_files.push(file);
                }
            }
        }

        if self.qr_waiting_for_scan && !self.queued_finder_files.is_empty() {
            self.cancel_qr_waiting();
        }
        if self.send_running || self.queued_finder_files.is_empty() {
            return;
        }

        let files = std::mem::take(&mut self.queued_finder_files);
        self.selected_files.clear();
        self.qr_code = None;
        self.qr_url = None;
        self.send_pin = None;
        self.selected_device_name = None;
        self.page = Page::Send;
        self.scroll_send_to_top = true;
        self.add_files(files);
        context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        context.send_viewport_cmd(egui::ViewportCommand::Focus);
        context.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
    }

    fn begin_send(&mut self) {
        if self.send_running || self.selected_files.is_empty() {
            return;
        }
        let qr = match QuickShareQrSession::generate() {
            Ok(qr) => qr,
            Err(error) => {
                self.send_status = error.to_string();
                return;
            }
        };
        let url = qr.url();
        let code = match QrCode::new(url.as_bytes()) {
            Ok(code) => code,
            Err(error) => {
                self.send_status = qr_error_status(self.language, &error.to_string());
                return;
            }
        };
        let files = self.selected_files.clone();
        let name = self.display_name.trim().to_owned();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        thread::spawn(move || {
            let event_sender = sender.clone();
            let result =
                send_files_via_qr_cancelable(files, &name, qr, &worker_cancel, move |progress| {
                    eprintln!("HiRodrop sender: {progress:?}");
                    let _ = event_sender.send(SenderEvent::Progress(progress));
                });
            if let Err(error) = result {
                eprintln!("HiRodrop sender failed: {error}");
                let _ = sender.send(SenderEvent::Failed(error));
            }
        });
        self.sender_rx = Some(receiver);
        self.send_cancel = Some(cancel);
        self.qr_waiting_for_scan = true;
        self.qr_url = Some(url);
        self.qr_code = Some(code);
        self.send_running = true;
        self.send_failed = false;
        self.send_progress = 0.0;
        self.send_bytes = 0;
        self.send_total = total_selected_bytes(&self.selected_files);
        self.transfer_started_at = Some(Instant::now());
        self.files_selected_at = None;
        self.send_pin = None;
        self.selected_device_name = Some(tr(self.language, "Android（QR）").into());
        self.send_status = tr(self.language, "請用手機相機或 Quick Share 掃描 QR code").into();
    }

    fn cancel_qr_waiting(&mut self) {
        if let Some(cancel) = self.send_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.sender_rx = None;
        self.qr_url = None;
        self.qr_code = None;
        self.qr_waiting_for_scan = false;
        self.send_running = false;
        self.send_failed = false;
        self.send_progress = 0.0;
        self.send_bytes = 0;
        self.send_pin = None;
        self.selected_device_name = None;
        self.send_status = tr(self.language, "已取消 QR，可修改檔案或選擇其他裝置").into();
    }

    fn begin_direct_send(&mut self, device: NearbyQuickShareDevice) {
        if self.send_running || self.selected_files.is_empty() {
            return;
        }
        let Some(address) = device.addresses.first().copied() else {
            self.send_status = tr(self.language, "裝置沒有可用的內網位址").into();
            return;
        };
        let files = self.selected_files.clone();
        let local_name = self.display_name.trim().to_owned();
        let device_name = device.name.clone();
        let worker_device_name = device_name.clone();
        let retry_failed_once = self.retry_failed_once;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let attempts = if retry_failed_once { 2 } else { 1 };
            let mut result = Ok(());
            for attempt in 0..attempts {
                let event_sender = sender.clone();
                result =
                    send_files_to_device(files.clone(), &local_name, address, move |progress| {
                        eprintln!("HiRodrop sender: {progress:?}");
                        let _ = event_sender.send(SenderEvent::Progress(progress));
                    });
                if result.is_ok() {
                    break;
                }
                if result.as_ref().is_err_and(|error| !error.is_retryable()) {
                    break;
                }
                if attempt + 1 < attempts {
                    let _ =
                        sender.send(SenderEvent::Progress(QuickShareSendProgress::Negotiating {
                            phase: "第一次連線失敗，正在自動重試",
                        }));
                }
            }
            if let Err(error) = result {
                eprintln!("HiRodrop sender failed for {worker_device_name}: {error}");
                let _ = sender.send(SenderEvent::Failed(error));
            }
        });
        self.sender_rx = Some(receiver);
        self.qr_url = None;
        self.qr_code = None;
        self.send_running = true;
        self.send_failed = false;
        self.send_progress = 0.0;
        self.send_bytes = 0;
        self.send_total = total_selected_bytes(&self.selected_files);
        self.transfer_started_at = Some(Instant::now());
        self.files_selected_at = None;
        self.send_pin = None;
        self.selected_device_name = Some(device_name.clone());
        self.send_status = connecting_status(self.language, &device_name);
    }

    fn poll_nearby(&mut self) {
        let Some(browser) = &self.browser else {
            return;
        };
        for update in browser.poll() {
            match update {
                QuickShareDiscoveryEvent::Found(device) => {
                    if device.name != self.display_name {
                        self.nearby.insert(device.id.clone(), device);
                    }
                }
                QuickShareDiscoveryEvent::Removed(id) => {
                    self.nearby.remove(&id);
                }
            }
        }
    }

    fn poll_events(&mut self, context: &egui::Context) {
        let mut service_events = Vec::new();
        if let Some(receiver) = &self.event_rx {
            while let Ok(event) = receiver.try_recv() {
                service_events.push(event);
            }
        }
        for event in service_events {
            match event {
                ServiceEvent::Listening(port) => {
                    self.status = tr(self.language, "可被 Android 原生 Quick Share 找到").into();
                    self.activity.push(listening_activity(self.language, port));
                }
                ServiceEvent::Offer(offer, reply) => {
                    if self.pending.is_some() {
                        let _ = reply.send(false);
                        self.activity
                            .push(concurrent_reject_activity(self.language, &offer.peer.name));
                        continue;
                    }
                    let peer_name = offer.peer.name.clone();
                    let items_len = offer.items.len();
                    let pin = offer.pin.clone();
                    let notification_body =
                        incoming_notification(self.language, &peer_name, items_len, &pin);

                    self.status = incoming_status(self.language, &offer.peer.name);
                    self.receive_progress = 0.0;
                    self.receive_bytes = 0;
                    self.receive_total = offer.items.iter().map(|item| item.size).sum();
                    self.transfer_started_at = Some(Instant::now());
                    self.pending = Some(PendingConsent { offer, reply });
                    self.page = Page::Receive;
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    context.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if self.show_notifications {
                        if let Err(error) = notify_rust::Notification::new()
                            .summary("HiRodrop")
                            .body(&notification_body)
                            .show()
                        {
                            eprintln!("Failed to show notification: {error}");
                        }
                    }
                    self.request_attention(context, egui::UserAttentionType::Critical);
                    self.notify(
                        tr(self.language, "收到 Quick Share 傳送要求"),
                        &notification_body,
                    );
                }
                ServiceEvent::Receiving {
                    received_bytes,
                    total_bytes,
                    file_name,
                } => {
                    self.receive_bytes = received_bytes;
                    self.receive_total = total_bytes;
                    self.receive_progress = if total_bytes == 0 {
                        1.0
                    } else {
                        received_bytes as f32 / total_bytes as f32
                    };
                    self.status = receiving_status(self.language, &file_name);
                }
                ServiceEvent::Complete { peer_name, paths } => {
                    self.status = tr(self.language, "接收完成，繼續待命").into();
                    self.receive_progress = 1.0;
                    self.receive_bytes = self.receive_total;
                    self.activity
                        .push(received_activity(self.language, &peer_name, &paths));
                    self.request_attention(context, egui::UserAttentionType::Informational);
                    self.notify(
                        tr(self.language, "HiRodrop 接收完成"),
                        tr(self.language, "檔案已安全儲存，接收服務繼續待命。"),
                    );
                }
                ServiceEvent::Error(message) => {
                    self.status = tr(self.language, "上一筆連線失敗，仍在待命").into();
                    self.activity
                        .push(connection_failed_activity(self.language, &message));
                }
                ServiceEvent::Stopped => {
                    self.running = false;
                    self.stop = None;
                    self.pending = None;
                    self.receive_progress = 0.0;
                    self.status = tr(self.language, "接收服務已停止").into();
                }
            }
        }

        let mut sender_events = Vec::new();
        if let Some(receiver) = &self.sender_rx {
            while let Ok(event) = receiver.try_recv() {
                sender_events.push(event);
            }
        }
        for event in sender_events {
            match event {
                SenderEvent::Progress(QuickShareSendProgress::WaitingForQrScan) => {
                    self.send_status = tr(self.language, "等待手機掃描 QR code…").into();
                }
                SenderEvent::Progress(QuickShareSendProgress::PhoneFound { name }) => {
                    self.qr_waiting_for_scan = false;
                    self.send_status = found_status(self.language, &name);
                }
                SenderEvent::Progress(QuickShareSendProgress::Connecting) => {
                    self.send_status = tr(self.language, "正在建立端對端加密連線…").into();
                }
                SenderEvent::Progress(QuickShareSendProgress::Negotiating { phase }) => {
                    self.send_status = negotiating_status(self.language, phase);
                }
                SenderEvent::Progress(QuickShareSendProgress::VerificationPin { pin }) => {
                    self.send_pin = Some(pin.clone());
                    self.send_status = pin_status(self.language, &pin);
                }
                SenderEvent::Progress(QuickShareSendProgress::WaitingForAcceptance) => {
                    self.send_status = tr(self.language, "等待手機接受傳送…").into();
                }
                SenderEvent::Progress(QuickShareSendProgress::Finalizing) => {
                    self.send_status = tr(self.language, "資料已送達，等待接收端完成寫入…").into();
                    self.send_progress = 1.0;
                }
                SenderEvent::Progress(QuickShareSendProgress::Sending {
                    sent_bytes,
                    total_bytes,
                    file_name,
                }) => {
                    self.send_bytes = sent_bytes;
                    self.send_total = total_bytes;
                    self.send_progress = if total_bytes == 0 {
                        1.0
                    } else {
                        sent_bytes as f32 / total_bytes as f32
                    };
                    self.send_status = sending_status(self.language, &file_name);
                }
                SenderEvent::Progress(QuickShareSendProgress::Complete) => {
                    self.send_running = false;
                    self.send_progress = 1.0;
                    self.send_bytes = self.send_total;
                    self.send_status = tr(self.language, "傳送完成").into();
                    self.activity
                        .push(sent_activity(self.language, self.selected_files.len()));
                    self.qr_code = None;
                    self.qr_url = None;
                    self.send_cancel = None;
                    self.qr_waiting_for_scan = false;
                    self.request_attention(context, egui::UserAttentionType::Informational);
                    self.notify(
                        tr(self.language, "HiRodrop 傳送完成"),
                        tr(self.language, "所有選取的項目都已送達。"),
                    );
                }
                SenderEvent::Failed(error) => {
                    self.send_running = false;
                    self.send_failed = true;
                    self.send_cancel = None;
                    self.qr_waiting_for_scan = false;
                    let message = send_error_message(self.language, &error);
                    self.send_status = failed_status(self.language, &message);
                    self.activity.push(self.send_status.clone());
                    self.notify(tr(self.language, "HiRodrop 傳送失敗"), &message);
                }
            }
        }
    }

    fn maybe_auto_start_qr(&mut self) {
        let ready = self.page == Page::Send
            && self.auto_show_qr
            && !self.selected_files.is_empty()
            && self.nearby.is_empty()
            && !self.send_running
            && self.qr_code.is_none()
            && !self.send_failed;
        if ready
            && self
                .files_selected_at
                .is_some_and(|selected| selected.elapsed() >= Duration::from_millis(1800))
        {
            self.begin_send();
        }
    }

    fn request_attention(&self, context: &egui::Context, kind: egui::UserAttentionType) {
        if self.show_notifications {
            context.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(kind));
        }
    }

    fn notify(&mut self, summary: &str, body: &str) {
        if !self.show_notifications {
            return;
        }
        self.toast = Some((summary.to_owned(), body.to_owned(), Instant::now()));
        #[cfg(target_os = "macos")]
        {
            let summary = summary.to_owned();
            let body = body.to_owned();
            thread::spawn(move || {
                let mut notification = notify_rust::Notification::new();
                notification
                    .appname("HiRodrop")
                    .summary(&summary)
                    .body(&body);
                let _ = notification.show();
            });
        }
    }

    fn poll_tray(&mut self, context: &egui::Context) {
        #[cfg(target_os = "macos")]
        if let Some(tray) = &self.tray {
            let show_id = tray.show_id.clone();
            let send_id = tray.send_id.clone();
            let quit_id = tray.quit_id.clone();
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == show_id {
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    context.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == send_id {
                    self.page = Page::Send;
                    context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    context.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == quit_id {
                    self.prepare_to_quit();
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.set_height(58.0);
        if self.page == Page::Settings {
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [BUTTON_HEIGHT, BUTTON_HEIGHT],
                        egui::Button::new(egui::RichText::new("←").size(22.0)).corner_radius(23),
                    )
                    .clicked()
                {
                    self.page = Page::Receive;
                }
                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(CARD)
                    .corner_radius(24)
                    .inner_margin(egui::Margin::symmetric(28, 11))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(tr(self.language, "設定"))
                                .size(20.0)
                                .strong(),
                        );
                    });
            });
            return;
        }

        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(CARD)
                .corner_radius(27)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        paint_avatar(ui, 38.0, &self.display_name);
                        ui.add_space(5.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&self.display_name).size(17.0).strong());
                            let state = tr(
                                self.language,
                                if self.running {
                                    "可接收"
                                } else {
                                    "未接收"
                                },
                            );
                            ui.label(egui::RichText::new(state).size(11.0).color(MUTED));
                        });
                    });
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized(
                        [BUTTON_HEIGHT, BUTTON_HEIGHT],
                        egui::Button::new(egui::RichText::new("⋮").size(25.0))
                            .fill(BG)
                            .corner_radius(23),
                    )
                    .on_hover_text(tr(self.language, "設定"))
                    .clicked()
                {
                    self.page = Page::Settings;
                }
            });
        });
    }

    fn receive_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        if let Some(pending) = &self.pending {
            let peer_name = pending.offer.peer.name.clone();
            let pin = pending.offer.pin.clone();
            let items = pending.offer.items.clone();
            let mut decision = None;
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(37, 49, 68))
                .stroke(egui::Stroke::new(1.5, ACCENT))
                .corner_radius(24)
                .inner_margin(egui::Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(tr(self.language, "收到傳送要求"))
                                .size(24.0)
                                .strong(),
                        );
                        ui.add_space(18.0);
                        ui.label(
                            egui::RichText::new(peer_label(self.language, &peer_name)).size(17.0),
                        );
                        ui.label(
                            egui::RichText::new(offer_summary(&items, self.language))
                                .size(14.0)
                                .color(MUTED),
                        );
                        ui.add_space(14.0);
                        for item in items.iter().take(3) {
                            ui.label(egui::RichText::new(&item.name).size(15.0).strong());
                        }
                        if items.len() > 3 {
                            ui.label(
                                egui::RichText::new(more_items_label(
                                    self.language,
                                    items.len() - 3,
                                ))
                                .color(MUTED),
                            );
                        }
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new(tr(self.language, "請在兩台裝置核對 PIN"))
                                .color(MUTED),
                        );
                        ui.label(
                            egui::RichText::new(&pin)
                                .size(36.0)
                                .strong()
                                .monospace()
                                .color(ACCENT),
                        );
                        ui.add_space(22.0);
                        if ui
                            .add_sized(
                                [ui.available_width(), BUTTON_HEIGHT],
                                egui::Button::new(
                                    egui::RichText::new(tr(self.language, "接受"))
                                        .strong()
                                        .color(CARD_DARK),
                                )
                                .fill(ACCENT)
                                .corner_radius(23),
                            )
                            .clicked()
                        {
                            decision = Some(true);
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                [ui.available_width(), BUTTON_HEIGHT],
                                egui::Button::new(tr(self.language, "拒絕")).corner_radius(23),
                            )
                            .clicked()
                        {
                            decision = Some(false);
                        }
                    });
                });
            if let Some(accepted) = decision {
                let pending = self.pending.take().expect("pending offer");
                let _ = pending.reply.send(accepted);
                self.status = if accepted {
                    tr(self.language, "正在接收檔案…").into()
                } else {
                    tr(self.language, "已拒絕，繼續待命").into()
                };
            }
            return;
        }

        egui::Frame::new()
            .fill(CARD)
            .corner_radius(24)
            .inner_margin(egui::Margin::same(22))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(390.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(78.0);
                    paint_receive_icon(ui, 74.0, self.running);
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new(tr(
                            self.language,
                            if self.running {
                                "接收準備中…"
                            } else {
                                "目前不接收"
                            },
                        ))
                        .size(23.0)
                        .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(tr(
                            self.language,
                            if self.running {
                                "附近的 Quick Share 裝置可以找到這台電腦"
                            } else {
                                "按下方按鈕即可重新進入接收狀態"
                            },
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    if self.receive_progress > 0.0 && self.receive_progress < 1.0 {
                        ui.add_space(18.0);
                        ui.add(
                            egui::ProgressBar::new(self.receive_progress)
                                .desired_width(300.0)
                                .show_percentage()
                                .animate(true),
                        );
                        if self.show_transfer_details {
                            ui.label(
                                egui::RichText::new(transfer_details(
                                    self.receive_bytes,
                                    self.receive_total,
                                    self.transfer_started_at,
                                ))
                                .size(12.0)
                                .color(MUTED),
                            );
                        }
                    }
                    if !self.running {
                        ui.add_space(22.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(tr(self.language, "開始接收"))
                                        .size(15.0)
                                        .color(CARD_DARK),
                                )
                                .fill(ACCENT)
                                .corner_radius(23)
                                .min_size(egui::vec2(180.0, BUTTON_HEIGHT)),
                            )
                            .clicked()
                        {
                            self.start();
                        }
                    }
                });
            });
    }

    fn send_page(&mut self, ui: &mut egui::Ui) {
        if std::mem::take(&mut self.scroll_send_to_top) {
            ui.scroll_to_cursor(Some(egui::Align::TOP));
        }
        ui.add_space(18.0);
        let hovering = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
        if self.selected_files.is_empty() {
            egui::Frame::new()
                .fill(if hovering {
                    egui::Color32::from_rgb(48, 58, 76)
                } else {
                    CARD
                })
                .stroke(egui::Stroke::new(1.5, if hovering { ACCENT } else { CARD }))
                .corner_radius(26)
                .inner_margin(egui::Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(430.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(76.0);
                        paint_send_icon(ui, 76.0);
                        ui.add_space(24.0);
                        ui.label(
                            egui::RichText::new(tr(self.language, "選擇要分享的內容"))
                                .size(23.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(tr(self.language, "也可以把檔案或資料夾拖進視窗"))
                                .size(14.0)
                                .color(MUTED),
                        );
                        ui.add_space(26.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_sized(
                                    [145.0, BUTTON_HEIGHT],
                                    egui::Button::new(
                                        egui::RichText::new(tr(self.language, "＋  選取檔案"))
                                            .size(15.0)
                                            .strong(),
                                    )
                                    .corner_radius(23),
                                )
                                .clicked()
                            {
                                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                                    self.add_files(paths);
                                }
                            }
                            if ui
                                .add_sized(
                                    [145.0, BUTTON_HEIGHT],
                                    egui::Button::new(
                                        egui::RichText::new(tr(self.language, "＋  選取資料夾"))
                                            .size(15.0)
                                            .strong(),
                                    )
                                    .corner_radius(23),
                                )
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    self.add_files([path]);
                                }
                            }
                        });
                    });
                });
            return;
        }

        egui::Frame::new()
            .fill(if hovering {
                egui::Color32::from_rgb(48, 58, 76)
            } else {
                CARD_DARK
            })
            .stroke(egui::Stroke::new(1.0, if hovering { ACCENT } else { CARD }))
            .corner_radius(20)
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {}",
                                selected_kind(&self.selected_files, self.language),
                                human_bytes(total_selected_bytes(&self.selected_files))
                            ))
                            .size(17.0)
                            .strong(),
                        );
                        let preview = self
                            .selected_files
                            .first()
                            .and_then(|path| path.file_name())
                            .and_then(|name| name.to_str())
                            .unwrap_or(tr(self.language, "檔案"));
                        ui.label(egui::RichText::new(preview).color(MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = tr(self.language, "編輯");
                        let can_change = !self.send_running || self.qr_waiting_for_scan;
                        if ui
                            .add_enabled(
                                can_change,
                                egui::Button::new(label)
                                    .corner_radius(22)
                                    .min_size(egui::vec2(92.0, BUTTON_HEIGHT)),
                            )
                            .clicked()
                        {
                            if self.qr_waiting_for_scan {
                                self.cancel_qr_waiting();
                            }
                            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                                self.add_files(paths);
                            }
                        }
                        if !self.selected_files.is_empty()
                            && can_change
                            && ui
                                .add_sized(
                                    [BUTTON_HEIGHT, BUTTON_HEIGHT],
                                    egui::Button::new("×").corner_radius(22),
                                )
                                .on_hover_text(tr(self.language, "清除"))
                                .clicked()
                        {
                            if self.qr_waiting_for_scan {
                                self.cancel_qr_waiting();
                            }
                            self.selected_files.clear();
                            self.files_selected_at = None;
                            self.selected_device_name = None;
                            self.send_pin = None;
                            self.send_failed = false;
                            self.send_status = tr(self.language, "選取檔案後即可傳送").into();
                        }
                    });
                });
            });

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(tr(self.language, "與附近裝置分享"))
                .size(18.0)
                .strong(),
        );
        ui.label(
            egui::RichText::new(tr(
                self.language,
                "透過現有區域網路自動尋找，不需要 Wi‑Fi 或藍牙介面卡",
            ))
            .size(12.0)
            .color(MUTED),
        );
        ui.add_space(8.0);
        let devices = self.nearby.values().cloned().collect::<Vec<_>>();
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(22)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if devices.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(tr(self.language, "正在搜尋 Quick Share 裝置…"))
                                .color(MUTED),
                        );
                        ui.label(
                            egui::RichText::new(tr(
                                self.language,
                                "請在 Windows 開啟 Quick Share 接收畫面",
                            ))
                            .size(12.0)
                            .color(MUTED),
                        );
                        ui.add_space(10.0);
                    });
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt("nearby-device-row")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for device in devices {
                                    let selected = self
                                        .selected_device_name
                                        .as_ref()
                                        .is_some_and(|name| name == &device.name);
                                    let label = format!("◉\n{}\nLAN", device.name);
                                    let can_switch = !self.send_running || self.qr_waiting_for_scan;
                                    if ui
                                        .add_enabled(
                                            can_switch,
                                            egui::Button::new(label)
                                                .fill(if selected {
                                                    egui::Color32::from_rgb(53, 72, 104)
                                                } else {
                                                    CARD_DARK
                                                })
                                                .stroke(egui::Stroke::new(
                                                    if selected { 1.5 } else { 0.0 },
                                                    ACCENT,
                                                ))
                                                .corner_radius(20)
                                                .min_size(egui::vec2(96.0, 92.0)),
                                        )
                                        .on_hover_text(tr(
                                            self.language,
                                            "按一下直接傳送，並核對兩台裝置的 PIN",
                                        ))
                                        .clicked()
                                    {
                                        if self.qr_waiting_for_scan {
                                            self.cancel_qr_waiting();
                                        }
                                        self.begin_direct_send(device);
                                    }
                                }
                            });
                        });
                    if let (Some(name), Some(pin)) = (&self.selected_device_name, &self.send_pin) {
                        if self.nearby.values().any(|device| &device.name == name) {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("{name} · PIN {pin}"))
                                    .strong()
                                    .monospace()
                                    .color(ACCENT),
                            );
                        }
                    }
                }
            });

        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(tr(self.language, "或使用 QR code"))
                .size(18.0)
                .strong(),
        );
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(22)
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(code) = self.qr_code.clone() {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(tr(
                                self.language,
                                "用 Android 原生 Quick Share 掃描",
                            ))
                            .size(16.0)
                            .strong(),
                        );
                        ui.add_space(12.0);
                        paint_qr(ui, &code, 220.0);
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(&self.send_status).color(ACCENT));
                        if self.qr_waiting_for_scan
                            && ui
                                .add_sized(
                                    [180.0, BUTTON_HEIGHT],
                                    egui::Button::new(tr(self.language, "取消 QR／改用其他方式"))
                                        .corner_radius(23),
                                )
                                .clicked()
                        {
                            self.cancel_qr_waiting();
                        }
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(tr(self.language, "手機不必安裝 App")).strong(),
                        );
                        ui.label(
                            egui::RichText::new(tr(
                                self.language,
                                if self.auto_show_qr && self.nearby.is_empty() {
                                    "找不到區網裝置時會自動顯示 QR"
                                } else {
                                    "掃描後由 Android 原生 Quick Share 接收"
                                },
                            ))
                            .color(MUTED),
                        );
                        ui.add_space(12.0);
                        if ui
                            .add_enabled(
                                !self.send_running,
                                egui::Button::new(
                                    egui::RichText::new(tr(self.language, "顯示 QR"))
                                        .strong()
                                        .color(CARD_DARK),
                                )
                                .fill(ACCENT)
                                .corner_radius(23)
                                .min_size(egui::vec2(150.0, BUTTON_HEIGHT)),
                            )
                            .clicked()
                        {
                            self.begin_send();
                        }
                    });
                }
            });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(tr(
                self.language,
                "Galaxy 提示：若開啟 AirDrop 相容模式會中斷 Wi‑Fi，請先關閉該模式再使用 LAN／QR 傳送。",
            ))
            .size(11.0)
            .color(MUTED),
        );

        if self.send_running || self.send_failed {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(CARD_DARK)
                .corner_radius(18)
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(&self.send_status).strong());
                    if self.send_progress > 0.0 {
                        ui.add(
                            egui::ProgressBar::new(self.send_progress)
                                .show_percentage()
                                .animate(self.send_running),
                        );
                    }
                    if self.show_transfer_details && self.send_total > 0 {
                        ui.label(
                            egui::RichText::new(transfer_details(
                                self.send_bytes,
                                self.send_total,
                                self.transfer_started_at,
                            ))
                            .size(12.0)
                            .color(MUTED),
                        );
                    }
                    if let Some(pin) = &self.send_pin {
                        ui.label(
                            egui::RichText::new(format!("PIN {pin}"))
                                .size(21.0)
                                .strong()
                                .monospace()
                                .color(ACCENT),
                        );
                    }
                });
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        ui.add_enabled_ui(!self.running, |ui| {
            setting_card(ui, tr(self.language, "裝置名稱"), |ui| {
                ui.text_edit_singleline(&mut self.display_name);
            });
            ui.add_space(8.0);
            setting_card(ui, tr(self.language, "裝置分享設定"), |ui| {
                ui.label(tr(self.language, "附近的所有人都能看到分享要求"));
                ui.label(
                    egui::RichText::new(tr(
                        self.language,
                        "目前依 Quick Share 相容模式固定，不能改成聯絡人限定",
                    ))
                    .size(12.0)
                    .color(MUTED),
                );
            });
            ui.add_space(8.0);
            setting_card(ui, tr(self.language, "接收檔案儲存位置"), |ui| {
                ui.label(egui::RichText::new(&self.download_directory).color(MUTED));
                if ui
                    .add_sized(
                        [120.0, 38.0],
                        egui::Button::new(tr(self.language, "選擇資料夾")).corner_radius(19),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.download_directory = path.display().to_string();
                    }
                }
            });
        });
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "接收與提醒"), |ui| {
            ui.checkbox(
                &mut self.start_receiving_on_launch,
                tr(self.language, "開啟 HiRodrop 時自動進入接收狀態"),
            );
            ui.checkbox(
                &mut self.show_notifications,
                tr(self.language, "收到要求與完成時通知我"),
            );
        });
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "傳送選項"), |ui| {
            ui.checkbox(
                &mut self.show_transfer_details,
                tr(self.language, "顯示速度、容量與傳送進度"),
            );
            ui.checkbox(
                &mut self.auto_show_qr,
                tr(self.language, "找不到區網裝置時自動顯示 QR"),
            );
            ui.checkbox(
                &mut self.retry_failed_once,
                tr(self.language, "區網傳送失敗時自動重試一次"),
            );
        });
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "語言"), |ui| {
            ui.radio_value(
                &mut self.language,
                InterfaceLanguage::TraditionalChinese,
                "繁體中文",
            );
            ui.radio_value(&mut self.language, InterfaceLanguage::Japanese, "日本語");
            ui.radio_value(&mut self.language, InterfaceLanguage::English, "English");
        });
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "安全性"), |ui| {
            ui.label(tr(
                self.language,
                "每次連線皆使用端對端加密，並以 PIN 核對裝置。",
            ));
            ui.label(
                egui::RichText::new(tr(
                    self.language,
                    "可信裝置將以密碼學身分實作；目前不會用名稱或 IP 自動放行。",
                ))
                .color(MUTED),
            );
        });
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "Galaxy 相容性"), |ui| {
            ui.label(tr(
                self.language,
                "若啟用三星的 AirDrop 相容模式後 Wi‑Fi 會斷線，請先將它關閉。",
            ));
            ui.label(
                egui::RichText::new(tr(
                    self.language,
                    "HiRodrop 不會自行更改手機的 Wi‑Fi 或分享設定。",
                ))
                .size(12.0)
                .color(MUTED),
            );
        });
        ui.add_space(12.0);
        if ui
            .add_sized(
                [ui.available_width(), BUTTON_HEIGHT],
                egui::Button::new(
                    egui::RichText::new(tr(self.language, "儲存設定"))
                        .strong()
                        .color(CARD_DARK),
                )
                .fill(ACCENT)
                .corner_radius(23),
            )
            .clicked()
        {
            self.save_settings();
            self.status = tr(self.language, "設定已儲存").into();
        }
        self.activity_bar(ui);
        ui.add_space(8.0);
        setting_card(ui, tr(self.language, "開源軟體致謝"), |ui| {
            ui.label(tr(
                self.language,
                "HiRodrop 以 GPLv3 或更新版本開源，感謝以下專案：",
            ));
            ui.add_space(6.0);
            for (name, license) in OPEN_SOURCE_ACKNOWLEDGEMENTS {
                ui.label(
                    egui::RichText::new(format!("{name} · {license}"))
                        .size(12.0)
                        .color(MUTED),
                );
            }
        });
        ui.add_space(22.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(APP_VERSION_LABEL)
                    .size(12.0)
                    .color(MUTED),
            );
        });
        ui.add_space(12.0);
    }

    fn activity_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        egui::CollapsingHeader::new(format!(
            "{}  ({})",
            tr(self.language, "活動紀錄"),
            self.activity.len()
        ))
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(110.0)
                .show(ui, |ui| {
                    if self.activity.is_empty() {
                        ui.label(
                            egui::RichText::new(tr(self.language, "目前沒有紀錄")).color(MUTED),
                        );
                    }
                    for line in self.activity.iter().rev().take(50) {
                        ui.label(line);
                    }
                });
        });
    }

    fn mode_navigation(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let show_stop = self.page == Page::Receive && self.running;
            let nav_width = if show_stop {
                (ui.available_width() - 78.0).max(240.0)
            } else {
                ui.available_width()
            };
            ui.allocate_ui_with_layout(
                egui::vec2(nav_width, 70.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::new()
                        .fill(CARD_DARK)
                        .corner_radius(35)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            ui.set_width(nav_width - 8.0);
                            ui.columns(2, |columns| {
                                let receive_selected = self.page == Page::Receive;
                                let send_selected = self.page == Page::Send;
                                if columns[0]
                                    .add_sized(
                                        [columns[0].available_width(), 62.0],
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "↓\n{}",
                                                tr(self.language, "接收")
                                            ))
                                            .size(16.0),
                                        )
                                        .fill(if receive_selected { CARD } else { CARD_DARK })
                                        .corner_radius(31),
                                    )
                                    .clicked()
                                {
                                    self.page = Page::Receive;
                                }
                                if columns[1]
                                    .add_sized(
                                        [columns[1].available_width(), 62.0],
                                        egui::Button::new(
                                            egui::RichText::new(format!(
                                                "↑\n{}",
                                                tr(self.language, "傳送")
                                            ))
                                            .size(16.0),
                                        )
                                        .fill(if send_selected { CARD } else { CARD_DARK })
                                        .corner_radius(31),
                                    )
                                    .clicked()
                                {
                                    self.page = Page::Send;
                                }
                            });
                        });
                },
            );
            if show_stop
                && ui
                    .add_sized(
                        [68.0, 68.0],
                        egui::Button::new(egui::RichText::new("×").size(27.0))
                            .fill(CARD)
                            .corner_radius(34),
                    )
                    .on_hover_text(tr(self.language, "停止接收"))
                    .clicked()
            {
                self.stop();
            }
        });
    }

    fn show_toast(&mut self, context: &egui::Context) {
        let Some((title, body, created)) = self.toast.clone() else {
            return;
        };
        if created.elapsed() >= Duration::from_secs(7) {
            self.toast = None;
            return;
        }
        egui::Area::new("hirodrop-toast".into())
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(37, 49, 68))
                    .stroke(egui::Stroke::new(1.0, ACCENT))
                    .corner_radius(18)
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(120),
                    })
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(egui::RichText::new(title).strong());
                        ui.label(egui::RichText::new(body).color(MUTED));
                    });
            });
    }
}

impl eframe::App for HirodropApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.auto_start_pending {
            self.auto_start_pending = false;
            self.start();
        }
        self.poll_events(ui.ctx());
        #[cfg(target_os = "macos")]
        self.poll_finder_files(ui.ctx());
        self.poll_nearby();
        self.maybe_auto_start_qr();
        self.poll_tray(ui.ctx());
        if ui.ctx().input(|input| input.viewport().close_requested()) && !self.quitting {
            self.prepare_to_quit();
        }
        let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
        if !dropped.is_empty() {
            self.page = Page::Send;
            self.add_files(dropped.into_iter().map(|file| file.path().to_owned()));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ui, |ui| {
                self.top_bar(ui);
                match self.page {
                    Page::Settings => {
                        egui::ScrollArea::vertical().show(ui, |ui| self.settings_page(ui));
                    }
                    Page::Receive | Page::Send => {
                        self.mode_navigation(ui);
                        let content_height = ui.available_height().max(160.0);
                        egui::ScrollArea::vertical()
                            .max_height(content_height)
                            .show(ui, |ui| match self.page {
                                Page::Receive => self.receive_page(ui),
                                Page::Send => self.send_page(ui),
                                Page::Settings => unreachable!(),
                            });
                    }
                }
            });
        self.show_toast(ui.ctx());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.prepare_to_quit();
    }
}

#[cfg(target_os = "macos")]
struct AppTray {
    _icon: TrayIcon,
    show_id: MenuId,
    send_id: MenuId,
    quit_id: MenuId,
}

#[cfg(target_os = "macos")]
impl AppTray {
    fn new(icon_data: &egui::IconData, language: InterfaceLanguage) -> Option<Self> {
        let menu = Menu::new();
        let show = MenuItem::new(tr(language, "顯示 HiRodrop"), true, None);
        let send = MenuItem::new(tr(language, "傳送檔案…"), true, None);
        let quit = MenuItem::new(tr(language, "結束 HiRodrop"), true, None);
        menu.append_items(&[&show, &send, &quit]).ok()?;
        let icon =
            Icon::from_rgba(icon_data.rgba.clone(), icon_data.width, icon_data.height).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tr(language, "HiRodrop · Quick Share 接收中"))
            .with_icon(icon)
            .build()
            .ok()?;
        Some(Self {
            _icon: tray,
            show_id: show.id().clone(),
            send_id: send.id().clone(),
            quit_id: quit.id().clone(),
        })
    }
}

fn setting_card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(18)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(15.0).strong());
            ui.add_space(5.0);
            body(ui);
        });
}

fn paint_avatar(ui: &mut egui::Ui, size: f32, name: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let hue = name
        .bytes()
        .fold(0_u8, |value, byte| value.wrapping_add(byte));
    let fill = egui::Color32::from_rgb(
        105_u8.saturating_add(hue % 45),
        128_u8.saturating_add(hue % 55),
        205_u8.saturating_add(hue % 40),
    );
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    ui.painter().circle_stroke(
        rect.center(),
        size / 2.0 - 1.0,
        egui::Stroke::new(2.0, ACCENT),
    );
    let initial = name.chars().next().unwrap_or('H').to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(size * 0.43),
        egui::Color32::WHITE,
    );
}

fn paint_send_icon(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, ACCENT);
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(27, 49, 84));
    let c = rect.center();
    ui.painter().line_segment(
        [c + egui::vec2(0.0, 17.0), c + egui::vec2(0.0, -14.0)],
        stroke,
    );
    ui.painter().line_segment(
        [c + egui::vec2(-11.0, -3.0), c + egui::vec2(0.0, -14.0)],
        stroke,
    );
    ui.painter().line_segment(
        [c + egui::vec2(11.0, -3.0), c + egui::vec2(0.0, -14.0)],
        stroke,
    );
    ui.painter().line_segment(
        [c + egui::vec2(-16.0, 22.0), c + egui::vec2(16.0, 22.0)],
        stroke,
    );
}

fn paint_receive_icon(ui: &mut egui::Ui, size: f32, active: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter();
    let fill = if active {
        ACCENT
    } else {
        egui::Color32::from_rgb(96, 99, 105)
    };
    painter.circle_filled(rect.center(), size / 2.0, fill);
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(27, 49, 84));
    let c = rect.center();
    painter.line_segment(
        [c + egui::vec2(0.0, -18.0), c + egui::vec2(0.0, 12.0)],
        stroke,
    );
    painter.line_segment(
        [c + egui::vec2(-10.0, 2.0), c + egui::vec2(0.0, 12.0)],
        stroke,
    );
    painter.line_segment(
        [c + egui::vec2(10.0, 2.0), c + egui::vec2(0.0, 12.0)],
        stroke,
    );
    painter.line_segment(
        [c + egui::vec2(-15.0, 20.0), c + egui::vec2(15.0, 20.0)],
        stroke,
    );
}

fn paint_qr(ui: &mut egui::Ui, code: &QrCode, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let quiet = 4_usize;
    let width = code.width() + quiet * 2;
    let module = size / width as f32;
    ui.painter().rect_filled(rect, 14.0, egui::Color32::WHITE);
    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == Color::Dark {
                let min =
                    rect.min + egui::vec2((x + quiet) as f32 * module, (y + quiet) as f32 * module);
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(module + 0.2, module + 0.2)),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn total_selected_bytes(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|path| selected_path_bytes(path)).sum()
}

fn selected_path_bytes(path: &std::path::Path) -> u64 {
    let Ok(metadata) = path.symlink_metadata() else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| selected_path_bytes(&entry.path()))
        .sum()
}

fn selected_kind(paths: &[PathBuf], language: InterfaceLanguage) -> String {
    let count = paths.len();
    let noun = if paths.iter().any(|path| path.is_dir()) {
        match language {
            InterfaceLanguage::TraditionalChinese => "項目",
            InterfaceLanguage::Japanese => "項目",
            InterfaceLanguage::English => "item",
        }
    } else if paths.iter().all(|path| is_image_name(path)) {
        match language {
            InterfaceLanguage::TraditionalChinese => "圖片",
            InterfaceLanguage::Japanese => "画像",
            InterfaceLanguage::English => "image",
        }
    } else if paths.iter().all(|path| is_video_name(path)) {
        match language {
            InterfaceLanguage::TraditionalChinese => "影片",
            InterfaceLanguage::Japanese => "動画",
            InterfaceLanguage::English => "video",
        }
    } else {
        match language {
            InterfaceLanguage::TraditionalChinese => "檔案",
            InterfaceLanguage::Japanese => "ファイル",
            InterfaceLanguage::English => "file",
        }
    };
    match language {
        InterfaceLanguage::TraditionalChinese => format!("{count} 個{noun}"),
        InterfaceLanguage::Japanese => format!("{noun} {count} 件"),
        InterfaceLanguage::English => {
            format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
        }
    }
}

fn offer_summary(items: &[TransferOfferItem], language: InterfaceLanguage) -> String {
    let total = items.iter().map(|item| item.size).sum();
    let noun = if items.iter().all(|item| is_image_text(&item.name)) {
        match language {
            InterfaceLanguage::TraditionalChinese => "圖片",
            InterfaceLanguage::Japanese => "画像",
            InterfaceLanguage::English => "image",
        }
    } else if items.iter().all(|item| is_video_text(&item.name)) {
        match language {
            InterfaceLanguage::TraditionalChinese => "影片",
            InterfaceLanguage::Japanese => "動画",
            InterfaceLanguage::English => "video",
        }
    } else if items.iter().all(|item| item.is_text) {
        match language {
            InterfaceLanguage::TraditionalChinese => "文字",
            InterfaceLanguage::Japanese => "テキスト",
            InterfaceLanguage::English => "text item",
        }
    } else {
        match language {
            InterfaceLanguage::TraditionalChinese => "檔案",
            InterfaceLanguage::Japanese => "ファイル",
            InterfaceLanguage::English => "file",
        }
    };
    match language {
        InterfaceLanguage::TraditionalChinese => {
            format!("{} 個{} · {}", items.len(), noun, human_bytes(total))
        }
        InterfaceLanguage::Japanese => {
            format!("{noun} {} 件 · {}", items.len(), human_bytes(total))
        }
        InterfaceLanguage::English => format!(
            "{} {noun}{} · {}",
            items.len(),
            if items.len() == 1 { "" } else { "s" },
            human_bytes(total)
        ),
    }
}

fn peer_label(language: InterfaceLanguage, peer_name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("來自：{peer_name}"),
        InterfaceLanguage::Japanese => format!("送信元：{peer_name}"),
        InterfaceLanguage::English => format!("From: {peer_name}"),
    }
}

fn more_items_label(language: InterfaceLanguage, count: usize) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("另有 {count} 個項目"),
        InterfaceLanguage::Japanese => format!("ほか {count} 件"),
        InterfaceLanguage::English => {
            format!("{count} more item{}", if count == 1 { "" } else { "s" })
        }
    }
}

fn selected_status(language: InterfaceLanguage, count: usize) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("已選取 {count} 個項目"),
        InterfaceLanguage::Japanese => format!("{count} 件の項目を選択しました"),
        InterfaceLanguage::English => {
            format!("Selected {count} item{}", if count == 1 { "" } else { "s" })
        }
    }
}

fn connecting_status(language: InterfaceLanguage, name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("正在連接 {name}…"),
        InterfaceLanguage::Japanese => format!("{name} に接続中…"),
        InterfaceLanguage::English => format!("Connecting to {name}…"),
    }
}

fn found_status(language: InterfaceLanguage, name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("已找到 {name}"),
        InterfaceLanguage::Japanese => format!("{name} が見つかりました"),
        InterfaceLanguage::English => format!("Found {name}"),
    }
}

fn negotiating_status(language: InterfaceLanguage, phase: &str) -> String {
    let phase = match (language, phase) {
        (InterfaceLanguage::Japanese, "連線確認") => "接続確認",
        (InterfaceLanguage::Japanese, "配對金鑰") => "ペアリングキー",
        (InterfaceLanguage::Japanese, "檔案資訊") => "ファイル情報",
        (InterfaceLanguage::Japanese, "第一次連線失敗，正在自動重試") => {
            "初回接続に失敗。自動再試行中"
        }
        (InterfaceLanguage::English, "連線確認") => "connection confirmation",
        (InterfaceLanguage::English, "配對金鑰") => "pairing key",
        (InterfaceLanguage::English, "檔案資訊") => "file information",
        (InterfaceLanguage::English, "第一次連線失敗，正在自動重試") => {
            "first connection failed; retrying automatically"
        }
        _ => phase,
    };
    match language {
        InterfaceLanguage::TraditionalChinese => format!("正在協商：{phase}…"),
        InterfaceLanguage::Japanese => format!("ネゴシエーション中：{phase}…"),
        InterfaceLanguage::English => format!("Negotiating: {phase}…"),
    }
}

fn pin_status(language: InterfaceLanguage, pin: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("驗證碼 {pin} · 請在接收裝置核對"),
        InterfaceLanguage::Japanese => format!("PIN {pin} · 受信端末で確認してください"),
        InterfaceLanguage::English => format!("PIN {pin} · verify it on the receiving device"),
    }
}

fn sending_status(language: InterfaceLanguage, file_name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("正在傳送 {file_name}"),
        InterfaceLanguage::Japanese => format!("{file_name} を送信中"),
        InterfaceLanguage::English => format!("Sending {file_name}"),
    }
}

fn failed_status(language: InterfaceLanguage, message: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("傳送失敗：{message}"),
        InterfaceLanguage::Japanese => format!("送信に失敗しました：{message}"),
        InterfaceLanguage::English => format!("Transfer failed: {message}"),
    }
}

fn send_error_message(language: InterfaceLanguage, error: &QuickShareSendError) -> String {
    match (language, error) {
        (InterfaceLanguage::TraditionalChinese, QuickShareSendError::Rejected) => {
            "接收端已拒絕這次傳送。".into()
        }
        (InterfaceLanguage::Japanese, QuickShareSendError::Rejected) => {
            "受信側が今回の送信を拒否しました。".into()
        }
        (InterfaceLanguage::English, QuickShareSendError::Rejected) => {
            "The receiver declined this transfer.".into()
        }
        (
            InterfaceLanguage::TraditionalChinese,
            QuickShareSendError::Qr(QuickShareQrError::TimedOut),
        ) => "等待掃描已逾時，請重新顯示 QR code。".into(),
        (InterfaceLanguage::Japanese, QuickShareSendError::Qr(QuickShareQrError::TimedOut)) => {
            "スキャン待機がタイムアウトしました。QR コードを表示し直してください。".into()
        }
        (InterfaceLanguage::English, QuickShareSendError::Qr(QuickShareQrError::TimedOut)) => {
            "The scan timed out. Show a new QR code and try again.".into()
        }
        (language, QuickShareSendError::Protocol(message)) if message.contains("disconnected") => {
            match language {
                InterfaceLanguage::TraditionalChinese => {
                    "接收端已中斷連線。請保持 Quick Share 畫面開啟後重試。".into()
                }
                InterfaceLanguage::Japanese => {
                    "受信側との接続が切れました。Quick Share 画面を開いたまま再試行してください。"
                        .into()
                }
                InterfaceLanguage::English => {
                    "The receiver disconnected. Keep its Quick Share screen open and try again."
                        .into()
                }
            }
        }
        (InterfaceLanguage::TraditionalChinese, QuickShareSendError::TransferTooLarge) => {
            "選取內容超過 100 GiB 安全上限。".into()
        }
        (InterfaceLanguage::Japanese, QuickShareSendError::TransferTooLarge) => {
            "選択内容が 100 GiB の上限を超えています。".into()
        }
        (InterfaceLanguage::English, QuickShareSendError::TransferTooLarge) => {
            "The selection exceeds the 100 GiB safety limit.".into()
        }
        _ => error.to_string(),
    }
}

fn incoming_notification(
    language: InterfaceLanguage,
    peer_name: &str,
    item_count: usize,
    pin: &str,
) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => {
            format!("{peer_name} 想傳送 {item_count} 個項目，核對碼 {pin}")
        }
        InterfaceLanguage::Japanese => {
            format!("{peer_name} が {item_count} 件を共有しようとしています。PIN {pin}")
        }
        InterfaceLanguage::English => format!(
            "{peer_name} wants to share {item_count} item{}. PIN {pin}",
            if item_count == 1 { "" } else { "s" }
        ),
    }
}

fn settings_save_failed(language: InterfaceLanguage, message: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("設定儲存失敗：{message}"),
        InterfaceLanguage::Japanese => format!("設定を保存できませんでした：{message}"),
        InterfaceLanguage::English => format!("Could not save settings: {message}"),
    }
}

fn qr_error_status(language: InterfaceLanguage, message: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("無法產生 QR：{message}"),
        InterfaceLanguage::Japanese => format!("QR を生成できません：{message}"),
        InterfaceLanguage::English => format!("Could not generate QR: {message}"),
    }
}

fn listening_activity(language: InterfaceLanguage, port: u16) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("接收服務已待命 · LAN TCP {port}"),
        InterfaceLanguage::Japanese => format!("受信サービス待機中 · LAN TCP {port}"),
        InterfaceLanguage::English => format!("Receiving service ready · LAN TCP {port}"),
    }
}

fn concurrent_reject_activity(language: InterfaceLanguage, peer_name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => {
            format!("已拒絕來自 {peer_name} 的同時傳送要求：目前正等待另一筆確認")
        }
        InterfaceLanguage::Japanese => {
            format!("{peer_name} からの同時リクエストを拒否：別の確認を待機中です")
        }
        InterfaceLanguage::English => {
            format!(
                "Declined a simultaneous request from {peer_name}: another confirmation is pending"
            )
        }
    }
}

fn incoming_status(language: InterfaceLanguage, peer_name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("{peer_name} 想傳送檔案"),
        InterfaceLanguage::Japanese => format!("{peer_name} がファイルを共有しようとしています"),
        InterfaceLanguage::English => format!("{peer_name} wants to share files"),
    }
}

fn receiving_status(language: InterfaceLanguage, file_name: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("正在接收 {file_name}"),
        InterfaceLanguage::Japanese => format!("{file_name} を受信中"),
        InterfaceLanguage::English => format!("Receiving {file_name}"),
    }
}

fn connection_failed_activity(language: InterfaceLanguage, message: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("連線失敗：{message}"),
        InterfaceLanguage::Japanese => format!("接続に失敗しました：{message}"),
        InterfaceLanguage::English => format!("Connection failed: {message}"),
    }
}

fn sent_activity(language: InterfaceLanguage, count: usize) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("已傳送 {count} 個檔案"),
        InterfaceLanguage::Japanese => format!("{count} 件のファイルを送信しました"),
        InterfaceLanguage::English => {
            format!("Sent {count} file{}", if count == 1 { "" } else { "s" })
        }
    }
}

fn received_activity(language: InterfaceLanguage, peer_name: &str, paths: &str) -> String {
    match language {
        InterfaceLanguage::TraditionalChinese => format!("來自 {peer_name}：{paths}"),
        InterfaceLanguage::Japanese => format!("{peer_name} から受信：{paths}"),
        InterfaceLanguage::English => format!("Received from {peer_name}: {paths}"),
    }
}

fn is_image_name(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_image_text)
}

fn is_video_name(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_video_text)
}

fn is_image_text(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "bmp" | "tiff"
    )
}

fn is_video_text(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "3gp"
    )
}

fn transfer_details(bytes: u64, total: u64, started_at: Option<Instant>) -> String {
    let elapsed = started_at.map_or(0.0, |start| start.elapsed().as_secs_f64());
    let speed = if elapsed > 0.2 {
        format!(" · {}/s", human_bytes((bytes as f64 / elapsed) as u64))
    } else {
        String::new()
    };
    format!("{} / {}{speed}", human_bytes(bytes), human_bytes(total))
}

fn app_icon() -> egui::IconData {
    const SIZE: u32 = 128;
    let source = image::load_from_memory(include_bytes!("../../assets/hirodrop-icon.png"))
        .expect("embedded HiRodrop icon must be a valid image");
    let crop_size = source.width().min(source.height());
    let cropped = source.crop_imm(
        (source.width() - crop_size) / 2,
        (source.height() - crop_size) / 2,
        crop_size,
        crop_size,
    );
    let mut rgba = cropped
        .resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    apply_rounded_corners(&mut rgba);
    egui::IconData {
        rgba: rgba.into_raw(),
        width: SIZE,
        height: SIZE,
    }
}

fn apply_rounded_corners(image: &mut image::RgbaImage) {
    let size = image.width().min(image.height()) as f32;
    let half = size / 2.0;
    let radius = size * 0.225;
    let straight_half = half - radius;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let dx = (x as f32 + 0.5 - half).abs() - straight_half;
        let dy = (y as f32 + 0.5 - half).abs() - straight_half;
        let outside = dx.max(0.0).hypot(dy.max(0.0));
        let inside = dx.max(dy).min(0.0);
        let distance = outside + inside - radius;
        let coverage = (0.5 - distance).clamp(0.0, 1.0);
        pixel.0[3] = (f32::from(pixel.0[3]) * coverage).round() as u8;
    }
}

fn receive_service(
    display_name: String,
    download_directory: PathBuf,
    stop: Arc<AtomicBool>,
    events: Sender<ServiceEvent>,
) {
    let mut config = QuickShareReceiverConfig::new(display_name.clone(), download_directory);
    config.consent_mode = ConsentMode::Prompt;
    let receiver = match QuickShareReceiver::bind(config) {
        Ok(receiver) => receiver,
        Err(error) => {
            let _ = events.send(ServiceEvent::Error(error.to_string()));
            let _ = events.send(ServiceEvent::Stopped);
            return;
        }
    };
    let port = match receiver.port() {
        Ok(port) => port,
        Err(error) => {
            let _ = events.send(ServiceEvent::Error(error.to_string()));
            let _ = events.send(ServiceEvent::Stopped);
            return;
        }
    };
    let advertisement = match QuickShareAdvertisement::start(&display_name, port) {
        Ok(advertisement) => advertisement,
        Err(error) => {
            let _ = events.send(ServiceEvent::Error(error.to_string()));
            let _ = events.send(ServiceEvent::Stopped);
            return;
        }
    };
    let _ = events.send(ServiceEvent::Listening(port));

    // Official Quick Share clients open short-lived probe connections while
    // discovering a target. Processing one socket synchronously lets a probe
    // block the real transfer for up to the read timeout, which explains why
    // Android sometimes only succeeded after several taps. Keep accepting new
    // sockets and give each one a bounded worker instead.
    let mut workers = Vec::<thread::JoinHandle<()>>::new();
    const MAX_CONNECTION_WORKERS: usize = 8;
    while !stop.load(Ordering::Relaxed) {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let worker = workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
        if workers.len() >= MAX_CONNECTION_WORKERS {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        match receiver.try_accept() {
            Ok(Some(connection)) => {
                let worker_events = events.clone();
                workers.push(thread::spawn(move || {
                    let offer_events = worker_events.clone();
                    let progress_events = worker_events.clone();
                    let mut pending_reply: Option<mpsc::Receiver<bool>> = None;
                    let result = connection.receive_with_consent_and_progress(
                        |offer| {
                            let reply_rx = pending_reply.get_or_insert_with(|| {
                                let (reply_tx, reply_rx) = mpsc::channel();
                                let _ =
                                    offer_events.send(ServiceEvent::Offer(offer.clone(), reply_tx));
                                reply_rx
                            });
                            match reply_rx.try_recv() {
                                Ok(decision) => Some(decision),
                                Err(mpsc::TryRecvError::Disconnected) => Some(false),
                                Err(mpsc::TryRecvError::Empty) => None,
                            }
                        },
                        |progress| match progress {
                            QuickShareReceiveProgress::Receiving {
                                received_bytes,
                                total_bytes,
                                file_name,
                            } => {
                                let _ = progress_events.send(ServiceEvent::Receiving {
                                    received_bytes,
                                    total_bytes,
                                    file_name,
                                });
                            }
                        },
                    );
                    match result {
                        Ok(summary) => {
                            let paths = summary
                                .saved_paths
                                .iter()
                                .map(|path| path.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            let _ = worker_events.send(ServiceEvent::Complete {
                                peer_name: summary.peer.name,
                                paths,
                            });
                        }
                        Err(QuickShareReceiveError::Rejected) => {}
                        Err(error) if error.is_benign_probe_disconnect() => {}
                        Err(error) => {
                            eprintln!("HiRodrop receiver failed: {error}");
                            let _ = worker_events.send(ServiceEvent::Error(error.to_string()));
                        }
                    }
                }));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                eprintln!("HiRodrop receiver listener failed: {error}");
                let _ = events.send(ServiceEvent::Error(error.to_string()));
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    let _ = advertisement.stop(Duration::from_secs(3));
    let _ = events.send(ServiceEvent::Stopped);
}
