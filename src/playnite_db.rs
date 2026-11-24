use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PlayniteGameRecord {
    pub id: String,
    pub name: String,
    pub game_id: String,
    pub source_name: String,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub platforms: Vec<String>,
    pub playtime_secs: u64,
    pub play_count: u64,
    pub last_activity_unix: i64,
    pub added_unix: i64,
    pub release_year: Option<i32>,
    pub community_score: Option<i32>,
    pub is_installed: bool,
    pub favorite: bool,
    pub install_dir: Option<PathBuf>,
    pub cover_path: Option<PathBuf>,
    pub background_path: Option<PathBuf>,
    pub icon_path: Option<PathBuf>,
}

pub fn default_playnite_library_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata).join("Playnite").join("library");
    if dir.join("games.db").exists() {
        Some(dir)
    } else {
        None
    }
}

pub fn load_playnite_library(lib_dir: &Path) -> Vec<PlayniteGameRecord> {
    let sources = load_named_table(&lib_dir.join("sources.db"));
    let genres = load_named_table(&lib_dir.join("genres.db"));
    let companies = load_named_table(&lib_dir.join("companies.db"));
    let platforms = load_named_table(&lib_dir.join("platforms.db"));

    let files_dir = lib_dir.join("files");
    let Ok(raw_games) = read_litedb_v4_documents(&lib_dir.join("games.db")) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(raw_games.len());
    for doc in raw_games {
        let Some(name) = doc.get_str("Name").map(|s| s.to_string()) else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        let hidden = doc.get_bool("Hidden").unwrap_or(false);
        if hidden {
            continue;
        }

        let id = doc
            .get_str("_id")
            .map(|s| s.to_string())
            .or_else(|| doc.get_guid_str("_id"))
            .unwrap_or_default();
        let game_id = doc.get_str("GameId").unwrap_or_default().to_string();
        let plugin_id = doc.get_guid_str("PluginId").unwrap_or_default();

        let source_name = doc
            .get_guid_str("SourceId")
            .and_then(|gid| sources.get(&gid).cloned())
            .unwrap_or_else(|| infer_source_from_plugin(&plugin_id));

        let genre_names = doc
            .get_guid_array("GenreIds")
            .into_iter()
            .filter_map(|g| genres.get(&g).cloned())
            .collect();
        let dev_names = doc
            .get_guid_array("DeveloperIds")
            .into_iter()
            .filter_map(|g| companies.get(&g).cloned())
            .collect();
        let plat_names = doc
            .get_guid_array("PlatformIds")
            .into_iter()
            .filter_map(|g| platforms.get(&g).cloned())
            .collect();

        let playtime_secs = doc.get_int("Playtime").unwrap_or(0).max(0) as u64;
        let play_count = doc.get_int("PlayCount").unwrap_or(0).max(0) as u64;
        let last_activity_unix = doc.get_datetime_unix("LastActivity").unwrap_or(0);
        let added_unix = doc.get_datetime_unix("Added").unwrap_or(0);
        let release_year = doc
            .get_datetime_unix("ReleaseDate")
            .map(unix_to_year)
            .filter(|&y| (1970..=2100).contains(&y));
        let community_score = doc.get_int("CommunityScore").map(|v| v as i32);

        let is_installed = doc.get_bool("IsInstalled").unwrap_or(false);
        let favorite = doc.get_bool("Favorite").unwrap_or(false);
        let install_dir = doc
            .get_str("InstallDirectory")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let cover_path = doc
            .get_str("CoverImage")
            .and_then(|s| resolve_playnite_media(&files_dir, s));
        let background_path = doc
            .get_str("BackgroundImage")
            .and_then(|s| resolve_playnite_media(&files_dir, s));
        let icon_path = doc
            .get_str("Icon")
            .and_then(|s| resolve_playnite_media(&files_dir, s));

        out.push(PlayniteGameRecord {
            id,
            name,
            game_id,
            source_name,
            genres: genre_names,
            developers: dev_names,
            platforms: plat_names,
            playtime_secs,
            play_count,
            last_activity_unix,
            added_unix,
            release_year,
            community_score,
            is_installed,
            favorite,
            install_dir,
            cover_path,
            background_path,
            icon_path,
        });
    }
    out
}

fn infer_source_from_plugin(plugin_id: &str) -> String {
    match plugin_id.to_ascii_lowercase().as_str() {
        "cb91dfc9-b977-43bf-8e70-55f46e410fab" => "Steam".to_string(),
        "00000002-dbd1-46c6-b5d0-b1ba559d10e4" => "Epic".to_string(),
        "aebe8b7c-6dc3-4a66-af31-e7375c6b5e9e" => "GOG".to_string(),
        "e3f53bca-64e6-49a4-a4bc-5243957f82f4" => "Xbox".to_string(),
        "c2f038e5-8b92-4877-91f1-da9094155fc5" => "Ubisoft".to_string(),
        "85dd7072-2f20-4696-ab32-aa32dbdfe334" => "EA".to_string(),
        "00000000-0000-0000-0000-000000000000" | "" => "PC".to_string(),
        _ => "Playnite".to_string(),
    }
}

fn resolve_playnite_media(files_dir: &Path, raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.starts_with("http://") || raw.starts_with("https://") {
        return None;
    }
    let p = Path::new(raw);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }
    let candidate = files_dir.join(raw);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn load_named_table(db_path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(docs) = read_litedb_v4_documents(db_path) else {
        return map;
    };
    for doc in docs {
        let Some(id) = doc
            .get_guid_str("_id")
            .or_else(|| doc.get_str("_id").map(|s| s.to_string()))
        else {
            continue;
        };
        if let Some(name) = doc.get_str("Name") {
            map.insert(id, name.to_string());
        }
    }
    map
}

fn unix_to_year(unix_secs: i64) -> i32 {
    let days = unix_secs.div_euclid(86400);
    // Approximate civil year from Unix epoch 1970-01-01
    1970 + ((days * 400) / 146_097) as i32
}

// --- Minimal LiteDB v4 (4096-byte page, version 7) + BSON reader ---

const LITEDB_PAGE_SIZE: usize = 4096;
const LITEDB_HEADER_SIZE: usize = 25;
const LITEDB_DATA_PAGE_TYPE: u8 = 4;

fn read_litedb_v4_documents(path: &Path) -> Result<Vec<BsonDoc>, String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    if data.len() < LITEDB_PAGE_SIZE {
        return Ok(Vec::new());
    }
    if !data[..64].windows(14).any(|w| w == b"LiteDB file **") {
        return Err("Not a LiteDB file".into());
    }

    let mut docs = Vec::new();
    for page in data.as_chunks::<LITEDB_PAGE_SIZE>().0 {
        let page_type = page[4];
        if page_type != LITEDB_DATA_PAGE_TYPE {
            continue;
        }
        let item_count = page[5] as usize;
        if item_count == 0 {
            continue;
        }

        // LiteDB v4 DataPage: footer slot directory stores 2-byte offset + 2-byte length
        // from the end of the page, OR sequential blocks starting at LITEDB_HEADER_SIZE.
        // We walk sequential data blocks safely using the slot directory at the page tail.
        let mut parsed_via_slots = false;
        let footer_bytes = item_count * 4;
        if LITEDB_HEADER_SIZE + footer_bytes <= LITEDB_PAGE_SIZE {
            // Check if the page tail contains valid slot offsets
            let mut slot_docs = Vec::new();
            let mut all_valid = true;
            for i in 0..item_count {
                let slot_pos = LITEDB_PAGE_SIZE - (i + 1) * 4;
                let offset = u16::from_le_bytes([page[slot_pos], page[slot_pos + 1]]) as usize;
                let len = u16::from_le_bytes([page[slot_pos + 2], page[slot_pos + 3]]) as usize;
                if offset == 0 && len == 0 {
                    continue;
                }
                if offset < LITEDB_HEADER_SIZE || offset + len > slot_pos || len < 5 {
                    all_valid = false;
                    break;
                }
                // LiteDB v4 DataBlock starts with 2-byte index + 2-byte data length or direct BSON
                // Try direct BSON first, then +4 header offset if needed.
                let slice = &page[offset..offset + len];
                if let Some(doc) = try_parse_litedb_block(slice) {
                    slot_docs.push(doc);
                }
            }
            if all_valid && !slot_docs.is_empty() {
                docs.extend(slot_docs);
                parsed_via_slots = true;
            }
        }

        if !parsed_via_slots {
            // Fallback: scan page body for valid BSON documents starting with `_id`
            docs.extend(scan_bson_docs_in_page(&page[LITEDB_HEADER_SIZE..]));
        }
    }

    Ok(docs)
}

fn try_parse_litedb_block(slice: &[u8]) -> Option<BsonDoc> {
    if slice.len() >= 5 {
        let doc_len = i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as usize;
        if doc_len >= 5 && doc_len <= slice.len() && slice[doc_len - 1] == 0 {
            if let Some(d) = parse_bson_doc(&slice[..doc_len]) {
                return Some(d);
            }
        }
    }
    if slice.len() >= 9 {
        let doc_len = i32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]) as usize;
        if doc_len >= 5 && 4 + doc_len <= slice.len() && slice[4 + doc_len - 1] == 0 {
            if let Some(d) = parse_bson_doc(&slice[4..4 + doc_len]) {
                return Some(d);
            }
        }
    }
    None
}

fn scan_bson_docs_in_page(body: &[u8]) -> Vec<BsonDoc> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 12 < body.len() {
        let doc_len = i32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
        if doc_len >= 12 && (i + doc_len as usize) <= body.len() {
            let end = i + doc_len as usize;
            if body[end - 1] == 0 {
                if let Some(doc) = parse_bson_doc(&body[i..end]) {
                    if doc.fields.contains_key("_id") {
                        out.push(doc);
                        i = end;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

#[derive(Debug, Clone)]
pub enum BsonValue {
    Str(String),
    Int(i64),
    Bool(bool),
    DateTimeMs(i64),
    Guid([u8; 16]),
    Array(Vec<BsonValue>),
    Other,
}

#[derive(Debug, Clone)]
pub struct BsonDoc {
    pub fields: HashMap<String, BsonValue>,
}

impl BsonDoc {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.fields.get(key) {
            Some(BsonValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.fields.get(key) {
            Some(BsonValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.fields.get(key) {
            Some(BsonValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn get_datetime_unix(&self, key: &str) -> Option<i64> {
        match self.fields.get(key) {
            Some(BsonValue::DateTimeMs(ms)) => Some(*ms / 1000),
            _ => None,
        }
    }

    pub fn get_guid_str(&self, key: &str) -> Option<String> {
        match self.fields.get(key) {
            Some(BsonValue::Guid(bytes)) => Some(format_dotnet_guid(bytes)),
            Some(BsonValue::Str(s)) => Some(s.to_ascii_lowercase()),
            _ => None,
        }
    }

    pub fn get_guid_array(&self, key: &str) -> Vec<String> {
        let Some(BsonValue::Array(arr)) = self.fields.get(key) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|v| match v {
                BsonValue::Guid(b) => Some(format_dotnet_guid(b)),
                BsonValue::Str(s) => Some(s.to_ascii_lowercase()),
                _ => None,
            })
            .collect()
    }
}

fn format_dotnet_guid(b: &[u8; 16]) -> String {
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d1, d2, d3, b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn parse_bson_doc(buf: &[u8]) -> Option<BsonDoc> {
    if buf.len() < 5 {
        return None;
    }
    let declared_len = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if declared_len != buf.len() || buf[declared_len - 1] != 0 {
        return None;
    }

    let mut fields = HashMap::new();
    let mut pos = 4usize;
    let end = declared_len - 1;

    while pos < end {
        let elem_type = buf[pos];
        pos += 1;
        if elem_type == 0 {
            break;
        }
        let nul_rel = buf[pos..end].iter().position(|&b| b == 0)?;
        let key = std::str::from_utf8(&buf[pos..pos + nul_rel])
            .ok()?
            .to_string();
        pos += nul_rel + 1;

        let val = parse_bson_value(elem_type, buf, &mut pos, end)?;
        fields.insert(key, val);
    }

    Some(BsonDoc { fields })
}

fn parse_bson_value(elem_type: u8, buf: &[u8], pos: &mut usize, end: usize) -> Option<BsonValue> {
    match elem_type {
        0x01 => {
            // Double
            if *pos + 8 > end {
                return None;
            }
            *pos += 8;
            Some(BsonValue::Other)
        }
        0x02 => {
            // UTF-8 String
            if *pos + 4 > end {
                return None;
            }
            let slen = i32::from_le_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]]);
            *pos += 4;
            if slen < 1 || *pos + (slen as usize) > end {
                return None;
            }
            let s = String::from_utf8_lossy(&buf[*pos..*pos + slen as usize - 1]).into_owned();
            *pos += slen as usize;
            Some(BsonValue::Str(s))
        }
        0x03 => {
            // Embedded Document
            if *pos + 4 > end {
                return None;
            }
            let dlen = i32::from_le_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]])
                as usize;
            if dlen < 5 || *pos + dlen > end {
                return None;
            }
            *pos += dlen;
            Some(BsonValue::Other)
        }
        0x04 => {
            // Array (stored as BSON document with numeric string keys)
            if *pos + 4 > end {
                return None;
            }
            let dlen = i32::from_le_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]])
                as usize;
            if dlen < 5 || *pos + dlen > end {
                return None;
            }
            let arr_doc = parse_bson_doc(&buf[*pos..*pos + dlen])?;
            *pos += dlen;
            Some(BsonValue::Array(arr_doc.fields.into_values().collect()))
        }
        0x05 => {
            // Binary
            if *pos + 5 > end {
                return None;
            }
            let blen = i32::from_le_bytes([buf[*pos], buf[*pos + 1], buf[*pos + 2], buf[*pos + 3]])
                as usize;
            let subtype = buf[*pos + 4];
            *pos += 5;
            if *pos + blen > end {
                return None;
            }
            let val = if (subtype == 0x03 || subtype == 0x04) && blen == 16 {
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&buf[*pos..*pos + 16]);
                BsonValue::Guid(arr)
            } else {
                BsonValue::Other
            };
            *pos += blen;
            Some(val)
        }
        0x07 => {
            // ObjectId
            if *pos + 12 > end {
                return None;
            }
            *pos += 12;
            Some(BsonValue::Other)
        }
        0x08 => {
            // Boolean
            if *pos + 1 > end {
                return None;
            }
            let b = buf[*pos] != 0;
            *pos += 1;
            Some(BsonValue::Bool(b))
        }
        0x09 => {
            // UTC datetime (ms since epoch)
            if *pos + 8 > end {
                return None;
            }
            let ms = i64::from_le_bytes(buf[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(BsonValue::DateTimeMs(ms))
        }
        0x0A | 0x06 | 0x7F | 0xFF => Some(BsonValue::Other),
        0x10 => {
            // Int32
            if *pos + 4 > end {
                return None;
            }
            let v = i32::from_le_bytes(buf[*pos..*pos + 4].try_into().ok()?) as i64;
            *pos += 4;
            Some(BsonValue::Int(v))
        }
        0x11 | 0x12 => {
            // Timestamp / Int64
            if *pos + 8 > end {
                return None;
            }
            let v = i64::from_le_bytes(buf[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(BsonValue::Int(v))
        }
        0x13 => {
            // Decimal128
            if *pos + 16 > end {
                return None;
            }
            *pos += 16;
            Some(BsonValue::Other)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_bson_game_document() {
        let mut body = Vec::new();
        // "_id" -> "g1"
        body.push(0x02);
        body.extend_from_slice(b"_id\0");
        body.extend_from_slice(&3i32.to_le_bytes());
        body.extend_from_slice(b"g1\0");
        // "Name" -> "Hollow Knight"
        body.push(0x02);
        body.extend_from_slice(b"Name\0");
        body.extend_from_slice(&14i32.to_le_bytes());
        body.extend_from_slice(b"Hollow Knight\0");
        // "Playtime" -> 3600 (int64)
        body.push(0x12);
        body.extend_from_slice(b"Playtime\0");
        body.extend_from_slice(&3600i64.to_le_bytes());
        // "Favorite" -> true
        body.push(0x08);
        body.extend_from_slice(b"Favorite\0");
        body.push(1);
        body.push(0x00);

        let total_len = (4 + body.len()) as i32;
        let mut raw = Vec::new();
        raw.extend_from_slice(&total_len.to_le_bytes());
        raw.extend_from_slice(&body);

        let doc = parse_bson_doc(&raw).expect("valid bson");
        assert_eq!(doc.get_str("Name"), Some("Hollow Knight"));
        assert_eq!(doc.get_int("Playtime"), Some(3600));
        assert_eq!(doc.get_bool("Favorite"), Some(true));
    }
}
