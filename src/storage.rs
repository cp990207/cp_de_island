use crate::memo::Memo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// v2 adds task fields (done/completed_at/priority/due) to Memo. They all
// carry serde(default), so v1 files — and the legacy bare array — load
// unchanged; the next save writes the enriched records.
const CURRENT_VERSION: u32 = 2;

/// On-disk schema. `version` allows future migrations; the loader also
/// accepts the legacy format (a bare JSON array of memos).
#[derive(Serialize, Deserialize)]
struct MemoFile {
    version: u32,
    memos: Vec<Memo>,
}

/// The directory all app data files live in (`%APPDATA%/MemoPill`, or a
/// fallback next to the executable). Never the current working directory —
/// it changes with the launch context and would scatter data files around.
pub fn data_dir() -> Option<PathBuf> {
    if let Ok(base) = std::env::var("APPDATA") {
        return Some(PathBuf::from(base).join("MemoPill"));
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))?;
    Some(dir.join("MemoPill"))
}

fn data_file() -> PathBuf {
    if let Some(dir) = data_dir() {
        return dir.join("memos.json");
    }
    PathBuf::from("MemoPill").join("memos.json")
}

/// Window placement, kept in a separate settings file so the memos rewrite
/// cycle never has to know about it.
#[derive(Serialize, Deserialize)]
struct SettingsFile {
    window_x: i32,
    window_y: i32,
}

fn settings_file() -> PathBuf {
    data_file().with_file_name("settings.json")
}

pub fn load_window_pos() -> Option<(i32, i32)> {
    let json = std::fs::read_to_string(settings_file()).ok()?;
    let s: SettingsFile = serde_json::from_str(&json).ok()?;
    Some((s.window_x, s.window_y))
}

pub fn save_window_pos(x: i32, y: i32) {
    let path = settings_file();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("[memo-pill] failed to create settings dir: {e}");
            return;
        }
    }
    let Ok(json) = serde_json::to_string(&SettingsFile {
        window_x: x,
        window_y: y,
    }) else {
        return;
    };
    // Same atomic-replace pattern as the memos file.
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("[memo-pill] failed to write {}: {e}", tmp.display());
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        eprintln!("[memo-pill] failed to replace {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

pub fn load_memos() -> Vec<Memo> {
    let path = data_file();
    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!("[memo-pill] failed to read {}: {e}", path.display());
            return Vec::new();
        }
    };

    // New envelope format first, then the legacy bare-array format.
    let parsed = serde_json::from_str::<MemoFile>(&json)
        .map(|f| f.memos)
        .or_else(|_| serde_json::from_str::<Vec<Memo>>(&json));

    match parsed {
        Ok(memos) => memos,
        Err(e) => {
            // Data corruption must never be silently overwritten: back the
            // original file aside so the user can recover it manually.
            eprintln!("[memo-pill] corrupt data file {}: {e}", path.display());
            let backup = path.with_file_name(format!(
                "memos.corrupt-{}.json",
                crate::memo::unix_now()
            ));
            match std::fs::copy(&path, &backup) {
                Ok(_) => eprintln!("[memo-pill] corrupt file backed up to {}", backup.display()),
                Err(be) => eprintln!("[memo-pill] failed to back up corrupt file: {be}"),
            }
            Vec::new()
        }
    }
}

pub fn save_memos(memos: &[Memo]) {
    let path = data_file();
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("[memo-pill] failed to create data dir: {e}");
            return;
        }
    }
    let file = MemoFile {
        version: CURRENT_VERSION,
        memos: memos.to_vec(),
    };
    let Ok(json) = serde_json::to_string_pretty(&file) else {
        return;
    };
    // Atomic replace: write a temp file, then rename over the target. A crash
    // mid-write can never leave a truncated memos.json behind.
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("[memo-pill] failed to write {}: {e}", tmp.display());
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        eprintln!("[memo-pill] failed to replace {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

fn providers_file() -> PathBuf {
    data_file().with_file_name("providers.json")
}

/// One monitored provider instance. Multiple entries with the same `provider`
/// are allowed (e.g. two GLM accounts) — they differ by `id`, which is stable
/// across renames and used as the key for balance_data / balance_errors.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MonitorEntry {
    /// Stable instance id, e.g. "glm-mon-1". Unique within the file.
    pub id: String,
    /// Provider type name, e.g. "GLM". Maps to a `Provider` impl in balance.
    pub provider: String,
    /// The API key for this instance.
    pub key: String,
}

#[derive(Serialize, Deserialize, Default)]
struct ProvidersFile {
    /// New schema: a flat list of monitor instances (allows duplicates of the
    /// same provider type). Written on every save.
    #[serde(default)]
    monitors: Vec<MonitorEntry>,
    /// Legacy schema (`{api_keys: {GLM: key}}`). Read-only — only used to
    /// migrate old files on first load. Never written back; once monitors is
    /// populated this stays empty on disk.
    #[serde(default)]
    api_keys: HashMap<String, String>,
}

/// Parse the on-disk file into its raw schema (monitors + legacy api_keys).
fn read_providers_file() -> ProvidersFile {
    let json = match std::fs::read_to_string(providers_file()) {
        Ok(j) => j,
        Err(_) => return ProvidersFile::default(),
    };
    serde_json::from_str(&json).unwrap_or_default()
}

fn write_providers_file(monitors: &[MonitorEntry]) {
    let path = providers_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Only write the new schema: drop the legacy api_keys field entirely so
    // re-migration can't double up on next load.
    #[derive(Serialize)]
    struct OutFile {
        monitors: Vec<MonitorEntry>,
    }
    let Ok(json) = serde_json::to_string_pretty(&OutFile {
        monitors: monitors.to_vec(),
    }) else {
        return;
    };
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &json) {
        eprintln!("[memo-pill] failed to write {}: {e}", tmp.display());
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        eprintln!("[memo-pill] failed to replace {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Load all monitor instances. If the file still uses the legacy
/// `{api_keys:{...}}` schema (and has no `monitors` yet), migrate each old
/// entry to `{provider}-mon-1`, persist the new schema, and return it. So the
/// caller always gets the list form and the file is upgraded transparently.
pub fn load_monitors() -> Vec<MonitorEntry> {
    let f = read_providers_file();
    if !f.monitors.is_empty() {
        return f.monitors;
    }
    // Legacy migration: turn the old name→key map into one monitor each.
    if f.api_keys.is_empty() {
        return Vec::new();
    }
    let migrated: Vec<MonitorEntry> = f
        .api_keys
        .iter()
        .map(|(provider, key)| MonitorEntry {
            id: format!("{}-mon-1", provider.to_lowercase()),
            provider: provider.clone(),
            key: key.clone(),
        })
        .collect();
    write_providers_file(&migrated);
    migrated
}

/// Build the next instance id for `provider`, e.g. "glm-mon-2" when a
/// "glm-mon-1" already exists. Ids are lowercase and 1-indexed.
fn next_monitor_id(provider: &str, existing: &[MonitorEntry]) -> String {
    let prefix = format!("{}-mon-", provider.to_lowercase());
    let max_n = existing
        .iter()
        .filter_map(|m| {
            if m.id.starts_with(&prefix) {
                m.id[prefix.len()..].parse::<u32>().ok()
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0);
    format!("{}{}", prefix, max_n + 1)
}

/// Append a new monitor instance for `provider` with `key`, persist, and
/// return the newly assigned id. Always appends (never dedupes) — adding a
/// second GLM is intentional.
pub fn save_monitor(provider: &str, key: &str) -> String {
    let mut monitors = load_monitors();
    let id = next_monitor_id(provider, &monitors);
    monitors.push(MonitorEntry {
        id: id.clone(),
        provider: provider.to_string(),
        key: key.to_string(),
    });
    write_providers_file(&monitors);
    id
}

/// Remove the monitor instance with `id`. No-op if it isn't present or the
/// file is missing.
pub fn remove_monitor(id: &str) {
    let mut monitors = load_monitors();
    let before = monitors.len();
    monitors.retain(|m| m.id != id);
    if monitors.len() != before {
        write_providers_file(&monitors);
    }
}
