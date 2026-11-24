use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};

use crate::messages::to_wide_null;

pub struct IconCache {
    bitmaps: HashMap<PathBuf, ID2D1Bitmap1>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            bitmaps: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.bitmaps.clear();
    }

    pub fn get_or_load(
        &mut self,
        ctx: &ID2D1DeviceContext,
        path: &Path,
        icon_px: i32,
    ) -> Option<&ID2D1Bitmap1> {
        if !self.bitmaps.contains_key(path) {
            if let Some(bmp) = extract_shell_bitmap(ctx, path, icon_px) {
                self.bitmaps.insert(path.to_path_buf(), bmp);
            }
        }
        self.bitmaps.get(path)
    }
}

fn extract_shell_bitmap(
    ctx: &ID2D1DeviceContext,
    path: &Path,
    icon_px: i32,
) -> Option<ID2D1Bitmap1> {
    let wide = to_wide_null(&path.to_string_lossy());
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let size = SIZE {
            cx: icon_px,
            cy: icon_px,
        };
        let hbitmap = factory
            .GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let result = hbitmap_to_d2d(ctx, hbitmap);
        let _ = DeleteObject(hbitmap.into());
        result
    }
}

unsafe fn hbitmap_to_d2d(ctx: &ID2D1DeviceContext, hbitmap: HBITMAP) -> Option<ID2D1Bitmap1> {
    let mut bm = BITMAP::default();
    if GetObjectW(
        hbitmap.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    ) == 0
    {
        return None;
    }

    let width = bm.bmWidth.max(1);
    let height = bm.bmHeight.abs().max(1);

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let hdc = CreateCompatibleDC(None);
    if hdc.is_invalid() {
        return None;
    }

    let scanlines = GetDIBits(
        hdc,
        hbitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(hdc);

    if scanlines == 0 {
        return None;
    }

    // Ensure premultiplied alpha for Direct2D
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            px[0] = ((px[0] as u16 * a) / 255) as u8;
            px[1] = ((px[1] as u16 * a) / 255) as u8;
            px[2] = ((px[2] as u16 * a) / 255) as u8;
        }
    }

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

    let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
        width: width as u32,
        height: height as u32,
    };

    ctx.CreateBitmap(
        size,
        Some(pixels.as_ptr() as *const _),
        (width * 4) as u32,
        &props,
    )
    .ok()
}
