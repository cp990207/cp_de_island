//! `providers.json`: the monitored provider instances (API keys). Supports
//! multiple instances of the same provider type (e.g. two GLM accounts),
//! distinguished by a stable instance id.

use super::{data_path, read_json, write_json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn providers_file() -> std::path::PathBuf {
    data_path("providers.json")
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

/// Only the new schema is ever written: the legacy api_keys field is dropped
/// entirely so re-migration can't double up on next load.
#[derive(Serialize)]
struct OutFile {
    monitors: Vec<MonitorEntry>,
}

fn write_providers_file(monitors: &[MonitorEntry]) {
    write_json(
        &providers_file(),
        &OutFile {
            monitors: monitors.to_vec(),
        },
    );
}

/// Load all monitor instances. If the file still uses the legacy
/// `{api_keys:{...}}` schema (and has no `monitors` yet), migrate each old
/// entry to `{provider}-mon-1`, persist the new schema, and return it. So the
/// caller always gets the list form and the file is upgraded transparently.
pub fn load_monitors() -> Vec<MonitorEntry> {
    let f: ProvidersFile = read_json(&providers_file());
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
