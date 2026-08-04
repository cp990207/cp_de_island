use super::{Provider, ProviderError, ProviderResult, QuotaInfo, QuotaSet};
use serde::Deserialize;

pub struct GlmProvider;

impl GlmProvider {
    pub fn new() -> Self {
        Self
    }
}

/// GLM Coding Plan 用量查询返回结构。
/// 端点：GET https://open.bigmodel.cn/api/monitor/usage/quota/limit
/// 认证：Authorization: <API_TOKEN>（裸 key，不带 Bearer 前缀）。
/// 该端点与官方 glm-plan-usage 插件、cc-switch 社区脚本一致。
#[derive(Deserialize)]
struct GlmResponse {
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<GlmData>,
}

#[derive(Deserialize)]
struct GlmData {
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    limits: Vec<GlmLimit>,
}

#[derive(Deserialize)]
struct GlmLimit {
    /// "TOKENS_LIMIT"（5h / weekly）或 "TIME_LIMIT"（MCP 每月）
    #[serde(rename = "type")]
    kind: String,
    /// 已用百分比（TOKENS_LIMIT）；TIME_LIMIT 可能缺省
    #[serde(default)]
    percentage: Option<u64>,
    /// 下次重置时间，毫秒时间戳（TOKENS_LIMIT 两个对象按它升序区分 5h/weekly）
    #[serde(default, rename = "nextResetTime")]
    next_reset_time: Option<i64>,
    /// TIME_LIMIT 的总量 / 已用 / 剩余
    #[serde(default)]
    usage: Option<u64>,
    #[serde(default, rename = "currentValue")]
    current_value: Option<u64>,
    #[serde(default)]
    remaining: Option<u64>,
}

/// 把毫秒（非法时按秒）时间戳转成 Rfc3339，供面板 reset_label 复用。
fn ms_to_rfc3339(ts: i64) -> Option<String> {
    // 1000_000_000_000 以下视为秒（13 位才是毫秒）。
    let secs = if ts > 1_000_000_000_000 { ts / 1000 } else { ts };
    let t = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    Some(t.format(&time::format_description::well_known::Rfc3339).ok()?)
}

/// 把"已用百分比"映射成 limit=100 的 QuotaInfo，方便复用 Kimi 的进度条渲染。
fn pct_quota(provider: &str, window: &str, pct: Option<u64>, reset_ts: Option<i64>) -> QuotaInfo {
    let used = pct.unwrap_or(0).min(100);
    QuotaInfo {
        provider: provider.into(),
        window: window.into(),
        limit: 100,
        used,
        remaining: 100 - used,
        reset_at: reset_ts.and_then(ms_to_rfc3339),
    }
}

#[async_trait::async_trait]
impl Provider for GlmProvider {
    fn name(&self) -> &str {
        "GLM"
    }

    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError> {
        let client = reqwest::Client::new();
        let resp = client
            .get("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
            // GLM 的 monitor 端点用裸 key 鉴权，不带 "Bearer " 前缀。
            .header("Authorization", api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth("glm api key invalid or expired".into()));
        }

        let body: GlmResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        if !body.success {
            return Err(ProviderError::Parse(format!(
                "glm api error: {}",
                body.msg.unwrap_or_else(|| "unknown error".into())
            )));
        }

        let data = body
            .data
            .ok_or_else(|| ProviderError::Parse("no usage data in response".into()))?;

        let mut quotas = Vec::new();

        // 两个 TOKENS_LIMIT 按 nextResetTime 升序：较早重置的是 5h，较晚的是 weekly。
        let mut token_limits: Vec<&GlmLimit> = data
            .limits
            .iter()
            .filter(|l| l.kind == "TOKENS_LIMIT")
            .collect();
        token_limits.sort_by_key(|l| l.next_reset_time.unwrap_or(0));
        if let Some(h5) = token_limits.first() {
            quotas.push(pct_quota(
                "GLM",
                "5h",
                h5.percentage,
                h5.next_reset_time,
            ));
        }
        if let Some(wk) = token_limits.get(1) {
            quotas.push(pct_quota(
                "GLM",
                "weekly",
                wk.percentage,
                wk.next_reset_time,
            ));
        }

        // TIME_LIMIT = MCP 每月次数。
        if let Some(mcp) = data.limits.iter().find(|l| l.kind == "TIME_LIMIT") {
            let limit = mcp.usage.unwrap_or(0);
            let used = mcp.current_value.unwrap_or(0);
            quotas.push(QuotaInfo {
                provider: "GLM".into(),
                window: "MCP".into(),
                limit,
                used,
                remaining: mcp.remaining.unwrap_or_else(|| limit.saturating_sub(used)),
                reset_at: None,
            });
        }

        Ok(ProviderResult::Quota(QuotaSet {
            plan: data.level,
            quotas,
        }))
    }
}
