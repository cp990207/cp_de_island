pub mod deepseek;
pub mod glm;
pub mod kimi;
pub mod kimi_local;
pub mod minimax;
pub mod quota_history;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub provider: String,
    pub currency: String,
    pub total: f64,
    pub used: f64,
    pub remaining: f64,
    #[serde(default)]
    pub breakdown: Option<BalanceBreakdown>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceBreakdown {
    pub paid: f64,
    pub granted: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaInfo {
    pub provider: String,
    pub window: String,
    pub limit: u64,
    pub used: u64,
    pub remaining: u64,
    pub reset_at: Option<String>,
}

/// A provider's quota windows plus the subscription plan name when the API
/// reports one (e.g. Kimi's membership level).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuotaSet {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProviderResult {
    Balance(BalanceInfo),
    Quota(QuotaSet),
    Both {
        balance: BalanceInfo,
        quotas: Vec<QuotaInfo>,
    },
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn fetch(&self, api_key: &str) -> Result<ProviderResult, ProviderError>;
}

#[derive(Debug)]
pub enum ProviderError {
    Network(String),
    Auth(String),
    Parse(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(e) => write!(f, "network: {e}"),
            Self::Auth(e) => write!(f, "auth: {e}"),
            Self::Parse(e) => write!(f, "parse: {e}"),
        }
    }
}

pub fn all_providers() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(kimi::KimiProvider::new()),
        Box::new(deepseek::DeepSeekProvider::new()),
        Box::new(minimax::MiniMaxProvider::new()),
        Box::new(glm::GlmProvider::new()),
    ]
}

pub async fn fetch_all(
    keys: &std::collections::HashMap<String, String>,
) -> Vec<(String, Result<ProviderResult, ProviderError>)> {
    let providers = all_providers();
    let mut results = Vec::new();
    for p in &providers {
        let name = p.name().to_string();
        if let Some(key) = keys.get(&name) {
            results.push((name, p.fetch(key).await));
        }
    }
    results
}
