use crate::messages::{to_wide_null, WM_APP_TRAY};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, SetForegroundWindow,
    TrackPopupMenu, IDI_APPLICATION, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
};

pub const CMD_NEW_FOLDER_PORTAL: u32 = 1001;
pub const CMD_NEW_VIRTUAL_PORTAL: u32 = 1002;
pub const CMD_TOGGLE_FRONT: u32 = 1003;
pub const CMD_TOGGLE_STARTUP: u32 = 1004;
pub const CMD_OPEN_CONFIG_DIR: u32 = 1005;
pub const CMD_EXIT: u32 = 1006;

pub struct TrayIcon {
    hwnd: HWND,
    id: u32,
}

impl TrayIcon {
    pub fn install(hwnd: HWND) -> Self {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
            ..Default::default()
        };

        let tip = to_wide_null("OpenDesktopDoors — Ctrl+Alt+D");
        let len = tip.len().min(nid.szTip.len());
        nid.szTip[..len].copy_from_slice(&tip[..len]);

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Self { hwnd, id: 1 }
    }

    pub fn show_menu(&self, brought_to_front: bool, startup_enabled: bool) -> Option<u32> {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return None;
            };

            let add_item = |id: u32, text: &str, checked: Option<bool>| {
                let wide = to_wide_null(text);
                let flags = match checked {
                    Some(true) => MF_STRING | MF_CHECKED,
                    Some(false) => MF_STRING | MF_UNCHECKED,
                    None => MF_STRING,
                };
                let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(wide.as_ptr()));
            };

            add_item(CMD_NEW_FOLDER_PORTAL, "New Folder Portal...", None);
            add_item(CMD_NEW_VIRTUAL_PORTAL, "New Quick Doors Portal", None);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            add_item(
                CMD_TOGGLE_FRONT,
                "Bring Portals to Front (Ctrl+Alt+D)",
                Some(brought_to_front),
            );
            add_item(
                CMD_TOGGLE_STARTUP,
                "Start with Windows",
                Some(startup_enabled),
            );
            add_item(CMD_OPEN_CONFIG_DIR, "Open Config Folder", None);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            add_item(CMD_EXIT, "Exit OpenDesktopDoors", None);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);

            let cmd = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD,
                pt.x,
                pt.y,
                None,
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);

            if cmd.0 > 0 {
                Some(cmd.0 as u32)
            } else {
                None
            }
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: self.id,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}
