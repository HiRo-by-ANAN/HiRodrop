//! Small settings file shared by the GUI and future service.

use crate::quick_share_receiver::{default_device_name, default_download_directory};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HirodropSettings {
    pub display_name: String,
    pub download_directory: PathBuf,
    #[serde(default = "default_true")]
    pub start_receiving_on_launch: bool,
    #[serde(default = "default_true")]
    pub show_notifications: bool,
    #[serde(default = "default_true")]
    pub show_transfer_details: bool,
    #[serde(default = "default_true")]
    pub auto_show_qr: bool,
    #[serde(default = "default_true")]
    pub retry_failed_once: bool,
    #[serde(default)]
    pub language: InterfaceLanguage,
    #[serde(default)]
    pub trusted_fingerprints: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum InterfaceLanguage {
    #[default]
    TraditionalChinese,
    Japanese,
    English,
}

const fn default_true() -> bool {
    true
}

impl Default for HirodropSettings {
    fn default() -> Self {
        Self {
            display_name: default_device_name(),
            download_directory: default_download_directory(),
            start_receiving_on_launch: true,
            show_notifications: true,
            show_transfer_details: true,
            auto_show_qr: true,
            retry_failed_once: true,
            language: InterfaceLanguage::TraditionalChinese,
            trusted_fingerprints: Vec::new(),
        }
    }
}

impl HirodropSettings {
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<PathBuf> {
        let path = settings_path()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user settings directory"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(&path, json)?;
        Ok(path)
    }
}

pub fn settings_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library/Application Support/HiRodrop/settings.json"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".config"))
            })
            .map(|path| path.join("hirodrop/settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_json_preserves_unicode_name_and_path() {
        let settings = HirodropSettings {
            display_name: "辦公室電腦".into(),
            download_directory: PathBuf::from("下載/HiRodrop"),
            start_receiving_on_launch: false,
            show_notifications: true,
            show_transfer_details: false,
            auto_show_qr: false,
            retry_failed_once: false,
            language: InterfaceLanguage::Japanese,
            trusted_fingerprints: vec!["fake_fingerprint".into()],
        };
        let json = serde_json::to_vec(&settings).unwrap();
        let restored: HirodropSettings = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.display_name, settings.display_name);
        assert_eq!(restored.download_directory, settings.download_directory);
        assert!(!restored.start_receiving_on_launch);
        assert!(restored.show_notifications);
        assert!(!restored.show_transfer_details);
        assert!(!restored.auto_show_qr);
        assert!(!restored.retry_failed_once);
        assert_eq!(restored.language, InterfaceLanguage::Japanese);
    }

    #[test]
    fn old_settings_gain_safe_defaults() {
        let restored: HirodropSettings =
            serde_json::from_str(r#"{"display_name":"舊設定","download_directory":"Downloads"}"#)
                .unwrap();
        assert!(restored.start_receiving_on_launch);
        assert!(restored.show_notifications);
        assert!(restored.show_transfer_details);
        assert!(restored.auto_show_qr);
        assert!(restored.retry_failed_once);
        assert_eq!(restored.language, InterfaceLanguage::TraditionalChinese);
    }
}
