use super::{browser_name, registry_executable};
use crate::platform::windows_registry::RegistryKey;
use std::path::PathBuf;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};

pub(super) fn candidates() -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            for executable in [
                "chrome.exe",
                "msedge.exe",
                "brave.exe",
                "chromium.exe",
                "vivaldi.exe",
                "opera.exe",
            ] {
                let key =
                    format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{executable}");
                if let Some(path) = RegistryKey::open(root, &key, view)
                    .and_then(|key| key.string(""))
                    .and_then(|value| registry_executable(&value))
                {
                    if let Some(name) = browser_name(&path) {
                        result.push((name, path));
                    }
                }
            }
            let clients = r"SOFTWARE\Clients\StartMenuInternet";
            if let Some(key) = RegistryKey::open(root, clients, view) {
                for child in key.children() {
                    let command = format!(r"{clients}\{child}\shell\open\command");
                    if let Some(path) = RegistryKey::open(root, &command, view)
                        .and_then(|key| key.string(""))
                        .and_then(|value| registry_executable(&value))
                    {
                        if let Some(name) = browser_name(&path) {
                            result.push((name, path));
                        }
                    }
                }
            }
        }
    }
    result
}
