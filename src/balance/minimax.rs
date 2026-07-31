use super::{Provider, ProviderError, ProviderResult, QuotaInfo};
use serde::Deserialize;

pub struct MiniMaxProvider;

impl MiniMaxProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct MiniMaxResponse {
    #[serde(default)]
    base_resp: Option<MiniMaxBaseResp>,
    #[serde(default)]
    remains: Option<Vec<MiniMaxRemain>>,
    #[serde(default, rename = "plan")]
    _plan: Option<MiniMaxPlan>,
}

#[derive(Deserialize)]
struct MiniMaxBaseResp {
    #[serde(default)]
    status_code: i64,
    #[serde(default)]
    status_msg: String,
}

#[derive(Deserialize)]
struct MiniMaxRemain {
    #[serde(default, rename = "windowName")]
    window_name: String,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    used: u64,
    #[serde(default)]
    remaining: u64,
    #[serde(default, rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Deserialize)]
struct MiniMaxPlan {
    #[serde(default, rename = "planName")]
    _plan_name: Option<String>,
}

const MINIMAX_URL: &str =
    "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains";

#[async_trait::async_trait]
impl Provider for MiniMaxProvider {
    fn name(&self) -> &str {
        "MiniMax"
    }

    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError> {
        let client = reqwest::Client::new();
        let resp = client
            .get(MINIMAX_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth(
                "minimax api token invalid or expired".into(),
            ));
        }

        let body: MiniMaxResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        if let Some(ref base) = body.base_resp {
            if base.status_code != 0 {
                return Err(ProviderError::Parse(format!(
                    "minimax api error {}: {}",
                    base.status_code, base.status_msg
                )));
            }
        }

        let remains = body
            .remains
            .ok_or_else(|| ProviderError::Parse("no remains data in response".into()))?;

        let quotas: Vec<QuotaInfo> = remains
            .into_iter()
            .map(|r| QuotaInfo {
                provider: "MiniMax".into(),
                window: r.window_name,
                limit: r.total,
                used: r.used,
                remaining: r.remaining,
                reset_at: r.reset_time,
            })
            .collect();

        Ok(ProviderResult::Quota(quotas))
    }
}
