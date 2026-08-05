//! Pure display-formatting helpers shared by the island and the panels.
//! No state, no I/O — data in, strings/classes out.

use crate::balance::{self, QuotaInfo};

pub fn local_time_hm() -> String {
    let now =
        time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:02}:{:02}", now.hour(), now.minute())
}

/// Short balance line for the island pill: (optional provider label, amount).
/// Shared by the live line and the roll-out overlay of the swap animation.
pub fn coding_pill_line(result: &balance::ProviderResult) -> (Option<String>, String) {
    match result {
        balance::ProviderResult::Balance(b)
        | balance::ProviderResult::Both { balance: b, .. } => (
            Some(b.provider.clone()),
            format!("{} {:.2}", b.currency, b.remaining),
        ),
        balance::ProviderResult::Quota(qs) => {
            if qs.quotas.is_empty() {
                return (None, "No data".to_string());
            }
            // Compact per-window usage: "5h 20% · 7d 55%". The weekly window
            // is shortened to 7d to match the history toggle labels.
            let line = qs
                .quotas
                .iter()
                .take(2)
                .map(|q| {
                    let w = if q.window == "weekly" {
                        "7d"
                    } else {
                        q.window.as_str()
                    };
                    format!("{w} {:.0}%", quota_pct(q))
                })
                .collect::<Vec<_>>()
                .join(" · ");
            (Some(qs.quotas[0].provider.clone()), line)
        }
    }
}

/// Human label for an ISO reset timestamp: today's times show as "HH:MM",
/// later dates as "M-D HH:MM". Unparseable values pass through unchanged.
pub fn reset_label(iso: &str) -> String {
    let Ok(t) = time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339)
    else {
        return iso.to_string();
    };
    let local = t.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    let today = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date();
    if local.date() == today {
        format!("{:02}:{:02}", local.hour(), local.minute())
    } else {
        format!(
            "{}-{} {:02}:{:02}",
            local.date().month() as u8,
            local.date().day(),
            local.hour(),
            local.minute()
        )
    }
}

/// Compact token counts: 1.2M / 45.3k / 123.
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else if n >= 1_000 {
        format!("{:.2}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

pub fn quota_pct(q: &QuotaInfo) -> f64 {
    if q.limit == 0 {
        return 0.0;
    }
    (q.used as f64 / q.limit as f64 * 100.0).clamp(0.0, 100.0)
}

pub fn bar_class(pct: f64) -> &'static str {
    if pct >= 95.0 {
        "provider-bar-fill danger"
    } else if pct >= 80.0 {
        "provider-bar-fill warn"
    } else {
        "provider-bar-fill"
    }
}
