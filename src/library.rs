use crate::config::{config_dir, PortalKind, SortBy};
use crate::launchers::{scan_epic_games, scan_steam_games};
use crate::playnite_db::{default_playnite_library_dir, load_playnite_library};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub id: String,
    pub name: String,
    pub launcher: String,
    pub steam_appid: Option<String>,
    pub launch_target: String,
    pub install_dir: Option<PathBuf>,
    pub cover_path: Option<PathBuf>,
    pub hero_path: Option<PathBuf>,
    pub icon_path: Option<PathBuf>,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub platforms: Vec<String>,
    pub release_year: Option<i32>,
    pub community_score: Option<i32>,
    pub playtime_secs: u64,
    pub play_count: u64,
    pub last_played_unix: i64,
    pub added_unix: i64,
    pub is_installed: bool,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserGameOverrides {
    pub extra_playtime_secs: u64,
    pub extra_play_count: u64,
    pub last_played_unix: i64,
    pub favorite: Option<bool>,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SavedLibraryState {
    overrides: HashMap<String, UserGameOverrides>,
    custom_games: Vec<GameEntry>,
}

pub struct GameLibrary {
    pub games: Vec<GameEntry>,
    saved: SavedLibraryState,
}

impl GameLibrary {
    pub fn load_and_scan() -> Self {
        let saved = load_saved_state();
        let mut lib = Self {
            games: Vec::new(),
            saved,
        };
        lib.rescan();
        lib
    }

    pub fn rescan(&mut self) {
        let mut by_key: HashMap<String, GameEntry> = HashMap::new();

        // 1. Load Playnite library first (richest metadata: genres, devs, covers, heroes, scores)
        if let Some(pn_dir) = default_playnite_library_dir() {
            for pg in load_playnite_library(&pn_dir) {
                let steam_appid = if pg.source_name.eq_ignore_ascii_case("Steam")
                    && pg.game_id.chars().all(|c| c.is_ascii_digit())
                    && !pg.game_id.is_empty()
                {
                    Some(pg.game_id.clone())
                } else {
                    None
                };

                let id = if let Some(ref appid) = steam_appid {
                    format!("steam:{appid}")
                } else if !pg.game_id.is_empty() {
                    format!("{}:{}", pg.source_name.to_ascii_lowercase(), pg.game_id)
                } else {
                    format!("playnite:{}", pg.id)
                };

                let launch_target = if let Some(ref appid) = steam_appid {
                    format!("steam://rungameid/{appid}")
                } else if pg.source_name.eq_ignore_ascii_case("Epic") && !pg.game_id.is_empty() {
                    format!(
                        "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                        pg.game_id
                    )
                } else if let Some(ref dir) = pg.install_dir {
                    find_primary_exe(dir)
                        .unwrap_or_else(|| dir.clone())
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::new()
                };

                let entry = GameEntry {
                    id: id.clone(),
                    name: pg.name,
                    launcher: pg.source_name,
                    steam_appid,
                    launch_target,
                    install_dir: pg.install_dir,
                    cover_path: pg.cover_path,
                    hero_path: pg.background_path,
                    icon_path: pg.icon_path,
                    genres: pg.genres,
                    developers: pg.developers,
                    platforms: pg.platforms,
                    release_year: pg.release_year,
                    community_score: pg.community_score,
                    playtime_secs: pg.playtime_secs,
                    play_count: pg.play_count,
                    last_played_unix: pg.last_activity_unix,
                    added_unix: pg.added_unix,
                    is_installed: pg.is_installed,
                    favorite: pg.favorite,
                };
                by_key.insert(id, entry);
            }
        }

        // 2. Merge live Steam manifests & localconfig (adds freshly installed games + live playtime)
        for sg in scan_steam_games() {
            if let Some(existing) = by_key.get_mut(&sg.id) {
                existing.is_installed = true;
                if existing.install_dir.is_none() {
                    existing.install_dir = sg.install_dir;
                }
                if existing.cover_path.is_none() {
                    existing.cover_path = sg.cover_path;
                }
                if existing.hero_path.is_none() {
                    existing.hero_path = sg.hero_path;
                }
                if existing.launch_target.is_empty() {
                    existing.launch_target = sg.launch_target;
                }
                existing.playtime_secs = existing.playtime_secs.max(sg.playtime_secs);
                existing.last_played_unix = existing.last_played_unix.max(sg.last_played_unix);
            } else {
                by_key.insert(
                    sg.id.clone(),
                    GameEntry {
                        id: sg.id,
                        name: sg.name,
                        launcher: sg.launcher,
                        steam_appid: sg.steam_appid,
                        launch_target: sg.launch_target,
                        install_dir: sg.install_dir,
                        cover_path: sg.cover_path,
                        hero_path: sg.hero_path,
                        icon_path: None,
                        genres: Vec::new(),
                        developers: Vec::new(),
                        platforms: vec!["PC (Windows)".to_string()],
                        release_year: None,
                        community_score: None,
                        playtime_secs: sg.playtime_secs,
                        play_count: 0,
                        last_played_unix: sg.last_played_unix,
                        added_unix: 0,
                        is_installed: true,
                        favorite: false,
                    },
                );
            }
        }

        // 3. Merge Epic Games manifests
        for eg in scan_epic_games() {
            if let Some(existing) = by_key.get_mut(&eg.id) {
                existing.is_installed = true;
                if existing.install_dir.is_none() {
                    existing.install_dir = eg.install_dir;
                }
                existing.launch_target = eg.launch_target;
            } else {
                by_key.insert(
                    eg.id.clone(),
                    GameEntry {
                        id: eg.id,
                        name: eg.name,
                        launcher: eg.launcher,
                        steam_appid: None,
                        launch_target: eg.launch_target,
                        install_dir: eg.install_dir,
                        cover_path: None,
                        hero_path: None,
                        icon_path: None,
                        genres: Vec::new(),
                        developers: Vec::new(),
                        platforms: vec!["PC (Windows)".to_string()],
                        release_year: None,
                        community_score: None,
                        playtime_secs: 0,
                        play_count: 0,
                        last_played_unix: 0,
                        added_unix: 0,
                        is_installed: true,
                        favorite: false,
                    },
                );
            }
        }

        // 4. Add custom dropped games
        for cg in &self.saved.custom_games {
            by_key.entry(cg.id.clone()).or_insert_with(|| cg.clone());
        }

        // 5. Apply cached Steam covers downloaded by OpenDesktopDoors + user overrides
        let covers_dir = crate::covers::covers_cache_dir();
        let mut list: Vec<GameEntry> = by_key
            .into_values()
            .filter_map(|mut g| {
                if let Some(ov) = self.saved.overrides.get(&g.id) {
                    if ov.hidden {
                        return None;
                    }
                    g.playtime_secs += ov.extra_playtime_secs;
                    g.play_count += ov.extra_play_count;
                    g.last_played_unix = g.last_played_unix.max(ov.last_played_unix);
                    if let Some(fav) = ov.favorite {
                        g.favorite = fav;
                    }
                }
                if g.cover_path.is_none() {
                    if let Some(ref appid) = g.steam_appid {
                        let cached = covers_dir.join(format!("{appid}_cover.jpg"));
                        if cached.exists() {
                            g.cover_path = Some(cached);
                        }
                    }
                }
                if g.hero_path.is_none() {
                    if let Some(ref appid) = g.steam_appid {
                        let cached_hero = covers_dir.join(format!("{appid}_hero.jpg"));
                        if cached_hero.exists() {
                            g.hero_path = Some(cached_hero);
                        }
                    }
                }
                Some(g)
            })
            .collect();

        list.sort_by_key(|a| a.name.to_lowercase());
        self.games = list;
    }

    pub fn games_for_portal(&self, kind: &PortalKind, sort: SortBy) -> Vec<GameEntry> {
        let mut filtered: Vec<GameEntry> = match kind {
            PortalKind::Recent => self
                .games
                .iter()
                .filter(|g| g.is_installed && (g.last_played_unix > 0 || g.playtime_secs > 0))
                .cloned()
                .collect(),
            PortalKind::Favorites => self.games.iter().filter(|g| g.favorite).cloned().collect(),
            PortalKind::Installed => self
                .games
                .iter()
                .filter(|g| g.is_installed)
                .cloned()
                .collect(),
            PortalKind::Launcher { name } => self
                .games
                .iter()
                .filter(|g| g.launcher.eq_ignore_ascii_case(name) && g.is_installed)
                .cloned()
                .collect(),
            PortalKind::Custom { game_ids } => game_ids
                .iter()
                .filter_map(|id| self.games.iter().find(|g| &g.id == id).cloned())
                .collect(),
        };

        // Fallback if Recent is empty (e.g. fresh machine): show installed games
        if matches!(kind, PortalKind::Recent) && filtered.is_empty() {
            filtered = self
                .games
                .iter()
                .filter(|g| g.is_installed)
                .take(12)
                .cloned()
                .collect();
        }

        match sort {
            SortBy::LastPlayed => filtered.sort_by(|a, b| {
                b.last_played_unix
                    .cmp(&a.last_played_unix)
                    .then_with(|| b.playtime_secs.cmp(&a.playtime_secs))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            SortBy::Playtime => filtered.sort_by(|a, b| {
                b.playtime_secs
                    .cmp(&a.playtime_secs)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            SortBy::Name => filtered.sort_by_key(|a| a.name.to_lowercase()),
        }

        filtered
    }

    pub fn toggle_favorite(&mut self, game_id: &str) -> bool {
        let mut new_fav = false;
        if let Some(g) = self.games.iter_mut().find(|g| g.id == game_id) {
            g.favorite = !g.favorite;
            new_fav = g.favorite;
        }
        let ov = self.saved.overrides.entry(game_id.to_string()).or_default();
        ov.favorite = Some(new_fav);
        save_state(&self.saved);
        new_fav
    }

    pub fn record_launch(&mut self, game_id: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Some(g) = self.games.iter_mut().find(|g| g.id == game_id) {
            g.play_count += 1;
            g.last_played_unix = now;
        }
        let ov = self.saved.overrides.entry(game_id.to_string()).or_default();
        ov.extra_play_count += 1;
        ov.last_played_unix = now;
        save_state(&self.saved);
    }

    pub fn add_playtime(&mut self, game_id: &str, delta_secs: u64) {
        if delta_secs == 0 {
            return;
        }
        if let Some(g) = self.games.iter_mut().find(|g| g.id == game_id) {
            g.playtime_secs += delta_secs;
        }
        let ov = self.saved.overrides.entry(game_id.to_string()).or_default();
        ov.extra_playtime_secs += delta_secs;
        save_state(&self.saved);
    }

    pub fn add_custom_path(&mut self, path: PathBuf) -> GameEntry {
        let id = format!("custom:{}", path.to_string_lossy().to_lowercase());
        if let Some(existing) = self.games.iter().find(|g| g.id == id) {
            return existing.clone();
        }
        let name = path
            .file_stem()
            .or_else(|| path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Game".to_string());
        let entry = GameEntry {
            id: id.clone(),
            name,
            launcher: "PC".to_string(),
            steam_appid: None,
            launch_target: path.to_string_lossy().into_owned(),
            install_dir: path.parent().map(|p| p.to_path_buf()),
            cover_path: None,
            hero_path: None,
            icon_path: Some(path),
            genres: Vec::new(),
            developers: Vec::new(),
            platforms: vec!["PC (Windows)".to_string()],
            release_year: None,
            community_score: None,
            playtime_secs: 0,
            play_count: 0,
            last_played_unix: 0,
            added_unix: 0,
            is_installed: true,
            favorite: false,
        };
        self.saved.custom_games.push(entry.clone());
        self.games.push(entry.clone());
        save_state(&self.saved);
        entry
    }

    pub fn apply_downloaded_cover(
        &mut self,
        game_id: &str,
        cover: Option<PathBuf>,
        hero: Option<PathBuf>,
    ) {
        if let Some(g) = self.games.iter_mut().find(|g| g.id == game_id) {
            if g.cover_path.is_none() {
                g.cover_path = cover;
            }
            if g.hero_path.is_none() {
                g.hero_path = hero;
            }
        }
    }
}

pub fn format_playtime(secs: u64) -> String {
    if secs == 0 {
        "Sin jugar".to_string()
    } else if secs < 3600 {
        format!("{} min", (secs / 60).max(1))
    } else {
        format!("{:.1} h", secs as f64 / 3600.0)
    }
}

pub fn format_last_played(unix_ts: i64) -> String {
    if unix_ts <= 0 {
        return "Nunca".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(unix_ts);
    let diff = (now - unix_ts).max(0);
    if diff < 3600 {
        "Hace un momento".to_string()
    } else if diff < 86400 {
        "Hoy".to_string()
    } else if diff < 172_800 {
        "Ayer".to_string()
    } else {
        let days = diff / 86400;
        if days < 30 {
            format!("Hace {days} d")
        } else {
            let months = days / 30;
            format!("Hace {months} mes(es)")
        }
    }
}

fn find_primary_exe(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() {
            if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
                if ext.eq_ignore_ascii_case("exe") {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if !name.contains("unins") && !name.contains("crash") {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn library_state_path() -> PathBuf {
    config_dir().join("library.json")
}

fn load_saved_state() -> SavedLibraryState {
    let p = library_state_path();
    let Ok(s) = fs::read_to_string(p) else {
        return SavedLibraryState::default();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

fn save_state(state: &SavedLibraryState) {
    let p = library_state_path();
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(p, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_playtime_cleanly() {
        assert_eq!(format_playtime(0), "Sin jugar");
        assert_eq!(format_playtime(1200), "20 min");
        assert_eq!(format_playtime(5400), "1.5 h");
    }
}
