use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};

pub const HOTKEY_TOGGLE_FRONT_ID: i32 = 1;

pub struct GlobalHotkey {
    hwnd: HWND,
    id: i32,
    registered: bool,
}

impl GlobalHotkey {
    pub fn register(hwnd: HWND, id: i32, modifiers: u32, vk: u32) -> Self {
        let ok =
            unsafe { RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(modifiers), vk).is_ok() };
        Self {
            hwnd,
            id,
            registered: ok,
        }
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        if self.registered {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), self.id);
            }
        }
    }
}
