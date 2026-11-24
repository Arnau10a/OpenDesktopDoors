use crate::messages::to_wide_null;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "OpenDesktopDoors";

pub fn is_startup_enabled() -> bool {
    let subkey = to_wide_null(RUN_KEY);
    let val_name = to_wide_null(VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let res = RegQueryValueExW(hkey, PCWSTR(val_name.as_ptr()), None, None, None, None);
        let _ = RegCloseKey(hkey);
        res.is_ok()
    }
}

pub fn set_startup_enabled(enable: bool) -> Result<(), String> {
    let subkey = to_wide_null(RUN_KEY);
    let val_name = to_wide_null(VALUE_NAME);
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_WRITE,
            &mut hkey,
        )
        .ok()
        .map_err(|e| e.to_string())?;

        let result = if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let quoted = format!("\"{}\"", exe.to_string_lossy());
            let wide = to_wide_null(&quoted);
            let byte_slice = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            RegSetValueExW(
                hkey,
                PCWSTR(val_name.as_ptr()),
                Some(0),
                REG_SZ,
                Some(byte_slice),
            )
            .ok()
            .map_err(|e| e.to_string())
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(val_name.as_ptr()));
            Ok(())
        };

        let _ = RegCloseKey(hkey);
        result
    }
}
