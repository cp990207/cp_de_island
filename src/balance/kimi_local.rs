//! Local Kimi Code CLI session-log mining for token usage and cost stats.
//!
//! Reads `~/.kimi-code/sessions/**/wire.jsonl` and picks out `usage.record`
//! lines, each of which carries the model, four token counters and a
//! millisecond timestamp in a single record:
//!
//! ```json
//! {"type":"usage.record","model":"kimi-code/k3-256k",
//!  "usage":{"inputOther":2656,"output":145,"inputCacheRead":18688,"inputCacheCreation":0},
//!  "usageScope":"turn","time":1785594051490}
//! ```
//!
//! Parsed records are cached per file (keyed by mtime+size) so the periodic
//! re-scan only re-reads files that actually changed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::PathBuf;

/// Four token classes reported by the CLI. `input` is the non-cached part.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct TokenCount {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl std::ops::AddAssign for TokenCount {
    fn add_assign(&mut self, o: Self) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
    }
}

impl TokenCount {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    pub ts_ms: i64,
    pub model: String,
    pub tokens: TokenCount,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FileScan {
    mtime: i64,
    size: u64,
    records: Vec<UsageRecord>,
}

/// On-disk scan cache: path -> (file identity, records parsed from it).
/// Replacing a file's whole record set on rescan keeps counts correct even
/// though wire.jsonl is append-only.
#[derive(Default, Serialize, Deserialize)]
struct ScanCache {
    files: BTreeMap<String, FileScan>,
}

fn sessions_root() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    let dir = PathBuf::from(home).join(".kimi-code").join("sessions");
    dir.is_dir().then_some(dir)
}

/// usage.record line shape; everything else in the wire is ignored.
#[derive(Deserialize)]
struct WireLine {
    model: Option<String>,
    usage: Option<WireUsage>,
    #[serde(rename = "usageScope")]
    usage_scope: Option<String>,
    time: Option<i64>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(rename = "inputOther", default)]
    input: u64,
    #[serde(default)]
    output: u64,
    #[serde(rename = "inputCacheRead", default)]
    cache_read: u64,
    #[serde(rename = "inputCacheCreation", default)]
    cache_write: u64,
}

const NEEDLE: &str = "\"type\":\"usage.record\"";

/// Normalize `kimi-code/k3-256k` style aliases to the bare lowercase model id.
fn normalize_model(raw: &str) -> String {
    raw.rsplit('/').next().unwrap_or(raw).to_lowercase()
}

fn scan_file(path: &std::path::Path) -> Vec<UsageRecord> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        if !line.contains(NEEDLE) {
            continue;
        }
        let Ok(w) = serde_json::from_str::<WireLine>(&line) else {
            continue;
        };
        // Only turn-scope records are full per-request totals; anything else
        // (session scope, other events) would double-count against them.
        if w.usage_scope.as_deref() != Some("turn") {
            continue;
        }
        let (Some(model), Some(usage), Some(ts_ms)) = (w.model, w.usage, w.time) else {
            continue;
        };
        records.push(UsageRecord {
            ts_ms,
            model: normalize_model(&model),
            tokens: TokenCount {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
            },
        });
    }
    records
}

fn file_identity(path: &std::path::Path) -> Option<(i64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((mtime, meta.len()))
}

fn collect_wire_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // sessions/<workspace>/<session>/agents/<agent>/wire.jsonl — walk a fixed
    // depth instead of a recursive glob so unrelated trees are never touched.
    for ws in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for sess in std::fs::read_dir(ws.path()).into_iter().flatten().flatten() {
            let agents = sess.path().join("agents");
            for agent in std::fs::read_dir(agents).into_iter().flatten().flatten() {
                let wire = agent.path().join("wire.jsonl");
                if wire.is_file() {
                    out.push(wire);
                }
            }
        }
    }
    out
}

/// Scan all session logs, returning every usage record (sorted by time).
/// The scan cache lives next to the other app data and keeps repeat scans
/// cheap: unchanged files are skipped, deleted files are dropped.
pub fn load_records(cache_path: &std::path::Path) -> Vec<UsageRecord> {
    let mut cache: ScanCache = std::fs::read_to_string(cache_path)
        .ok()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    let Some(root) = sessions_root() else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut dirty = false;
    for path in collect_wire_files(&root) {
        let key = path.to_string_lossy().to_string();
        seen.insert(key.clone());
        let Some((mtime, size)) = file_identity(&path) else {
            continue;
        };
        let fresh = cache
            .files
            .get(&key)
            .is_some_and(|f| f.mtime == mtime && f.size == size);
        if fresh {
            continue;
        }
        let records = scan_file(&path);
        cache.files.insert(
            key,
            FileScan {
                mtime,
                size,
                records,
            },
        );
        dirty = true;
    }
    let before = cache.files.len();
    cache.files.retain(|k, _| seen.contains(k));
    dirty = dirty || cache.files.len() != before;

    if dirty {
        if let Some(dir) = cache_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(&cache) {
            // Same atomic-replace pattern as the other app data files.
            let tmp = cache_path.with_extension("tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, cache_path);
            }
        }
    }

    let mut records: Vec<UsageRecord> = cache
        .files
        .values()
        .flat_map(|f| f.records.iter().cloned())
        .collect();
    records.sort_by_key(|r| r.ts_ms);
    records
}

fn local_day(ts_ms: i64) -> String {
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let dt = time::OffsetDateTime::from_unix_timestamp(ts_ms / 1000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(offset);
    format!(
        "{:04}-{:02}-{:02}",
        dt.date().year(),
        dt.date().month() as u8,
        dt.date().day()
    )
}

fn today_string() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{:04}-{:02}-{:02}",
        now.date().year(),
        now.date().month() as u8,
        now.date().day()
    )
}

/// CNY list price per 1M tokens for one model. Cache-write tokens have no
/// separate rate on the Kimi open platform and are billed at the input rate.
#[derive(Clone, Copy, Debug)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

/// Kimi open-platform CNY list prices (per 1M tokens), mirroring the table
/// codexBar uses for its equivalent-cost estimate. k3-256k is half the k3
/// rate. Unknown models return None and are excluded from cost figures
/// (their token counts still show up in the token stats).
pub fn price_for(model: &str) -> Option<ModelPrice> {
    match model {
        "kimi-for-coding" => Some(ModelPrice {
            input: 6.5,
            output: 27.0,
            cache_read: 1.3,
        }),
        "kimi-for-coding-highspeed" => Some(ModelPrice {
            input: 13.0,
            output: 54.0,
            cache_read: 2.6,
        }),
        "k3" => Some(ModelPrice {
            input: 20.0,
            output: 100.0,
            cache_read: 2.0,
        }),
        "k3-256k" => Some(ModelPrice {
            input: 10.0,
            output: 50.0,
            cache_read: 1.0,
        }),
        _ => None,
    }
}

pub fn cost_of(model: &str, t: &TokenCount) -> Option<f64> {
    let p = price_for(model)?;
    Some(
        ((t.input + t.cache_write) as f64 * p.input
            + t.output as f64 * p.output
            + t.cache_read as f64 * p.cache_read)
            / 1_000_000.0,
    )
}

/// One day's aggregate for the cost chart.
#[derive(Clone, Copy, Debug, Default)]
pub struct DailyStat {
    pub cost: f64,
    pub tokens: TokenCount,
}

/// Everything the cost panel needs, pre-aggregated.
#[derive(Clone, Debug, Default)]
pub struct CostReport {
    pub today_cost: f64,
    pub month_cost: f64,
    pub today_tokens: TokenCount,
    pub month_tokens: TokenCount,
    pub today_requests: u64,
    pub last_request: Option<(i64, String, TokenCount)>,
    /// (day, stats) ascending, trailing 30 days.
    pub daily: Vec<(String, DailyStat)>,
}

/// Unified read-only view over the two cost-report structs
/// (`kimi_local::CostReport` and `zcode_local::ZcodeCostReport`) so a single
/// rendering function can drive the detail block for any provider. The two
/// structs share their first seven fields verbatim; `by_source` defaults to
/// an empty slice for Kimi (which has no such dimension).
pub trait CostData {
    fn today_cost(&self) -> f64;
    fn month_cost(&self) -> f64;
    fn today_tokens(&self) -> &TokenCount;
    fn month_tokens(&self) -> &TokenCount;
    fn today_requests(&self) -> u64;
    fn last_request(&self) -> Option<&(i64, String, TokenCount)>;
    fn daily(&self) -> &[(String, DailyStat)];
    fn by_source(&self) -> &[super::zcode_local::DimensionStat] {
        &[]
    }
}

impl CostData for CostReport {
    fn today_cost(&self) -> f64 {
        self.today_cost
    }
    fn month_cost(&self) -> f64 {
        self.month_cost
    }
    fn today_tokens(&self) -> &TokenCount {
        &self.today_tokens
    }
    fn month_tokens(&self) -> &TokenCount {
        &self.month_tokens
    }
    fn today_requests(&self) -> u64 {
        self.today_requests
    }
    fn last_request(&self) -> Option<&(i64, String, TokenCount)> {
        self.last_request.as_ref()
    }
    fn daily(&self) -> &[(String, DailyStat)] {
        &self.daily
    }
}

pub fn build_report(records: &[UsageRecord]) -> CostReport {
    let mut report = CostReport::default();
    let today = today_string();
    // 30-day rolling window boundary as a ymd string (string compare works
    // on zero-padded dates).
    let cutoff = {
        let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let d = now.date() - time::Duration::days(29);
        format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
    };
    let mut daily: BTreeMap<String, DailyStat> = BTreeMap::new();

    for r in records {
        let day = local_day(r.ts_ms);
        // Unknown models have no list price: excluded from cost, but their
        // tokens still count toward the token totals.
        let cost = cost_of(&r.model, &r.tokens).unwrap_or(0.0);
        if day >= cutoff {
            report.month_cost += cost;
            report.month_tokens += r.tokens;
            let d = daily.entry(day.clone()).or_default();
            d.cost += cost;
            d.tokens += r.tokens;
        }
        if day == today {
            report.today_cost += cost;
            report.today_tokens += r.tokens;
            report.today_requests += 1;
        }
    }
    report.daily = daily.into_iter().collect();
    report.last_request = records
        .last()
        .map(|r| (r.ts_ms, r.model.clone(), r.tokens));
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("memo-pill-test-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn scan_picks_turn_records_and_normalizes_model() {
        let path = write_temp(
            "scan",
            concat!(
                r#"{"type":"usage.record","model":"kimi-code/k3-256k","usage":{"inputOther":10,"output":2,"inputCacheRead":30,"inputCacheCreation":4},"usageScope":"turn","time":1785594051490}"#,
                "\n",
                // session scope must be skipped (would double-count)
                r#"{"type":"usage.record","model":"k3","usage":{"inputOther":999,"output":0,"inputCacheRead":0,"inputCacheCreation":0},"usageScope":"session","time":1785594051491}"#,
                "\n",
                // other event types are ignored even when they carry usage
                r#"{"type":"context.append_loop_event","event":{"type":"step.end","usage":{"inputOther":1,"output":1,"inputCacheRead":1,"inputCacheCreation":1}},"time":1785594051492}"#,
                "\n",
                "not json at all\n",
            ),
        );
        let records = scan_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.model, "k3-256k");
        assert_eq!(r.tokens.input, 10);
        assert_eq!(r.tokens.output, 2);
        assert_eq!(r.tokens.cache_read, 30);
        assert_eq!(r.tokens.cache_write, 4);
        assert_eq!(r.ts_ms, 1785594051490);
    }

    #[test]
    fn prices_match_kimi_list() {
        let k3 = price_for("k3").unwrap();
        assert_eq!((k3.input, k3.output, k3.cache_read), (20.0, 100.0, 2.0));
        let k3_256k = price_for("k3-256k").unwrap();
        assert_eq!(
            (k3_256k.input, k3_256k.output, k3_256k.cache_read),
            (10.0, 50.0, 1.0)
        );
        assert!(price_for("mystery-model").is_none());
    }

    #[test]
    fn cost_uses_input_rate_for_cache_write() {
        // 1M input + 1M cache write + 1M cache read + 1M output on k3
        // = ¥20 + ¥20 + ¥2 + ¥100 = ¥142
        let t = TokenCount {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write: 1_000_000,
        };
        assert_eq!(cost_of("k3", &t), Some(142.0));
    }

    #[test]
    fn unknown_model_costs_nothing_but_counts_tokens() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let records = vec![
            UsageRecord {
                ts_ms: now_ms,
                model: "mystery".into(),
                tokens: TokenCount {
                    input: 1000,
                    output: 500,
                    cache_read: 0,
                    cache_write: 0,
                },
            },
            UsageRecord {
                ts_ms: now_ms,
                model: "k3".into(),
                tokens: TokenCount {
                    input: 1_000_000,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                },
            },
        ];
        let report = build_report(&records);
        assert_eq!(report.today_requests, 2);
        assert_eq!(report.today_tokens.input, 1_001_000);
        // Only the k3 request is priced: 1M input @ ¥20.
        assert_eq!(report.today_cost, 20.0);
        assert!(report.last_request.is_some());
        // Today's daily entry carries both cost and total tokens.
        let (_, day) = &report.daily[0];
        assert_eq!(day.cost, 20.0);
        assert_eq!(day.tokens.total(), 1_001_500);
    }
}
