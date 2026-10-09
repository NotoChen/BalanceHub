use super::{browser_name, registry_executable};
use std::{path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SZ,
    },
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
impl Key {
    fn open(root: HKEY, path: &str, view: u32) -> Option<Self> {
        let path = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut key = ptr::null_mut();
        // Read only, explicitly checking both registry views and both install scopes.
        (unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | view, &mut key) }
            == ERROR_SUCCESS)
            .then(|| Self(key))
    }
    fn value(&self) -> Option<String> {
        let mut kind = 0;
        let mut size = 0;
        if unsafe {
            RegQueryValueExW(
                self.0,
                ptr::null(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        } != ERROR_SUCCESS
            || !matches!(kind, REG_SZ | REG_EXPAND_SZ)
            || !(2..=32 * 1024).contains(&size)
        {
            return None;
        }
        let mut bytes = vec![0u8; size as usize];
        if unsafe {
            RegQueryValueExW(
                self.0,
                ptr::null(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        } != ERROR_SUCCESS
        {
            return None;
        }
        let wide = bytes[..size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|value| *value != 0)
            .collect::<Vec<_>>();
        String::from_utf16(&wide).ok()
    }
    fn children(&self) -> Vec<String> {
        let mut children = Vec::new();
        for index in 0..128 {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            if unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            } != ERROR_SUCCESS
            {
                break;
            }
            if let Ok(name) = String::from_utf16(&name[..length as usize]) {
                children.push(name);
            }
        }
        children
    }
}

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
                if let Some(path) = Key::open(root, &key, view)
                    .and_then(|key| key.value())
                    .and_then(|value| registry_executable(&value))
                {
                    if let Some(name) = browser_name(&path) {
                        result.push((name, path));
                    }
                }
            }
            let clients = r"SOFTWARE\Clients\StartMenuInternet";
            if let Some(key) = Key::open(root, clients, view) {
                for child in key.children() {
                    let command = format!(r"{clients}\{child}\shell\open\command");
                    if let Some(path) = Key::open(root, &command, view)
                        .and_then(|key| key.value())
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
