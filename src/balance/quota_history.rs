//! Per-cycle quota consumption history ("plan utilization"), mirroring
//! codexBar's history view: every quota fetch samples the current windows,
//! and samples are compressed to one peak entry per cycle. A cycle is
//! identified by its server-provided reset timestamp; usage only grows
//! within a cycle, so the max observed percent per reset timestamp is the
//! per-cycle consumption series directly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CyclePeak {
    /// Cycle identity: the window's reset timestamp as reported by the API.
    pub reset: String,
    /// Highest used-percent observed in this cycle (0-100).
    pub pct: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuotaHistory {
    /// 5-hour session window peaks, oldest first.
    pub session: Vec<CyclePeak>,
    /// Weekly window peaks, oldest first.
    pub weekly: Vec<CyclePeak>,
}

/// How many cycles each series retains: ~10 days of 5h windows, ~3 months
/// of weekly windows.
const SESSION_KEEP: usize = 48;
const WEEKLY_KEEP: usize = 12;

#[derive(Clone, Copy, PartialEq)]
pub enum Series {
    Session,
    Weekly,
}

impl QuotaHistory {
    /// Record one sample. `reset_at` is the cycle identity; samples without
    /// one are dropped. Returns true when the history changed.
    pub fn record(&mut self, series: Series, reset_at: Option<&str>, used: u64, limit: u64) -> bool {
        let (Some(reset), true) = (reset_at, limit > 0) else {
            return false;
        };
        let pct = (used as f64 / limit as f64 * 100.0).clamp(0.0, 100.0);
        let (entries, keep) = match series {
            Series::Session => (&mut self.session, SESSION_KEEP),
            Series::Weekly => (&mut self.weekly, WEEKLY_KEEP),
        };
        if let Some(last) = entries.last_mut() {
            if last.reset == reset {
                if pct > last.pct {
                    last.pct = pct;
                    return true;
                }
                return false;
            }
        }
        entries.push(CyclePeak {
            reset: reset.to_string(),
            pct,
        });
        if entries.len() > keep {
            let overflow = entries.len() - keep;
            entries.drain(..overflow);
        }
        true
    }
}

/// Path for a named history file inside the app data dir. Shared by Kimi
/// (kimi-quota-history.json) and GLM (glm-quota-history.json) so each
/// provider keeps its own per-cycle series.
fn named_path(filename: &str) -> Option<std::path::PathBuf> {
    Some(crate::storage::data_dir()?.join(filename))
}

pub fn load() -> QuotaHistory {
    load_named("kimi-quota-history.json")
}

/// Load a named history file (e.g. "glm-quota-history.json").
pub fn load_named(filename: &str) -> QuotaHistory {
    let Some(path) = named_path(filename) else {
        return QuotaHistory::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

pub fn save(history: &QuotaHistory) {
    save_named("kimi-quota-history.json", history);
}

/// Save a named history file (e.g. "glm-quota-history.json").
pub fn save_named(filename: &str, history: &QuotaHistory) {
    let Some(path) = named_path(filename) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(json) = serde_json::to_string_pretty(history) else {
        return;
    };
    // Atomic replace, same pattern as the other app data files.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_cycle_keeps_peak_only() {
        let mut h = QuotaHistory::default();
        assert!(h.record(Series::Session, Some("r1"), 10, 100));
        assert!(h.record(Series::Session, Some("r1"), 40, 100));
        // A lower sample inside the same cycle must not move the peak.
        assert!(!h.record(Series::Session, Some("r1"), 25, 100));
        assert_eq!(h.session.len(), 1);
        assert_eq!(h.session[0].pct, 40.0);
    }

    #[test]
    fn new_reset_starts_new_cycle_and_caps() {
        let mut h = QuotaHistory::default();
        for i in 0..60 {
            h.record(Series::Session, Some(&format!("r{i}")), 1, 2);
        }
        assert_eq!(h.session.len(), SESSION_KEEP);
        // Oldest entries are trimmed first.
        assert_eq!(h.session[0].reset, "r12");
        assert_eq!(h.session.last().unwrap().reset, "r59");
    }

    #[test]
    fn unusable_samples_are_dropped() {
        let mut h = QuotaHistory::default();
        assert!(!h.record(Series::Weekly, None, 10, 100));
        assert!(!h.record(Series::Weekly, Some("r1"), 10, 0));
        assert!(h.weekly.is_empty());
    }
}
