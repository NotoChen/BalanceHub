use std::ptr;
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_DWORD,
        REG_EXPAND_SZ, REG_SZ,
    },
};

/// Read-only, bounded Unicode registry access shared by browser and proxy discovery.
pub(crate) struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

impl RegistryKey {
    pub(crate) fn open(root: HKEY, path: &str, view: u32) -> Option<Self> {
        let path = path.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut key = ptr::null_mut();
        (unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | view, &mut key) }
            == ERROR_SUCCESS)
            .then(|| Self(key))
    }

    fn value(&self, name: &str) -> Option<(u32, Vec<u8>)> {
        let name = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut kind = 0;
        let mut size = 0;
        if unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        } != ERROR_SUCCESS
            || !(2..=32 * 1024).contains(&size)
        {
            return None;
        }
        let mut bytes = vec![0; size as usize];
        if unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        } != ERROR_SUCCESS
        {
            return None;
        }
        bytes.truncate(size as usize);
        Some((kind, bytes))
    }

    pub(crate) fn string(&self, name: &str) -> Option<String> {
        let (kind, bytes) = self.value(name)?;
        if !matches!(kind, REG_SZ | REG_EXPAND_SZ) || bytes.len() % 2 != 0 {
            return None;
        }
        let wide = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|value| *value != 0)
            .collect::<Vec<_>>();
        String::from_utf16(&wide).ok()
    }

    pub(crate) fn dword(&self, name: &str) -> Option<u32> {
        let (kind, bytes) = self.value(name)?;
        (kind == REG_DWORD).then_some(())?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    pub(crate) fn children(&self) -> Vec<String> {
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
