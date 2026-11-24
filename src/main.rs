#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod desktop_pin;
mod dnd;
mod fs_watch;
mod hotkey;
mod icons;
mod messages;
mod portal;
mod render;
mod spring;
mod startup;
mod tray;
mod window;

use config::{AppConfig, PortalConfig, PortalKind, PortalTheme};
use desktop_pin::DesktopPinGuard;
use hotkey::{GlobalHotkey, HOTKEY_TOGGLE_FRONT_ID};
use messages::{
    to_wide_null, WM_APP_CONFIG_DIRTY, WM_APP_PORTAL_CLOSED, WM_APP_TRAY, WM_APP_WIN_D_GUARD,
};
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use tray::{
    TrayIcon, CMD_EXIT, CMD_NEW_FOLDER_PORTAL, CMD_NEW_VIRTUAL_PORTAL, CMD_OPEN_CONFIG_DIR,
    CMD_TOGGLE_FRONT, CMD_TOGGLE_STARTUP,
};
use window::{
    apply_z_order_hwnd, is_bring_to_front, pick_folder_dialog, set_bring_to_front, PortalWindow,
};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    PostQuitMessage, RegisterClassExW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA,
    HWND_MESSAGE, MSG, SW_SHOWNORMAL, WM_CREATE, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NCDESTROY,
    WM_RBUTTONUP, WNDCLASSEXW,
};

const CONTROLLER_CLASS: &str = "OpenDesktopDoorsController";

struct AppState {
    hwnd: HWND,
    hinstance: windows::Win32::Foundation::HINSTANCE,
    config: AppConfig,
    portals: Vec<Rc<RefCell<PortalWindow>>>,
    tray: Option<TrayIcon>,
    _hotkey: Option<GlobalHotkey>,
    _pin_guard: Option<DesktopPinGuard>,
}

impl AppState {
    fn save_current_config(&mut self) {
        self.config.portals = self
            .portals
            .iter()
            .map(|p| p.borrow().config.clone())
            .collect();
        let _ = config::save_config(&self.config);
    }

    fn portal_hwnds(&self) -> Vec<HWND> {
        self.portals.iter().map(|p| p.borrow().hwnd).collect()
    }

    fn add_folder_portal(&mut self, folder: std::path::PathBuf) {
        let title = folder
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Folder".to_string());
        let offset = (self.portals.len() as i32) * 32;
        let cfg = PortalConfig {
            id: format!("portal-{}", self.portals.len() + 1),
            title,
            kind: PortalKind::Folder { path: folder },
            x: 80 + offset,
            y: 80 + offset,
            width: 340,
            expanded_height: 360,
            collapsed: false,
            auto_collapse: false,
            theme: PortalTheme::default(),
        };
        if let Some(win) = PortalWindow::spawn(self.hinstance, self.hwnd, cfg) {
            self.portals.push(win);
            self.save_current_config();
        }
    }

    fn spawn_new_virtual_portal(&mut self) {
        let offset = (self.portals.len() as i32) * 32;
        let cfg = PortalConfig {
            id: format!("portal-virtual-{}", self.portals.len() + 1),
            title: "Quick Doors".to_string(),
            kind: PortalKind::Virtual { items: Vec::new() },
            x: 120 + offset,
            y: 120 + offset,
            width: 320,
            expanded_height: 240,
            collapsed: false,
            auto_collapse: true,
            theme: PortalTheme::default(),
        };
        if let Some(win) = PortalWindow::spawn(self.hinstance, self.hwnd, cfg) {
            self.portals.push(win);
            self.save_current_config();
        }
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if OleInitialize(None).is_err() {
            return;
        }

        let mutex_name = to_wide_null("Local\\OpenDesktopDoors_SingleInstance");
        let _mutex = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            OleUninitialize();
            return;
        }

        let hinstance: windows::Win32::Foundation::HINSTANCE =
            GetModuleHandleW(None).unwrap_or_default().into();

        PortalWindow::register_class(hinstance);

        let cls_w = to_wide_null(CONTROLLER_CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(controller_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(cls_w.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let cfg = config::load_or_default();
        let app = Rc::new(RefCell::new(AppState {
            hwnd: HWND::default(),
            hinstance,
            config: cfg.clone(),
            portals: Vec::new(),
            tray: None,
            _hotkey: None,
            _pin_guard: None,
        }));

        let app_ptr = Rc::into_raw(Rc::clone(&app)) as *const c_void;
        let Ok(ctrl_hwnd) = CreateWindowExW(
            Default::default(),
            PCWSTR(cls_w.as_ptr()),
            PCWSTR::null(),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            Some(app_ptr),
        ) else {
            let _ = Rc::from_raw(app_ptr as *const RefCell<AppState>);
            OleUninitialize();
            return;
        };

        {
            let mut state = app.borrow_mut();
            state.hwnd = ctrl_hwnd;
            state.tray = Some(TrayIcon::install(ctrl_hwnd));
            state._hotkey = Some(GlobalHotkey::register(
                ctrl_hwnd,
                HOTKEY_TOGGLE_FRONT_ID,
                cfg.hotkey_mods,
                cfg.hotkey_vk,
            ));
            state._pin_guard = Some(DesktopPinGuard::install(ctrl_hwnd));

            for portal_cfg in cfg.portals {
                if let Some(p) = PortalWindow::spawn(hinstance, ctrl_hwnd, portal_cfg) {
                    state.portals.push(p);
                }
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        app.borrow_mut().save_current_config();
        OleUninitialize();
    }
}

unsafe extern "system" fn controller_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let cs = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return LRESULT(0);
    }

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<AppState>;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = Rc::from_raw(ptr);
        return LRESULT(0);
    }

    let rc = Rc::from_raw(ptr);
    let rc_clone = Rc::clone(&rc);
    std::mem::forget(rc);

    match msg {
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_TOGGLE_FRONT_ID {
                let hwnds = {
                    let next = !is_bring_to_front();
                    set_bring_to_front(next);
                    rc_clone.borrow().portal_hwnds()
                };
                for h in hwnds {
                    apply_z_order_hwnd(h);
                }
            }
            LRESULT(0)
        }
        WM_APP_WIN_D_GUARD => {
            if !is_bring_to_front() {
                let hwnds = rc_clone.borrow().portal_hwnds();
                for h in hwnds {
                    apply_z_order_hwnd(h);
                }
            }
            LRESULT(0)
        }
        WM_APP_CONFIG_DIRTY => {
            rc_clone.borrow_mut().save_current_config();
            LRESULT(0)
        }
        WM_APP_PORTAL_CLOSED => {
            let closed_raw = wparam.0;
            let mut state = rc_clone.borrow_mut();
            state
                .portals
                .retain(|p| p.borrow().hwnd.0 as usize != closed_raw);
            state.save_current_config();
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_LBUTTONDBLCLK {
                let hwnds = {
                    let next = !is_bring_to_front();
                    set_bring_to_front(next);
                    rc_clone.borrow().portal_hwnds()
                };
                for h in hwnds {
                    apply_z_order_hwnd(h);
                }
            } else if event == WM_RBUTTONUP {
                let startup_on = startup::is_startup_enabled();
                let cmd = {
                    let state = rc_clone.borrow();
                    state
                        .tray
                        .as_ref()
                        .and_then(|t| t.show_menu(is_bring_to_front(), startup_on))
                };
                match cmd {
                    Some(CMD_NEW_FOLDER_PORTAL) => {
                        if let Some(folder) = pick_folder_dialog(hwnd) {
                            rc_clone.borrow_mut().add_folder_portal(folder);
                        }
                    }
                    Some(CMD_NEW_VIRTUAL_PORTAL) => {
                        rc_clone.borrow_mut().spawn_new_virtual_portal();
                    }
                    Some(CMD_TOGGLE_FRONT) => {
                        let hwnds = {
                            let next = !is_bring_to_front();
                            set_bring_to_front(next);
                            rc_clone.borrow().portal_hwnds()
                        };
                        for h in hwnds {
                            apply_z_order_hwnd(h);
                        }
                    }
                    Some(CMD_TOGGLE_STARTUP) => {
                        let _ = startup::set_startup_enabled(!startup_on);
                    }
                    Some(CMD_OPEN_CONFIG_DIR) => {
                        let dir = config::config_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        let dir_w = to_wide_null(&dir.to_string_lossy());
                        let verb = to_wide_null("open");
                        ShellExecuteW(
                            None,
                            PCWSTR(verb.as_ptr()),
                            PCWSTR(dir_w.as_ptr()),
                            PCWSTR::null(),
                            PCWSTR::null(),
                            SW_SHOWNORMAL,
                        );
                    }
                    Some(CMD_EXIT) => {
                        let hwnds = {
                            let mut state = rc_clone.borrow_mut();
                            state.save_current_config();
                            let hs = state.portal_hwnds();
                            state.portals.clear();
                            hs
                        };
                        let _ = window::hide_window;
                        for h in hwnds {
                            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(h);
                        }
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
