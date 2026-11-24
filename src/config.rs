use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortalKind {
    Recent,
    Favorites,
    Installed,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalTheme {
    /// Playnite-inspired deep slate-navy tint over DWM Acrylic.
    pub tint_r: f32,
    pub tint_g: f32,
    pub tint_b: f32,
    pub tint_a: f32,
    pub corner_radius: f32,
}

impl Default for PortalTheme {
    fn default() -> Self {
        Self {
            // Playnite MainColorDark (#101B22)
            tint_r: 0.063,
            tint_g: 0.106,
            tint_b: 0.133,
            tint_a: 0.74,
            corner_radius: 8.0,
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
        self.expanded_height = self.expanded_height.clamp(240, 1080);
        self.theme.corner_radius = 8.0;
        self.theme.tint_a = self.theme.tint_a.clamp(0.15, 0.94);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_hotkey_vk")]
    pub hotkey_vk: u32,
    #[serde(default = "default_hotkey_mods")]
    pub hotkey_mods: u32,
    pub portals: Vec<PortalConfig>,
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
            hotkey_vk: default_hotkey_vk(),
            hotkey_mods: default_hotkey_mods(),
            portals: vec![
                PortalConfig {
                    id: "portal-recent".to_string(),
                    title: "Jugados recientemente".to_string(),
                    kind: PortalKind::Recent,
                    sort_by: SortBy::LastPlayed,
                    x: 40,
                    y: 40,
                    width: 520,
                    expanded_height: 440,
                    collapsed: false,
                    auto_collapse: false,
                    show_details: true,
                    theme: PortalTheme::default(),
                },
                PortalConfig {
                    id: "portal-installed".to_string(),
                    title: "Instalados".to_string(),
                    kind: PortalKind::Installed,
                    sort_by: SortBy::Playtime,
                    x: 584,
                    y: 40,
                    width: 400,
                    expanded_height: 440,
                    collapsed: false,
                    auto_collapse: false,
                    show_details: true,
                    theme: PortalTheme {
                        tint_r: 0.075,
                        tint_g: 0.114,
                        tint_b: 0.157,
                        tint_a: 0.76,
                        corner_radius: 8.0,
                    },
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
    load_from_path(&path).unwrap_or_else(|_| {
        let cfg = AppConfig::default();
        let _ = save_to_path(&path, &cfg);
        cfg
    })
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
    fn roundtrip_serialization_preserves_game_portals() {
        let cfg = AppConfig {
            hotkey_vk: 0x44,
            hotkey_mods: 3,
            portals: vec![
                PortalConfig {
                    id: "p1".into(),
                    title: "Recent".into(),
                    kind: PortalKind::Recent,
                    sort_by: SortBy::LastPlayed,
                    x: 40,
                    y: 40,
                    width: 120, // clamps to 280
                    expanded_height: 440,
                    collapsed: false,
                    auto_collapse: true,
                    show_details: true,
                    theme: PortalTheme::default(),
                },
                PortalConfig {
                    id: "p2".into(),
                    title: "Steam".into(),
                    kind: PortalKind::Launcher {
                        name: "Steam".into(),
                    },
                    sort_by: SortBy::Playtime,
                    x: 500,
                    y: 40,
                    width: 400,
                    expanded_height: 440,
                    collapsed: true,
                    auto_collapse: false,
                    show_details: false,
                    theme: PortalTheme::default(),
                },
            ],
        };

        let tmp_dir = std::env::temp_dir().join("odd_game_cfg_test");
        let tmp_file = tmp_dir.join("portals.json");
        save_to_path(&tmp_file, &cfg).expect("save");
        let loaded = load_from_path(&tmp_file).expect("load");
        assert_eq!(loaded.portals.len(), 2);
        assert_eq!(loaded.portals[0].width, 280);
        assert!(matches!(
            loaded.portals[1].kind,
            PortalKind::Launcher { .. }
        ));
        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
