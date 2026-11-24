use crate::messages::to_wide_null;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
};

#[derive(Debug, Clone)]
pub struct DetectedGame {
    pub id: String,
    pub name: String,
    pub launcher: String,
    pub steam_appid: Option<String>,
    pub launch_target: String,
    pub install_dir: Option<PathBuf>,
    pub cover_path: Option<PathBuf>,
    pub hero_path: Option<PathBuf>,
    pub playtime_secs: u64,
    pub last_played_unix: i64,
}

pub fn scan_steam_games() -> Vec<DetectedGame> {
    let Some(steam_root) = read_steam_path_from_registry() else {
        return Vec::new();
    };
    let lib_folders = parse_steam_library_folders(&steam_root);
    let stats = parse_steam_user_stats(&steam_root);
    let cache_dir = steam_root.join("appcache").join("librarycache");

    let mut games = Vec::new();
    let mut seen_appids = std::collections::HashSet::new();

    for folder in lib_folders {
        let steamapps = folder.join("steamapps");
        let Ok(entries) = fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy();
            if !fname_str.starts_with("appmanifest_") || !fname_str.ends_with(".acf") {
                continue;
            }
            let Ok(content) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some((appid, name, install_dir_name)) = parse_app_manifest(&content) else {
                continue;
            };
            // Skip Steamworks Common Redistributables (228980)
            if appid == "228980" || !seen_appids.insert(appid.clone()) {
                continue;
            }

            let install_dir = install_dir_name
                .map(|d| steamapps.join("common").join(d))
                .filter(|p| p.exists());

            let (playtime_mins, last_played) = stats.get(&appid).copied().unwrap_or((0, 0));
            let (cover_path, hero_path) = find_steam_cached_art(&cache_dir, &appid);

            games.push(DetectedGame {
                id: format!("steam:{appid}"),
                name,
                launcher: "Steam".to_string(),
                steam_appid: Some(appid.clone()),
                launch_target: format!("steam://rungameid/{appid}"),
                install_dir,
                cover_path,
                hero_path,
                playtime_secs: playtime_mins * 60,
                last_played_unix: last_played,
            });
        }
    }

    games
}

fn read_steam_path_from_registry() -> Option<PathBuf> {
    let subkey = to_wide_null(r"Software\Valve\Steam");
    let val_name = to_wide_null("SteamPath");
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return None;
        }
        let mut reg_type = REG_SZ;
        let mut buf = [0u16; 512];
        let mut byte_len = (buf.len() * 2) as u32;
        let res = RegQueryValueExW(
            hkey,
            PCWSTR(val_name.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut byte_len),
        );
        let _ = RegCloseKey(hkey);
        if res.is_err() || byte_len < 2 {
            return None;
        }
        let chars = (byte_len as usize / 2).saturating_sub(1);
        let s = String::from_utf16_lossy(&buf[..chars]);
        let p = PathBuf::from(s);
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }
}

pub fn parse_steam_library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![steam_root.to_path_buf()];
    let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(vdf_path) else {
        return folders;
    };

    for line in content.lines() {
        let tokens = extract_vdf_quoted_tokens(line);
        if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("path") {
            let unescaped = tokens[1].replace("\\\\", "\\");
            let p = PathBuf::from(unescaped);
            if p.exists() && !folders.contains(&p) {
                folders.push(p);
            }
        }
    }
    folders
}

pub fn parse_app_manifest(content: &str) -> Option<(String, String, Option<String>)> {
    let mut appid = None;
    let mut name = None;
    let mut installdir = None;

    for line in content.lines() {
        let tokens = extract_vdf_quoted_tokens(line);
        if tokens.len() < 2 {
            continue;
        }
        match tokens[0].to_ascii_lowercase().as_str() {
            "appid" => appid = Some(tokens[1].clone()),
            "name" => name = Some(tokens[1].clone()),
            "installdir" => installdir = Some(tokens[1].clone()),
            _ => {}
        }
    }

    Some((appid?, name?, installdir))
}

/// Reads `userdata/<id>/config/localconfig.vdf` to extract `(Playtime_minutes, LastPlayed_unix)` per AppID.
fn parse_steam_user_stats(steam_root: &Path) -> HashMap<String, (u64, i64)> {
    let mut stats: HashMap<String, (u64, i64)> = HashMap::new();
    let userdata = steam_root.join("userdata");
    let Ok(users) = fs::read_dir(userdata) else {
        return stats;
    };

    for user_dir in users.flatten() {
        let cfg = user_dir.path().join("config").join("localconfig.vdf");
        let Ok(content) = fs::read_to_string(cfg) else {
            continue;
        };
        parse_localconfig_apps(&content, &mut stats);
    }
    stats
}

pub fn parse_localconfig_apps(content: &str, out: &mut HashMap<String, (u64, i64)>) {
    let mut in_apps_depth: Option<usize> = None;
    let mut current_appid: Option<String> = None;
    let mut depth: usize = 0;
    let mut cur_playtime: u64 = 0;
    let mut cur_last_played: i64 = 0;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line == "{" {
            depth += 1;
            continue;
        }
        if line == "}" {
            if let (Some(apps_d), Some(appid)) = (in_apps_depth, current_appid.as_ref()) {
                if depth == apps_d + 1 {
                    let entry = out.entry(appid.clone()).or_insert((0, 0));
                    entry.0 = entry.0.max(cur_playtime);
                    entry.1 = entry.1.max(cur_last_played);
                    current_appid = None;
                    cur_playtime = 0;
                    cur_last_played = 0;
                } else if depth == apps_d {
                    in_apps_depth = None;
                }
            }
            depth = depth.saturating_sub(1);
            continue;
        }

        let tokens = extract_vdf_quoted_tokens(line);
        if tokens.len() == 1 {
            if tokens[0].eq_ignore_ascii_case("apps") && in_apps_depth.is_none() {
                in_apps_depth = Some(depth + 1);
            } else if let Some(apps_d) = in_apps_depth {
                if depth == apps_d && tokens[0].chars().all(|c| c.is_ascii_digit()) {
                    current_appid = Some(tokens[0].clone());
                    cur_playtime = 0;
                    cur_last_played = 0;
                }
            }
        } else if tokens.len() >= 2 && current_appid.is_some() {
            if tokens[0].eq_ignore_ascii_case("Playtime") {
                if let Ok(m) = tokens[1].parse::<u64>() {
                    cur_playtime = m;
                }
            } else if tokens[0].eq_ignore_ascii_case("LastPlayed") {
                if let Ok(ts) = tokens[1].parse::<i64>() {
                    cur_last_played = ts;
                }
            }
        }
    }
}

pub fn find_steam_cached_art(cache_dir: &Path, appid: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    // New Steam 2024+ layout: librarycache/<appid>/library_600x900.jpg
    let app_dir = cache_dir.join(appid);
    let mut cover = None;
    let mut hero = None;

    for candidate in [
        app_dir.join("library_600x900_2x.jpg"),
        app_dir.join("library_600x900.jpg"),
        cache_dir.join(format!("{appid}_library_600x900.jpg")),
    ] {
        if candidate.exists() {
            cover = Some(candidate);
            break;
        }
    }

    // Fallback: scan app_dir for header.jpg or library_header.jpg
    if cover.is_none() && app_dir.exists() && app_dir.join("header.jpg").exists() {
        cover = Some(app_dir.join("header.jpg"));
    }

    for candidate in [
        app_dir.join("library_hero.jpg"),
        cache_dir.join(format!("{appid}_library_hero.jpg")),
        app_dir.join("header.jpg"),
    ] {
        if candidate.exists() {
            hero = Some(candidate);
            break;
        }
    }

    (cover, hero)
}

fn extract_vdf_quoted_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut val = String::new();
            while let Some(nc) = chars.next() {
                if nc == '\\' {
                    if let Some(esc) = chars.next() {
                        val.push('\\');
                        val.push(esc);
                    }
                } else if nc == '"' {
                    break;
                } else {
                    val.push(nc);
                }
            }
            tokens.push(val);
        }
    }
    tokens
}

// --- Epic Games Launcher Manifests ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicManifest {
    display_name: Option<String>,
    app_name: Option<String>,
    catalog_namespace: Option<String>,
    catalog_item_id: Option<String>,
    install_location: Option<String>,
    #[serde(rename = "bIsIncompleteInstall")]
    is_incomplete: Option<bool>,
    app_categories: Option<Vec<String>>,
}

pub fn scan_epic_games() -> Vec<DetectedGame> {
    let Some(prog_data) = std::env::var_os("ProgramData") else {
        return Vec::new();
    };
    let manifests = PathBuf::from(prog_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    let Ok(entries) = fs::read_dir(manifests) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(item): Result<EpicManifest, _> = serde_json::from_str(&content) else {
            continue;
        };
        if item.is_incomplete.unwrap_or(false) {
            continue;
        }
        let cats = item.app_categories.unwrap_or_default();
        if !cats.is_empty() && !cats.iter().any(|c| c == "games") {
            continue;
        }
        let (Some(name), Some(app_name), Some(ns), Some(cid)) = (
            item.display_name,
            item.app_name,
            item.catalog_namespace,
            item.catalog_item_id,
        ) else {
            continue;
        };

        out.push(DetectedGame {
            id: format!("epic:{app_name}"),
            name,
            launcher: "Epic".to_string(),
            steam_appid: None,
            launch_target: format!(
                "com.epicgames.launcher://apps/{ns}%3A{cid}%3A{app_name}?action=launch&silent=true"
            ),
            install_dir: item.install_location.map(PathBuf::from),
            cover_path: None,
            hero_path: None,
            playtime_secs: 0,
            last_played_unix: 0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steam_acf_and_localconfig_stats() {
        let acf = r#"
"AppState"
{
    "appid"     "367520"
    "name"      "Hollow Knight"
    "installdir" "Hollow Knight"
}
"#;
        let (appid, name, dir) = parse_app_manifest(acf).expect("acf");
        assert_eq!(appid, "367520");
        assert_eq!(name, "Hollow Knight");
        assert_eq!(dir.as_deref(), Some("Hollow Knight"));

        let vdf = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "367520"
                    {
                        "LastPlayed"    "1730000000"
                        "Playtime"      "420"
                    }
                }
            }
        }
    }
}
"#;
        let mut stats = HashMap::new();
        parse_localconfig_apps(vdf, &mut stats);
        assert_eq!(stats.get("367520"), Some(&(420, 1_730_000_000)));
    }
}
