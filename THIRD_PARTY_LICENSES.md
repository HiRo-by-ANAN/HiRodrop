# 第三方軟體

HiRodrop 使用多項開源 Rust crate。每個相依套件仍適用其原有授權；實際解析的版本與間接相依套件記錄於 `Cargo.lock`。

| 專案 | 授權 |
| --- | --- |
| eframe / egui | MIT OR Apache-2.0 |
| egui 預設字型 | OFL-1.1 AND Ubuntu-font-1.0 |
| RustCrypto：aes、aes-gcm、cbc、hkdf、hmac、sha2、p256 | MIT OR Apache-2.0 |
| base64 | MIT OR Apache-2.0 |
| image | MIT OR Apache-2.0 |
| mdns-sd | MIT OR Apache-2.0 |
| notify-rust | MIT OR Apache-2.0 |
| prost | Apache-2.0 |
| qrcode | MIT OR Apache-2.0 |
| rand | MIT OR Apache-2.0 |
| rfd | MIT |
| serde / serde_json | MIT OR Apache-2.0 |
| socket2 | MIT OR Apache-2.0 |
| thiserror | MIT OR Apache-2.0 |
| tray-icon | MIT OR Apache-2.0 |
| cc（建置相依套件） | MIT OR Apache-2.0 |

協定參考資料及其上游授權列於 `docs/protocol-sources.md`。這份說明不會取代或修改任何上游授權條款。發行版 App 內也會包含自動產生的 `THIRD_PARTY_NOTICES.txt`，收錄 macOS 建置相依樹中每個 crate 隨附的原始授權文字。
