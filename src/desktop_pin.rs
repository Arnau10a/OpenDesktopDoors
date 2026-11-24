use crate::messages::WM_APP_WIN_D_GUARD;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, PostMessageW, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

static CONTROLLER_HWND: AtomicIsize = AtomicIsize::new(0);

pub struct DesktopPinGuard {
    hook_fg: HWINEVENTHOOK,
    hook_min: HWINEVENTHOOK,
}

impl DesktopPinGuard {
    pub fn install(controller_hwnd: HWND) -> Self {
        CONTROLLER_HWND.store(controller_hwnd.0 as isize, Ordering::SeqCst);
        unsafe {
            let hook_fg = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            let hook_min = SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            Self { hook_fg, hook_min }
        }
    }
}

impl Drop for DesktopPinGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.hook_fg.is_invalid() {
                let _ = UnhookWinEvent(self.hook_fg);
            }
            if !self.hook_min.is_invalid() {
                let _ = UnhookWinEvent(self.hook_min);
            }
        }
        CONTROLLER_HWND.store(0, Ordering::SeqCst);
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _time: u32,
) {
    let target_raw = CONTROLLER_HWND.load(Ordering::Relaxed);
    if target_raw == 0 {
        return;
    }
    let controller = HWND(target_raw as *mut _);

    if event == EVENT_SYSTEM_FOREGROUND {
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            if cls == "WorkerW" || cls == "Progman" {
                let _ = PostMessageW(
                    Some(controller),
                    WM_APP_WIN_D_GUARD,
                    Default::default(),
                    Default::default(),
                );
            }
        }
    } else if event == EVENT_SYSTEM_MINIMIZESTART || event == EVENT_SYSTEM_MINIMIZEEND {
        let _ = PostMessageW(
            Some(controller),
            WM_APP_WIN_D_GUARD,
            Default::default(),
            Default::default(),
        );
    }
}
