//! Persistence layer: JSON files under `%APPDATA%/MemoPill` (or a fallback
//! next to the executable). One submodule per on-disk file; every write goes
//! through `atomic_write` so a crash mid-write can never truncate real data.

mod memos;
mod monitors;
mod settings;

pub use memos::{load_memos, save_memos};
pub use monitors::{MonitorEntry, load_monitors, remove_monitor, save_monitor};
pub use settings::{load_window_pos, save_window_pos};

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

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

/// Full path of a data file by name inside the data dir.
fn data_path(name: &str) -> PathBuf {
    match data_dir() {
        Some(dir) => dir.join(name),
        None => PathBuf::from("MemoPill").join(name),
    }
}

/// Read + parse a JSON data file; any failure (missing file, bad JSON)
/// yields `T::default()`.
fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(json) = std::fs::read_to_string(path) else {
        return T::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Atomic replace: write a temp file, then rename over the target. A crash
/// mid-write can never leave a truncated file behind.
fn atomic_write(path: &Path, contents: &str) {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("[memo-pill] failed to create data dir: {e}");
        return;
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, contents) {
        eprintln!("[memo-pill] failed to write {}: {e}", tmp.display());
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        eprintln!("[memo-pill] failed to replace {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Serialize `value` as pretty JSON and atomically replace `path`.
fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Ok(json) = serde_json::to_string_pretty(value) else {
        return;
    };
    atomic_write(path, &json);
}
