//! `settings.json`: window placement, kept separate from the memos file so
//! the memos rewrite cycle never has to know about it.

use super::{data_path, write_json};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    window_x: i32,
    window_y: i32,
}

fn settings_file() -> std::path::PathBuf {
    data_path("settings.json")
}

pub fn load_window_pos() -> Option<(i32, i32)> {
    let json = std::fs::read_to_string(settings_file()).ok()?;
    let s: SettingsFile = serde_json::from_str(&json).ok()?;
    Some((s.window_x, s.window_y))
}

pub fn save_window_pos(x: i32, y: i32) {
    write_json(&settings_file(), &SettingsFile {
        window_x: x,
        window_y: y,
    });
}
