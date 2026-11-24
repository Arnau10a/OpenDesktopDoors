use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_FS_CHANGED: u32 = WM_APP + 2;
pub const WM_APP_WIN_D_GUARD: u32 = WM_APP + 3;
pub const WM_APP_CONFIG_DIRTY: u32 = WM_APP + 4;
pub const WM_APP_DROP_PATHS: u32 = WM_APP + 5;
pub const WM_APP_PORTAL_CLOSED: u32 = WM_APP + 6;

pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
