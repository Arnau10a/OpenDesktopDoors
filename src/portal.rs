pub const HEADER_HEIGHT: f32 = 44.0;
pub const GRID_PADDING_X: f32 = 14.0;
pub const GRID_PADDING_TOP: f32 = 12.0;
pub const GRID_PADDING_BOTTOM: f32 = 14.0;

// Playnite 2:3 vertical box-art card geometry
pub const CARD_WIDTH: f32 = 104.0;
pub const COVER_HEIGHT: f32 = 156.0; // 2:3 ratio
pub const CARD_HEIGHT: f32 = 188.0; // cover + title/meta strip
pub const CARD_GAP: f32 = 12.0;
pub const DETAILS_PANEL_HEIGHT: f32 = 132.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RectF {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Card(usize),
    CardPlayButton(usize),
    DetailsPlayButton,
    DetailsFavButton,
}

#[derive(Debug, Clone)]
pub struct GameGridLayout {
    pub columns: usize,
    pub total_content_height: f32,
    pub max_scroll: f32,
    pub origin_x: f32,
    pub grid_bottom: f32,
    pub details_rect: Option<RectF>,
    pub details_play_rect: Option<RectF>,
    pub details_fav_rect: Option<RectF>,
}

impl GameGridLayout {
    pub fn compute(width: f32, height: f32, item_count: usize, show_details: bool) -> Self {
        let has_details =
            show_details && item_count > 0 && height > HEADER_HEIGHT + DETAILS_PANEL_HEIGHT + 80.0;
        let grid_bottom = if has_details {
            height - DETAILS_PANEL_HEIGHT
        } else {
            height
        };

        let usable_w = (width - GRID_PADDING_X * 2.0).max(CARD_WIDTH);
        let columns = (((usable_w + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).floor() as usize).max(1);
        let rows = if item_count == 0 {
            0
        } else {
            item_count.div_ceil(columns)
        };

        let grid_w = columns as f32 * CARD_WIDTH + (columns.saturating_sub(1)) as f32 * CARD_GAP;
        let origin_x = ((width - grid_w) * 0.5).max(GRID_PADDING_X);

        let content_h = if rows == 0 {
            0.0
        } else {
            GRID_PADDING_TOP
                + rows as f32 * CARD_HEIGHT
                + (rows.saturating_sub(1)) as f32 * CARD_GAP
                + GRID_PADDING_BOTTOM
        };

        let viewport_h = (grid_bottom - HEADER_HEIGHT).max(0.0);
        let max_scroll = (content_h - viewport_h).max(0.0);

        let (details_rect, details_play_rect, details_fav_rect) = if has_details {
            let dr = RectF {
                left: 0.0,
                top: grid_bottom,
                right: width,
                bottom: height,
            };
            let play_btn = RectF {
                left: width - 118.0,
                top: height - 48.0,
                right: width - 16.0,
                bottom: height - 14.0,
            };
            let fav_btn = RectF {
                left: width - 160.0,
                top: height - 48.0,
                right: width - 126.0,
                bottom: height - 14.0,
            };
            (Some(dr), Some(play_btn), Some(fav_btn))
        } else {
            (None, None, None)
        };

        Self {
            columns,
            total_content_height: content_h,
            max_scroll,
            origin_x,
            grid_bottom,
            details_rect,
            details_play_rect,
            details_fav_rect,
        }
    }

    pub fn card_rect(&self, index: usize, scroll_y: f32) -> RectF {
        let col = index % self.columns;
        let row = index / self.columns;
        let left = self.origin_x + col as f32 * (CARD_WIDTH + CARD_GAP);
        let top =
            HEADER_HEIGHT + GRID_PADDING_TOP + row as f32 * (CARD_HEIGHT + CARD_GAP) - scroll_y;
        RectF {
            left,
            top,
            right: left + CARD_WIDTH,
            bottom: top + CARD_HEIGHT,
        }
    }

    pub fn cover_rect(&self, index: usize, scroll_y: f32) -> RectF {
        let c = self.card_rect(index, scroll_y);
        RectF {
            left: c.left,
            top: c.top,
            right: c.right,
            bottom: c.top + COVER_HEIGHT,
        }
    }

    pub fn card_play_circle_contains(&self, index: usize, scroll_y: f32, x: f32, y: f32) -> bool {
        let cov = self.cover_rect(index, scroll_y);
        let cx = cov.left + cov.width() * 0.5;
        let cy = cov.top + cov.height() * 0.5;
        let dx = x - cx;
        let dy = y - cy;
        (dx * dx + dy * dy) <= (22.0 * 22.0)
    }

    pub fn hit_test(&self, x: f32, y: f32, item_count: usize, scroll_y: f32) -> Option<HitTarget> {
        if let Some(play_r) = self.details_play_rect {
            if play_r.contains(x, y) {
                return Some(HitTarget::DetailsPlayButton);
            }
        }
        if let Some(fav_r) = self.details_fav_rect {
            if fav_r.contains(x, y) {
                return Some(HitTarget::DetailsFavButton);
            }
        }
        if y < HEADER_HEIGHT || y >= self.grid_bottom {
            return None;
        }
        for idx in 0..item_count {
            if self.card_rect(idx, scroll_y).contains(x, y) {
                if self.card_play_circle_contains(idx, scroll_y, x, y) {
                    return Some(HitTarget::CardPlayButton(idx));
                }
                return Some(HitTarget::Card(idx));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_2_to_3_cover_grid_and_details_panel_hits() {
        let layout = GameGridLayout::compute(520.0, 440.0, 8, true);
        assert!(layout.columns >= 4);
        assert!(layout.details_rect.is_some());

        let c0 = layout.cover_rect(0, 0.0);
        let cx = (c0.left + c0.right) * 0.5;
        let cy = (c0.top + c0.bottom) * 0.5;
        assert_eq!(
            layout.hit_test(cx, cy, 8, 0.0),
            Some(HitTarget::CardPlayButton(0))
        );
        assert_eq!(
            layout.hit_test(c0.left + 4.0, c0.top + 4.0, 8, 0.0),
            Some(HitTarget::Card(0))
        );

        let dp = layout.details_play_rect.unwrap();
        assert_eq!(
            layout.hit_test(dp.left + 10.0, dp.top + 10.0, 8, 0.0),
            Some(HitTarget::DetailsPlayButton)
        );
    }
}
