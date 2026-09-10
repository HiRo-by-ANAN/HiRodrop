# 第一次將 HiRodrop 發布至 GitHub

最安全的首次公開方式，是透過 GitHub 網站上傳乾淨的匯出資料夾。乾淨匯出只包含已經檢查並由 Git 追蹤的檔案，不包含 `.git`、`dist`、`target`、`.DS_Store` 或尚未提交的測試檔案。

## 1. 保護提交紀錄顯示的電子郵件

在 GitHub 打開「Settings → Emails」，啟用「Keep my email addresses private」。如果頁面提供阻擋命令列推送洩漏個人信箱的選項，也請一併開啟。之後透過 GitHub 網站建立的提交，會依照 GitHub 帳號的隱私設定顯示身份。

請勿在上傳檔案中放入 Apple ID、Developer Team ID、憑證名稱、個人電子郵件或驗證權杖。

## 2. 建立空白 GitHub 儲存庫

在 GitHub 選擇「New repository」。可自行選擇公開或私人，但不要勾選自動加入 README、`.gitignore` 或 License，因為本專案已經包含這些檔案。

## 3. 建立乾淨上傳資料夾

```zsh
mkdir -p /tmp/HiRodrop-GitHub-Upload
git archive HEAD | tar -x -C /tmp/HiRodrop-GitHub-Upload
```

這只會匯出目前已提交的原始碼。上傳前請再次檢查該資料夾。建立匯出資料夾不會連線至 GitHub，也不會傳送任何資料。

## 4. 使用瀏覽器上傳

打開空白儲存庫，選擇「Add file → Upload files」。在 Finder 打開乾淨匯出資料夾；若看不到 `.github`，請按 `Command-Shift-.` 顯示隱藏檔。選取資料夾內的全部內容，再拖入 GitHub 上傳頁面。

根目錄的 `README.md` 必須直接位於儲存庫根目錄，不能多包在一層外部資料夾裡。

檢查 GitHub 列出的完整檔案清單，輸入例如 `Initial open source release` 的提交訊息，再確認提交。使用這種瀏覽器流程不會替 Mac 設定 Git remote，因此之後也不會在本機意外執行 `git push`。

## 5. 將 DMG 發布為 Release 附件

建立發行檔：

```zsh
zsh packaging/macos/release-macos.sh
```

在 GitHub 打開「Releases → Draft a new release」，建立版本標籤，並上傳 `dist/HiRodrop-macOS-universal.dmg`。`dist` 目錄刻意受到 Git 忽略；DMG 應放在 GitHub Release，不應提交到原始碼紀錄。
