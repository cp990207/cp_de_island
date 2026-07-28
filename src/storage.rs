use crate::memo::Memo;
use std::path::PathBuf;

fn data_file() -> PathBuf {
    let base = std::env::var("APPDATA")
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("MemoPill").join("memos.json")
}

pub fn load_memos() -> Vec<Memo> {
    std::fs::read_to_string(data_file())
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn save_memos(memos: &[Memo]) {
    let path = data_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(memos) {
        let _ = std::fs::write(&path, json);
    }
}
