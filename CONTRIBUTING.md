# 參與 HiRodrop 開發

感謝你協助改善 HiRodrop。

## 提交修改前

- 建立新 Issue 前，請先搜尋是否已有相同問題。
- 修改範圍請集中於 macOS App，以及與官方 Android 或 Windows Quick Share 裝置的相容性。
- 請勿加入雲端中繼、強制帳號或強制子網路掃描行為。
- 絕對不要提交裝置名稱、本機路徑、封包擷取檔、帳號識別碼、簽章檔案，或含有個人資料的測試文件。

若要修改大範圍協定或使用者介面，請先建立 Issue，以便討論設計與互通風險。

## 建置與測試

HiRodrop 需要 macOS 12 或更新版本、Xcode Command Line Tools 與 Rust。

```zsh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
zsh packaging/macos/build-app.sh
```

涉及實體裝置協定的修改，請註明測試過的 Android 或官方 Windows Quick Share 傳送方向。請勿將私人封包擷取檔附加到公開 Issue。

## Pull Request

請說明具體問題、修改後的行為及測試方式。提交貢獻即表示你同意以 GPL-3.0-or-later 授權該項貢獻。
