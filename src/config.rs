use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortalKind {
    Recent,
    Favorites,
    Installed,
    Stats,
    Launcher { name: String },
    Custom { game_ids: Vec<String> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    LastPlayed,
    Playtime,
    Name,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayoutStyle {
    #[default]
    Cards,
    CompactList,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalTheme {
    /// Deep Burgundy / Mahogany base glass (#241014)
    pub tint_r: f32,
    pub tint_g: f32,
    pub tint_b: f32,
    pub tint_a: f32,
    pub corner_radius: f32,
}

impl Default for PortalTheme {
    fn default() -> Self {
        Self {
            tint_r: 0.149,
            tint_g: 0.067,
            tint_b: 0.082,
            tint_a: 0.94,
            corner_radius: 18.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalConfig {
    pub id: String,
    pub title: String,
    pub kind: PortalKind,
    #[serde(default)]
    pub sort_by: SortBy,
    #[serde(default)]
    pub layout_style: LayoutStyle,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub expanded_height: i32,
    pub collapsed: bool,
    pub auto_collapse: bool,
    #[serde(default = "default_show_details")]
    pub show_details: bool,
    #[serde(default)]
    pub theme: PortalTheme,
}

fn default_show_details() -> bool {
    true
}

impl PortalConfig {
    pub fn clamp_dimensions(&mut self) {
        self.width = self.width.clamp(280, 1280);
        self.expanded_height = self.expanded_height.clamp(220, 1080);
        self.theme.corner_radius = self.theme.corner_radius.clamp(10.0, 28.0);
        self.theme.tint_a = self.theme.tint_a.clamp(0.45, 0.98);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_theme_version")]
    pub theme_version: u32,
    #[serde(default = "default_hotkey_vk")]
    pub hotkey_vk: u32,
    #[serde(default = "default_hotkey_mods")]
    pub hotkey_mods: u32,
    pub portals: Vec<PortalConfig>,
}

fn default_theme_version() -> u32 {
    2
}

fn default_hotkey_vk() -> u32 {
    0x44 // 'D'
}

fn default_hotkey_mods() -> u32 {
    0x0002 | 0x0001 // MOD_CONTROL | MOD_ALT
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme_version: default_theme_version(),
            hotkey_vk: default_hotkey_vk(),
            hotkey_mods: default_hotkey_mods(),
            portals: vec![
                PortalConfig {
                    id: "portal-recent".to_string(),
                    title: "Jugados recientemente".to_string(),
                    kind: PortalKind::Recent,
                    sort_by: SortBy::LastPlayed,
                    layout_style: LayoutStyle::Cards,
                    x: 36,
                    y: 36,
                    width: 520,
                    expanded_height: 496,
                    collapsed: false,
                    auto_collapse: false,
                    show_details: true,
                    theme: PortalTheme::default(),
                },
                PortalConfig {
                    id: "portal-installed".to_string(),
                    title: "Biblioteca rápida".to_string(),
                    kind: PortalKind::Installed,
                    sort_by: SortBy::Playtime,
                    layout_style: LayoutStyle::CompactList,
                    x: 576,
                    y: 36,
                    width: 340,
                    expanded_height: 248,
                    collapsed: false,
                    auto_collapse: false,
                    show_details: false,
                    theme: PortalTheme::default(),
                },
                PortalConfig {
                    id: "portal-stats".to_string(),
                    title: "Tus Estadísticas".to_string(),
                    kind: PortalKind::Stats,
                    sort_by: SortBy::Playtime,
                    layout_style: LayoutStyle::Cards,
                    x: 576,
                    y: 300,
                    width: 340,
                    expanded_height: 280,
                    collapsed: false,
                    auto_collapse: false,
                    show_details: false,
                    theme: PortalTheme::default(),
                },
            ],
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("OpenDesktopDoors")
    } else {
        PathBuf::from(".")
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("portals.json")
}

pub fn load_or_default() -> AppConfig {
    let path = config_path();
    match load_from_path(&path) {
        Ok(cfg) if cfg.theme_version >= default_theme_version() => cfg,
        _ => {
            let cfg = AppConfig::default();
            let _ = save_to_path(&path, &cfg);
            cfg
        }
    }
}

pub fn load_from_path(path: &Path) -> Result<AppConfig, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut cfg: AppConfig = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    for p in &mut cfg.portals {
        p.clamp_dimensions();
    }
    Ok(cfg)
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    save_to_path(&config_path(), cfg)
}

pub fn save_to_path(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_serialization_preserves_burgundy_portals() {
        let cfg = AppConfig::default();
        let tmp_dir = std::env::temp_dir().join("odd_burgundy_cfg_test");
        let tmp_file = tmp_dir.join("portals.json");
        save_to_path(&tmp_file, &cfg).expect("save");
        let loaded = load_from_path(&tmp_file).expect("load");
        assert_eq!(loaded.portals.len(), 3);
        assert_eq!(loaded.theme_version, 2);
        assert_eq!(loaded.portals[1].layout_style, LayoutStyle::CompactList);
        assert!(matches!(loaded.portals[2].kind, PortalKind::Stats));
        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
