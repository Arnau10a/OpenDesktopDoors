use crate::config::PortalKind;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

pub const HEADER_HEIGHT: f32 = 44.0;
pub const GRID_PADDING_X: f32 = 14.0;
pub const GRID_PADDING_TOP: f32 = 10.0;
pub const GRID_PADDING_BOTTOM: f32 = 14.0;
pub const CELL_WIDTH: f32 = 82.0;
pub const CELL_HEIGHT: f32 = 90.0;
pub const CELL_GAP: f32 = 8.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalItem {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl ItemRect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

#[derive(Debug, Clone)]
pub struct GridLayout {
    pub columns: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub rows: usize,
    pub total_content_height: f32,
    pub max_scroll: f32,
    pub origin_x: f32,
}

impl GridLayout {
    pub fn compute(width: f32, height: f32, item_count: usize) -> Self {
        let usable_w = (width - GRID_PADDING_X * 2.0).max(CELL_WIDTH);
        let columns = (((usable_w + CELL_GAP) / (CELL_WIDTH + CELL_GAP)).floor() as usize).max(1);
        let rows = if item_count == 0 {
            0
        } else {
            item_count.div_ceil(columns)
        };

        let grid_w = columns as f32 * CELL_WIDTH + (columns.saturating_sub(1)) as f32 * CELL_GAP;
        let origin_x = ((width - grid_w) * 0.5).max(GRID_PADDING_X);

        let content_h = if rows == 0 {
            0.0
        } else {
            GRID_PADDING_TOP
                + rows as f32 * CELL_HEIGHT
                + (rows.saturating_sub(1)) as f32 * CELL_GAP
                + GRID_PADDING_BOTTOM
        };

        let viewport_h = (height - HEADER_HEIGHT).max(0.0);
        let max_scroll = (content_h - viewport_h).max(0.0);

        Self {
            columns,
            rows,
            total_content_height: content_h,
            max_scroll,
            origin_x,
        }
    }

    pub fn item_rect(&self, index: usize, scroll_y: f32) -> ItemRect {
        let col = index % self.columns;
        let row = index / self.columns;
        let left = self.origin_x + col as f32 * (CELL_WIDTH + CELL_GAP);
        let top =
            HEADER_HEIGHT + GRID_PADDING_TOP + row as f32 * (CELL_HEIGHT + CELL_GAP) - scroll_y;
        ItemRect {
            left,
            top,
            right: left + CELL_WIDTH,
            bottom: top + CELL_HEIGHT,
        }
    }

    pub fn hit_test(&self, x: f32, y: f32, item_count: usize, scroll_y: f32) -> Option<usize> {
        if y < HEADER_HEIGHT {
            return None;
        }
        (0..item_count).find(|&idx| self.item_rect(idx, scroll_y).contains(x, y))
    }
}

pub fn load_items(kind: &PortalKind) -> Vec<PortalItem> {
    match kind {
        PortalKind::Folder { path } => load_folder_items(path),
        PortalKind::Virtual { items } => items
            .iter()
            .map(|v| PortalItem {
                name: v.name.clone(),
                path: v.target.clone(),
                is_dir: v.target.is_dir(),
            })
            .collect(),
    }
}

pub fn load_folder_items(dir: &Path) -> Vec<PortalItem> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if should_skip_filename(&name_str) {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let display_name = strip_lnk_extension(&name_str, is_dir);
        items.push(PortalItem {
            name: display_name,
            path,
            is_dir,
        });
    }

    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    items
}

fn should_skip_filename(name: &str) -> bool {
    name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db")
}

fn strip_lnk_extension(name: &str, is_dir: bool) -> String {
    if !is_dir && name.to_lowercase().ends_with(".lnk") && name.len() > 4 {
        name[..name.len() - 4].to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_layout_computes_columns_and_hit_tests() {
        let layout = GridLayout::compute(340.0, 360.0, 7);
        assert_eq!(layout.columns, 3);
        assert_eq!(layout.rows, 3);

        let r0 = layout.item_rect(0, 0.0);
        let cx = (r0.left + r0.right) * 0.5;
        let cy = (r0.top + r0.bottom) * 0.5;
        assert_eq!(layout.hit_test(cx, cy, 7, 0.0), Some(0));
        assert_eq!(layout.hit_test(cx, 10.0, 7, 0.0), None); // in header
    }

    #[test]
    fn skips_desktop_ini_and_strips_lnk() {
        assert!(should_skip_filename("desktop.ini"));
        assert!(!should_skip_filename("notes.txt"));
        assert_eq!(strip_lnk_extension("Firefox.lnk", false), "Firefox");
        assert_eq!(strip_lnk_extension("folder.lnk", true), "folder.lnk");
    }
}
