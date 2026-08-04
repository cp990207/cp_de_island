//! ZCode 本地用量挖掘 —— 从 ZCode 客户端的 SQLite 库读取 model_usage，
//! 聚合成与 kimi_local::CostReport 同口径的成本报告（外加请求来源分布，
//! 这是 ZCode 数据比 Kimi 多出来的维度）。
//!
//! 数据源：%USERPROFILE%\.zcode\cli\db\db.sqlite（WAL 模式，只读连接即可）。
//! token 口径：ZCode 的 input_tokens 已含 cache_read，需拆成非缓存输入 +
//! 缓存读，与 kimi_local 的 TokenCount 语义对齐后才能共用成本算法。

use super::kimi_local::{DailyStat, ModelPrice, TokenCount};
use rusqlite::Connection;
use std::collections::BTreeMap;

/// 一条 model_usage 记录（已做 token 口径转换）。
#[derive(Clone, Debug)]
struct ZcodeRecord {
    ts_ms: i64,
    model: String,
    source: String,
    tokens: TokenCount,
}

/// 请求来源维度的聚合条目：(名称, 请求数, token)。
#[derive(Clone, Debug)]
pub struct DimensionStat {
    pub name: String,
    pub requests: u64,
    pub tokens: TokenCount,
    pub cost: f64,
}

/// 面板所需的全部聚合数据。today/month/daily 与 kimi_local::CostReport 同口径，
/// by_source 是 ZCode 独有的请求来源维度。
#[derive(Clone, Debug, Default)]
pub struct ZcodeCostReport {
    pub today_cost: f64,
    pub month_cost: f64,
    pub today_tokens: TokenCount,
    pub month_tokens: TokenCount,
    pub today_requests: u64,
    pub last_request: Option<(i64, String, TokenCount)>,
    /// (day, stats) 升序，最近 30 天。
    pub daily: Vec<(String, DailyStat)>,
    /// 请求来源分布（main_turn / subagent / session_title / compact）。
    pub by_source: Vec<DimensionStat>,
}

impl super::kimi_local::CostData for ZcodeCostReport {
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
    fn by_source(&self) -> &[DimensionStat] {
        &self.by_source
    }
}

fn db_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(std::path::PathBuf::from(home).join(".zcode").join("cli").join("db").join("db.sqlite"))
}

/// GLM 系列 CNY 列表价（每 1M token），来自 bigmodel.cn/pricing 与
/// docs.z.ai 国际版定价（cache_read 比例 0.26/1.4 ≈ 18.57%，国内对应约 ¥1.5）。
/// 未知模型返回 None：不计成本，但 token 仍计入统计。
fn glm_price_for(model: &str) -> Option<ModelPrice> {
    match model {
        "glm-5.2" | "glm-5.1" => Some(ModelPrice {
            input: 8.0,
            output: 28.0,
            cache_read: 1.5,
        }),
        "glm-5" => Some(ModelPrice {
            input: 7.2,
            output: 23.0,
            cache_read: 1.4,
        }),
        "glm-5-turbo" => Some(ModelPrice {
            input: 8.6,
            output: 28.8,
            cache_read: 1.7,
        }),
        "glm-4.6" | "glm-4.7" | "glm-4.5" => Some(ModelPrice {
            input: 4.3,
            output: 15.8,
            cache_read: 0.8,
        }),
        _ => None,
    }
}

fn cost_of(model: &str, t: &TokenCount) -> Option<f64> {
    let p = glm_price_for(model)?;
    Some(
        ((t.input + t.cache_write) as f64 * p.input
            + t.output as f64 * p.output
            + t.cache_read as f64 * p.cache_read)
            / 1_000_000.0,
    )
}

/// 把毫秒时间戳转成本地 ymd 字符串（与 kimi_local::local_day 同口径）。
fn local_day(ts_ms: i64) -> String {
    let secs = ts_ms / 1000;
    let t = time::OffsetDateTime::from_unix_timestamp(secs).unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let local = t.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    format!("{:04}-{:02}-{:02}", local.year(), local.month() as u8, local.day())
}

fn today_string() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:04}-{:02}-{:02}", now.year(), now.month() as u8, now.day())
}

/// 读取 ZCode SQLite，返回全部已完成请求的记录。库不存在 / 被独占 /
/// 表缺失时返回空 Vec（绝不 panic —— ZCode 未安装是正常情况）。
fn load_records() -> Vec<ZcodeRecord> {
    let Some(path) = db_path() else {
        return Vec::new();
    };
    if !path.exists() {
        return Vec::new();
    }
    // 只读 URI 连接：即使 ZCode 正在写（WAL），也能读到已提交数据，
    // 且绝不会锁住对方的库。Windows 路径的反斜杠在 URI 里要编码成 /。
    let uri = format!(
        "file:{}?mode=ro",
        path.to_string_lossy().replace('\\', "/")
    );
    let conn = match Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut stmt = match conn.prepare(
        "SELECT m.started_at, m.model_id, m.query_source,
                m.input_tokens, m.output_tokens, m.cache_read_input_tokens, m.reasoning_tokens
         FROM model_usage m
         WHERE m.status = 'completed'",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        let input_total: i64 = row.get(3)?;
        let cache_read: i64 = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
        let output: i64 = row.get(4)?;
        // reasoning_tokens 计费上属输出，并入 output 侧。
        let reasoning: i64 = row.get::<_, Option<i64>>(6)?.unwrap_or(0);
        // ZCode 的 input_tokens 已含 cache_read；拆成非缓存输入 + 缓存读，
        // 与 kimi_local::TokenCount（input = 非缓存部分）语义对齐。
        let non_cache = (input_total - cache_read).max(0) as u64;
        Ok(ZcodeRecord {
            ts_ms: row.get(0)?,
            model: row.get::<_, String>(1)?.to_lowercase(),
            source: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            tokens: TokenCount {
                input: non_cache,
                output: (output + reasoning).max(0) as u64,
                cache_read: cache_read.max(0) as u64,
                cache_write: 0,
            },
        })
    }) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    rows.filter_map(Result::ok).collect()
}

pub fn build_report() -> ZcodeCostReport {
    let records = load_records();
    if records.is_empty() {
        return ZcodeCostReport::default();
    }

    let today = today_string();
    let cutoff = {
        let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let d = now.date() - time::Duration::days(29);
        format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
    };

    let mut report = ZcodeCostReport::default();
    let mut daily: BTreeMap<String, DailyStat> = BTreeMap::new();
    let mut by_source: BTreeMap<String, DimensionStat> = BTreeMap::new();

    for r in &records {
        let day = local_day(r.ts_ms);
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

        // 来源维度。
        let src = by_source.entry(r.source.clone()).or_insert_with(|| DimensionStat {
            name: r.source.clone(),
            requests: 0,
            tokens: TokenCount::default(),
            cost: 0.0,
        });
        src.requests += 1;
        src.tokens += r.tokens;
        src.cost += cost;
    }

    report.daily = daily.into_iter().collect();
    report.by_source = by_source.into_values().collect();
    // 来源按请求数降序，便于稳定展示。
    report.by_source.sort_by(|a, b| b.requests.cmp(&a.requests));

    report.last_request = records.last().map(|r| (r.ts_ms, r.model.clone(), r.tokens));
    report
}
