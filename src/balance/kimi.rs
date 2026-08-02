use super::{Provider, ProviderError, ProviderResult, QuotaInfo, QuotaSet};
use serde::Deserialize;

pub struct KimiProvider;

impl KimiProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct KimiResponse {
    usage: KimiUsage,
    #[serde(default)]
    limits: Vec<KimiLimit>,
    #[serde(default)]
    user: Option<KimiUser>,
}

#[derive(Deserialize)]
struct KimiUser {
    #[serde(default)]
    membership: Option<KimiMembership>,
}

#[derive(Deserialize)]
struct KimiMembership {
    #[serde(default)]
    level: Option<String>,
}

/// Display name for a `LEVEL_*` membership id. Matches codexBar: the one
/// confirmed mapping is hardcoded, everything else falls back to a
/// humanized suffix so future levels still render something readable.
fn plan_display(level: Option<&str>) -> Option<String> {
    let level = level?;
    if level == "LEVEL_INTERMEDIATE" {
        return Some("Allegretto".into());
    }
    let pretty = level
        .strip_prefix("LEVEL_")
        .unwrap_or(level)
        .to_lowercase();
    let mut chars = pretty.chars();
    Some(match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => return None,
    })
}

#[derive(Deserialize)]
struct KimiUsage {
    limit: String,
    #[serde(default)]
    used: Option<String>,
    remaining: String,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Deserialize)]
struct KimiLimit {
    window: KimiWindow,
    detail: KimiUsage,
}

#[derive(Deserialize)]
struct KimiWindow {
    duration: u64,
    #[serde(rename = "timeUnit")]
    time_unit: String,
}

fn parse_u64(s: &str) -> Result<u64, ProviderError> {
    s.parse::<u64>()
        .map_err(|e| ProviderError::Parse(format!("invalid number '{s}': {e}")))
}

fn window_label(w: &KimiWindow) -> String {
    match w.time_unit.as_str() {
        // The 300-minute window is the well-known 5-hour session quota —
        // render whole hours instead of "300min".
        "TIME_UNIT_MINUTE" if w.duration >= 60 && w.duration % 60 == 0 => {
            format!("{}h", w.duration / 60)
        }
        "TIME_UNIT_MINUTE" => format!("{}min", w.duration),
        "TIME_UNIT_HOUR" => format!("{}h", w.duration),
        "TIME_UNIT_DAY" => format!("{}d", w.duration),
        _ => format!("{} {}", w.duration, w.time_unit),
    }
}

#[async_trait::async_trait]
impl Provider for KimiProvider {
    fn name(&self) -> &str {
        "Kimi"
    }

    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError> {
        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.kimi.com/coding/v1/usages")
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth("kimi api key invalid or expired".into()));
        }

        let body: KimiResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let mut quotas = Vec::new();

        // Rate windows first (5h), weekly last: the pill line and the
        // detail rows read in this order.
        for limit in &body.limits {
            let l = parse_u64(&limit.detail.limit)?;
            let r = parse_u64(&limit.detail.remaining)?;
            // Some window details omit "used"; compute as limit - remaining.
            let u = match limit.detail.used.as_deref() {
                Some(s) if !s.is_empty() => parse_u64(s)?,
                _ => l.saturating_sub(r),
            };
            quotas.push(QuotaInfo {
                provider: "Kimi".into(),
                window: window_label(&limit.window),
                limit: l,
                used: u,
                remaining: r,
                reset_at: limit.detail.reset_time.clone(),
            });
        }

        let weekly_limit = parse_u64(&body.usage.limit)?;
        let weekly_used = parse_u64(body.usage.used.as_deref().unwrap_or("0"))?;
        let weekly_remaining = parse_u64(&body.usage.remaining)?;
        quotas.push(QuotaInfo {
            provider: "Kimi".into(),
            window: "weekly".into(),
            limit: weekly_limit,
            used: weekly_used,
            remaining: weekly_remaining,
            reset_at: body.usage.reset_time.clone(),
        });

        Ok(ProviderResult::Quota(QuotaSet {
            plan: plan_display(
                body.user
                    .as_ref()
                    .and_then(|u| u.membership.as_ref())
                    .and_then(|m| m.level.as_deref()),
            ),
            quotas,
        }))
    }
}
