use crate::config::{LayoutStyle, PortalConfig, PortalKind, SortBy};
use crate::desktop_pin::desktop_insert_after;
use crate::dnd::PortalDropTarget;
use crate::library::{GameEntry, GameLibrary};
use crate::messages::{
    to_wide_null, WM_APP_CONFIG_DIRTY, WM_APP_DROP_PATHS, WM_APP_LIBRARY_UPDATED,
    WM_APP_PORTAL_CLOSED,
};
use crate::portal::{GameGridLayout, HitTarget, HEADER_HEIGHT};
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
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_DISALLOW_PEEK,
    DWMWA_EXCLUDED_FROM_PEEK, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::System::Ole::{IDropTarget, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect, KillTimer, LoadCursorW,
    PostMessageW, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TrackPopupMenu, CS_DBLCLKS, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW, IDC_SIZENWSE, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SW_SHOWNOACTIVATE, SW_SHOWNORMAL, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN, WINDOWPOS,
    WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SIZE,
    WM_TIMER, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const PORTAL_CLASS_NAME: &str = "OpenDesktopDoorsPortal";
const TIMER_ANIM_ID: usize = 1;
const TIMER_AUTO_COLLAPSE_ID: usize = 2;
const COLLAPSED_HEIGHT: f32 = HEADER_HEIGHT;

const CTX_TOGGLE_COLLAPSE: u32 = 2001;
const CTX_TOGGLE_AUTO_COLLAPSE: u32 = 2002;
const CTX_TOGGLE_DETAILS: u32 = 2003;
const CTX_SORT_LAST_PLAYED: u32 = 2004;
const CTX_SORT_PLAYTIME: u32 = 2005;
const CTX_SORT_NAME: u32 = 2006;
const CTX_KIND_RECENT: u32 = 2007;
const CTX_KIND_FAVORITES: u32 = 2008;
const CTX_KIND_INSTALLED: u32 = 2009;
const CTX_KIND_STEAM: u32 = 2010;
const CTX_KIND_EPIC: u32 = 2011;
const CTX_KIND_STATS: u32 = 2012;
const CTX_STYLE_CARDS: u32 = 2013;
const CTX_STYLE_LIST: u32 = 2014;
const CTX_CLOSE_PORTAL: u32 = 2015;

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
}

pub struct PortalWindow {
    pub hwnd: HWND,
    pub config: PortalConfig,
    controller_hwnd: HWND,
    library: Rc<RefCell<GameLibrary>>,
    games: Vec<GameEntry>,
    layout: GameGridLayout,
    renderer: Option<PortalRenderer>,
    _drop_target: Option<IDropTarget>,
    height_spring: Spring,
    last_tick: Instant,
    animating: bool,
    scroll_y: f32,
    hovered: Option<HitTarget>,
    selected_index: Option<usize>,
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
        library: Rc<RefCell<GameLibrary>>,
        mut config: PortalConfig,
    ) -> Option<Rc<RefCell<Self>>> {
        config.clamp_dimensions();
        let initial_h = if config.collapsed {
            COLLAPSED_HEIGHT as i32
        } else {
            config.expanded_height
        };

        let games = library
            .borrow()
            .games_for_portal(&config.kind, config.sort_by);
        let is_stats = matches!(config.kind, PortalKind::Stats);
        let layout = GameGridLayout::compute(
            config.width as f32,
            config.expanded_height as f32,
            games.len(),
            config.show_details,
            config.layout_style,
            is_stats,
        );
        let selected_index = if games.is_empty() { None } else { Some(0) };

        let state = Rc::new(RefCell::new(Self {
            hwnd: HWND::default(),
            height_spring: Spring::new(initial_h as f32, 300.0, 32.0),
            config: config.clone(),
            controller_hwnd,
            library,
            games,
            layout,
            renderer: None,
            _drop_target: None,
            last_tick: Instant::now(),
            animating: false,
            scroll_y: 0.0,
            hovered: None,
            selected_index,
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

            let (pw, ph, dpi) = {
                let mut s = state.borrow_mut();
                s.hwnd = hwnd;
                s.dpi = GetDpiForWindow(hwnd) as f32;
                if s.dpi <= 0.0 {
                    s.dpi = 96.0;
                }
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let (pw, ph) = rect_size(&rc);
                (pw, ph, s.dpi)
            };

            setup_dwm_attributes(hwnd);

            {
                let mut s = state.borrow_mut();
                let radius = s.config.theme.corner_radius;
                match PortalRenderer::new(hwnd, pw, ph, dpi, radius) {
                    Ok(r) => s.renderer = Some(r),
                    Err(err) => log_diag(&format!("PortalRenderer::new failed: {err:?}")),
                }
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

    pub fn refresh_from_library(&mut self) {
        self.games = self
            .library
            .borrow()
            .games_for_portal(&self.config.kind, self.config.sort_by);
        if self.games.is_empty() {
            self.selected_index = None;
        } else if self
            .selected_index
            .map(|i| i >= self.games.len())
            .unwrap_or(true)
        {
            self.selected_index = Some(0);
        }
        self.recompute_layout();
        self.draw();
    }

    fn recompute_layout(&mut self) {
        let is_stats = matches!(self.config.kind, PortalKind::Stats);
        self.layout = GameGridLayout::compute(
            self.config.width as f32,
            self.config.expanded_height as f32,
            self.games.len(),
            self.config.show_details,
            self.config.layout_style,
            is_stats,
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

    fn set_collapsed_if_idle(&mut self) {
        if self.config.auto_collapse && !self.mouse_inside && !self.is_drag_over {
            self.set_collapsed(true);
        }
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
        let width_px = self.dip_to_px(self.config.width as f32).round().max(220.0) as i32;

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
        let total_library_secs = self.library.borrow().total_playtime_secs();
        if let Some(renderer) = &mut self.renderer {
            let state = RenderState {
                title: &self.config.title,
                games: &self.games,
                total_library_secs,
                layout: &self.layout,
                width: w_dip,
                height: h_dip,
                dpi: self.dpi,
                scroll_y: self.scroll_y,
                hovered: self.hovered,
                selected_index: self.selected_index,
                collapsed: self.config.collapsed,
                auto_collapse: self.config.auto_collapse,
                is_drag_over: self.is_drag_over,
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

    fn notify_library_updated(&self) {
        unsafe {
            let _ = PostMessageW(
                Some(self.controller_hwnd),
                WM_APP_LIBRARY_UPDATED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }

    fn launch_game_at(&mut self, idx: usize) {
        let Some(game) = self.games.get(idx).cloned() else {
            return;
        };
        if game.launch_target.is_empty() {
            if let Some(ref dir) = game.install_dir {
                shell_open_str(self.hwnd, &dir.to_string_lossy());
            }
            return;
        }

        self.library.borrow_mut().record_launch(&game.id);
        self.notify_library_updated();

        let target_path = PathBuf::from(&game.launch_target);
        if target_path.exists() && target_path.is_file() {
            // Spawn direct executable and track exact session duration
            let game_id = game.id.clone();
            let ctrl_raw = self.controller_hwnd.0 as isize;
            let library_ref = Rc::clone(&self.library);
            let _ = library_ref; // updated on main thread via message or direct record
            let started = Instant::now();
            if let Ok(mut child) = std::process::Command::new(&target_path)
                .current_dir(target_path.parent().unwrap_or(std::path::Path::new(".")))
                .spawn()
            {
                std::thread::spawn(move || {
                    let _ = child.wait();
                    let secs = started.elapsed().as_secs();
                    if secs > 0 {
                        let payload = Box::new((game_id, secs));
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(ctrl_raw as *mut _)),
                                WM_APP_LIBRARY_UPDATED,
                                WPARAM(1),
                                LPARAM(Box::into_raw(payload) as isize),
                            );
                        }
                    }
                });
                return;
            }
        }

        shell_open_str(self.hwnd, &game.launch_target);
    }

    fn handle_drop_paths(&mut self, paths: Vec<PathBuf>) {
        let mut added_ids = Vec::new();
        {
            let mut lib = self.library.borrow_mut();
            for p in paths {
                let entry = lib.add_custom_path(p);
                added_ids.push(entry.id);
            }
        }

        match &mut self.config.kind {
            PortalKind::Custom { game_ids } => {
                for id in added_ids {
                    if !game_ids.contains(&id) {
                        game_ids.push(id);
                    }
                }
                self.notify_config_dirty();
            }
            _ => {
                // Switch dropped non-custom portal? No, custom games appear automatically in Installed.
            }
        }
        self.notify_library_updated();
    }
}

pub fn apply_z_order_hwnd(hwnd: HWND) {
    unsafe {
        let insert_after = if is_bring_to_front() {
            HWND_TOPMOST
        } else {
            desktop_insert_after(hwnd)
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

fn log_diag(msg: &str) {
    let dir = crate::config::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let log_path = dir.join("runtime.log");
    let line = format!("{msg}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
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
    let yes: i32 = 1;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_EXCLUDED_FROM_PEEK,
        &yes as *const _ as *const c_void,
        std::mem::size_of::<i32>() as u32,
    );
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_DISALLOW_PEEK,
        &yes as *const _ as *const c_void,
        std::mem::size_of::<i32>() as u32,
    );
    // Keep client area transparent for DirectComposition 18px rounded clipping without
    // forcing Win11's 8px DWM backdrop box around our custom rounded corners.
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
}

fn shell_open_str(owner: HWND, target: &str) {
    let path_w = to_wide_null(target);
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

    if msg == WM_WINDOWPOSCHANGING {
        let wp = &mut *(lparam.0 as *mut WINDOWPOS);
        wp.hwndInsertAfter = if is_bring_to_front() {
            HWND_TOPMOST
        } else {
            desktop_insert_after(hwnd)
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
            if let Ok(mut p) = rc_clone.try_borrow_mut() {
                let dpi = p.dpi;
                let radius = p.config.theme.corner_radius;
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
                        let new_w = (start_w + dx_dip).clamp(280, 1280);
                        let new_h = (start_h + dy_dip).clamp(240, 1080);
                        portal.config.width = new_w;
                        portal.config.expanded_height = new_h;
                        portal.height_spring.snap(new_h as f32);
                        portal.recompute_layout();
                        let pw = portal.dip_to_px(new_w as f32).round() as i32;
                        let ph = portal.dip_to_px(new_h as f32).round() as i32;
                        MoveAction::ResizeWin(pw, ph)
                    }
                    DragMode::None => {
                        let new_hover = if portal.config.collapsed {
                            None
                        } else {
                            portal.layout.hit_test(
                                x_dip,
                                y_dip,
                                portal.games.len(),
                                portal.scroll_y,
                            )
                        };
                        if new_hover != portal.hovered {
                            portal.hovered = new_hover;
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
                MoveAction::None => {}
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let start_collapse_timer = {
                let mut portal = rc_clone.borrow_mut();
                portal.mouse_inside = false;
                if portal.hovered.is_some() {
                    portal.hovered = None;
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
                } else {
                    match portal
                        .layout
                        .hit_test(x_dip, y_dip, portal.games.len(), portal.scroll_y)
                    {
                        Some(HitTarget::CardPlayButton(idx))
                        | Some(HitTarget::StatsTopGame(idx)) => {
                            portal.selected_index = Some(idx);
                            portal.launch_game_at(idx);
                            portal.draw();
                        }
                        Some(HitTarget::CardFavButton(idx)) => {
                            if let Some(game_id) = portal.games.get(idx).map(|g| g.id.clone()) {
                                portal.library.borrow_mut().toggle_favorite(&game_id);
                                portal.notify_library_updated();
                            }
                        }
                        Some(HitTarget::Card(idx)) => {
                            portal.selected_index = Some(idx);
                            portal.draw();
                        }
                        Some(HitTarget::HeroPlayButton) => {
                            if let Some(idx) = portal.selected_index {
                                portal.launch_game_at(idx);
                            }
                        }
                        Some(HitTarget::HeroFavButton) => {
                            if let Some(idx) = portal.selected_index {
                                if let Some(game_id) = portal.games.get(idx).map(|g| g.id.clone()) {
                                    portal.library.borrow_mut().toggle_favorite(&game_id);
                                    portal.notify_library_updated();
                                }
                            }
                        }
                        None => {}
                    }
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
                DragMode::None => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            let lx = (lparam.0 & 0xFFFF) as i16 as i32;
            let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut portal = rc_clone.borrow_mut();
            let x_dip = portal.px_to_dip(lx);
            let y_dip = portal.px_to_dip(ly);

            if y_dip <= HEADER_HEIGHT {
                let next = !portal.config.collapsed;
                portal.set_collapsed(next);
            } else if let Some(HitTarget::Card(idx) | HitTarget::CardPlayButton(idx)) = portal
                .layout
                .hit_test(x_dip, y_dip, portal.games.len(), portal.scroll_y)
            {
                portal.selected_index = Some(idx);
                portal.launch_game_at(idx);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (
                collapsed,
                auto_collapse,
                show_details,
                sort_by,
                layout_style,
                kind,
                controller_hwnd,
            ) = {
                let p = rc_clone.borrow();
                (
                    p.config.collapsed,
                    p.config.auto_collapse,
                    p.config.show_details,
                    p.config.sort_by,
                    p.config.layout_style,
                    p.config.kind.clone(),
                    p.controller_hwnd,
                )
            };

            let cmd = show_portal_menu(
                hwnd,
                collapsed,
                auto_collapse,
                show_details,
                sort_by,
                layout_style,
                &kind,
            );
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
                Some(CTX_TOGGLE_DETAILS) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.show_details = !p.config.show_details;
                    p.recompute_layout();
                    p.notify_config_dirty();
                    p.draw();
                }
                Some(CTX_STYLE_CARDS) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.layout_style = LayoutStyle::Cards;
                    p.recompute_layout();
                    p.notify_config_dirty();
                    p.draw();
                }
                Some(CTX_STYLE_LIST) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.layout_style = LayoutStyle::CompactList;
                    p.recompute_layout();
                    p.notify_config_dirty();
                    p.draw();
                }
                Some(CTX_SORT_LAST_PLAYED) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.sort_by = SortBy::LastPlayed;
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_SORT_PLAYTIME) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.sort_by = SortBy::Playtime;
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_SORT_NAME) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.sort_by = SortBy::Name;
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_RECENT) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Recent;
                    p.config.title = "Jugados recientemente".into();
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_FAVORITES) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Favorites;
                    p.config.title = "Favoritos".into();
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_INSTALLED) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Installed;
                    p.config.title = "Instalados".into();
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_STEAM) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Launcher {
                        name: "Steam".into(),
                    };
                    p.config.title = "Steam".into();
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_EPIC) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Launcher {
                        name: "Epic".into(),
                    };
                    p.config.title = "Epic Games".into();
                    p.refresh_from_library();
                    p.notify_config_dirty();
                }
                Some(CTX_KIND_STATS) => {
                    let mut p = rc_clone.borrow_mut();
                    p.config.kind = PortalKind::Stats;
                    p.config.title = "Tus Estadísticas".into();
                    p.recompute_layout();
                    p.refresh_from_library();
                    p.notify_config_dirty();
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
            let step = -(delta / 120.0) * 48.0;
            let mut portal = rc_clone.borrow_mut();
            let next = (portal.scroll_y + step).clamp(0.0, portal.layout.max_scroll);
            if (next - portal.scroll_y).abs() > 0.1 {
                portal.scroll_y = next;
                portal.draw();
            }
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
                portal.handle_drop_paths(*boxed);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let had_dt = rc_clone.borrow_mut()._drop_target.take().is_some();
            if had_dt {
                let _ = RevokeDragDrop(hwnd);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_portal_menu(
    hwnd: HWND,
    collapsed: bool,
    auto_collapse: bool,
    show_details: bool,
    sort_by: SortBy,
    layout_style: LayoutStyle,
    kind: &PortalKind,
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
            "Expandir portal"
        } else {
            "Colapsar portal"
        },
        None,
    );
    add_item(
        CTX_TOGGLE_AUTO_COLLAPSE,
        "Auto-colapsar al salir",
        Some(auto_collapse),
    );
    add_item(
        CTX_TOGGLE_DETAILS,
        "Mostrar tarjeta destacada (Hero)",
        Some(show_details),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    add_item(
        CTX_STYLE_CARDS,
        "Vista: Tarjetas",
        Some(layout_style == LayoutStyle::Cards),
    );
    add_item(
        CTX_STYLE_LIST,
        "Vista: Lista compacta",
        Some(layout_style == LayoutStyle::CompactList),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    add_item(
        CTX_SORT_LAST_PLAYED,
        "Ordenar: Última partida",
        Some(sort_by == SortBy::LastPlayed),
    );
    add_item(
        CTX_SORT_PLAYTIME,
        "Ordenar: Horas jugadas",
        Some(sort_by == SortBy::Playtime),
    );
    add_item(
        CTX_SORT_NAME,
        "Ordenar: Nombre (A-Z)",
        Some(sort_by == SortBy::Name),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    add_item(
        CTX_KIND_RECENT,
        "Filtro: Recientes",
        Some(matches!(kind, PortalKind::Recent)),
    );
    add_item(
        CTX_KIND_FAVORITES,
        "Filtro: Favoritos",
        Some(matches!(kind, PortalKind::Favorites)),
    );
    add_item(
        CTX_KIND_INSTALLED,
        "Filtro: Instalados",
        Some(matches!(kind, PortalKind::Installed)),
    );
    add_item(
        CTX_KIND_STEAM,
        "Filtro: Steam",
        Some(matches!(kind, PortalKind::Launcher { name } if name.eq_ignore_ascii_case("Steam"))),
    );
    add_item(
        CTX_KIND_EPIC,
        "Filtro: Epic Games",
        Some(matches!(kind, PortalKind::Launcher { name } if name.eq_ignore_ascii_case("Epic"))),
    );
    add_item(
        CTX_KIND_STATS,
        "Filtro: Estadísticas",
        Some(matches!(kind, PortalKind::Stats)),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    add_item(CTX_CLOSE_PORTAL, "Eliminar portal", None);

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
