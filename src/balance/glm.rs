use super::{BalanceInfo, Provider, ProviderError, ProviderResult};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

pub struct GlmProvider;

impl GlmProvider {
    pub fn new() -> Self {
        Self
    }
}

fn generate_jwt(api_key: &str) -> Result<String, ProviderError> {
    let parts: Vec<&str> = api_key.split('.').collect();
    if parts.len() != 2 {
        return Err(ProviderError::Auth(
            "glm api key must be in format 'id.secret'".into(),
        ));
    }
    let secret = parts[1].as_bytes();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let header = serde_json::json!({
        "alg": "HS256",
        "sign_type": "SIGN"
    });
    let payload = serde_json::json!({
        "api_key": api_key,
        "exp": now + 3600_000,
        "timestamp": now,
    });

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
    let signing_input = format!("{header_b64}.{payload_b64}");

    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|e| ProviderError::Auth(format!("hmac key error: {e}")))?;
    mac.update(signing_input.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    Ok(format!("{signing_input}.{sig}"))
}

#[derive(Deserialize)]
struct GlmResponse {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Option<GlmBalanceData>,
}

#[derive(Deserialize)]
struct GlmBalanceData {
    #[serde(default)]
    balance: f64,
    #[serde(default, rename = "totalBalance")]
    total_balance: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
}

#[async_trait::async_trait]
impl Provider for GlmProvider {
    fn name(&self) -> &str {
        "GLM"
    }

    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError> {
        let token = generate_jwt(api_key)?;

        let client = reqwest::Client::new();
        let resp = client
            .get("https://open.bigmodel.cn/api/paas/v4/dashboard/billing/balance")
            .bearer_auth(token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(ProviderError::Auth(
                "glm api key invalid or expired".into(),
            ));
        }

        let body: GlmResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        if let Some(ref code) = body.code {
            if code != "200" && code != "0" {
                return Err(ProviderError::Parse(format!(
                    "glm api error {}: {}",
                    code,
                    body.message.unwrap_or_default()
                )));
            }
        }

        let data = body
            .data
            .ok_or_else(|| ProviderError::Parse("no balance data in response".into()))?;

        let total = data.total_balance.unwrap_or(data.balance);

        Ok(ProviderResult::Balance(BalanceInfo {
            provider: "GLM".into(),
            currency: data.currency.unwrap_or_else(|| "CNY".into()),
            total,
            used: 0.0,
            remaining: data.balance,
            breakdown: None,
        }))
    }
}
