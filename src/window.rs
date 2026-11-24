use crate::config::{PortalConfig, PortalKind, VirtualItem};
use crate::dnd::{copy_or_move_into_folder, start_file_drag, PortalDropTarget};
use crate::fs_watch::FolderWatcher;
use crate::messages::{
    to_wide_null, WM_APP_CONFIG_DIRTY, WM_APP_DROP_PATHS, WM_APP_FS_CHANGED, WM_APP_PORTAL_CLOSED,
};
use crate::portal::{load_items, GridLayout, PortalItem, HEADER_HEIGHT};
use crate::render::{rect_size, PortalRenderer, RenderState};
use crate::spring::Spring;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows::Win32::System::Ole::{IDropTarget, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, ShellExecuteW, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect, KillTimer, LoadCursorW,
    PostMessageW, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TrackPopupMenu, CS_DBLCLKS, GWLP_USERDATA, HWND_BOTTOM, HWND_TOPMOST, IDC_ARROW, IDC_SIZENWSE,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_TOPALIGN, WINDOWPOS, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE, WM_TIMER, WM_WINDOWPOSCHANGING, WNDCLASSEXW,
    WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const PORTAL_CLASS_NAME: &str = "OpenDesktopDoorsPortal";
const TIMER_ANIM_ID: usize = 1;
const TIMER_AUTO_COLLAPSE_ID: usize = 2;
const COLLAPSED_HEIGHT: f32 = HEADER_HEIGHT;

const CTX_TOGGLE_COLLAPSE: u32 = 2001;
const CTX_TOGGLE_AUTO_COLLAPSE: u32 = 2002;
const CTX_OPEN_FOLDER: u32 = 2003;
const CTX_CHANGE_FOLDER: u32 = 2004;
const CTX_CLOSE_PORTAL: u32 = 2005;

static BRING_TO_FRONT: AtomicBool = AtomicBool::new(false);

pub fn set_bring_to_front(front: bool) {
    BRING_TO_FRONT.store(front, Ordering::SeqCst);
}

pub fn is_bring_to_front() -> bool {
    BRING_TO_FRONT.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragMode {
    None,
    MoveWindow {
        start_screen_x: i32,
        start_screen_y: i32,
        win_x: i32,
        win_y: i32,
        moved: bool,
    },
    ResizeWindow {
        start_screen_x: i32,
        start_screen_y: i32,
        start_w: i32,
        start_h: i32,
    },
    PendingItemDrag {
        index: usize,
        start_x: f32,
        start_y: f32,
    },
}

pub struct PortalWindow {
    pub hwnd: HWND,
    pub config: PortalConfig,
    controller_hwnd: HWND,
    items: Vec<PortalItem>,
    layout: GridLayout,
    renderer: Option<PortalRenderer>,
    watcher: Option<FolderWatcher>,
    _drop_target: Option<IDropTarget>,
    height_spring: Spring,
    last_tick: Instant,
    animating: bool,
    scroll_y: f32,
    hovered_index: Option<usize>,
    mouse_inside: bool,
    is_drag_over: bool,
    drag_mode: DragMode,
    dpi: f32,
}

impl PortalWindow {
    pub fn register_class(hinstance: HINSTANCE) {
        let cls_name = to_wide_null(PORTAL_CLASS_NAME);
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(portal_wnd_proc),
                hInstance: hinstance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: PCWSTR(cls_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    }

    pub fn spawn(
        hinstance: HINSTANCE,
        controller_hwnd: HWND,
        mut config: PortalConfig,
    ) -> Option<Rc<RefCell<Self>>> {
        config.clamp_dimensions();
        let initial_h = if config.collapsed {
            COLLAPSED_HEIGHT as i32
        } else {
            config.expanded_height
        };

        let items = load_items(&config.kind);
        let layout = GridLayout::compute(
            config.width as f32,
            config.expanded_height as f32,
            items.len(),
        );

        let state = Rc::new(RefCell::new(Self {
            hwnd: HWND::default(),
            height_spring: Spring::new(initial_h as f32, 300.0, 32.0),
            config: config.clone(),
            controller_hwnd,
            items,
            layout,
            renderer: None,
            watcher: None,
            _drop_target: None,
            last_tick: Instant::now(),
            animating: false,
            scroll_y: 0.0,
            hovered_index: None,
            mouse_inside: false,
            is_drag_over: false,
            drag_mode: DragMode::None,
            dpi: 96.0,
        }));

        let cls_name = to_wide_null(PORTAL_CLASS_NAME);
        let title = to_wide_null(&config.title);
        let raw_ptr = Rc::into_raw(Rc::clone(&state)) as *const c_void;

        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE,
                PCWSTR(cls_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP | WS_CLIPCHILDREN,
                config.x,
                config.y,
                config.width,
                initial_h,
                None,
                None,
                Some(hinstance),
                Some(raw_ptr),
            )
            .ok();

            let Some(hwnd) = hwnd else {
                let _ = Rc::from_raw(raw_ptr as *const RefCell<Self>);
                return None;
            };

            let (pw, ph, dpi, radius) = {
                let mut s = state.borrow_mut();
                s.hwnd = hwnd;
                s.dpi = GetDpiForWindow(hwnd) as f32;
                if s.dpi <= 0.0 {
                    s.dpi = 96.0;
                }
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let (pw, ph) = rect_size(&rc);
                (pw, ph, s.dpi, s.config.theme.corner_radius)
            };

            // Win32/DWM calls happen with NO RefCell borrow held to avoid re-entrancy panics.
            setup_dwm_attributes(hwnd);
            apply_window_region(hwnd, pw as i32, ph as i32, dpi, radius);

            {
                let mut s = state.borrow_mut();
                s.renderer = PortalRenderer::new(hwnd, pw, ph, dpi, radius).ok();
                s.restart_watcher();
                s.register_dnd();
            }

            apply_z_order_hwnd(hwnd);
            state.borrow_mut().draw();
        }

        Some(state)
    }

    unsafe fn register_dnd(&mut self) {
        let dt: IDropTarget = PortalDropTarget::new(self.hwnd).into();
        if RegisterDragDrop(self.hwnd, &dt).is_ok() {
            self._drop_target = Some(dt);
        }
    }

    fn restart_watcher(&mut self) {
        self.watcher = None;
        if let PortalKind::Folder { path } = &self.config.kind {
            self.watcher = FolderWatcher::start(path.clone(), self.hwnd);
        }
    }

    pub fn reload_items(&mut self) {
        self.items = load_items(&self.config.kind);
        self.recompute_layout();
        self.draw();
    }

    fn recompute_layout(&mut self) {
        self.layout = GridLayout::compute(
            self.config.width as f32,
            self.config.expanded_height as f32,
            self.items.len(),
        );
        self.scroll_y = self.scroll_y.clamp(0.0, self.layout.max_scroll);
    }

    pub fn set_collapsed(&mut self, collapsed: bool) {
        if self.config.collapsed == collapsed {
            return;
        }
        self.config.collapsed = collapsed;
        let target_h = if collapsed {
            COLLAPSED_HEIGHT
        } else {
            self.config.expanded_height as f32
        };
        self.height_spring.set_target(target_h);
        self.start_animation();
        self.notify_config_dirty();
    }

    fn start_animation(&mut self) {
        if !self.animating {
            self.animating = true;
            self.last_tick = Instant::now();
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_ANIM_ID, 8, None);
            }
        }
    }

    fn compute_animation_step(&mut self) -> (i32, i32, bool) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;

        let still_moving = self.height_spring.step(dt);
        let target_px = self.dip_to_px(self.height_spring.value).round().max(40.0) as i32;
        let width_px = self.dip_to_px(self.config.width as f32).round().max(180.0) as i32;

        if !still_moving {
            self.animating = false;
        }
        (width_px, target_px, still_moving)
    }

    fn dip_to_px(&self, dip: f32) -> f32 {
        dip * (self.dpi / 96.0)
    }

    fn px_to_dip(&self, px: i32) -> f32 {
        px as f32 * (96.0 / self.dpi)
    }

    fn current_dip_size(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        (self.px_to_dip(rc.right), self.px_to_dip(rc.bottom))
    }

    pub fn draw(&mut self) {
        let (w_dip, h_dip) = self.current_dip_size();
        let is_virtual = matches!(self.config.kind, PortalKind::Virtual { .. });
        if let Some(renderer) = &mut self.renderer {
            let state = RenderState {
                title: &self.config.title,
                items: &self.items,
                layout: &self.layout,
                width: w_dip,
                height: h_dip,
                dpi: self.dpi,
                scroll_y: self.scroll_y,
                hovered_index: self.hovered_index,
                collapsed: self.config.collapsed,
                auto_collapse: self.config.auto_collapse,
                is_drag_over: self.is_drag_over,
                is_virtual,
                theme: &self.config.theme,
            };
            let _ = renderer.render(&state);
        }
    }

    fn notify_config_dirty(&self) {
        unsafe {
            let _ = PostMessageW(
                Some(self.controller_hwnd),
                WM_APP_CONFIG_DIRTY,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    fn handle_drop_paths(&mut self, paths: Vec<PathBuf>, move_files: bool) {
        match &mut self.config.kind {
            PortalKind::Folder { path } => {
                copy_or_move_into_folder(&paths, path, move_files);
                self.reload_items();
            }
            PortalKind::Virtual { items } => {
                for p in paths {
                    let name = p
                        .file_stem()
                        .or_else(|| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Shortcut".to_string());
                    if !items.iter().any(|existing| existing.target == p) {
                        items.push(VirtualItem { name, target: p });
                    }
                }
                self.reload_items();
                self.notify_config_dirty();
            }
        }
    }
}

pub fn apply_z_order_hwnd(hwnd: HWND) {
    unsafe {
        let insert_after = if is_bring_to_front() {
            HWND_TOPMOST
        } else {
            HWND_BOTTOM
        };
        let _ = SetWindowPos(
            hwnd,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

unsafe fn setup_dwm_attributes(hwnd: HWND) {
    let dark: i32 = 1;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const _ as *const c_void,
        std::mem::size_of::<i32>() as u32,
    );
    let backdrop = DWMSBT_TRANSIENTWINDOW;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_SYSTEMBACKDROP_TYPE,
        &backdrop as *const _ as *const c_void,
        std::mem::size_of_val(&backdrop) as u32,
    );
    let corner = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &corner as *const _ as *const c_void,
        std::mem::size_of_val(&corner) as u32,
    );
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
}

unsafe fn apply_window_region(hwnd: HWND, pw: i32, ph: i32, dpi: f32, corner_radius: f32) {
    let radius_px = (corner_radius * (dpi / 96.0)).round() as i32;
    let diameter = (radius_px * 2).min(pw).min(ph).max(2);
    let rgn = CreateRoundRectRgn(0, 0, pw + 1, ph + 1, diameter, diameter);
    if !rgn.is_invalid() {
        let _ = SetWindowRgn(hwnd, Some(rgn), false);
    }
}

fn shell_open_path(owner: HWND, path: &std::path::Path) {
    let path_w = to_wide_null(&path.to_string_lossy());
    let verb_w = to_wide_null("open");
    unsafe {
        ShellExecuteW(
            Some(owner),
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn pick_folder_dialog(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = windows::Win32::System::Com::CoCreateInstance(
            &FileOpenDialog,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .ok()?;
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_PICKFOLDERS).ok()?;
        if dialog.Show(Some(owner)).is_err() {
            return None;
        }
        let item = dialog.GetResult().ok()?;
        let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path_str = pwstr.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as *const c_void));
        Some(PathBuf::from(path_str))
    }
}

pub fn hide_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

unsafe extern "system" fn portal_wnd_proc(
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

    // Handle WM_WINDOWPOSCHANGING before touching RefCell to prevent any re-entrant borrow panics.
    if msg == WM_WINDOWPOSCHANGING {
        let wp = &mut *(lparam.0 as *mut WINDOWPOS);
        wp.hwndInsertAfter = if is_bring_to_front() {
            HWND_TOPMOST
        } else {
            HWND_BOTTOM
        };
        return LRESULT(0);
    }

    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<PortalWindow>;
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
        WM_SIZE => {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let (pw, ph) = rect_size(&rect);
            let (dpi, radius) = {
                let p = rc_clone.borrow();
                (p.dpi, p.config.theme.corner_radius)
            };
            apply_window_region(hwnd, pw as i32, ph as i32, dpi, radius);
            if let Ok(mut p) = rc_clone.try_borrow_mut() {
                if let Some(renderer) = &mut p.renderer {
                    let _ = renderer.resize(pw, ph, dpi, radius);
                }
                p.draw();
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as f32;
            if let Ok(mut p) = rc_clone.try_borrow_mut() {
                p.dpi = if new_dpi > 0.0 { new_dpi } else { 96.0 };
            }
            let suggested = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if let Ok(mut p) = rc_clone.try_borrow_mut() {
                p.draw();
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
            if let Ok(mut p) = rc_clone.try_borrow_mut() {
                p.draw();
            }
            let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIM_ID {
                let (width_px, target_px, still_moving) =
                    rc_clone.borrow_mut().compute_animation_step();
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    width_px,
                    target_px,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                if !still_moving {
                    let _ = KillTimer(Some(hwnd), TIMER_ANIM_ID);
                }
                if let Ok(mut p) = rc_clone.try_borrow_mut() {
                    p.draw();
                }
            } else if wparam.0 == TIMER_AUTO_COLLAPSE_ID {
                let _ = KillTimer(Some(hwnd), TIMER_AUTO_COLLAPSE_ID);
                rc_clone.borrow_mut().set_collapsed_if_idle();
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut win_rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut win_rc);
            let is_resize = {
                let p = rc_clone.borrow();
                let x_dip = p.px_to_dip(pt.x - win_rc.left);
                let y_dip = p.px_to_dip(pt.y - win_rc.top);
                let (w_dip, h_dip) = p.current_dip_size();
                !p.config.collapsed && x_dip >= w_dip - 18.0 && y_dip >= h_dip - 18.0
            };
            if is_resize {
                if let Ok(cur) = LoadCursorW(None, IDC_SIZENWSE) {
                    windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cur));
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            enum MoveAction {
                None,
                MoveWin(i32, i32),
                ResizeWin(i32, i32),
                StartDrag(PathBuf),
            }

            let lx = (lparam.0 & 0xFFFF) as i16 as i32;
            let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            let (need_track, action) = {
                let mut portal = rc_clone.borrow_mut();
                let mut track = false;
                if !portal.mouse_inside {
                    portal.mouse_inside = true;
                    track = true;
                    if portal.config.auto_collapse && portal.config.collapsed {
                        portal.set_collapsed(false);
                    }
                }

                let x_dip = portal.px_to_dip(lx);
                let y_dip = portal.px_to_dip(ly);

                let act = match portal.drag_mode {
                    DragMode::MoveWindow {
                        start_screen_x,
                        start_screen_y,
                        win_x,
                        win_y,
                        moved: _,
                    } => {
                        let dx = pt.x - start_screen_x;
                        let dy = pt.y - start_screen_y;
                        if dx.abs() > 3 || dy.abs() > 3 {
                            portal.drag_mode = DragMode::MoveWindow {
                                start_screen_x,
                                start_screen_y,
                                win_x,
                                win_y,
                                moved: true,
                            };
                            let nx = win_x + dx;
                            let ny = win_y + dy;
                            portal.config.x = nx;
                            portal.config.y = ny;
                            MoveAction::MoveWin(nx, ny)
                        } else {
                            MoveAction::None
                        }
                    }
                    DragMode::ResizeWindow {
                        start_screen_x,
                        start_screen_y,
                        start_w,
                        start_h,
                    } => {
                        let dx_dip = portal.px_to_dip(pt.x - start_screen_x).round() as i32;
                        let dy_dip = portal.px_to_dip(pt.y - start_screen_y).round() as i32;
                        let new_w = (start_w + dx_dip).clamp(220, 960);
                        let new_h = (start_h + dy_dip).clamp(160, 1080);
                        portal.config.width = new_w;
                        portal.config.expanded_height = new_h;
                        portal.height_spring.snap(new_h as f32);
                        portal.recompute_layout();
                        let pw = portal.dip_to_px(new_w as f32).round() as i32;
                        let ph = portal.dip_to_px(new_h as f32).round() as i32;
                        MoveAction::ResizeWin(pw, ph)
                    }
                    DragMode::PendingItemDrag {
                        index,
                        start_x,
                        start_y,
                    } => {
                        if (x_dip - start_x).abs() > 6.0 || (y_dip - start_y).abs() > 6.0 {
                            portal.drag_mode = DragMode::None;
                            if let Some(item) = portal.items.get(index) {
                                MoveAction::StartDrag(item.path.clone())
                            } else {
                                MoveAction::None
                            }
                        } else {
                            MoveAction::None
                        }
                    }
                    DragMode::None => {
                        let new_hover = if portal.config.collapsed {
                            None
                        } else {
                            portal.layout.hit_test(
                                x_dip,
                                y_dip,
                                portal.items.len(),
                                portal.scroll_y,
                            )
                        };
                        if new_hover != portal.hovered_index {
                            portal.hovered_index = new_hover;
                            portal.draw();
                        }
                        MoveAction::None
                    }
                };
                (track, act)
            };

            if need_track {
                let _ = KillTimer(Some(hwnd), TIMER_AUTO_COLLAPSE_ID);
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }

            match action {
                MoveAction::MoveWin(nx, ny) => {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        nx,
                        ny,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                MoveAction::ResizeWin(pw, ph) => {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        pw,
                        ph,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                MoveAction::StartDrag(path) => {
                    let _ = ReleaseCapture();
                    start_file_drag(&path);
                }
                MoveAction::None => {}
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let start_collapse_timer = {
                let mut portal = rc_clone.borrow_mut();
                portal.mouse_inside = false;
                if portal.hovered_index.is_some() {
                    portal.hovered_index = None;
                    portal.draw();
                }
                portal.config.auto_collapse && !portal.config.collapsed
            };
            if start_collapse_timer {
                SetTimer(Some(hwnd), TIMER_AUTO_COLLAPSE_ID, 550, None);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let lx = (lparam.0 & 0xFFFF) as i16 as i32;
            let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut win_rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut win_rc);

            let capture = {
                let mut portal = rc_clone.borrow_mut();
                let x_dip = portal.px_to_dip(lx);
                let y_dip = portal.px_to_dip(ly);
                let (w_dip, h_dip) = portal.current_dip_size();

                if !portal.config.collapsed && x_dip >= w_dip - 18.0 && y_dip >= h_dip - 18.0 {
                    portal.drag_mode = DragMode::ResizeWindow {
                        start_screen_x: pt.x,
                        start_screen_y: pt.y,
                        start_w: portal.config.width,
                        start_h: portal.config.expanded_height,
                    };
                    true
                } else if y_dip <= HEADER_HEIGHT {
                    if x_dip >= w_dip - 42.0 {
                        let next = !portal.config.collapsed;
                        portal.set_collapsed(next);
                        false
                    } else {
                        portal.drag_mode = DragMode::MoveWindow {
                            start_screen_x: pt.x,
                            start_screen_y: pt.y,
                            win_x: win_rc.left,
                            win_y: win_rc.top,
                            moved: false,
                        };
                        true
                    }
                } else if let Some(idx) =
                    portal
                        .layout
                        .hit_test(x_dip, y_dip, portal.items.len(), portal.scroll_y)
                {
                    portal.drag_mode = DragMode::PendingItemDrag {
                        index: idx,
                        start_x: x_dip,
                        start_y: y_dip,
                    };
                    true
                } else {
                    false
                }
            };

            if capture {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let mut portal = rc_clone.borrow_mut();
            match portal.drag_mode {
                DragMode::MoveWindow { moved, .. } => {
                    portal.drag_mode = DragMode::None;
                    if moved {
                        portal.notify_config_dirty();
                    }
                }
                DragMode::ResizeWindow { .. } => {
                    portal.drag_mode = DragMode::None;
                    portal.notify_config_dirty();
                }
                DragMode::PendingItemDrag { .. } => {
                    portal.drag_mode = DragMode::None;
                }
                DragMode::None => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let lx = (lparam.0 & 0xFFFF) as i16 as i32;
            let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let launch_path = {
                let mut portal = rc_clone.borrow_mut();
                let x_dip = portal.px_to_dip(lx);
                let y_dip = portal.px_to_dip(ly);

                if y_dip <= HEADER_HEIGHT {
                    let next = !portal.config.collapsed;
                    portal.set_collapsed(next);
                    None
                } else {
                    portal
                        .layout
                        .hit_test(x_dip, y_dip, portal.items.len(), portal.scroll_y)
                        .and_then(|idx| portal.items.get(idx).map(|it| it.path.clone()))
                }
            };
            if let Some(path) = launch_path {
                shell_open_path(hwnd, &path);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (collapsed, auto_collapse, folder_path, controller_hwnd) = {
                let p = rc_clone.borrow();
                let fp = match &p.config.kind {
                    PortalKind::Folder { path } => Some(path.clone()),
                    PortalKind::Virtual { .. } => None,
                };
                (
                    p.config.collapsed,
                    p.config.auto_collapse,
                    fp,
                    p.controller_hwnd,
                )
            };

            let cmd = show_portal_menu(hwnd, collapsed, auto_collapse, folder_path.is_some());
            match cmd {
                Some(CTX_TOGGLE_COLLAPSE) => {
                    rc_clone.borrow_mut().set_collapsed(!collapsed);
                }
                Some(CTX_TOGGLE_AUTO_COLLAPSE) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.auto_collapse = !p.config.auto_collapse;
                    p.notify_config_dirty();
                    p.draw();
                }
                Some(CTX_OPEN_FOLDER) => {
                    if let Some(path) = folder_path {
                        shell_open_path(hwnd, &path);
                    }
                }
                Some(CTX_CHANGE_FOLDER) => {
                    if let Some(new_folder) = pick_folder_dialog(hwnd) {
                        let mut p = rc_clone.borrow_mut();
                        if let Some(name) = new_folder.file_name() {
                            p.config.title = name.to_string_lossy().into_owned();
                        }
                        p.config.kind = PortalKind::Folder { path: new_folder };
                        if let Some(r) = &mut p.renderer {
                            r.clear_icon_cache();
                        }
                        p.restart_watcher();
                        p.reload_items();
                        p.notify_config_dirty();
                    }
                }
                Some(CTX_CLOSE_PORTAL) => {
                    let _ = PostMessageW(
                        Some(controller_hwnd),
                        WM_APP_PORTAL_CLOSED,
                        WPARAM(hwnd.0 as usize),
                        LPARAM(0),
                    );
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as f32;
            let step = -(delta / 120.0) * 38.0;
            let mut portal = rc_clone.borrow_mut();
            let next = (portal.scroll_y + step).clamp(0.0, portal.layout.max_scroll);
            if (next - portal.scroll_y).abs() > 0.1 {
                portal.scroll_y = next;
                portal.draw();
            }
            LRESULT(0)
        }
        WM_APP_FS_CHANGED => {
            rc_clone.borrow_mut().reload_items();
            LRESULT(0)
        }
        WM_APP_DROP_PATHS => {
            let mode = wparam.0;
            let mut portal = rc_clone.borrow_mut();
            if mode == 0 || mode == 1 {
                portal.is_drag_over = mode == 1;
                if portal.is_drag_over && portal.config.collapsed {
                    portal.set_collapsed(false);
                }
                portal.draw();
            } else if (mode == 2 || mode == 3) && lparam.0 != 0 {
                portal.is_drag_over = false;
                let boxed = Box::from_raw(lparam.0 as *mut Vec<PathBuf>);
                portal.handle_drop_paths(*boxed, mode == 3);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let had_dt = {
                let mut portal = rc_clone.borrow_mut();
                portal.watcher = None;
                portal._drop_target.take().is_some()
            };
            if had_dt {
                let _ = RevokeDragDrop(hwnd);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl PortalWindow {
    fn set_collapsed_if_idle(&mut self) {
        if self.config.auto_collapse && !self.mouse_inside && !self.is_drag_over {
            self.set_collapsed(true);
        }
    }
}

unsafe fn show_portal_menu(
    hwnd: HWND,
    collapsed: bool,
    auto_collapse: bool,
    is_folder: bool,
) -> Option<u32> {
    let Ok(menu) = CreatePopupMenu() else {
        return None;
    };
    let add_item = |id: u32, text: &str, checked: Option<bool>| {
        let w = to_wide_null(text);
        let flags = match checked {
            Some(true) => MF_STRING | MF_CHECKED,
            Some(false) => MF_STRING | MF_UNCHECKED,
            None => MF_STRING,
        };
        let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(w.as_ptr()));
    };

    add_item(
        CTX_TOGGLE_COLLAPSE,
        if collapsed {
            "Expand Portal"
        } else {
            "Collapse Portal"
        },
        None,
    );
    add_item(
        CTX_TOGGLE_AUTO_COLLAPSE,
        "Auto-Collapse on Leave",
        Some(auto_collapse),
    );
    if is_folder {
        add_item(CTX_OPEN_FOLDER, "Open in Explorer", None);
        add_item(CTX_CHANGE_FOLDER, "Change Folder...", None);
    }
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    add_item(CTX_CLOSE_PORTAL, "Remove Portal", None);

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let cmd = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD,
        pt.x,
        pt.y,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    if cmd.0 > 0 {
        Some(cmd.0 as u32)
    } else {
        None
    }
}
