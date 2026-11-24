use crate::config::config_dir;
use crate::library::GameEntry;
use crate::messages::{to_wide_null, WM_APP_COVER_READY};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub fn covers_cache_dir() -> PathBuf {
    config_dir().join("covers")
}

pub struct CoverCache {
    wic: Option<IWICImagingFactory>,
    bitmaps: HashMap<PathBuf, ID2D1Bitmap1>,
    failed: HashSet<PathBuf>,
}

impl CoverCache {
    pub fn new() -> Self {
        let wic =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
        Self {
            wic,
            bitmaps: HashMap::new(),
            failed: HashSet::new(),
        }
    }

    pub fn get_or_load(&mut self, ctx: &ID2D1DeviceContext, path: &Path) -> Option<&ID2D1Bitmap1> {
        if self.failed.contains(path) {
            return None;
        }
        if !self.bitmaps.contains_key(path) {
            if let Some(bmp) = self.decode_to_d2d(ctx, path) {
                self.bitmaps.insert(path.to_path_buf(), bmp);
            } else {
                self.failed.insert(path.to_path_buf());
                return None;
            }
        }
        self.bitmaps.get(path)
    }

    fn decode_to_d2d(&self, ctx: &ID2D1DeviceContext, path: &Path) -> Option<ID2D1Bitmap1> {
        let wic = self.wic.as_ref()?;
        let wide = to_wide_null(&path.to_string_lossy());
        unsafe {
            let decoder = wic
                .CreateDecoderFromFilename(
                    PCWSTR(wide.as_ptr()),
                    None,
                    windows::Win32::Foundation::GENERIC_READ,
                    WICDecodeMetadataCacheOnLoad,
                )
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let converter = wic.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;

            let mut w = 0u32;
            let mut h = 0u32;
            converter.GetSize(&mut w, &mut h).ok()?;
            if w == 0 || h == 0 {
                return None;
            }

            let stride = w * 4;
            let mut buf = vec![0u8; (stride * h) as usize];
            converter
                .CopyPixels(std::ptr::null(), stride, &mut buf)
                .ok()?;

            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                ..Default::default()
            };

            ctx.CreateBitmap(
                D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                Some(buf.as_ptr() as *const _),
                stride,
                &props,
            )
            .ok()
        }
    }
}

pub type PendingCovers = Arc<Mutex<Vec<(String, Option<PathBuf>, Option<PathBuf>)>>>;

pub fn spawn_missing_cover_fetcher(
    games: &[GameEntry],
    controller_hwnd: HWND,
    pending: PendingCovers,
) {
    let jobs: Vec<(String, String, bool, bool)> = games
        .iter()
        .filter_map(|g| {
            let appid = g.steam_appid.as_ref()?;
            let need_cover = g.cover_path.is_none();
            let need_hero = g.hero_path.is_none();
            if need_cover || need_hero {
                Some((g.id.clone(), appid.clone(), need_cover, need_hero))
            } else {
                None
            }
        })
        .take(24)
        .collect();

    if jobs.is_empty() {
        return;
    }

    let hwnd_raw = controller_hwnd.0 as isize;
    std::thread::spawn(move || {
        let dir = covers_cache_dir();
        let _ = fs::create_dir_all(&dir);

        for (game_id, appid, need_cover, need_hero) in jobs {
            let mut got_cover = None;
            let mut got_hero = None;

            if need_cover {
                let dst = dir.join(format!("{appid}_cover.jpg"));
                if dst.exists() {
                    got_cover = Some(dst);
                } else {
                    let paths = [
                        format!("/store_item_assets/steam/apps/{appid}/library_600x900_2x.jpg"),
                        format!("/store_item_assets/steam/apps/{appid}/library_600x900.jpg"),
                        format!("/steam/apps/{appid}/library_600x900.jpg"),
                    ];
                    for p in paths {
                        if let Some(bytes) = winhttp_get("shared.fastly.steamstatic.com", &p) {
                            if fs::write(&dst, bytes).is_ok() {
                                got_cover = Some(dst);
                                break;
                            }
                        }
                    }
                }
            }

            if need_hero {
                let dst_h = dir.join(format!("{appid}_hero.jpg"));
                if dst_h.exists() {
                    got_hero = Some(dst_h);
                } else {
                    let paths = [
                        format!("/store_item_assets/steam/apps/{appid}/library_hero.jpg"),
                        format!("/steam/apps/{appid}/library_hero.jpg"),
                    ];
                    for p in paths {
                        if let Some(bytes) = winhttp_get("shared.fastly.steamstatic.com", &p) {
                            if fs::write(&dst_h, bytes).is_ok() {
                                got_hero = Some(dst_h);
                                break;
                            }
                        }
                    }
                }
            }

            if got_cover.is_some() || got_hero.is_some() {
                if let Ok(mut q) = pending.lock() {
                    q.push((game_id, got_cover, got_hero));
                }
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd_raw as *mut _)),
                        WM_APP_COVER_READY,
                        Default::default(),
                        Default::default(),
                    );
                }
            }
        }
    });
}

fn winhttp_get(host: &str, path: &str) -> Option<Vec<u8>> {
    let agent_w = to_wide_null("OpenDesktopDoors/0.2");
    let host_w = to_wide_null(host);
    let path_w = to_wide_null(path);
    let verb_w = to_wide_null("GET");

    unsafe {
        let session = WinHttpOpen(
            PCWSTR(agent_w.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return None;
        }
        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null_mut(),
            WINHTTP_OPEN_REQUEST_FLAGS(0x0080_0000),
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return None;
        }

        let ok = WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
            && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok();

        let mut body = None;
        if ok {
            let mut status: u32 = 0;
            let mut len = std::mem::size_of::<u32>() as u32;
            let _ = WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut _),
                &mut len,
                std::ptr::null_mut(),
            );
            if status == 200 {
                let mut out = Vec::new();
                loop {
                    let mut avail = 0u32;
                    if WinHttpQueryDataAvailable(request, &mut avail).is_err() || avail == 0 {
                        break;
                    }
                    let mut chunk = vec![0u8; avail as usize];
                    let mut read = 0u32;
                    if WinHttpReadData(request, chunk.as_mut_ptr() as *mut _, avail, &mut read)
                        .is_err()
                        || read == 0
                    {
                        break;
                    }
                    out.extend_from_slice(&chunk[..read as usize]);
                }
                if !out.is_empty() {
                    body = Some(out);
                }
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
        body
    }
}
