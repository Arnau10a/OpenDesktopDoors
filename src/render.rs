use crate::config::PortalTheme;
use crate::icons::IconCache;
use crate::messages::to_wide_null;
use crate::portal::{GridLayout, PortalItem, HEADER_HEIGHT};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1SolidColorBrush, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_ROUNDED_RECT, D2D1_UNIT_MODE_DIPS,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionRectangleClip, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

pub struct RenderState<'a> {
    pub title: &'a str,
    pub items: &'a [PortalItem],
    pub layout: &'a GridLayout,
    pub width: f32,
    pub height: f32,
    pub dpi: f32,
    pub scroll_y: f32,
    pub hovered_index: Option<usize>,
    pub collapsed: bool,
    pub auto_collapse: bool,
    pub is_drag_over: bool,
    pub is_virtual: bool,
    pub theme: &'a PortalTheme,
}

pub struct PortalRenderer {
    _d3d_device: ID3D11Device,
    _d2d_factory: ID2D1Factory1,
    _d2d_device: ID2D1Device,
    d2d_ctx: ID2D1DeviceContext,
    swap_chain: IDXGISwapChain1,
    dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    dcomp_visual: IDCompositionVisual,
    dcomp_clip: IDCompositionRectangleClip,
    target_bitmap: Option<ID2D1Bitmap1>,
    _dwrite: IDWriteFactory,
    title_fmt: IDWriteTextFormat,
    badge_fmt: IDWriteTextFormat,
    label_fmt: IDWriteTextFormat,
    empty_fmt: IDWriteTextFormat,
    icon_cache: IconCache,
    pixel_w: u32,
    pixel_h: u32,
    dpi: f32,
}

impl PortalRenderer {
    pub fn new(
        hwnd: HWND,
        pixel_w: u32,
        pixel_h: u32,
        dpi: f32,
        radius: f32,
    ) -> windows::core::Result<Self> {
        unsafe {
            let feature_levels = [
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_0,
            ];
            let mut d3d_device = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                None,
            )?;
            let d3d_device = d3d_device.unwrap();
            let dxgi_device: IDXGIDevice = d3d_device.cast()?;
            let dxgi_adapter = dxgi_device.GetAdapter()?;
            let dxgi_factory: IDXGIFactory2 = dxgi_adapter.GetParent()?;

            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let d2d_ctx = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            d2d_ctx.SetDpi(dpi, dpi);
            d2d_ctx.SetUnitMode(D2D1_UNIT_MODE_DIPS);

            let pw = pixel_w.max(1);
            let ph = pixel_h.max(1);
            let sc_desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: pw,
                Height: ph,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };

            let swap_chain =
                dxgi_factory.CreateSwapChainForComposition(&d3d_device, &sc_desc, None)?;

            let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let dcomp_target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
            let dcomp_visual = dcomp_device.CreateVisual()?;
            let dcomp_clip = dcomp_device.CreateRectangleClip()?;
            dcomp_visual.SetContent(&swap_chain)?;
            dcomp_visual.SetClip(&dcomp_clip)?;
            dcomp_target.SetRoot(&dcomp_visual)?;

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let font_name = to_wide_null("Segoe UI Variable Text");
            let locale = to_wide_null("en-US");

            let title_fmt = dwrite.CreateTextFormat(
                windows::core::PCWSTR(font_name.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.5,
                windows::core::PCWSTR(locale.as_ptr()),
            )?;
            title_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
            title_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            title_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let ellipsis = dwrite.CreateEllipsisTrimmingSign(&title_fmt)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            title_fmt.SetTrimming(&trimming, &ellipsis)?;

            let badge_fmt = dwrite.CreateTextFormat(
                windows::core::PCWSTR(font_name.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.5,
                windows::core::PCWSTR(locale.as_ptr()),
            )?;
            badge_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            badge_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

            let label_fmt = dwrite.CreateTextFormat(
                windows::core::PCWSTR(font_name.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_MEDIUM,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.5,
                windows::core::PCWSTR(locale.as_ptr()),
            )?;
            label_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            label_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            label_fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            let label_ellipsis = dwrite.CreateEllipsisTrimmingSign(&label_fmt)?;
            label_fmt.SetTrimming(&trimming, &label_ellipsis)?;

            let empty_fmt = dwrite.CreateTextFormat(
                windows::core::PCWSTR(font_name.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_NORMAL_VAL(),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
                windows::core::PCWSTR(locale.as_ptr()),
            )?;
            empty_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            empty_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

            let mut renderer = Self {
                _d3d_device: d3d_device,
                _d2d_factory: d2d_factory,
                _d2d_device: d2d_device,
                d2d_ctx,
                swap_chain,
                dcomp_device,
                _dcomp_target: dcomp_target,
                dcomp_visual,
                dcomp_clip,
                target_bitmap: None,
                _dwrite: dwrite,
                title_fmt,
                badge_fmt,
                label_fmt,
                empty_fmt,
                icon_cache: IconCache::new(),
                pixel_w: pw,
                pixel_h: ph,
                dpi,
            };

            renderer.bind_backbuffer()?;
            renderer.update_clip(pw as f32, ph as f32, radius * (dpi / 96.0))?;
            Ok(renderer)
        }
    }

    pub fn resize(
        &mut self,
        pixel_w: u32,
        pixel_h: u32,
        dpi: f32,
        radius: f32,
    ) -> windows::core::Result<()> {
        let pw = pixel_w.max(1);
        let ph = pixel_h.max(1);
        self.dpi = dpi;
        unsafe {
            self.d2d_ctx.SetDpi(dpi, dpi);
            if pw != self.pixel_w || ph != self.pixel_h {
                self.d2d_ctx.SetTarget(None);
                self.target_bitmap = None;
                self.swap_chain.ResizeBuffers(
                    2,
                    pw,
                    ph,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    Default::default(),
                )?;
                self.pixel_w = pw;
                self.pixel_h = ph;
                self.bind_backbuffer()?;
            }
            self.update_clip(pw as f32, ph as f32, radius * (dpi / 96.0))?;
        }
        Ok(())
    }

    pub fn clear_icon_cache(&mut self) {
        self.icon_cache.clear();
    }

    unsafe fn bind_backbuffer(&mut self) -> windows::core::Result<()> {
        let surface: IDXGISurface = self.swap_chain.GetBuffer(0)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: self.dpi,
            dpiY: self.dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        let bmp = self
            .d2d_ctx
            .CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.d2d_ctx.SetTarget(&bmp);
        self.target_bitmap = Some(bmp);
        Ok(())
    }

    unsafe fn update_clip(
        &self,
        w_px: f32,
        h_px: f32,
        radius_px: f32,
    ) -> windows::core::Result<()> {
        let r = radius_px.min(w_px * 0.5).min(h_px * 0.5);
        self.dcomp_clip.SetLeft2(0.0)?;
        self.dcomp_clip.SetTop2(0.0)?;
        self.dcomp_clip.SetRight2(w_px)?;
        self.dcomp_clip.SetBottom2(h_px)?;
        self.dcomp_clip.SetTopLeftRadiusX2(r)?;
        self.dcomp_clip.SetTopLeftRadiusY2(r)?;
        self.dcomp_clip.SetTopRightRadiusX2(r)?;
        self.dcomp_clip.SetTopRightRadiusY2(r)?;
        self.dcomp_clip.SetBottomLeftRadiusX2(r)?;
        self.dcomp_clip.SetBottomLeftRadiusY2(r)?;
        self.dcomp_clip.SetBottomRightRadiusX2(r)?;
        self.dcomp_clip.SetBottomRightRadiusY2(r)?;
        self.dcomp_visual.SetClip(&self.dcomp_clip)?;
        self.dcomp_device.Commit()?;
        Ok(())
    }

    pub fn render(&mut self, state: &RenderState) -> windows::core::Result<()> {
        self.draw_to_target(state)?;
        unsafe {
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
            self.dcomp_device.Commit()?;
        }
        Ok(())
    }

    fn draw_to_target(&mut self, state: &RenderState) -> windows::core::Result<()> {
        unsafe {
            let ctx = &self.d2d_ctx;
            ctx.BeginDraw();
            ctx.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));

            let r = state
                .theme
                .corner_radius
                .min(state.width * 0.5)
                .min(state.height * 0.5);

            let full_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: state.width,
                bottom: state.height,
            };
            let full_rounded = D2D1_ROUNDED_RECT {
                rect: full_rect,
                radiusX: r,
                radiusY: r,
            };

            // 1. iOS Frosted Glass tint overlay over native DWM Acrylic
            let glass_brush = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: state.theme.tint_r,
                    g: state.theme.tint_g,
                    b: state.theme.tint_b,
                    a: state.theme.tint_a,
                },
                None,
            )?;
            ctx.FillRoundedRectangle(&full_rounded, &glass_brush);

            // 2. Subtle top header sheen
            let header_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: state.width,
                bottom: HEADER_HEIGHT.min(state.height),
            };
            let sheen_brush = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.06,
                },
                None,
            )?;
            ctx.FillRectangle(&header_rect, &sheen_brush);

            // 3. Header divider (only when expanded)
            if state.height > HEADER_HEIGHT + 6.0 {
                let div_brush = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.11,
                    },
                    None,
                )?;
                ctx.DrawLine(
                    windows_numerics::Vector2 {
                        X: 16.0,
                        Y: HEADER_HEIGHT,
                    },
                    windows_numerics::Vector2 {
                        X: state.width - 16.0,
                        Y: HEADER_HEIGHT,
                    },
                    &div_brush,
                    1.0,
                    None,
                );
            }

            // 4. Header content: indicator dot + title + count/chevron
            let dot_color = if state.auto_collapse {
                D2D1_COLOR_F {
                    r: 0.35,
                    g: 0.78,
                    b: 0.98,
                    a: 0.95,
                }
            } else if state.is_virtual {
                D2D1_COLOR_F {
                    r: 0.68,
                    g: 0.56,
                    b: 0.98,
                    a: 0.95,
                }
            } else {
                D2D1_COLOR_F {
                    r: 0.42,
                    g: 0.88,
                    b: 0.62,
                    a: 0.95,
                }
            };
            let dot_brush = ctx.CreateSolidColorBrush(&dot_color, None)?;
            ctx.FillEllipse(
                &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 {
                        X: 20.0,
                        Y: HEADER_HEIGHT * 0.5,
                    },
                    radiusX: 4.5,
                    radiusY: 4.5,
                },
                &dot_brush,
            );

            let text_white = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.96,
                    g: 0.97,
                    b: 0.99,
                    a: 0.96,
                },
                None,
            )?;
            let text_muted = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.82,
                    g: 0.85,
                    b: 0.90,
                    a: 0.65,
                },
                None,
            )?;

            let title_w = to_wide_null(state.title);
            let title_rect = D2D_RECT_F {
                left: 32.0,
                top: 0.0,
                right: (state.width - 78.0).max(40.0),
                bottom: HEADER_HEIGHT,
            };
            ctx.DrawText(
                &title_w[..title_w.len() - 1],
                &self.title_fmt,
                &title_rect,
                &text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            let chevron = if state.collapsed { "▾" } else { "▴" };
            let badge_str = format!("{}  {}", state.items.len(), chevron);
            let badge_w = to_wide_null(&badge_str);
            let badge_rect = D2D_RECT_F {
                left: (state.width - 76.0).max(32.0),
                top: 0.0,
                right: state.width - 16.0,
                bottom: HEADER_HEIGHT,
            };
            ctx.DrawText(
                &badge_w[..badge_w.len() - 1],
                &self.badge_fmt,
                &badge_rect,
                &text_muted,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            // 5. Items grid (clipped below header)
            if state.height > HEADER_HEIGHT + 8.0 {
                let body_clip = D2D_RECT_F {
                    left: 4.0,
                    top: HEADER_HEIGHT + 1.0,
                    right: state.width - 4.0,
                    bottom: state.height - 4.0,
                };
                ctx.PushAxisAlignedClip(
                    &body_clip,
                    windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );

                if state.items.is_empty() {
                    let msg = to_wide_null("Drop files or folders here");
                    ctx.DrawText(
                        &msg[..msg.len() - 1],
                        &self.empty_fmt,
                        &body_clip,
                        &text_muted,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                        Default::default(),
                    );
                } else {
                    let hover_brush: ID2D1SolidColorBrush = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.14,
                        },
                        None,
                    )?;
                    let icon_size_px = ((42.0 * state.dpi) / 96.0).round() as i32;

                    for (idx, item) in state.items.iter().enumerate() {
                        let rect = state.layout.item_rect(idx, state.scroll_y);
                        if rect.bottom < HEADER_HEIGHT || rect.top > state.height {
                            continue;
                        }

                        if state.hovered_index == Some(idx) {
                            let hrect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F {
                                    left: rect.left,
                                    top: rect.top,
                                    right: rect.right,
                                    bottom: rect.bottom,
                                },
                                radiusX: 14.0,
                                radiusY: 14.0,
                            };
                            ctx.FillRoundedRectangle(&hrect, &hover_brush);
                        }

                        let icon_left = rect.left + (rect.right - rect.left - 42.0) * 0.5;
                        let icon_top = rect.top + 8.0;
                        let icon_rect = D2D_RECT_F {
                            left: icon_left,
                            top: icon_top,
                            right: icon_left + 42.0,
                            bottom: icon_top + 42.0,
                        };

                        if let Some(bmp) =
                            self.icon_cache.get_or_load(ctx, &item.path, icon_size_px)
                        {
                            ctx.DrawBitmap(
                                bmp,
                                Some(&icon_rect),
                                1.0,
                                windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                                None,
                                None,
                            );
                        }

                        let label_w = to_wide_null(&item.name);
                        let label_rect = D2D_RECT_F {
                            left: rect.left + 4.0,
                            top: icon_top + 46.0,
                            right: rect.right - 4.0,
                            bottom: rect.bottom - 2.0,
                        };
                        ctx.DrawText(
                            &label_w[..label_w.len() - 1],
                            &self.label_fmt,
                            &label_rect,
                            &text_white,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            Default::default(),
                        );
                    }
                }

                ctx.PopAxisAlignedClip();

                // Subtle iOS-style pill scrollbar when content overflows
                if state.layout.max_scroll > 0.0 && state.layout.total_content_height > 0.0 {
                    let track_top = HEADER_HEIGHT + 8.0;
                    let track_h = (state.height - track_top - 12.0).max(20.0);
                    let visible_h = (state.height - HEADER_HEIGHT).max(1.0);
                    let thumb_h = (track_h * (visible_h / state.layout.total_content_height))
                        .clamp(20.0, track_h);
                    let ratio = (state.scroll_y / state.layout.max_scroll).clamp(0.0, 1.0);
                    let thumb_top = track_top + (track_h - thumb_h) * ratio;
                    let sb_brush = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.28,
                        },
                        None,
                    )?;
                    let sb_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: state.width - 7.5,
                            top: thumb_top,
                            right: state.width - 4.0,
                            bottom: thumb_top + thumb_h,
                        },
                        radiusX: 1.75,
                        radiusY: 1.75,
                    };
                    ctx.FillRoundedRectangle(&sb_rect, &sb_brush);
                }
            }

            // 6. Specular glass rim + Drag-over accent glow
            let border_color = if state.is_drag_over {
                D2D1_COLOR_F {
                    r: 0.36,
                    g: 0.78,
                    b: 1.0,
                    a: 0.85,
                }
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.24,
                }
            };
            let border_width = if state.is_drag_over { 2.0 } else { 1.0 };
            let border_brush = ctx.CreateSolidColorBrush(&border_color, None)?;
            let inset_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 0.75,
                    top: 0.75,
                    right: (state.width - 0.75).max(1.0),
                    bottom: (state.height - 0.75).max(1.0),
                },
                radiusX: (r - 0.75).max(2.0),
                radiusY: (r - 0.75).max(2.0),
            };
            ctx.DrawRoundedRectangle(&inset_rect, &border_brush, border_width, None);

            // Resize grip indicator (bottom-right, when expanded)
            if state.height > HEADER_HEIGHT + 40.0 {
                let grip_brush = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.25,
                    },
                    None,
                )?;
                ctx.DrawLine(
                    windows_numerics::Vector2 {
                        X: state.width - 14.0,
                        Y: state.height - 6.0,
                    },
                    windows_numerics::Vector2 {
                        X: state.width - 6.0,
                        Y: state.height - 14.0,
                    },
                    &grip_brush,
                    1.2,
                    None,
                );
            }

            ctx.EndDraw(None, None)?;
            Ok(())
        }
    }
}

#[allow(non_snake_case)]
const fn DWRITE_FONT_WEIGHT_NORMAL_VAL() -> windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT
{
    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT(400)
}

pub fn rect_size(rc: &RECT) -> (u32, u32) {
    (
        (rc.right - rc.left).max(1) as u32,
        (rc.bottom - rc.top).max(1) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PortalConfig, PortalKind};
    use crate::messages::to_wide_null;
    use crate::portal::load_items;
    use windows::Win32::Graphics::Direct2D::{D2D1_MAPPED_RECT, D2D1_MAP_OPTIONS_READ};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Ole::OleInitialize;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WNDCLASSEXW,
        WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    unsafe extern "system" fn test_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    use windows::Win32::Foundation::{LPARAM, WPARAM};

    #[test]
    fn visual_render_smoke_and_capture() {
        unsafe {
            let _ = OleInitialize(None);
            let hinst: windows::Win32::Foundation::HINSTANCE =
                GetModuleHandleW(None).unwrap_or_default().into();
            let cls = to_wide_null("ODDVisualTestWin");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(test_wnd_proc),
                hInstance: hinst,
                lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE,
                windows::core::PCWSTR(cls.as_ptr()),
                windows::core::PCWSTR::null(),
                WS_POPUP | WS_CLIPCHILDREN,
                40,
                40,
                340,
                360,
                None,
                None,
                Some(hinst),
                None,
            )
            .expect("CreateWindowExW failed");

            let mut renderer = PortalRenderer::new(hwnd, 340, 360, 96.0, 24.0)
                .expect("PortalRenderer::new failed");

            let cfg = PortalConfig {
                id: "test".into(),
                title: "Downloads".into(),
                kind: PortalKind::Virtual {
                    items: vec![
                        crate::config::VirtualItem {
                            name: "Documents".into(),
                            target: std::env::temp_dir(),
                        },
                        crate::config::VirtualItem {
                            name: "Windows".into(),
                            target: std::path::PathBuf::from(r"C:\Windows"),
                        },
                        crate::config::VirtualItem {
                            name: "Notepad".into(),
                            target: std::path::PathBuf::from(r"C:\Windows\notepad.exe"),
                        },
                    ],
                },
                x: 40,
                y: 40,
                width: 340,
                expanded_height: 360,
                collapsed: false,
                auto_collapse: false,
                theme: PortalTheme::default(),
            };
            let items = load_items(&cfg.kind);
            let layout = GridLayout::compute(340.0, 360.0, items.len());
            let state = RenderState {
                title: &cfg.title,
                items: &items,
                layout: &layout,
                width: 340.0,
                height: 360.0,
                dpi: 96.0,
                scroll_y: 0.0,
                hovered_index: Some(0),
                collapsed: false,
                auto_collapse: false,
                is_drag_over: false,
                is_virtual: false,
                theme: &cfg.theme,
            };

            renderer
                .draw_to_target(&state)
                .expect("draw_to_target failed");

            // Read back target_bitmap via CPU-readable D2D bitmap before Present flips buffer 0
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CANNOT_DRAW
                    | windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS_CPU_READ,
                ..Default::default()
            };
            let cpu_bmp = renderer
                .d2d_ctx
                .CreateBitmap(
                    windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                        width: 340,
                        height: 360,
                    },
                    None,
                    0,
                    &props,
                )
                .expect("CreateBitmap cpu_read");
            let target = renderer.target_bitmap.as_ref().expect("target_bitmap");
            cpu_bmp
                .CopyFromBitmap(None, target, None)
                .expect("CopyFromBitmap");
            let mapped: D2D1_MAPPED_RECT = cpu_bmp.Map(D2D1_MAP_OPTIONS_READ).expect("Map");

            // Composite onto a vibrant desktop-like gradient and write BMP
            let mut bmp_bytes = Vec::with_capacity(54 + 340 * 360 * 4);
            let file_size = (54 + 340 * 360 * 4) as u32;
            bmp_bytes.extend_from_slice(b"BM");
            bmp_bytes.extend_from_slice(&file_size.to_le_bytes());
            bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
            bmp_bytes.extend_from_slice(&54u32.to_le_bytes());
            bmp_bytes.extend_from_slice(&40u32.to_le_bytes());
            bmp_bytes.extend_from_slice(&340i32.to_le_bytes());
            bmp_bytes.extend_from_slice(&(-360i32).to_le_bytes());
            bmp_bytes.extend_from_slice(&1u16.to_le_bytes());
            bmp_bytes.extend_from_slice(&32u16.to_le_bytes());
            bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
            bmp_bytes.extend_from_slice(&((340 * 360 * 4) as u32).to_le_bytes());
            bmp_bytes.extend_from_slice(&[0u8; 16]);

            let mut non_zero_alpha = 0usize;
            for y in 0..360usize {
                let row =
                    std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), 340 * 4);
                for x in 0..340usize {
                    let b = row[x * 4] as f32;
                    let g = row[x * 4 + 1] as f32;
                    let r = row[x * 4 + 2] as f32;
                    let a = row[x * 4 + 3];
                    if a > 0 {
                        non_zero_alpha += 1;
                    }
                    let af = a as f32 / 255.0;
                    // Simulated wallpaper behind Acrylic
                    let bg_r = 45.0 + (x as f32 / 340.0) * 90.0;
                    let bg_g = 90.0 + (y as f32 / 360.0) * 80.0;
                    let bg_b = 175.0 - (x as f32 / 340.0) * 40.0;
                    let out_b = (b + bg_b * (1.0 - af)).clamp(0.0, 255.0) as u8;
                    let out_g = (g + bg_g * (1.0 - af)).clamp(0.0, 255.0) as u8;
                    let out_r = (r + bg_r * (1.0 - af)).clamp(0.0, 255.0) as u8;
                    bmp_bytes.extend_from_slice(&[out_b, out_g, out_r, 255]);
                }
            }
            let _ = cpu_bmp.Unmap();
            let _ = DestroyWindow(hwnd);

            assert!(non_zero_alpha > 10_000, "rendered frame was transparent!");
            let out_dir = std::env::temp_dir().join("open_desktop_doors_test");
            let _ = std::fs::create_dir_all(&out_dir);
            std::fs::write(out_dir.join("portal_render.bmp"), &bmp_bytes).unwrap();
            let _ = std::fs::remove_dir_all(&out_dir);
            println!("RENDERED_OK non_zero_alpha={}", non_zero_alpha);
        }
    }
}
