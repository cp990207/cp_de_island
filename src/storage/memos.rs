//! `memos.json`: the task list, with a versioned envelope and a corruption
//! guard. v2 adds task fields (done/completed_at/priority/due) to Memo. They
//! all carry serde(default), so v1 files — and the legacy bare array — load
//! unchanged; the next save writes the enriched records.

use super::{data_path, write_json};
use crate::memo::Memo;
use serde::{Deserialize, Serialize};

const CURRENT_VERSION: u32 = 2;

/// On-disk schema. `version` allows future migrations; the loader also
/// accepts the legacy format (a bare JSON array of memos).
#[derive(Serialize, Deserialize)]
struct MemoFile {
    version: u32,
    memos: Vec<Memo>,
}

fn data_file() -> std::path::PathBuf {
    data_path("memos.json")
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
    let file = MemoFile {
        version: CURRENT_VERSION,
        memos: memos.to_vec(),
    };
    write_json(&data_file(), &file);
}
