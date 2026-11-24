use crate::config::{LayoutStyle, PortalTheme};
use crate::covers::CoverCache;
use crate::icons::IconCache;
use crate::library::{format_last_played, format_playtime, GameEntry};
use crate::messages::to_wide_null;
use crate::portal::{GameGridLayout, HitTarget, RectF, HEADER_HEIGHT};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_BRUSH_PROPERTIES1,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_UNIT_MODE_DIPS,
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
    pub total_library_secs: u64,
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
    stats_big_fmt: IDWriteTextFormat,
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
                DWRITE_FONT_WEIGHT_BOLD,
                12.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let card_meta_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_MEDIUM,
                10.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let pill_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                10.0,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let hero_title_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                18.5,
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            )?;
            let play_btn_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                12.0,
                DWRITE_TEXT_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            )?;
            let stats_big_fmt = make_fmt(
                DWRITE_FONT_WEIGHT_BOLD,
                20.0,
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
                stats_big_fmt,
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

            // 1. Warm Burgundy-Mahogany gradient base (#220F12 -> #32181E)
            let bg_stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: D2D1_COLOR_F {
                        r: state.theme.tint_r * 0.92,
                        g: state.theme.tint_g * 0.90,
                        b: state.theme.tint_b * 0.92,
                        a: state.theme.tint_a,
                    },
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: D2D1_COLOR_F {
                        r: (state.theme.tint_r * 1.32).min(0.4),
                        g: (state.theme.tint_g * 1.25).min(0.25),
                        b: (state.theme.tint_b * 1.30).min(0.3),
                        a: state.theme.tint_a,
                    },
                },
            ];
            let bg_col = ID2D1RenderTarget::CreateGradientStopCollection(
                ctx,
                &bg_stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            )?;
            let bg_brush = ctx.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 { X: 0.0, Y: 0.0 },
                    endPoint: Vector2 {
                        X: state.width,
                        Y: state.height,
                    },
                },
                None,
                &bg_col,
            )?;
            ctx.FillRoundedRectangle(&full_rounded, &bg_brush);

            // Common theme brushes
            let coral_accent = D2D1_COLOR_F {
                r: 0.89,
                g: 0.32,
                b: 0.32,
                a: 1.0,
            };
            let coral_brush = ctx.CreateSolidColorBrush(&coral_accent, None)?;
            let text_white = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.98,
                    g: 0.95,
                    b: 0.95,
                    a: 0.99,
                },
                None,
            )?;
            let text_muted = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.82,
                    g: 0.69,
                    b: 0.71,
                    a: 0.78,
                },
                None,
            )?;
            let dark_plum_brush = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.125,
                    g: 0.055,
                    b: 0.067,
                    a: 0.82,
                },
                None,
            )?;

            // 2. Header: Coral rounded square emblem + Title + dark pill counter + chevron
            let emblem = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 16.0,
                    top: 13.0,
                    right: 38.0,
                    bottom: 35.0,
                },
                radiusX: 7.0,
                radiusY: 7.0,
            };
            ctx.FillRoundedRectangle(&emblem, &coral_brush);
            let emblem_glyph = to_wide_null("▶");
            ctx.DrawText(
                &emblem_glyph[..emblem_glyph.len() - 1],
                &self.pill_fmt,
                &emblem.rect,
                &text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            let title_w = to_wide_null(state.title);
            let title_rect = D2D_RECT_F {
                left: 48.0,
                top: 0.0,
                right: (state.width - 94.0).max(60.0),
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

            let pill_count_r = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: (state.width - 88.0).max(48.0),
                    top: 12.0,
                    right: state.width - 14.0,
                    bottom: 36.0,
                },
                radiusX: 12.0,
                radiusY: 12.0,
            };
            ctx.FillRoundedRectangle(&pill_count_r, &dark_plum_brush);

            let chevron = if state.collapsed { "▾" } else { "▴" };
            let auto_dot = if state.auto_collapse { "● " } else { "" };
            let badge_str = format!("{}{}  {}", auto_dot, state.games.len(), chevron);
            let badge_w = to_wide_null(&badge_str);
            let badge_rect = D2D_RECT_F {
                left: pill_count_r.rect.left + 4.0,
                top: 0.0,
                right: pill_count_r.rect.right - 10.0,
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

            // 3. Body content (Stats ring OR Hero + Cards/List)
            if state.height > HEADER_HEIGHT + 12.0 {
                if state.layout.is_stats {
                    self.draw_stats_portal(ctx, state, &text_white, &text_muted, &dark_plum_brush)?;
                } else {
                    // Featured Coral-Crimson Hero Banner (like Valorant in reference)
                    if let Some(hr) = state.layout.hero_rect {
                        let sel_game = state
                            .selected_index
                            .and_then(|i| state.games.get(i))
                            .or_else(|| state.games.first());
                        if let Some(g) = sel_game {
                            self.draw_featured_hero_card(
                                ctx,
                                state,
                                hr,
                                g,
                                &text_white,
                                &coral_brush,
                            )?;
                        }
                    }

                    // Games Grid / Compact List
                    let clip_r = D2D_RECT_F {
                        left: 2.0,
                        top: state.layout.grid_top - 2.0,
                        right: state.width - 2.0,
                        bottom: state.height - 6.0,
                    };
                    ctx.PushAxisAlignedClip(&clip_r, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                    if state.games.is_empty() {
                        let msg = to_wide_null("No hay juegos — arrastra .exe o .lnk aquí");
                        ctx.DrawText(
                            &msg[..msg.len() - 1],
                            &self.empty_fmt,
                            &clip_r,
                            &text_muted,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            Default::default(),
                        );
                    } else {
                        match state.layout.style {
                            LayoutStyle::Cards => {
                                self.draw_cards_grid(
                                    ctx,
                                    state,
                                    &clip_r,
                                    &text_white,
                                    &text_muted,
                                    &coral_brush,
                                )?;
                            }
                            LayoutStyle::CompactList => {
                                self.draw_compact_list(
                                    ctx,
                                    state,
                                    &clip_r,
                                    &text_white,
                                    &text_muted,
                                    &coral_brush,
                                )?;
                            }
                        }
                    }

                    ctx.PopAxisAlignedClip();

                    // Subtle scrollbar
                    if state.layout.max_scroll > 0.0 && state.layout.total_content_height > 0.0 {
                        let track_top = state.layout.grid_top + 4.0;
                        let track_h = (state.height - track_top - 14.0).max(20.0);
                        let visible_h = (state.height - state.layout.grid_top).max(1.0);
                        let thumb_h = (track_h * (visible_h / state.layout.total_content_height))
                            .clamp(22.0, track_h);
                        let ratio = (state.scroll_y / state.layout.max_scroll).clamp(0.0, 1.0);
                        let thumb_top = track_top + (track_h - thumb_h) * ratio;
                        let sb_brush = ctx.CreateSolidColorBrush(
                            &D2D1_COLOR_F {
                                r: 1.0,
                                g: 0.75,
                                b: 0.75,
                                a: 0.25,
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
            }

            // 4. Outer warm specular border + drag-over coral glow
            let border_color = if state.is_drag_over {
                coral_accent
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.85,
                    b: 0.85,
                    a: 0.14,
                }
            };
            let border_w = if state.is_drag_over { 2.0 } else { 1.0 };
            let border_brush = ctx.CreateSolidColorBrush(&border_color, None)?;
            let inset = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: 0.75,
                    top: 0.75,
                    right: (state.width - 0.75).max(1.0),
                    bottom: (state.height - 0.75).max(1.0),
                },
                radiusX: (r - 0.75).max(2.0),
                radiusY: (r - 0.75).max(2.0),
            };
            ctx.DrawRoundedRectangle(&inset, &border_brush, border_w, None);

            // Resize grip
            if state.height > HEADER_HEIGHT + 40.0 {
                let grip_brush = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.82,
                        b: 0.82,
                        a: 0.24,
                    },
                    None,
                )?;
                ctx.DrawLine(
                    Vector2 {
                        X: state.width - 14.0,
                        Y: state.height - 6.0,
                    },
                    Vector2 {
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

    unsafe fn draw_featured_hero_card(
        &mut self,
        ctx: &ID2D1DeviceContext,
        state: &RenderState,
        hr: RectF,
        game: &GameEntry,
        text_white: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        coral_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        let hero_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: hr.left,
                top: hr.top,
                right: hr.right,
                bottom: hr.bottom,
            },
            radiusX: 16.0,
            radiusY: 16.0,
        };

        // 1. Coral-Crimson gradient base (like Valorant hero card)
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: D2D1_COLOR_F {
                    r: 0.91,
                    g: 0.34,
                    b: 0.34,
                    a: 0.98,
                },
            },
            D2D1_GRADIENT_STOP {
                position: 0.55,
                color: D2D1_COLOR_F {
                    r: 0.72,
                    g: 0.22,
                    b: 0.29,
                    a: 0.95,
                },
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: D2D1_COLOR_F {
                    r: 0.34,
                    g: 0.12,
                    b: 0.20,
                    a: 0.95,
                },
            },
        ];
        let sc = ID2D1RenderTarget::CreateGradientStopCollection(
            ctx,
            &stops,
            D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;
        let gbrush = ctx.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 {
                    X: hr.left,
                    Y: hr.top,
                },
                endPoint: Vector2 {
                    X: hr.right,
                    Y: hr.bottom,
                },
            },
            None,
            &sc,
        )?;
        ctx.FillRoundedRectangle(&hero_rr, &gbrush);

        // 2. Right-side blended Hero artwork
        let art_src = game.hero_path.as_ref().or(game.cover_path.as_ref());
        if let Some(hp) = art_src {
            if let Some(bmp) = self.cover_cache.get_or_load(ctx, hp) {
                let art_rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: hr.left + hr.width() * 0.42,
                        top: hr.top,
                        right: hr.right,
                        bottom: hr.bottom,
                    },
                    radiusX: 16.0,
                    radiusY: 16.0,
                };
                draw_bitmap_cover_rounded(ctx, bmp, &art_rr);

                // Smooth horizontal fade from coral-crimson into the artwork
                let fade_stops = [
                    D2D1_GRADIENT_STOP {
                        position: 0.0,
                        color: D2D1_COLOR_F {
                            r: 0.78,
                            g: 0.25,
                            b: 0.31,
                            a: 1.0,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 0.45,
                        color: D2D1_COLOR_F {
                            r: 0.65,
                            g: 0.20,
                            b: 0.26,
                            a: 0.55,
                        },
                    },
                    D2D1_GRADIENT_STOP {
                        position: 1.0,
                        color: D2D1_COLOR_F {
                            r: 0.24,
                            g: 0.09,
                            b: 0.14,
                            a: 0.20,
                        },
                    },
                ];
                let fsc = ID2D1RenderTarget::CreateGradientStopCollection(
                    ctx,
                    &fade_stops,
                    D2D1_GAMMA_2_2,
                    D2D1_EXTEND_MODE_CLAMP,
                )?;
                let fbrush = ctx.CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2 {
                            X: art_rr.rect.left,
                            Y: hr.top,
                        },
                        endPoint: Vector2 {
                            X: hr.right,
                            Y: hr.top,
                        },
                    },
                    None,
                    &fsc,
                )?;
                ctx.FillRoundedRectangle(&art_rr, &fbrush);
            }
        }

        // 3. Pastel Yellow Pill ("★ Favorito" or "● Popular") + Dark Launcher Pill
        let yellow_pill_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.945,
                b: 0.66,
                a: 0.98,
            },
            None,
        )?;
        let dark_text = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.16,
                g: 0.08,
                b: 0.08,
                a: 1.0,
            },
            None,
        )?;
        let tag_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: hr.left + 16.0,
                top: hr.top + 14.0,
                right: hr.left + 94.0,
                bottom: hr.top + 34.0,
            },
            radiusX: 10.0,
            radiusY: 10.0,
        };
        ctx.FillRoundedRectangle(&tag_rect, &yellow_pill_bg);
        let tag_txt = to_wide_null(if game.favorite {
            "★ Favorito"
        } else {
            "● Destacado"
        });
        ctx.DrawText(
            &tag_txt[..tag_txt.len() - 1],
            &self.pill_fmt,
            &tag_rect.rect,
            &dark_text,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        let dark_badge = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.12,
                g: 0.05,
                b: 0.07,
                a: 0.60,
            },
            None,
        )?;
        let plat_rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: hr.left + 102.0,
                top: hr.top + 14.0,
                right: hr.left + 158.0,
                bottom: hr.top + 34.0,
            },
            radiusX: 10.0,
            radiusY: 10.0,
        };
        ctx.FillRoundedRectangle(&plat_rect, &dark_badge);
        let plat_txt = to_wide_null(short_launcher(&game.launcher));
        ctx.DrawText(
            &plat_txt[..plat_txt.len() - 1],
            &self.pill_fmt,
            &plat_rect.rect,
            text_white,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        // 4. Game Title + Metadata Description
        let title_r = D2D_RECT_F {
            left: hr.left + 16.0,
            top: hr.top + 42.0,
            right: (hr.right - 16.0).max(hr.left + 80.0),
            bottom: hr.top + 68.0,
        };
        let gw = to_wide_null(&game.name);
        ctx.DrawText(
            &gw[..gw.len() - 1],
            &self.hero_title_fmt,
            &title_r,
            text_white,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        let mut parts = Vec::new();
        if !game.genres.is_empty() {
            parts.push(
                game.genres
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !game.developers.is_empty() {
            parts.push(game.developers[0].clone());
        }
        if let Some(y) = game.release_year {
            parts.push(y.to_string());
        }
        parts.push(format!(
            "{} • {}",
            format_playtime(game.playtime_secs),
            format_last_played(game.last_played_unix)
        ));
        let sub = parts.join("  •  ");
        let sub_r = D2D_RECT_F {
            left: hr.left + 16.0,
            top: hr.top + 72.0,
            right: (hr.right - 20.0).max(hr.left + 80.0),
            bottom: hr.top + 94.0,
        };
        let sw = to_wide_null(&sub);
        let sub_brush = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.90,
                b: 0.90,
                a: 0.88,
            },
            None,
        )?;
        ctx.DrawText(
            &sw[..sw.len() - 1],
            &self.card_meta_fmt,
            &sub_r,
            &sub_brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        // 5. Crisp White "▶ Jugar" Pill + Circular Favorite button
        if let Some(play_r) = state.layout.hero_play_rect {
            let hovering = matches!(state.hovered, Some(HitTarget::HeroPlayButton));
            let white_pill = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: if hovering { 0.93 } else { 0.98 },
                    b: if hovering { 0.93 } else { 0.98 },
                    a: 1.0,
                },
                None,
            )?;
            let pr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: play_r.left,
                    top: play_r.top,
                    right: play_r.right,
                    bottom: play_r.bottom,
                },
                radiusX: 14.0,
                radiusY: 14.0,
            };
            ctx.FillRoundedRectangle(&pr, &white_pill);
            let lbl = to_wide_null("▶  Jugar");
            ctx.DrawText(
                &lbl[..lbl.len() - 1],
                &self.play_btn_fmt,
                &pr.rect,
                coral_brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        if let Some(fav_r) = state.layout.hero_fav_rect {
            let hovering = matches!(state.hovered, Some(HitTarget::HeroFavButton));
            let fav_bg = ctx.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.05,
                    b: 0.07,
                    a: if hovering { 0.72 } else { 0.50 },
                },
                None,
            )?;
            let fr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: fav_r.left,
                    top: fav_r.top,
                    right: fav_r.right,
                    bottom: fav_r.bottom,
                },
                radiusX: 14.0,
                radiusY: 14.0,
            };
            ctx.FillRoundedRectangle(&fr, &fav_bg);
            let star = to_wide_null(if game.favorite { "★" } else { "☆" });
            let star_brush = if game.favorite {
                yellow_pill_bg
            } else {
                text_white.clone()
            };
            ctx.DrawText(
                &star[..star.len() - 1],
                &self.play_btn_fmt,
                &fr.rect,
                &star_brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        Ok(())
    }

    unsafe fn draw_cards_grid(
        &mut self,
        ctx: &ID2D1DeviceContext,
        state: &RenderState,
        clip_r: &D2D_RECT_F,
        text_white: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        text_muted: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        coral_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        let fallback_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.21,
                g: 0.10,
                b: 0.125,
                a: 0.94,
            },
            None,
        )?;
        let white_circle = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.96,
                b: 0.96,
                a: 0.94,
            },
            None,
        )?;
        let dark_circle = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.12,
                g: 0.05,
                b: 0.07,
                a: 0.72,
            },
            None,
        )?;
        let yellow_star = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.91,
                b: 0.52,
                a: 1.0,
            },
            None,
        )?;

        let icon_px = ((48.0 * state.dpi) / 96.0).round() as i32;

        for (idx, game) in state.games.iter().enumerate() {
            let card_r = state.layout.card_rect(idx, state.scroll_y);
            if card_r.bottom < clip_r.top || card_r.top > clip_r.bottom {
                continue;
            }
            let card_rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: card_r.left,
                    top: card_r.top,
                    right: card_r.right,
                    bottom: card_r.bottom,
                },
                radiusX: 16.0,
                radiusY: 16.0,
            };

            let drew_cover = if let Some(ref cpath) = game.cover_path {
                if let Some(bmp) = self.cover_cache.get_or_load(ctx, cpath) {
                    draw_bitmap_cover_rounded(ctx, bmp, &card_rr);
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !drew_cover {
                ctx.FillRoundedRectangle(&card_rr, &fallback_bg);
                let icon_src = game.icon_path.as_deref().or(game.install_dir.as_deref());
                if let Some(ip) = icon_src {
                    if let Some(ibmp) = self.icon_cache.get_or_load(ctx, ip, icon_px) {
                        let cx = card_r.left + (card_r.width() - 48.0) * 0.5;
                        let cy = card_r.top + 38.0;
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

            // Rich bottom dark-burgundy scrim inside card (like Uncharted 4 / Dishonored)
            let scrim_stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: D2D1_COLOR_F {
                        r: 0.10,
                        g: 0.04,
                        b: 0.05,
                        a: 0.0,
                    },
                },
                D2D1_GRADIENT_STOP {
                    position: 0.52,
                    color: D2D1_COLOR_F {
                        r: 0.10,
                        g: 0.04,
                        b: 0.05,
                        a: 0.52,
                    },
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: D2D1_COLOR_F {
                        r: 0.08,
                        g: 0.03,
                        b: 0.04,
                        a: 0.94,
                    },
                },
            ];
            let ssc = ID2D1RenderTarget::CreateGradientStopCollection(
                ctx,
                &scrim_stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            )?;
            let sbrush = ctx.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2 {
                        X: card_r.left,
                        Y: card_r.top + 42.0,
                    },
                    endPoint: Vector2 {
                        X: card_r.left,
                        Y: card_r.bottom,
                    },
                },
                None,
                &ssc,
            )?;
            ctx.FillRoundedRectangle(&card_rr, &sbrush);

            // Top-left Coral Play circle
            let play_r = state.layout.card_play_rect(idx, state.scroll_y);
            let play_ell = D2D1_ELLIPSE {
                point: Vector2 {
                    X: (play_r.left + play_r.right) * 0.5,
                    Y: (play_r.top + play_r.bottom) * 0.5,
                },
                radiusX: 15.0,
                radiusY: 15.0,
            };
            let play_hover =
                matches!(state.hovered, Some(HitTarget::CardPlayButton(i)) if i == idx);
            if play_hover {
                ctx.FillEllipse(&play_ell, &white_circle);
                let g = to_wide_null("▶");
                ctx.DrawText(
                    &g[..g.len() - 1],
                    &self.pill_fmt,
                    &D2D_RECT_F {
                        left: play_r.left,
                        top: play_r.top,
                        right: play_r.right,
                        bottom: play_r.bottom,
                    },
                    coral_brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    Default::default(),
                );
            } else {
                ctx.FillEllipse(&play_ell, coral_brush);
                let g = to_wide_null("▶");
                ctx.DrawText(
                    &g[..g.len() - 1],
                    &self.pill_fmt,
                    &D2D_RECT_F {
                        left: play_r.left,
                        top: play_r.top,
                        right: play_r.right,
                        bottom: play_r.bottom,
                    },
                    text_white,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    Default::default(),
                );
            }

            // Top-right Favorite circle
            let fav_r = state.layout.card_fav_rect(idx, state.scroll_y);
            let fav_ell = D2D1_ELLIPSE {
                point: Vector2 {
                    X: (fav_r.left + fav_r.right) * 0.5,
                    Y: (fav_r.top + fav_r.bottom) * 0.5,
                },
                radiusX: 13.0,
                radiusY: 13.0,
            };
            ctx.FillEllipse(
                &fav_ell,
                if game.favorite {
                    &white_circle
                } else {
                    &dark_circle
                },
            );
            let star_w = to_wide_null(if game.favorite { "★" } else { "☆" });
            ctx.DrawText(
                &star_w[..star_w.len() - 1],
                &self.pill_fmt,
                &D2D_RECT_F {
                    left: fav_r.left,
                    top: fav_r.top,
                    right: fav_r.right,
                    bottom: fav_r.bottom,
                },
                if game.favorite {
                    coral_brush
                } else {
                    &yellow_star
                },
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            // Title & Playtime inside bottom of card
            let t_rect = D2D_RECT_F {
                left: card_r.left + 12.0,
                top: card_r.bottom - 44.0,
                right: card_r.right - 10.0,
                bottom: card_r.bottom - 24.0,
            };
            let gw = to_wide_null(&game.name);
            ctx.DrawText(
                &gw[..gw.len() - 1],
                &self.card_title_fmt,
                &t_rect,
                text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            let meta = format!(
                "{} • {}",
                short_launcher(&game.launcher),
                format_playtime(game.playtime_secs)
            );
            let m_rect = D2D_RECT_F {
                left: card_r.left + 12.0,
                top: card_r.bottom - 24.0,
                right: card_r.right - 10.0,
                bottom: card_r.bottom - 6.0,
            };
            let mw = to_wide_null(&meta);
            ctx.DrawText(
                &mw[..mw.len() - 1],
                &self.card_meta_fmt,
                &m_rect,
                text_muted,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            let is_hovered = matches!(
                state.hovered,
                Some(HitTarget::Card(i))
                    | Some(HitTarget::CardPlayButton(i))
                    | Some(HitTarget::CardFavButton(i))
                    if i == idx
            );
            let is_selected = state.selected_index == Some(idx);
            if is_hovered || is_selected {
                ctx.DrawRoundedRectangle(&card_rr, coral_brush, 2.0, None);
            } else {
                let rim = ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.85,
                        b: 0.85,
                        a: 0.14,
                    },
                    None,
                )?;
                ctx.DrawRoundedRectangle(&card_rr, &rim, 0.9, None);
            }
        }

        Ok(())
    }

    unsafe fn draw_compact_list(
        &mut self,
        ctx: &ID2D1DeviceContext,
        state: &RenderState,
        clip_r: &D2D_RECT_F,
        text_white: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        text_muted: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        coral_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        let row_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.11,
                g: 0.048,
                b: 0.058,
                a: 0.86,
            },
            None,
        )?;
        let row_hover_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.19,
                g: 0.082,
                b: 0.10,
                a: 0.92,
            },
            None,
        )?;
        let thumb_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.25,
                g: 0.12,
                b: 0.14,
                a: 0.95,
            },
            None,
        )?;
        let icon_px = ((38.0 * state.dpi) / 96.0).round() as i32;

        for (idx, game) in state.games.iter().enumerate() {
            let r = state.layout.card_rect(idx, state.scroll_y);
            if r.bottom < clip_r.top || r.top > clip_r.bottom {
                continue;
            }
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                radiusX: 14.0,
                radiusY: 14.0,
            };

            let is_hovered = matches!(
                state.hovered,
                Some(HitTarget::Card(i))
                    | Some(HitTarget::CardPlayButton(i))
                    | Some(HitTarget::CardFavButton(i))
                    if i == idx
            );
            ctx.FillRoundedRectangle(&rr, if is_hovered { &row_hover_bg } else { &row_bg });

            // Square rounded thumbnail on left
            let thumb = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: r.left + 8.0,
                    top: r.top + 8.0,
                    right: r.left + 46.0,
                    bottom: r.bottom - 8.0,
                },
                radiusX: 10.0,
                radiusY: 10.0,
            };
            let drew = if let Some(ref cp) = game.cover_path {
                if let Some(bmp) = self.cover_cache.get_or_load(ctx, cp) {
                    draw_bitmap_cover_rounded(ctx, bmp, &thumb);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !drew {
                ctx.FillRoundedRectangle(&thumb, &thumb_bg);
                if let Some(ip) = game.icon_path.as_deref().or(game.install_dir.as_deref()) {
                    if let Some(ibmp) = self.icon_cache.get_or_load(ctx, ip, icon_px) {
                        ctx.DrawBitmap(
                            ibmp,
                            Some(&thumb.rect),
                            1.0,
                            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                            None,
                            None,
                        );
                    }
                }
            }

            // Title & metadata
            let title_r = D2D_RECT_F {
                left: r.left + 58.0,
                top: r.top + 10.0,
                right: r.right - 50.0,
                bottom: r.top + 28.0,
            };
            let gw = to_wide_null(&game.name);
            ctx.DrawText(
                &gw[..gw.len() - 1],
                &self.card_title_fmt,
                &title_r,
                text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            let sub = format!(
                "{} • {}",
                game.launcher,
                format_playtime(game.playtime_secs)
            );
            let sub_r = D2D_RECT_F {
                left: r.left + 58.0,
                top: r.top + 29.0,
                right: r.right - 50.0,
                bottom: r.bottom - 8.0,
            };
            let sw = to_wide_null(&sub);
            ctx.DrawText(
                &sw[..sw.len() - 1],
                &self.card_meta_fmt,
                &sub_r,
                text_muted,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );

            // Right circular play arrow button (like Unravel 2 / Subway Surf rows)
            let pr = state.layout.card_play_rect(idx, state.scroll_y);
            let play_hover =
                matches!(state.hovered, Some(HitTarget::CardPlayButton(i)) if i == idx);
            let ell = D2D1_ELLIPSE {
                point: Vector2 {
                    X: (pr.left + pr.right) * 0.5,
                    Y: (pr.top + pr.bottom) * 0.5,
                },
                radiusX: 13.0,
                radiusY: 13.0,
            };
            let circle_bg = if play_hover {
                coral_brush.clone()
            } else {
                ctx.CreateSolidColorBrush(
                    &D2D1_COLOR_F {
                        r: 1.0,
                        g: 0.85,
                        b: 0.85,
                        a: 0.12,
                    },
                    None,
                )?
            };
            ctx.FillEllipse(&ell, &circle_bg);
            let arrow = to_wide_null("›");
            ctx.DrawText(
                &arrow[..arrow.len() - 1],
                &self.play_btn_fmt,
                &D2D_RECT_F {
                    left: pr.left,
                    top: pr.top - 1.0,
                    right: pr.right,
                    bottom: pr.bottom,
                },
                text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

        Ok(())
    }

    unsafe fn draw_stats_portal(
        &mut self,
        ctx: &ID2D1DeviceContext,
        state: &RenderState,
        text_white: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        text_muted: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
        dark_plum_brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        let card_rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: 16.0,
                top: HEADER_HEIGHT + 6.0,
                right: state.width - 16.0,
                bottom: state.height - 16.0,
            },
            radiusX: 16.0,
            radiusY: 16.0,
        };
        let panel_bg = ctx.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.19,
                g: 0.088,
                b: 0.11,
                a: 0.85,
            },
            None,
        )?;
        ctx.FillRoundedRectangle(&card_rr, &panel_bg);

        let cx = state.width * 0.5;
        let cy = HEADER_HEIGHT + (state.height - HEADER_HEIGHT - 86.0).max(110.0) * 0.5;

        // Multi-layered glowing ring (coral, soft gold, lavender) matching "Your Statistic"
        let ring_layers = [
            (
                -6.0,
                -4.0,
                56.0,
                D2D1_COLOR_F {
                    r: 0.90,
                    g: 0.34,
                    b: 0.34,
                    a: 0.85,
                },
            ),
            (
                6.0,
                -2.0,
                55.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.88,
                    b: 0.62,
                    a: 0.78,
                },
            ),
            (
                0.0,
                6.0,
                55.0,
                D2D1_COLOR_F {
                    r: 0.56,
                    g: 0.45,
                    b: 0.96,
                    a: 0.80,
                },
            ),
        ];

        for (ox, oy, rad, col) in ring_layers {
            let b = ctx.CreateSolidColorBrush(&col, None)?;
            ctx.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 {
                        X: cx + ox,
                        Y: cy + oy,
                    },
                    radiusX: rad,
                    radiusY: rad,
                },
                &b,
            );
        }

        // Inner dark center disc
        ctx.FillEllipse(
            &D2D1_ELLIPSE {
                point: Vector2 { X: cx, Y: cy },
                radiusX: 46.0,
                radiusY: 46.0,
            },
            dark_plum_brush,
        );

        let lbl_top = to_wide_null("Total horas");
        ctx.DrawText(
            &lbl_top[..lbl_top.len() - 1],
            &self.card_meta_fmt,
            &D2D_RECT_F {
                left: cx - 44.0,
                top: cy - 22.0,
                right: cx + 44.0,
                bottom: cy - 4.0,
            },
            text_muted,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );
        // Center align "Total horas" via pill_fmt
        ctx.DrawText(
            &lbl_top[..lbl_top.len() - 1],
            &self.pill_fmt,
            &D2D_RECT_F {
                left: cx - 44.0,
                top: cy - 24.0,
                right: cx + 44.0,
                bottom: cy - 4.0,
            },
            text_muted,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        let total_h = state.total_library_secs / 3600;
        let total_str = format!("{total_h}h");
        let total_w = to_wide_null(&total_str);
        ctx.DrawText(
            &total_w[..total_w.len() - 1],
            &self.stats_big_fmt,
            &D2D_RECT_F {
                left: cx - 46.0,
                top: cy - 4.0,
                right: cx + 46.0,
                bottom: cy + 28.0,
            },
            text_white,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            Default::default(),
        );

        // Top 3 most played games underneath
        let badge_colors = [
            D2D1_COLOR_F {
                r: 0.89,
                g: 0.32,
                b: 0.32,
                a: 0.95,
            },
            D2D1_COLOR_F {
                r: 1.0,
                g: 0.92,
                b: 0.62,
                a: 0.95,
            },
            D2D1_COLOR_F {
                r: 0.49,
                g: 0.42,
                b: 0.92,
                a: 0.95,
            },
        ];
        let icon_px = ((26.0 * state.dpi) / 96.0).round() as i32;

        for (i, game) in state.games.iter().take(3).enumerate() {
            let slot = state.layout.stats_top_rects[i];
            let slot_cx = (slot.left + slot.right) * 0.5;
            let circle_cy = slot.top + 22.0;

            let hover = matches!(state.hovered, Some(HitTarget::StatsTopGame(idx)) if idx == i);
            let bg = ctx.CreateSolidColorBrush(&badge_colors[i % 3], None)?;
            ctx.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 {
                        X: slot_cx,
                        Y: circle_cy,
                    },
                    radiusX: if hover { 21.0 } else { 19.0 },
                    radiusY: if hover { 21.0 } else { 19.0 },
                },
                &bg,
            );

            let icon_src = game.icon_path.as_deref().or(game.install_dir.as_deref());
            let mut drew_icon = false;
            if let Some(ip) = icon_src {
                if let Some(ibmp) = self.icon_cache.get_or_load(ctx, ip, icon_px) {
                    let ir = D2D_RECT_F {
                        left: slot_cx - 11.0,
                        top: circle_cy - 11.0,
                        right: slot_cx + 11.0,
                        bottom: circle_cy + 11.0,
                    };
                    ctx.DrawBitmap(
                        ibmp,
                        Some(&ir),
                        1.0,
                        D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                        None,
                        None,
                    );
                    drew_icon = true;
                }
            }
            if !drew_icon {
                let initial = game.name.chars().next().unwrap_or('G').to_string();
                let iw = to_wide_null(&initial);
                let fg = if i == 1 {
                    ctx.CreateSolidColorBrush(
                        &D2D1_COLOR_F {
                            r: 0.16,
                            g: 0.08,
                            b: 0.08,
                            a: 1.0,
                        },
                        None,
                    )?
                } else {
                    text_white.clone()
                };
                ctx.DrawText(
                    &iw[..iw.len() - 1],
                    &self.play_btn_fmt,
                    &D2D_RECT_F {
                        left: slot_cx - 16.0,
                        top: circle_cy - 16.0,
                        right: slot_cx + 16.0,
                        bottom: circle_cy + 16.0,
                    },
                    &fg,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    Default::default(),
                );
            }

            let hrs = format_playtime(game.playtime_secs);
            let hw = to_wide_null(&hrs);
            ctx.DrawText(
                &hw[..hw.len() - 1],
                &self.pill_fmt,
                &D2D_RECT_F {
                    left: slot.left,
                    top: circle_cy + 22.0,
                    right: slot.right,
                    bottom: circle_cy + 38.0,
                },
                text_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                Default::default(),
            );
        }

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
    fn burgundy_theme_visual_render_smoke() {
        unsafe {
            let _ = OleInitialize(None);
            let hinst: windows::Win32::Foundation::HINSTANCE =
                GetModuleHandleW(None).unwrap_or_default().into();
            let cls = to_wide_null("ODDBurgundyVisualTest");
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
                496,
                None,
                None,
                Some(hinst),
                None,
            )
            .expect("CreateWindowExW failed");

            let mut renderer = PortalRenderer::new(hwnd, 520, 496, 96.0, 18.0)
                .expect("PortalRenderer::new failed");

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
                    name: "Dishonored: Standard".into(),
                    launcher: "Epic".into(),
                    steam_appid: None,
                    launch_target: String::new(),
                    install_dir: None,
                    cover_path: None,
                    hero_path: None,
                    icon_path: Some(std::path::PathBuf::from(r"C:\Windows\explorer.exe")),
                    genres: vec!["Action".into()],
                    developers: vec!["Arkane Studios".into()],
                    platforms: vec!["PC (Windows)".into()],
                    release_year: Some(2012),
                    community_score: Some(95),
                    playtime_secs: 34_200,
                    play_count: 8,
                    last_played_unix: 1_795_000_000,
                    added_unix: 0,
                    is_installed: true,
                    favorite: false,
                },
                GameEntry {
                    id: "steam:1245620".into(),
                    name: "Elden Ring".into(),
                    launcher: "Steam".into(),
                    steam_appid: Some("1245620".into()),
                    launch_target: String::new(),
                    install_dir: None,
                    cover_path: None,
                    hero_path: None,
                    icon_path: Some(std::path::PathBuf::from(r"C:\Windows\notepad.exe")),
                    genres: vec!["RPG".into()],
                    developers: vec!["FromSoftware".into()],
                    platforms: vec!["PC (Windows)".into()],
                    release_year: Some(2022),
                    community_score: Some(96),
                    playtime_secs: 420_000,
                    play_count: 42,
                    last_played_unix: 1_794_000_000,
                    added_unix: 0,
                    is_installed: true,
                    favorite: true,
                },
            ];

            let layout =
                GameGridLayout::compute(520.0, 496.0, games.len(), true, LayoutStyle::Cards, false);
            let theme = PortalTheme::default();
            let state = RenderState {
                title: "Jugados recientemente",
                games: &games,
                total_library_secs: 607_200,
                layout: &layout,
                width: 520.0,
                height: 496.0,
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
                        height: 496,
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

            let cap_path = std::env::var_os("ODD_CAPTURE_BMP");
            let mut bmp_bytes = if cap_path.is_some() {
                let mut v = Vec::with_capacity(54 + 520 * 496 * 4);
                let file_size = (54 + 520 * 496 * 4) as u32;
                v.extend_from_slice(b"BM");
                v.extend_from_slice(&file_size.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
                v.extend_from_slice(&54u32.to_le_bytes());
                v.extend_from_slice(&40u32.to_le_bytes());
                v.extend_from_slice(&520i32.to_le_bytes());
                v.extend_from_slice(&(-496i32).to_le_bytes());
                v.extend_from_slice(&1u16.to_le_bytes());
                v.extend_from_slice(&32u16.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
                v.extend_from_slice(&((520 * 496 * 4) as u32).to_le_bytes());
                v.extend_from_slice(&[0u8; 16]);
                v
            } else {
                Vec::new()
            };

            let mut non_zero = 0usize;
            for y in 0..496usize {
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
                        // Warm peach-cream canvas backdrop like the reference presentation
                        let bg_r = 245.0;
                        let bg_g = 228.0;
                        let bg_b = 226.0;
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
