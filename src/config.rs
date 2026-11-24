use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortalKind {
    Folder { path: PathBuf },
    Virtual { items: Vec<VirtualItem> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VirtualItem {
    pub name: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalTheme {
    /// RGBA glass tint overlay over native DWM Acrylic (values 0.0..=1.0).
    pub tint_r: f32,
    pub tint_g: f32,
    pub tint_b: f32,
    pub tint_a: f32,
    pub corner_radius: f32,
}

impl Default for PortalTheme {
    fn default() -> Self {
        Self {
            tint_r: 0.11,
            tint_g: 0.12,
            tint_b: 0.16,
            tint_a: 0.48,
            corner_radius: 24.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortalConfig {
    pub id: String,
    pub title: String,
    pub kind: PortalKind,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub expanded_height: i32,
    pub collapsed: bool,
    pub auto_collapse: bool,
    #[serde(default)]
    pub theme: PortalTheme,
}

impl PortalConfig {
    pub fn clamp_dimensions(&mut self) {
        self.width = self.width.clamp(220, 960);
        self.expanded_height = self.expanded_height.clamp(160, 1080);
        self.theme.corner_radius = self.theme.corner_radius.clamp(8.0, 36.0);
        self.theme.tint_a = self.theme.tint_a.clamp(0.05, 0.90);
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
        let mut portals = Vec::new();

        if let Some(downloads) = user_profile_subdir("Downloads") {
            portals.push(PortalConfig {
                id: "portal-downloads".to_string(),
                title: "Downloads".to_string(),
                kind: PortalKind::Folder { path: downloads },
                x: 48,
                y: 48,
                width: 340,
                expanded_height: 360,
                collapsed: false,
                auto_collapse: false,
                theme: PortalTheme::default(),
            });
        }

        let mut quick_items = Vec::new();
        if let Some(docs) = user_profile_subdir("Documents") {
            quick_items.push(VirtualItem {
                name: "Documents".to_string(),
                target: docs,
            });
        }
        if let Some(desktop) = user_profile_subdir("Desktop") {
            quick_items.push(VirtualItem {
                name: "Desktop".to_string(),
                target: desktop,
            });
        }

        portals.push(PortalConfig {
            id: "portal-quick-access".to_string(),
            title: "Quick Doors".to_string(),
            kind: PortalKind::Virtual { items: quick_items },
            x: 416,
            y: 48,
            width: 320,
            expanded_height: 240,
            collapsed: false,
            auto_collapse: true,
            theme: PortalTheme {
                tint_r: 0.10,
                tint_g: 0.14,
                tint_b: 0.20,
                tint_a: 0.46,
                corner_radius: 24.0,
            },
        });

        Self {
            hotkey_vk: default_hotkey_vk(),
            hotkey_mods: default_hotkey_mods(),
            portals,
        }
    }
}

fn user_profile_subdir(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("USERPROFILE")?;
    let p = PathBuf::from(base).join(name);
    if p.exists() {
        Some(p)
    } else {
        None
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
    fn roundtrip_serialization_preserves_portals() {
        let cfg = AppConfig {
            hotkey_vk: 0x44,
            hotkey_mods: 3,
            portals: vec![
                PortalConfig {
                    id: "p1".into(),
                    title: "Projects".into(),
                    kind: PortalKind::Folder {
                        path: PathBuf::from(r"C:\Projects"),
                    },
                    x: 100,
                    y: 200,
                    width: 100, // should clamp to 220 on load
                    expanded_height: 400,
                    collapsed: false,
                    auto_collapse: true,
                    theme: PortalTheme::default(),
                },
                PortalConfig {
                    id: "p2".into(),
                    title: "Pinned".into(),
                    kind: PortalKind::Virtual {
                        items: vec![VirtualItem {
                            name: "Notepad".into(),
                            target: PathBuf::from(r"C:\Windows\notepad.exe"),
                        }],
                    },
                    x: 500,
                    y: 200,
                    width: 320,
                    expanded_height: 260,
                    collapsed: true,
                    auto_collapse: false,
                    theme: PortalTheme::default(),
                },
            ],
        };

        let tmp_dir = std::env::temp_dir().join("odd_test_cfg");
        let tmp_file = tmp_dir.join("portals.json");
        save_to_path(&tmp_file, &cfg).expect("save config");

        let loaded = load_from_path(&tmp_file).expect("load config");
        assert_eq!(loaded.portals.len(), 2);
        assert_eq!(loaded.portals[0].width, 220);
        assert_eq!(loaded.portals[1].title, "Pinned");
        assert!(loaded.portals[1].collapsed);

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
