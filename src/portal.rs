use crate::config::LayoutStyle;

pub const HEADER_HEIGHT: f32 = 48.0;
pub const GRID_PADDING_X: f32 = 16.0;
pub const GRID_PADDING_TOP: f32 = 10.0;
pub const GRID_PADDING_BOTTOM: f32 = 16.0;

// Rounded Burgundy-Theme Game Cards (like Uncharted 4 / Dishonored in reference)
pub const CARD_WIDTH: f32 = 148.0;
pub const CARD_HEIGHT: f32 = 176.0;
pub const CARD_GAP: f32 = 14.0;

// Featured Coral Hero Card at top (like Valorant in reference)
pub const HERO_CARD_HEIGHT: f32 = 154.0;
pub const HERO_GAP: f32 = 14.0;

// Compact List Row (like Unravel 2 / Subway Surf / RDR3 in reference)
pub const LIST_ROW_HEIGHT: f32 = 54.0;
pub const LIST_ROW_GAP: f32 = 8.0;

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
    CardFavButton(usize),
    HeroPlayButton,
    HeroFavButton,
    StatsTopGame(usize),
}

#[derive(Debug, Clone)]
pub struct GameGridLayout {
    pub style: LayoutStyle,
    pub is_stats: bool,
    pub columns: usize,
    pub total_content_height: f32,
    pub max_scroll: f32,
    pub origin_x: f32,
    pub grid_top: f32,
    pub hero_rect: Option<RectF>,
    pub hero_play_rect: Option<RectF>,
    pub hero_fav_rect: Option<RectF>,
    pub list_width: f32,
    pub stats_top_rects: [RectF; 3],
}

impl GameGridLayout {
    pub fn compute(
        width: f32,
        height: f32,
        item_count: usize,
        show_details: bool,
        style: LayoutStyle,
        is_stats: bool,
    ) -> Self {
        let zero_rect = RectF {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };

        if is_stats {
            let cy = HEADER_HEIGHT + (height - HEADER_HEIGHT - 86.0).max(110.0) * 0.5;
            let badge_y = (cy + 62.0).max(HEADER_HEIGHT + 130.0);
            let spacing = ((width - 32.0) / 3.0).max(76.0);
            let mut top_rects = [zero_rect; 3];
            for (i, slot) in top_rects.iter_mut().enumerate() {
                let cx = 16.0 + spacing * (i as f32 + 0.5);
                *slot = RectF {
                    left: cx - 34.0,
                    top: badge_y - 24.0,
                    right: cx + 34.0,
                    bottom: badge_y + 42.0,
                };
            }
            return Self {
                style,
                is_stats: true,
                columns: 3,
                total_content_height: height - HEADER_HEIGHT,
                max_scroll: 0.0,
                origin_x: 16.0,
                grid_top: HEADER_HEIGHT,
                hero_rect: None,
                hero_play_rect: None,
                hero_fav_rect: None,
                list_width: width - 32.0,
                stats_top_rects: top_rects,
            };
        }

        let has_hero =
            show_details && item_count > 0 && height > HEADER_HEIGHT + HERO_CARD_HEIGHT + 120.0;

        let (hero_rect, hero_play_rect, hero_fav_rect, grid_top) = if has_hero {
            let hr = RectF {
                left: GRID_PADDING_X,
                top: HEADER_HEIGHT + 6.0,
                right: width - GRID_PADDING_X,
                bottom: HEADER_HEIGHT + 6.0 + HERO_CARD_HEIGHT,
            };
            let play_btn = RectF {
                left: hr.left + 16.0,
                top: hr.bottom - 42.0,
                right: hr.left + 114.0,
                bottom: hr.bottom - 14.0,
            };
            let fav_btn = RectF {
                left: hr.left + 122.0,
                top: hr.bottom - 42.0,
                right: hr.left + 154.0,
                bottom: hr.bottom - 14.0,
            };
            (
                Some(hr),
                Some(play_btn),
                Some(fav_btn),
                hr.bottom + HERO_GAP,
            )
        } else {
            (None, None, None, HEADER_HEIGHT + GRID_PADDING_TOP)
        };

        let list_width = (width - GRID_PADDING_X * 2.0).max(200.0);
        let (columns, content_h, origin_x) = match style {
            LayoutStyle::Cards => {
                let usable_w = (width - GRID_PADDING_X * 2.0).max(CARD_WIDTH);
                let cols =
                    (((usable_w + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).floor() as usize).max(1);
                let rows = if item_count == 0 {
                    0
                } else {
                    item_count.div_ceil(cols)
                };
                let grid_w = cols as f32 * CARD_WIDTH + (cols.saturating_sub(1)) as f32 * CARD_GAP;
                let ox = ((width - grid_w) * 0.5).max(GRID_PADDING_X);
                let ch = if rows == 0 {
                    0.0
                } else {
                    rows as f32 * CARD_HEIGHT
                        + (rows.saturating_sub(1)) as f32 * CARD_GAP
                        + GRID_PADDING_BOTTOM
                };
                (cols, ch, ox)
            }
            LayoutStyle::CompactList => {
                let ch = if item_count == 0 {
                    0.0
                } else {
                    item_count as f32 * LIST_ROW_HEIGHT
                        + (item_count.saturating_sub(1)) as f32 * LIST_ROW_GAP
                        + GRID_PADDING_BOTTOM
                };
                (1, ch, GRID_PADDING_X)
            }
        };

        let viewport_h = (height - grid_top).max(0.0);
        let max_scroll = (content_h - viewport_h).max(0.0);

        Self {
            style,
            is_stats: false,
            columns,
            total_content_height: content_h,
            max_scroll,
            origin_x,
            grid_top,
            hero_rect,
            hero_play_rect,
            hero_fav_rect,
            list_width,
            stats_top_rects: [zero_rect; 3],
        }
    }

    pub fn card_rect(&self, index: usize, scroll_y: f32) -> RectF {
        match self.style {
            LayoutStyle::Cards => {
                let col = index % self.columns;
                let row = index / self.columns;
                let left = self.origin_x + col as f32 * (CARD_WIDTH + CARD_GAP);
                let top = self.grid_top + row as f32 * (CARD_HEIGHT + CARD_GAP) - scroll_y;
                RectF {
                    left,
                    top,
                    right: left + CARD_WIDTH,
                    bottom: top + CARD_HEIGHT,
                }
            }
            LayoutStyle::CompactList => {
                let top =
                    self.grid_top + index as f32 * (LIST_ROW_HEIGHT + LIST_ROW_GAP) - scroll_y;
                RectF {
                    left: self.origin_x,
                    top,
                    right: self.origin_x + self.list_width,
                    bottom: top + LIST_ROW_HEIGHT,
                }
            }
        }
    }

    pub fn card_play_rect(&self, index: usize, scroll_y: f32) -> RectF {
        let r = self.card_rect(index, scroll_y);
        match self.style {
            LayoutStyle::Cards => RectF {
                left: r.left + 8.0,
                top: r.top + 8.0,
                right: r.left + 38.0,
                bottom: r.top + 38.0,
            },
            LayoutStyle::CompactList => RectF {
                left: r.right - 38.0,
                top: r.top + (r.height() - 28.0) * 0.5,
                right: r.right - 10.0,
                bottom: r.top + (r.height() + 28.0) * 0.5,
            },
        }
    }

    pub fn card_fav_rect(&self, index: usize, scroll_y: f32) -> RectF {
        let r = self.card_rect(index, scroll_y);
        match self.style {
            LayoutStyle::Cards => RectF {
                left: r.right - 36.0,
                top: r.top + 8.0,
                right: r.right - 8.0,
                bottom: r.top + 36.0,
            },
            LayoutStyle::CompactList => RectF {
                left: r.right - 68.0,
                top: r.top + (r.height() - 24.0) * 0.5,
                right: r.right - 44.0,
                bottom: r.top + (r.height() + 24.0) * 0.5,
            },
        }
    }

    pub fn hit_test(&self, x: f32, y: f32, item_count: usize, scroll_y: f32) -> Option<HitTarget> {
        if self.is_stats {
            for (i, r) in self.stats_top_rects.iter().enumerate() {
                if i < item_count && r.contains(x, y) {
                    return Some(HitTarget::StatsTopGame(i));
                }
            }
            return None;
        }

        if let Some(play_r) = self.hero_play_rect {
            if play_r.contains(x, y) {
                return Some(HitTarget::HeroPlayButton);
            }
        }
        if let Some(fav_r) = self.hero_fav_rect {
            if fav_r.contains(x, y) {
                return Some(HitTarget::HeroFavButton);
            }
        }
        if y < self.grid_top {
            return None;
        }
        for idx in 0..item_count {
            let cr = self.card_rect(idx, scroll_y);
            if cr.contains(x, y) {
                if self.card_play_rect(idx, scroll_y).contains(x, y) {
                    return Some(HitTarget::CardPlayButton(idx));
                }
                if self.card_fav_rect(idx, scroll_y).contains(x, y) {
                    return Some(HitTarget::CardFavButton(idx));
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
    fn computes_burgundy_hero_cards_and_stats_hits() {
        let cards = GameGridLayout::compute(520.0, 496.0, 6, true, LayoutStyle::Cards, false);
        assert!(cards.hero_rect.is_some());
        assert_eq!(cards.columns, 3);
        let hp = cards.hero_play_rect.unwrap();
        assert_eq!(
            cards.hit_test(hp.left + 10.0, hp.top + 8.0, 6, 0.0),
            Some(HitTarget::HeroPlayButton)
        );
        let cp = cards.card_play_rect(0, 0.0);
        assert_eq!(
            cards.hit_test(cp.left + 8.0, cp.top + 8.0, 6, 0.0),
            Some(HitTarget::CardPlayButton(0))
        );

        let stats = GameGridLayout::compute(340.0, 280.0, 3, false, LayoutStyle::Cards, true);
        let r0 = stats.stats_top_rects[0];
        assert_eq!(
            stats.hit_test(r0.left + 10.0, r0.top + 10.0, 3, 0.0),
            Some(HitTarget::StatsTopGame(0))
        );
    }
}
