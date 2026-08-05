pub mod deepseek;
pub mod glm;
pub mod kimi;
pub mod kimi_local;
pub mod minimax;
pub mod quota_history;
pub mod zcode_local;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub provider: String,
    pub currency: String,
    pub total: f64,
    pub used: f64,
    pub remaining: f64,
    #[serde(default)]
    pub breakdown: Option<BalanceBreakdown>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalanceBreakdown {
    pub paid: f64,
    pub granted: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuotaSet {
    pub plan: Option<String>,
    pub quotas: Vec<QuotaInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// One monitor instance to fetch: `id` is the stable instance id (e.g.
/// "glm-mon-2"), `provider` is the type name used to look up the Provider
/// impl, `key` is the API key for this instance.
pub struct MonitorKey {
    pub id: String,
    pub provider: String,
    pub key: String,
}

/// Fetch each monitor instance independently. The same provider type may
/// appear more than once (multiple accounts) — each is fetched on its own.
/// Returns `(id, provider_type_name, result)` tuples keyed by instance id.
pub async fn fetch_all(
    monitors: &[MonitorKey],
) -> Vec<(String, String, Result<ProviderResult, ProviderError>)> {
    let providers = all_providers();
    let mut results = Vec::new();
    for m in monitors {
        // Find the Provider impl whose name matches this instance's type.
        if let Some(p) = providers.iter().find(|p| p.name() == m.provider) {
            let result = p.fetch(&m.key).await;
            results.push((m.id.clone(), m.provider.clone(), result));
        }
    }
    results
}
