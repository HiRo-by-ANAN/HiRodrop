# Quick Share 相容協定參考來源

HiRodrop 的相容層是獨立開發的 Rust 實作。線路資料模型使用 Google／Chromium 公開協定定義中的欄位編號與列舉值；互通行為則依據公開協定筆記，以及使用者自己的 Android 裝置所產生的封包進行核對。

主要參考資料：

- Chromium Quick Share `wire_format.proto`（Chromium BSD 類型授權）：
  <https://chromium.googlesource.com/chromium/src/+/HEAD/chrome/services/sharing/public/proto/wire_format.proto>
- Google SecureMessage 專案（Apache-2.0）：
  <https://github.com/google/securemessage>
- Google Nearby Connections 概覽：
  <https://developers.google.com/nearby/overview>
- NearDrop 協定筆記與整理後的 protobuf schema（Unlicense；部分 schema 帶有 Google Apache-2.0 標頭）：
  <https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md>

本專案沒有複製或連結 GPL-3.0 `rquickshare` 的實作。相容行為與公開線路格式皆直接在此儲存庫中實作。HiRodrop 本身採用 GPL-3.0-or-later 授權。
