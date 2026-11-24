#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod covers;
mod desktop_pin;
mod dnd;
mod hotkey;
mod icons;
mod launchers;
mod library;
mod messages;
mod playnite_db;
mod portal;
mod render;
mod spring;
mod startup;
mod tray;
mod window;

use config::{AppConfig, LayoutStyle, PortalConfig, PortalKind, PortalTheme, SortBy};
use covers::{spawn_missing_cover_fetcher, PendingCovers};
use desktop_pin::DesktopPinGuard;
use hotkey::{GlobalHotkey, HOTKEY_TOGGLE_FRONT_ID};
use library::GameLibrary;
use messages::{
    to_wide_null, WM_APP_CONFIG_DIRTY, WM_APP_COVER_READY, WM_APP_LIBRARY_UPDATED,
    WM_APP_PORTAL_CLOSED, WM_APP_TRAY, WM_APP_WIN_D_GUARD,
};
use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tray::{
    TrayIcon, CMD_EXIT, CMD_NEW_CUSTOM_PORTAL, CMD_NEW_FAVORITES_PORTAL, CMD_NEW_INSTALLED_PORTAL,
    CMD_NEW_RECENT_PORTAL, CMD_NEW_STATS_PORTAL, CMD_NEW_STEAM_PORTAL, CMD_OPEN_CONFIG_DIR,
    CMD_RESCAN_LIBRARY, CMD_TOGGLE_FRONT, CMD_TOGGLE_STARTUP,
};
use window::{apply_z_order_hwnd, is_bring_to_front, set_bring_to_front, PortalWindow};
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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW, KillTimer,
    PostQuitMessage, RegisterClassExW, SetTimer, SetWindowLongPtrW, TranslateMessage,
    GWLP_USERDATA, HWND_MESSAGE, MSG, SW_SHOWNORMAL, WM_CREATE, WM_HOTKEY, WM_LBUTTONDBLCLK,
    WM_NCDESTROY, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};

const CONTROLLER_CLASS: &str = "OpenDesktopDoorsController";
const TIMER_WIN_D_RECHECK_1: usize = 101;
const TIMER_WIN_D_RECHECK_2: usize = 102;

struct AppState {
    hwnd: HWND,
    hinstance: windows::Win32::Foundation::HINSTANCE,
    config: AppConfig,
    library: Rc<RefCell<GameLibrary>>,
    pending_covers: PendingCovers,
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

    fn refresh_all_portals(&self) {
        for p in &self.portals {
            p.borrow_mut().refresh_from_library();
        }
    }

    fn spawn_portal(&mut self, title: &str, kind: PortalKind, sort_by: SortBy) {
        let offset = (self.portals.len() as i32) * 32;
        let cfg = PortalConfig {
            id: format!("portal-{}", self.portals.len() + 1),
            title: title.to_string(),
            kind,
            sort_by,
            layout_style: LayoutStyle::Cards,
            x: 64 + offset,
            y: 64 + offset,
            width: 480,
            expanded_height: 440,
            collapsed: false,
            auto_collapse: false,
            show_details: true,
            theme: PortalTheme::default(),
        };
        if let Some(win) =
            PortalWindow::spawn(self.hinstance, self.hwnd, Rc::clone(&self.library), cfg)
        {
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
        let library = Rc::new(RefCell::new(GameLibrary::load_and_scan()));
        let pending_covers: PendingCovers = Arc::new(Mutex::new(Vec::new()));

        let app = Rc::new(RefCell::new(AppState {
            hwnd: HWND::default(),
            hinstance,
            config: cfg.clone(),
            library: Rc::clone(&library),
            pending_covers: Arc::clone(&pending_covers),
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
                if let Some(p) =
                    PortalWindow::spawn(hinstance, ctrl_hwnd, Rc::clone(&library), portal_cfg)
                {
                    state.portals.push(p);
                }
            }
        }

        // Background fetch missing 2:3 Steam covers via WinHTTP
        spawn_missing_cover_fetcher(&library.borrow().games, ctrl_hwnd, pending_covers);

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
                let _ = SetTimer(Some(hwnd), TIMER_WIN_D_RECHECK_1, 60, None);
                let _ = SetTimer(Some(hwnd), TIMER_WIN_D_RECHECK_2, 180, None);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_WIN_D_RECHECK_1 || wparam.0 == TIMER_WIN_D_RECHECK_2 {
                let _ = KillTimer(Some(hwnd), wparam.0);
                if !is_bring_to_front() {
                    let hwnds = rc_clone.borrow().portal_hwnds();
                    for h in hwnds {
                        apply_z_order_hwnd(h);
                    }
                }
            }
            LRESULT(0)
        }
        WM_APP_CONFIG_DIRTY => {
            rc_clone.borrow_mut().save_current_config();
            LRESULT(0)
        }
        WM_APP_COVER_READY => {
            let state = rc_clone.borrow();
            let updates: Vec<_> = {
                if let Ok(mut q) = state.pending_covers.lock() {
                    std::mem::take(&mut *q)
                } else {
                    Vec::new()
                }
            };
            if !updates.is_empty() {
                let mut lib = state.library.borrow_mut();
                for (gid, cov, hero) in updates {
                    lib.apply_downloaded_cover(&gid, cov, hero);
                }
                drop(lib);
                state.refresh_all_portals();
            }
            LRESULT(0)
        }
        WM_APP_LIBRARY_UPDATED => {
            if wparam.0 == 1 && lparam.0 != 0 {
                let boxed = Box::from_raw(lparam.0 as *mut (String, u64));
                let (game_id, delta_secs) = *boxed;
                rc_clone
                    .borrow()
                    .library
                    .borrow_mut()
                    .add_playtime(&game_id, delta_secs);
            }
            rc_clone.borrow().refresh_all_portals();
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
                    Some(CMD_NEW_RECENT_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Jugados recientemente",
                            PortalKind::Recent,
                            SortBy::LastPlayed,
                        );
                    }
                    Some(CMD_NEW_FAVORITES_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Favoritos",
                            PortalKind::Favorites,
                            SortBy::LastPlayed,
                        );
                    }
                    Some(CMD_NEW_INSTALLED_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Instalados",
                            PortalKind::Installed,
                            SortBy::Playtime,
                        );
                    }
                    Some(CMD_NEW_STEAM_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Steam",
                            PortalKind::Launcher {
                                name: "Steam".into(),
                            },
                            SortBy::LastPlayed,
                        );
                    }
                    Some(CMD_NEW_CUSTOM_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Colección",
                            PortalKind::Custom {
                                game_ids: Vec::new(),
                            },
                            SortBy::Name,
                        );
                    }
                    Some(CMD_NEW_STATS_PORTAL) => {
                        rc_clone.borrow_mut().spawn_portal(
                            "Tus Estadísticas",
                            PortalKind::Stats,
                            SortBy::Playtime,
                        );
                    }
                    Some(CMD_RESCAN_LIBRARY) => {
                        let state = rc_clone.borrow();
                        state.library.borrow_mut().rescan();
                        state.refresh_all_portals();
                        spawn_missing_cover_fetcher(
                            &state.library.borrow().games,
                            hwnd,
                            Arc::clone(&state.pending_covers),
                        );
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
