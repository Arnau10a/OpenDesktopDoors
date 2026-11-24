use crate::messages::{to_wide_null, WM_APP_WIN_D_GUARD};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetShellWindow, GetWindow, IsIconic,
    IsWindowVisible, PostMessageW, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, GW_HWNDPREV, HWND_TOP, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
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

/// Finds the `hWndInsertAfter` handle that places `portal_hwnd` immediately above the
/// Windows Desktop (`WorkerW`/`Progman`) and below all normal non-minimized app windows.
pub fn desktop_insert_after(portal_hwnd: HWND) -> HWND {
    unsafe {
        let Some(desktop) = find_desktop_host_hwnd() else {
            return HWND_TOP;
        };

        let mut prev = GetWindow(desktop, GW_HWNDPREV);
        while let Ok(candidate) = prev {
            if candidate != portal_hwnd
                && IsWindowVisible(candidate).as_bool()
                && !IsIconic(candidate).as_bool()
            {
                let cls = window_class_name(candidate);
                if !is_shell_or_portal_class(&cls) {
                    return candidate;
                }
            }
            prev = GetWindow(candidate, GW_HWNDPREV);
        }

        // No visible non-minimized app windows above desktop (e.g. Win+D / Show Desktop)
        HWND_TOP
    }
}

fn is_shell_or_portal_class(cls: &str) -> bool {
    matches!(
        cls,
        "OpenDesktopDoorsPortal"
            | "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "TopLevelWindowForOverflowXamlIsland"
    )
}

unsafe fn find_desktop_host_hwnd() -> Option<HWND> {
    let shell_def_view = to_wide_null("SHELLDLL_DefView");
    let worker_w = to_wide_null("WorkerW");

    let progman_cls = to_wide_null("Progman");
    if let Ok(progman) = FindWindowW(PCWSTR(progman_cls.as_ptr()), PCWSTR::null()) {
        if FindWindowExW(
            Some(progman),
            None,
            PCWSTR(shell_def_view.as_ptr()),
            PCWSTR::null(),
        )
        .is_ok()
        {
            return Some(progman);
        }
    }

    let mut found: HWND = HWND::default();
    let _ = EnumWindows(
        Some(enum_worker_proc),
        LPARAM(&mut found as *mut HWND as isize),
    );
    if !found.0.is_null() {
        return Some(found);
    }

    let shell = GetShellWindow();
    if !shell.0.is_null() {
        return Some(shell);
    }

    FindWindowW(PCWSTR(worker_w.as_ptr()), PCWSTR::null()).ok()
}

unsafe extern "system" fn enum_worker_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let def_view = to_wide_null("SHELLDLL_DefView");
    if FindWindowExW(Some(hwnd), None, PCWSTR(def_view.as_ptr()), PCWSTR::null()).is_ok() {
        let out = &mut *(lparam.0 as *mut HWND);
        *out = hwnd;
        return BOOL(0);
    }
    BOOL(1)
}

unsafe fn window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let len = GetClassNameW(hwnd, &mut buf);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
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
        let cls = window_class_name(hwnd);
        if matches!(
            cls.as_str(),
            "WorkerW" | "Progman" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            let _ = PostMessageW(
                Some(controller),
                WM_APP_WIN_D_GUARD,
                Default::default(),
                Default::default(),
            );
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
