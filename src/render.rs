use crate::config::PortalTheme;
use crate::covers::CoverCache;
use crate::icons::IconCache;
use crate::library::{format_last_played, format_playtime, GameEntry};
use crate::messages::to_wide_null;
use crate::portal::{GameGridLayout, HitTarget, COVER_HEIGHT, HEADER_HEIGHT};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_BRUSH_PROPERTIES1,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_UNIT_MODE_DIPS,
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
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows_numerics::{Matrix3x2, Vector2};

pub struct RenderState<'a> {
    pub title: &'a str,
    pub games: &'a [GameEntry],
    pub layout: &'a GameGridLayout,
    pub width: f32,
    pub height: f32,
    pub dpi: f32,
    pub scroll_y: f32,
    pub hovered: Option<HitTarget>,
    pub selected_index: Option<usize>,
    pub collapsed: bool,
    pub auto_collapse: bool,
    pub is_drag_over: bool,
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
    card_title_fmt: IDWriteTextFormat,
    card_meta_fmt: IDWriteTextFormat,
    pill_fmt: IDWriteTextFormat,
    hero_title_fmt: IDWriteTextFormat,
    play_btn_fmt: IDWriteTextFormat,
    empty_fmt: IDWriteTextFormat,
    cover_cache: CoverCache,
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
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };

            let make_fmt =
                |weight, size: f32, align, valign| -> windows::core::Result<IDWriteTextFormat> {
                    let fmt = dwrite.CreateTextFormat(
                        windows::core::PCWSTR(font_name.as_ptr()),
                        None,
                        weight,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size,
                        windows::core::PCWSTR(locale.as_ptr()),
                    )?;
                    fmt.SetTextAlignment(align)?;
                    fmt.SetParagraphAlignment(valign)?;
                    fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                    let ellipsis = dwrite.CreateEllipsisTrimmingSign(&fmt)?;
                    fmt.SetTrimming(&trimming, &ellipsis)?;
                    Ok(fmt)
                };

            let title_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                14.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let badge_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_MEDIUM,
                11.5,
                DWRITE_TEXT_ALIGNMENT_TRAILING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let card_title_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                11.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let card_meta_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_MEDIUM,
                10.0,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let pill_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                9.5,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let hero_title_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                16.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let play_btn_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                12.5,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let empty_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_MEDIUM,
                12.5,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;

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
                card_title_fmt,
                card_meta_fmt,
                pill_fmt,
                hero_title_fmt,
                play_btn_fmt,
                empty_fmt,
                cover_cache: CoverCache::new(),
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
            let ctx_owned = self.d2d_ctx.clone();
            let ctx = &ctx_owned;
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

            // 1. Playnite deep slate-navy glass tint over Win11 DWM Acrylic
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

            // 2. Playnite header gradient (#2C3A67 subtle top wash)
            let header_rect = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: state.width,
                bottom: HEADER_HEIGHT.min(state.height),
            };
            let header_brush = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.173,
                    g: 0.227,
                    b: 0.404,
                    a: 0.34,
                },
                None,
            )?;
            ctx.FillRectangle(&header_rect, &header_brush);

            if state.height > HEADER_HEIGHT + 6.0 {
                let div_brush = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.10,
                    },
                    None,
                )?;
                ctx.DrawLine(
                    Vector2 {
                        X: 0.0,
                        Y: HEADER_HEIGHT,
                    },
                    Vector2 {
                        X: state.width,
                        Y: HEADER_HEIGHT,
                    },
                    &div_brush,
                    1.0,
                    None,
                );
            }

            // 3. Header: Playnite accent bar + title + count/chevron
            let accent_blue = D2D1_COLOR_F {
                r: 0.22,
                g: 0.412,
                b: 0.996,
                a: 1.0,
            };
            let accent_brush = ctx.CreateSolidColorBrush(&accent_blue, None)?;
            let accent_pill = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 14.0,
                    top: 13.0,
                    right: 18.0,
                    bottom: 31.0,
                },
                radiusX: 2.0,
                radiusY: 2.0,
            };
            ctx.FillRoundedRectangle(&accent_pill, &accent_brush);

            let text_white = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.95,
                    g: 0.96,
                    b: 0.98,
                    a: 0.98,
                },
                None,
            )?;
            let text_muted = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.72,
                    g: 0.78,
                    b: 0.86,
                    a: 0.78,
                },
                None,
            )?;

            let title_w = to_wide_null(state.title);
            let title_rect = D2D_RECT_F {
                left: 26.0,
                top: 0.0,
                right: (state.width - 84.0).max(50.0),
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
            let auto_dot = if state.auto_collapse { "● " } else { "" };
            let badge_str = format!("{}{}  {}", auto_dot, state.games.len(), chevron);
            let badge_w = to_wide_null(&badge_str);
            let badge_rect = D2D_RECT_F {
                left: (state.width - 86.0).max(26.0),
                top: 0.0,
                right: state.width - 14.0,
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

            // 4. 2:3 Game Cover Grid
            if state.height > HEADER_HEIGHT + 10.0 {
                let grid_clip = D2D_RECT_F {
                    left: 2.0,
                    top: HEADER_HEIGHT + 1.0,
                    right: state.width - 2.0,
                    bottom: state.layout.grid_bottom.min(state.height - 2.0),
                };
                ctx.PushAxisAlignedClip(&grid_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                if state.games.is_empty() {
                    let msg = to_wide_null("No hay juegos aquí — arrastra un .exe o .lnk");
                    ctx.DrawText(
                        &msg[..msg.len() - 1],
                        &self.empty_fmt,
                        &grid_clip,
                        &text_muted,
                        D2D1_DRAW_TEXT_OPTIONS_CLIP,
                        Default::default(),
                    );
                } else {
                    let card_fallback_bg = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.11,
                            g: 0.15,
                            b: 0.22,
                            a: 0.92,
                        },
                        None,
                    )?;
                    let pill_bg = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.04,
                            g: 0.07,
                            b: 0.10,
                            a: 0.80,
                        },
                        None,
                    )?;
                    let hover_scrim = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.04,
                            g: 0.07,
                            b: 0.12,
                            a: 0.48,
                        },
                        None,
                    )?;
                    let fav_gold = ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 1.0,
                            g: 0.82,
                            b: 0.36,
                            a: 0.98,
                        },
                        None,
                    )?;

                    let icon_px = ((48.0 * state.dpi) / 96.0).round() as i32;

                    for (idx, game) in state.games.iter().enumerate() {
                        let card_r = state.layout.card_rect(idx, state.scroll_y);
                        if card_r.bottom < HEADER_HEIGHT || card_r.top > grid_clip.bottom {
                            continue;
                        }
                        let cov_r = state.layout.cover_rect(idx, state.scroll_y);
                        let cov_d2d = D2D_RECT_F {
                            left: cov_r.left,
                            top: cov_r.top,
                            right: cov_r.right,
                            bottom: cov_r.bottom,
                        };
                        let cov_rounded = D2D1_ROUNDED_RECT {
                            rect: cov_d2d,
                            radiusX: 6.0,
                            radiusY: 6.0,
                        };

                        // Draw cover image (center-cropped into rounded 2:3 rect) or fallback card
                        let drew_cover = if let Some(ref cpath) = game.cover_path {
                            if let Some(bmp) = self.cover_cache.get_or_load(ctx, cpath) {
                                draw_bitmap_cover_rounded(ctx, bmp, &cov_rounded);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if !drew_cover {
                            ctx.FillRoundedRectangle(&cov_rounded, &card_fallback_bg);
                            let icon_source =
                                game.icon_path.as_deref().or(game.install_dir.as_deref());
                            if let Some(ip) = icon_source {
                                if let Some(ibmp) = self.icon_cache.get_or_load(ctx, ip, icon_px) {
                                    let cx = cov_r.left + (cov_r.width() - 48.0) * 0.5;
                                    let cy = cov_r.top + (COVER_HEIGHT - 48.0) * 0.5;
                                    let ir = D2D_RECT_F {
                                        left: cx,
                                        top: cy,
                                        right: cx + 48.0,
                                        bottom: cy + 48.0,
                                    };
                                    ctx.DrawBitmap(
                                        ibmp,
                                        Some(&ir),
                                        1.0,
                                        D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                                        None,
                                        None,
                                    );
                                }
                            }
                        }

                        // Dim uninstalled games slightly
                        if !game.is_installed {
                            let dim_brush = ctx.CreateSolidColorBrush(
                                &D2D1_COLOR_F {
                                    r: 0.04,
                                    g: 0.06,
                                    b: 0.09,
                                    a: 0.45,
                                },
                                None,
                            )?;
                            ctx.FillRoundedRectangle(&cov_rounded, &dim_brush);
                        }

                        // Launcher pill badge (top-left)
                        let badge_txt = short_launcher(&game.launcher);
                        let l_pill = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: cov_r.left + 6.0,
                                top: cov_r.top + 6.0,
                                right: cov_r.left + 44.0,
                                bottom: cov_r.top + 22.0,
                            },
                            radiusX: 4.0,
                            radiusY: 4.0,
                        };
                        ctx.FillRoundedRectangle(&l_pill, &pill_bg);
                        let lw = to_wide_null(badge_txt);
                        ctx.DrawText(
                            &lw[..lw.len() - 1],
                            &self.pill_fmt,
                            &l_pill.rect,
                            &text_white,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            Default::default(),
                        );

                        // Favorite star (top-right)
                        if game.favorite {
                            let star_r = D2D_RECT_F {
                                left: cov_r.right - 22.0,
                                top: cov_r.top + 4.0,
                                right: cov_r.right - 4.0,
                                bottom: cov_r.top + 22.0,
                            };
                            let sw = to_wide_null("★");
                            ctx.DrawText(
                                &sw[..sw.len() - 1],
                                &self.pill_fmt,
                                &star_r,
                                &fav_gold,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                Default::default(),
                            );
                        }

                        let is_hovered = matches!(
                            state.hovered,
                            Some(HitTarget::Card(i)) | Some(HitTarget::CardPlayButton(i)) if i == idx
                        );
                        let is_selected = state.selected_index == Some(idx);

                        if is_hovered {
                            ctx.FillRoundedRectangle(&cov_rounded, &hover_scrim);

                            // Centered Playnite Play Circle
                            let cx = cov_r.left + cov_r.width() * 0.5;
                            let cy = cov_r.top + COVER_HEIGHT * 0.5;
                            let play_circle = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                                point: Vector2 { X: cx, Y: cy },
                                radiusX: 20.0,
                                radiusY: 20.0,
                            };
                            let play_hover = matches!(state.hovered, Some(HitTarget::CardPlayButton(i)) if i == idx);
                            let circle_color = if play_hover {
                                D2D1_COLOR_F {
                                    r: 0.30,
                                    g: 0.50,
                                    b: 1.0,
                                    a: 0.98,
                                }
                            } else {
                                D2D1_COLOR_F {
                                    r: 0.22,
                                    g: 0.412,
                                    b: 0.996,
                                    a: 0.90,
                                }
                            };
                            let circle_brush = ctx.CreateSolidColorBrush(&circle_color, None)?;
                            ctx.FillEllipse(&play_circle, &circle_brush);

                            let play_glyph = to_wide_null("▶");
                            let glyph_rect = D2D_RECT_F {
                                left: cx - 18.0,
                                top: cy - 18.0,
                                right: cx + 20.0,
                                bottom: cy + 18.0,
                            };
                            ctx.DrawText(
                                &play_glyph[..play_glyph.len() - 1],
                                &self.play_btn_fmt,
                                &glyph_rect,
                                &text_white,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                Default::default(),
                            );
                        }

                        if is_hovered || is_selected {
                            ctx.DrawRoundedRectangle(&cov_rounded, &accent_brush, 2.0, None);
                        } else {
                            let subtle_card_rim = ctx.CreateSolidColorBrush(
                                &D2D1_COLOR_F {
                                    r: 1.0,
                                    g: 1.0,
                                    b: 1.0,
                                    a: 0.12,
                                },
                                None,
                            )?;
                            ctx.DrawRoundedRectangle(&cov_rounded, &subtle_card_rim, 0.8, None);
                        }

                        // Title & Playtime below cover
                        let t_rect = D2D_RECT_F {
                            left: card_r.left + 1.0,
                            top: cov_r.bottom + 5.0,
                            right: card_r.right - 1.0,
                            bottom: cov_r.bottom + 20.0,
                        };
                        let gw = to_wide_null(&game.name);
                        ctx.DrawText(
                            &gw[..gw.len() - 1],
                            &self.card_title_fmt,
                            &t_rect,
                            &text_white,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            Default::default(),
                        );

                        let meta_str = format_playtime(game.playtime_secs);
                        let m_rect = D2D_RECT_F {
                            left: card_r.left + 1.0,
                            top: cov_r.bottom + 20.0,
                            right: card_r.right - 1.0,
                            bottom: card_r.bottom,
                        };
                        let mw = to_wide_null(&meta_str);
                        ctx.DrawText(
                            &mw[..mw.len() - 1],
                            &self.card_meta_fmt,
                            &m_rect,
                            &text_muted,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            Default::default(),
                        );
                    }
                }

                ctx.PopAxisAlignedClip();

                // Subtle scrollbar
                if state.layout.max_scroll > 0.0 && state.layout.total_content_height > 0.0 {
                    let track_top = HEADER_HEIGHT + 8.0;
                    let track_h = (grid_clip.bottom - track_top - 8.0).max(20.0);
                    let visible_h = (grid_clip.bottom - HEADER_HEIGHT).max(1.0);
                    let thumb_h = (track_h * (visible_h / state.layout.total_content_height))
                        .clamp(22.0, track_h);
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

                // 5. Playnite Bottom Hero Details Drawer
                if let Some(dr) = state.layout.details_rect {
                    let sel_game = state
                        .selected_index
                        .and_then(|i| state.games.get(i))
                        .or_else(|| state.games.first());
                    if let Some(g) = sel_game {
                        self.draw_details_drawer(ctx, state, dr, g, &text_white, &text_muted)?;
                    }
                }
            }

            // 6. Outer glass border + drag-over highlight
            let border_color = if state.is_drag_over {
                accent_blue
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.18,
                }
            };
            let border_w = if state.is_drag_over { 2.0 } else { 1.0 };
            let border_brush = ctx.CreateSolidColorBrush(&border_color, None)?;
            let inset = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 0.5,
                    top: 0.5,
                    right: (state.width - 0.5).max(1.0),
                    bottom: (state.height - 0.5).max(1.0),
                },
                radiusX: (r - 0.5).max(1.0),
                radiusY: (r - 0.5).max(1.0),
            };
            ctx.DrawRoundedRectangle(&inset, &border_brush, border_w, None);

            // Resize grip
            if state.height > HEADER_HEIGHT + 40.0 {
                let grip_brush = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.28,
                    },
                    None,
                )?;
                ctx.DrawLine(
                    Vector2 {
                        X: state.width - 13.0,
                        Y: state.height - 5.0,
                    },
                    Vector2 {
                        X: state.width - 5.0,
                        Y: state.height - 13.0,
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

    unsafe fn draw_details_drawer(
        &mut self,
        ctx: &ID2D1DeviceContext,
        state: &RenderState,
        dr: crate::portal::RectF,
        game: &GameEntry,
        text_white: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        text_muted: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        let clip_rect = D2D_RECT_F {
            left: 1.0,
            top: dr.top,
            right: state.width - 1.0,
            bottom: dr.bottom - 1.0,
        };
        ctx.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

        // Hero artwork behind drawer
        let hero_src = game.hero_path.as_ref().or(game.cover_path.as_ref());
        if let Some(hp) = hero_src {
            if let Some(bmp) = self.cover_cache.get_or_load(ctx, hp) {
                let rr = D2D1_ROUNDED_RECT {
                    rect: clip_rect,
                    radiusX: 0.0,
                    radiusY: 0.0,
                };
                draw_bitmap_cover_rounded(ctx, bmp, &rr);
            }
        }

        // Playnite dark-slate gradient scrim over hero image
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: D2D1_COLOR_F {
                    r: 0.055,
                    g: 0.09,
                    b: 0.125,
                    a: 0.94,
                },
            },
            D2D1_GRADIENT_STOP {
                position: 0.65,
                color: D2D1_COLOR_F {
                    r: 0.063,
                    g: 0.106,
                    b: 0.145,
                    a: 0.84,
                },
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: D2D1_COLOR_F {
                    r: 0.063,
                    g: 0.106,
                    b: 0.145,
                    a: 0.68,
                },
            },
        ];
        let stop_col = ID2D1RenderTarget::CreateGradientStopCollection(
            ctx,
            &stops,
            windows::Win32::Graphics::Direct2D::D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;
        let grad_brush = ctx.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 {
                    X: clip_rect.left,
                    Y: clip_rect.top,
                },
                endPoint: Vector2 {
                    X: clip_rect.right,
                    Y: clip_rect.top,
                },
            },
            None,
            &stop_col,
        )?;
        ctx.FillRectangle(&clip_rect, &grad_brush);

        // Top separator line
        let div_brush = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.14,
            },
            None,
        )?;
        ctx.DrawLine(
            Vector2 { X: 0.0, Y: dr.top },
            Vector2 {
                X: state.width,
                Y: dr.top,
            },
            &div_brush,
            1.0,
            None,
        );

        // Title + Metadata + Playtime/LastPlayed
        let title_r = D2D_RECT_F {
            left: 16.0,
            top: dr.top + 12.0,
            right: state.width - 16.0,
            bottom: dr.top + 38.0,
        };
        let tw = to_wide_null(&game.name);
        ctx.DrawText(
            &tw[..tw.len() - 1],
            &self.hero_title_fmt,
            &title_r,
            text_white,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        let mut meta_parts = vec![game.launcher.clone()];
        if !game.genres.is_empty() {
            meta_parts.push(
                game.genres
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !game.developers.is_empty() {
            meta_parts.push(game.developers[0].clone());
        }
        if let Some(y) = game.release_year {
            meta_parts.push(y.to_string());
        }
        let meta_line = meta_parts.join("  •  ");
        let meta_r = D2D_RECT_F {
            left: 16.0,
            top: dr.top + 40.0,
            right: state.width - 16.0,
            bottom: dr.top + 58.0,
        };
        let mw = to_wide_null(&meta_line);
        ctx.DrawText(
            &mw[..mw.len() - 1],
            &self.card_title_fmt,
            &meta_r,
            text_muted,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        let score_part = game
            .community_score
            .map(|s| format!("  •  Score {s}%"))
            .unwrap_or_default();
        let stats_line = format!(
            "Tiempo: {}  •  Última vez: {}{}",
            format_playtime(game.playtime_secs),
            format_last_played(game.last_played_unix),
            score_part
        );
        let stats_r = D2D_RECT_F {
            left: 16.0,
            top: dr.top + 62.0,
            right: (state.width - 172.0).max(60.0),
            bottom: dr.bottom - 14.0,
        };
        let sw = to_wide_null(&stats_line);
        ctx.DrawText(
            &sw[..sw.len() - 1],
            &self.card_meta_fmt,
            &stats_r,
            text_muted,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        // Favorite toggle button
        if let Some(fav_r) = state.layout.details_fav_rect {
            let hover_fav = matches!(state.hovered, Some(HitTarget::DetailsFavButton));
            let fav_bg = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: if hover_fav { 0.22 } else { 0.12 },
                },
                None,
            )?;
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: fav_r.left,
                    top: fav_r.top,
                    right: fav_r.right,
                    bottom: fav_r.bottom,
                },
                radiusX: 8.0,
                radiusY: 8.0,
            };
            ctx.FillRoundedRectangle(&rr, &fav_bg);
            let star_brush = if game.favorite {
                ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.82,
                        b: 0.36,
                        a: 1.0,
                    },
                    None,
                )?
            } else {
                text_white.clone()
            };
            let fg = to_wide_null(if game.favorite { "★" } else { "☆" });
            ctx.DrawText(
                &fg[..fg.len() - 1],
                &self.play_btn_fmt,
                &rr.rect,
                &star_brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        // Playnite Blue Play button
        if let Some(play_r) = state.layout.details_play_rect {
            let hover_play = matches!(state.hovered, Some(HitTarget::DetailsPlayButton));
            let btn_bg = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: if hover_play { 0.29 } else { 0.22 },
                    g: if hover_play { 0.49 } else { 0.412 },
                    b: 0.996,
                    a: 1.0,
                },
                None,
            )?;
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: play_r.left,
                    top: play_r.top,
                    right: play_r.right,
                    bottom: play_r.bottom,
                },
                radiusX: 8.0,
                radiusY: 8.0,
            };
            ctx.FillRoundedRectangle(&rr, &btn_bg);
            let label = to_wide_null("▶  JUGAR");
            ctx.DrawText(
                &label[..label.len() - 1],
                &self.play_btn_fmt,
                &rr.rect,
                text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        ctx.PopAxisAlignedClip();
        Ok(())
    }
}

/// Center-crops and scales `bmp` inside `rounded` using `ID2D1BitmapBrush`.
unsafe fn draw_bitmap_cover_rounded(
    ctx: &ID2D1DeviceContext,
    bmp: &ID2D1Bitmap1,
    rounded: &D2D1_ROUNDED_RECT,
) {
    let sz = bmp.GetSize();
    if sz.width <= 0.0 || sz.height <= 0.0 {
        return;
    }
    let dst_w = (rounded.rect.right - rounded.rect.left).max(1.0);
    let dst_h = (rounded.rect.bottom - rounded.rect.top).max(1.0);
    let scale = (dst_w / sz.width).max(dst_h / sz.height);
    let scaled_w = sz.width * scale;
    let scaled_h = sz.height * scale;
    let tx = rounded.rect.left + (dst_w - scaled_w) * 0.5;
    let ty = rounded.rect.top + (dst_h - scaled_h) * 0.5;

    let props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
        extendModeX: D2D1_EXTEND_MODE_CLAMP,
        extendModeY: D2D1_EXTEND_MODE_CLAMP,
        interpolationMode: D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    };
    if let Ok(brush) = ctx.CreateBitmapBrush(bmp, Some(&props), None) {
        let xf = Matrix3x2 {
            M11: scale,
            M12: 0.0,
            M21: 0.0,
            M22: scale,
            M31: tx,
            M32: ty,
        };
        brush.SetTransform(&xf);
        ctx.FillRoundedRectangle(rounded, &brush);
    }
}

fn short_launcher(s: &str) -> &str {
    match s.to_ascii_lowercase().as_str() {
        "steam" => "STEAM",
        "epic" => "EPIC",
        "gog" => "GOG",
        "xbox" => "XBOX",
        "ubisoft" => "UBI",
        "ea" => "EA",
        _ => "PC",
    }
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
    use crate::messages::to_wide_null;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
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

    #[test]
    fn playnite_visual_render_smoke_and_capture() {
        unsafe {
            let _ = OleInitialize(None);
            let hinst: windows::Win32::Foundation::HINSTANCE =
                GetModuleHandleW(None).unwrap_or_default().into();
            let cls = to_wide_null("ODDPlayniteVisualTest");
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
                520,
                440,
                None,
                None,
                Some(hinst),
                None,
            )
            .expect("CreateWindowExW failed");

            let mut renderer =
                PortalRenderer::new(hwnd, 520, 440, 96.0, 8.0).expect("PortalRenderer::new failed");

            let games = vec![
                GameEntry {
                    id: "steam:367520".into(),
                    name: "Hollow Knight".into(),
                    launcher: "Steam".into(),
                    steam_appid: Some("367520".into()),
                    launch_target: "steam://rungameid/367520".into(),
                    install_dir: Some(std::path::PathBuf::from(r"C:\Windows")),
                    cover_path: None,
                    hero_path: None,
                    icon_path: Some(std::path::PathBuf::from(r"C:\Windows\notepad.exe")),
                    genres: vec!["Metroidvania".into(), "Indie".into()],
                    developers: vec!["Team Cherry".into()],
                    platforms: vec!["PC (Windows)".into()],
                    release_year: Some(2017),
                    community_score: Some(97),
                    playtime_secs: 153_000,
                    play_count: 24,
                    last_played_unix: 1_795_500_000,
                    added_unix: 0,
                    is_installed: true,
                    favorite: true,
                },
                GameEntry {
                    id: "epic:celeste".into(),
                    name: "Celeste".into(),
                    launcher: "Epic".into(),
                    steam_appid: None,
                    launch_target: String::new(),
                    install_dir: None,
                    cover_path: None,
                    hero_path: None,
                    icon_path: Some(std::path::PathBuf::from(r"C:\Windows\explorer.exe")),
                    genres: vec!["Platformer".into()],
                    developers: vec!["Maddy Makes Games".into()],
                    platforms: vec!["PC (Windows)".into()],
                    release_year: Some(2018),
                    community_score: Some(95),
                    playtime_secs: 34_200,
                    play_count: 8,
                    last_played_unix: 1_795_000_000,
                    added_unix: 0,
                    is_installed: true,
                    favorite: false,
                },
            ];

            let layout = GameGridLayout::compute(520.0, 440.0, games.len(), true);
            let theme = PortalTheme::default();
            let state = RenderState {
                title: "Jugados recientemente",
                games: &games,
                layout: &layout,
                width: 520.0,
                height: 440.0,
                dpi: 96.0,
                scroll_y: 0.0,
                hovered: Some(HitTarget::CardPlayButton(0)),
                selected_index: Some(0),
                collapsed: false,
                auto_collapse: false,
                is_drag_over: false,
                theme: &theme,
            };

            renderer
                .draw_to_target(&state)
                .expect("draw_to_target failed");

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
                        width: 520,
                        height: 440,
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

            let mut non_zero = 0usize;
            let cap_path = std::env::var_os("ODD_CAPTURE_BMP");
            let mut bmp_bytes = if cap_path.is_some() {
                let mut v = Vec::with_capacity(54 + 520 * 440 * 4);
                let file_size = (54 + 520 * 440 * 4) as u32;
                v.extend_from_slice(b"BM");
                v.extend_from_slice(&file_size.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
                v.extend_from_slice(&54u32.to_le_bytes());
                v.extend_from_slice(&40u32.to_le_bytes());
                v.extend_from_slice(&520i32.to_le_bytes());
                v.extend_from_slice(&(-440i32).to_le_bytes());
                v.extend_from_slice(&1u16.to_le_bytes());
                v.extend_from_slice(&32u16.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
                v.extend_from_slice(&((520 * 440 * 4) as u32).to_le_bytes());
                v.extend_from_slice(&[0u8; 16]);
                v
            } else {
                Vec::new()
            };

            for y in 0..440usize {
                let row =
                    std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), 520 * 4);
                for x in 0..520usize {
                    let b = row[x * 4] as f32;
                    let g = row[x * 4 + 1] as f32;
                    let r = row[x * 4 + 2] as f32;
                    let a = row[x * 4 + 3];
                    if a > 0 {
                        non_zero += 1;
                    }
                    if cap_path.is_some() {
                        let af = a as f32 / 255.0;
                        let bg_r = 35.0 + (x as f32 / 520.0) * 60.0;
                        let bg_g = 60.0 + (y as f32 / 440.0) * 55.0;
                        let bg_b = 125.0 - (x as f32 / 520.0) * 20.0;
                        bmp_bytes.push((b + bg_b * (1.0 - af)).clamp(0.0, 255.0) as u8);
                        bmp_bytes.push((g + bg_g * (1.0 - af)).clamp(0.0, 255.0) as u8);
                        bmp_bytes.push((r + bg_r * (1.0 - af)).clamp(0.0, 255.0) as u8);
                        bmp_bytes.push(255);
                    }
                }
            }
            let _ = cpu_bmp.Unmap();
            let _ = DestroyWindow(hwnd);
            if let Some(out) = cap_path {
                let _ = std::fs::write(out, &bmp_bytes);
            }
            assert!(non_zero > 50_000, "rendered frame was empty");
        }
    }
}
