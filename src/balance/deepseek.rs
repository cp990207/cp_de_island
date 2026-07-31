use super::{BalanceBreakdown, BalanceInfo, Provider, ProviderError, ProviderResult};
use serde::Deserialize;

pub struct DeepSeekProvider;

impl DeepSeekProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct DeepSeekResponse {
    #[serde(default, rename = "is_available")]
    _is_available: bool,
    #[serde(default, rename = "balance_infos")]
    balance_infos: Vec<DeepSeekBalance>,
}

#[derive(Deserialize)]
struct DeepSeekBalance {
    #[serde(default)]
    currency: String,
    #[serde(default, rename = "total_balance")]
    total_balance: String,
    #[serde(default, rename = "granted_balance")]
    granted_balance: String,
    #[serde(default, rename = "topped_up_balance")]
    topped_up_balance: String,
}

fn parse_f64(s: &str) -> Result<f64, ProviderError> {
    s.parse::<f64>()
        .map_err(|e| ProviderError::Parse(format!("invalid number '{s}': {e}")))
}

#[async_trait::async_trait]
impl Provider for DeepSeekProvider {
    fn name(&self) -> &str {
        "DeepSeek"
    }

    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError> {
        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.deepseek.com/user/balance")
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth(
                "deepseek api key invalid or expired".into(),
            ));
        }

        let body: DeepSeekResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let entry = body
            .balance_infos
            .iter()
            .find(|b| b.currency.to_uppercase() == "USD")
            .or_else(|| body.balance_infos.first())
            .ok_or_else(|| ProviderError::Parse("no balance info returned".into()))?;

        let total = parse_f64(&entry.total_balance)?;
        let granted = parse_f64(&entry.granted_balance)?;
        let paid = parse_f64(&entry.topped_up_balance)?;

        Ok(ProviderResult::Balance(BalanceInfo {
            provider: "DeepSeek".into(),
            currency: entry.currency.clone(),
            total,
            used: 0.0,
            remaining: total,
            breakdown: Some(BalanceBreakdown { paid, granted }),
        }))
    }
}
