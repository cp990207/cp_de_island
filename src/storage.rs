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

#[derive(Serialize, Deserialize, Default)]
struct ProvidersFile {
    api_keys: HashMap<String, String>,
}

pub fn load_provider_key(provider: &str) -> Option<String> {
    let json = std::fs::read_to_string(providers_file()).ok()?;
    let f: ProvidersFile = serde_json::from_str(&json).ok()?;
    f.api_keys.get(provider).cloned()
}

pub fn save_provider_key(provider: &str, key: &str) {
    let path = providers_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut file = std::fs::read_to_string(&path)
        .ok()
        .and_then(|j| serde_json::from_str::<ProvidersFile>(&j).ok())
        .unwrap_or_default();
    file.api_keys
        .insert(provider.to_string(), key.to_string());
    let Ok(json) = serde_json::to_string_pretty(&file) else {
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

pub fn load_all_provider_keys() -> HashMap<String, String> {
    let json = match std::fs::read_to_string(providers_file()) {
        Ok(j) => j,
        Err(_) => return HashMap::new(),
    };
    let f: ProvidersFile = match serde_json::from_str(&json) {
        Ok(f) => f,
        Err(_) => return HashMap::new(),
    };
    f.api_keys
}
