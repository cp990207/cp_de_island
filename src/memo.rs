use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memo {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Memo {
    pub fn new(content: String) -> Self {
        let now = unix_now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            created_at: now,
            updated_at: now,
        }
    }
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn time_ago(timestamp: i64) -> String {
    let now = unix_now();
    let secs = now.saturating_sub(timestamp);
    match secs {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s if s < 604800 => format!("{}d ago", s / 86400),
        s => format!("{}w ago", s / 604800),
    }
}
