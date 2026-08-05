//! The coding panel: one collapsible card per saved monitor instance (quota
//! bars, usage history, local token cost), plus the always-visible "Add
//! monitor" row. Fetch results come in via `BalanceState`; everything else
//! (card expand state, history view toggle, add-row draft) is panel-local.

use super::format::{self, fmt_tokens};
use super::state::BalanceState;
use crate::balance::{ProviderResult, kimi_local::CostData};
use crate::storage;
use dioxus::prelude::*;
use std::collections::HashSet;

#[component]
pub fn CodingPanel(balance: BalanceState) -> Element {
    // Coding side: per-provider card expand state. Each provider card can be
    // expanded independently (multiple open at once); collapsed cards show a
    // compact 5h/weekly summary, expanded cards show the full detail block.
    let card_expanded = use_signal(HashSet::<String>::new);
    // History chart view: false = 5h cycles, true = weekly cycles.
    let hist_weekly = use_signal(|| false);
    // Bumped whenever a provider key is saved or removed, so the
    // "saved keys" list re-reads from disk.
    let saved_keys_version = use_signal(|| 0u32);
    // Always-on "Add monitor" row at the bottom of the provider list: a
    // one-line form (provider select + key input + Add button) so there's
    // always a visible entry point. The card list only renders instances
    // already saved, so without this row there'd be no way to add the very
    // first (or next, or duplicate) provider.
    let add_provider = use_signal(|| String::from("Kimi"));
    let add_key = use_signal(String::new);

    rsx! {
        div {
            class: "coding-panel-shell",
            div {
                class: "coding-panel",
                onclick: move |evt: MouseEvent| evt.stop_propagation(),
                {
                    let data = balance.data.read();
                    let errs = balance.errors.read();
                    let meta = balance.meta.read();
                    // Instance list = successes ∪ failures, so a fetch that
                    // errored still has a card to show. Keys here are monitor
                    // instance ids (e.g. "glm-mon-2"), not provider names.
                    let mut ids: Vec<&String> = data.keys().collect();
                    for k in errs.keys() {
                        if !ids.contains(&k) {
                            ids.push(k);
                        }
                    }
                    if ids.is_empty() {
                        // Distinguish three cases when nothing is shown:
                        //  1. No monitor saved at all        -> "No API keys configured"
                        //  2. Monitor saved but fetch errored -> show the error message
                        //  3. Monitor saved, fetch in-flight  -> "Fetching balance..."
                        let monitors = storage::load_monitors();
                        let err = balance.last_error.read().clone();
                        if monitors.is_empty() {
                            rsx! {
                                div {
                                    class: "coding-empty",
                                    span { class: "coding-empty-icon", "⚡" }
                                    p { "No API keys configured." }
                                    p { "Add one below to get started." }
                                    AddMonitorRow {
                                        add_provider,
                                        add_key,
                                        saved_keys_version,
                                        balance,
                                    }
                                }
                            }
                        } else if !err.is_empty() {
                            rsx! {
                                div {
                                    class: "coding-empty",
                                    span { class: "coding-empty-icon", "⚠" }
                                    p { "Couldn't fetch balance:" }
                                    p { "{err}" }
                                    p { "Check the API key is valid and reachable." }
                                }
                            }
                        } else {
                            rsx! {
                                div {
                                    class: "coding-empty",
                                    span { class: "coding-empty-icon", "⏳" }
                                    p { "Fetching balance..." }
                                }
                            }
                        }
                    } else {
                        // Stable display order: by provider type (Kimi/GLM/
                        // DeepSeek/MiniMax), then by instance id so duplicate
                        // providers stay grouped and stable. Each instance is
                        // its own collapsible card (multiple open at once).
                        let meta_for_sort = meta.clone();
                        let mut sorted: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
                        sorted.sort_by(|a, b| {
                            let rank = |id: &str| -> i32 {
                                match meta_for_sort.get(id).map(|s| s.as_str()) {
                                    Some("Kimi") => 0,
                                    Some("GLM") => 1,
                                    Some("DeepSeek") => 2,
                                    Some("MiniMax") => 3,
                                    _ => 99,
                                }
                            };
                            rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
                        });
                        let _v = *saved_keys_version.read();
                        let monitors = storage::load_monitors();
                        rsx! {
                            div {
                                class: "provider-list",
                                for id in sorted {
                                    ProviderCard {
                                        key: "{id}",
                                        id: id.clone(),
                                        result: data.get(&id).cloned(),
                                        error: errs.get(&id).cloned(),
                                        pname: meta.get(&id).cloned().unwrap_or_default(),
                                        has_key: monitors.iter().any(|m| m.id == id),
                                        masked_key: monitors
                                            .iter()
                                            .find(|m| m.id == id)
                                            .map(|m| mask_key(&m.key)),
                                        balance,
                                        card_expanded,
                                        hist_weekly,
                                        saved_keys_version,
                                    }
                                }
                                // Persistent one-line "Add monitor" entry at
                                // the list bottom: [provider select] [key] [Add].
                                // Always visible — no dedup, so adding a second
                                // GLM (etc.) just works.
                                AddMonitorRow {
                                    add_provider,
                                    add_key,
                                    saved_keys_version,
                                    balance,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Mask an API key for display: first 4 + last 3 chars, or bullets when too
/// short to be worth revealing.
fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "••••".to_string()
    } else {
        format!("{}…{}", &key[..4], &key[key.len() - 3..])
    }
}

/// One provider card: header (icon, title, quick summary, key dot, chevron)
/// plus, when open, the detail block — error banner, quota bars, usage
/// history chart, local token cost, and inline key management.
#[component]
fn ProviderCard(
    id: String,
    result: Option<ProviderResult>,
    error: Option<String>,
    /// Provider TYPE name for this instance (e.g. "GLM"); duplicate instances
    /// share the title and are told apart by the masked key in the body.
    pname: String,
    has_key: bool,
    masked_key: Option<String>,
    balance: BalanceState,
    card_expanded: Signal<HashSet<String>>,
    hist_weekly: Signal<bool>,
    saved_keys_version: Signal<u32>,
) -> Element {
    let mut card_expanded = card_expanded;
    let mut hist_weekly = hist_weekly;
    let mut saved_keys_version = saved_keys_version;

    let is_open = card_expanded.read().contains(&id);
    let err_msg = error;
    let icon_letter = match pname.as_str() {
        "Kimi" => "K",
        "DeepSeek" => "DS",
        "MiniMax" => "MM",
        "GLM" => "G",
        _ => "?",
    };
    let icon_class = match pname.as_str() {
        "Kimi" => "provider-card-icon kimi",
        "DeepSeek" => "provider-card-icon deepseek",
        "MiniMax" => "provider-card-icon minimax",
        "GLM" => "provider-card-icon glm",
        _ => "provider-card-icon",
    };
    // Compact collapsed summary: first two quota windows (5h + weekly) as
    // "5h 20% · 7d 55%".
    let quick = result.as_ref().and_then(|r| match r {
        ProviderResult::Quota(qs) => {
            let line = qs
                .quotas
                .iter()
                .take(2)
                .map(|q| {
                    let w = if q.window == "weekly" { "7d" } else { q.window.as_str() };
                    format!("{w} {:.0}%", format::quota_pct(q))
                })
                .collect::<Vec<_>>()
                .join(" · ");
            if line.is_empty() { None } else { Some(line) }
        }
        ProviderResult::Balance(b) => Some(format!("{:.2}", b.remaining)),
        ProviderResult::Both { balance: b, .. } => Some(format!("{:.2}", b.remaining)),
    });
    let id_for_toggle = id.clone();
    // Card title shows the provider TYPE name (e.g. "GLM"); duplicate
    // instances share the title and are told apart by the masked key.
    let title = pname.clone();
    // Pre-compute the cost view + history for this provider so the detail
    // block below stays uniform.
    let weekly_view = *hist_weekly.read();
    let kimi_hist = balance.kimi_history.read();
    let glm_hist = balance.glm_history.read();
    let kimi_cost_r = balance.kimi_cost.read();
    let zcode_cost_r = balance.zcode_cost.read();
    let is_kimi = pname == "Kimi";
    let is_glm = pname == "GLM";
    let hist_bars: Vec<f64> = if is_kimi {
        if weekly_view {
            kimi_hist.weekly.iter().map(|c| c.pct).collect()
        } else {
            kimi_hist.session.iter().map(|c| c.pct).collect()
        }
    } else if is_glm {
        if weekly_view {
            glm_hist.weekly.iter().map(|c| c.pct).collect()
        } else {
            glm_hist.session.iter().map(|c| c.pct).collect()
        }
    } else {
        Vec::new()
    };
    // Cost report as trait object for uniform render.
    let cost_opt: Option<&dyn CostData> = if is_kimi {
        kimi_cost_r.as_ref().map(|r| r as &dyn CostData)
    } else if is_glm {
        zcode_cost_r.as_ref().map(|r| r as &dyn CostData)
    } else {
        None
    };
    let cost_max = cost_opt
        .map(|r| r.daily().iter().map(|(_, d)| d.cost).fold(0.0f64, f64::max).max(1e-9))
        .unwrap_or(1e-9);
    let total_source_reqs: u64 = cost_opt
        .map(|r| r.by_source().iter().map(|s| s.requests).sum())
        .unwrap_or(0);
    let cost_title = if is_glm { "Token cost (est.) · ZCode" } else { "Token cost (est.)" };

    rsx! {
        div {
            class: if is_open { "coding-card open" } else { "coding-card" },
            key: "{id}",
            div {
                class: "coding-card-header",
                onclick: move |_| {
                    let mut s = card_expanded.write();
                    if s.contains(&id_for_toggle) {
                        s.remove(&id_for_toggle);
                    } else {
                        s.insert(id_for_toggle.clone());
                    }
                },
                div { class: "{icon_class}", "{icon_letter}" }
                span { class: "coding-card-title", "{title}" }
                if !is_open {
                    if let Some(ref e) = err_msg {
                        span { class: "coding-card-quick danger", title: "{e}", "⚠ fetch failed" }
                    } else if let Some(q) = quick {
                        span { class: "coding-card-quick", "{q}" }
                    } else {
                        span { class: "coding-card-quick", "—" }
                    }
                }
                span {
                    class: if has_key { "coding-card-key-dot on" } else { "coding-card-key-dot off" },
                    title: if has_key { "API key configured" } else { "No API key" },
                }
                span { class: "coding-card-chevron", "▸" }
            }
            if is_open {
                div {
                    class: "coding-card-body",
                    // Error banner when fetch failed.
                    if let Some(ref e) = err_msg {
                        div { class: "coding-card-error",
                            span { "⚠ {e}" }
                        }
                    }
                    // Quota windows with progress bars.
                    if let Some(ProviderResult::Quota(qs)) = &result {
                        if let Some(ref plan) = qs.plan {
                            span { class: "provider-summary-plan", "{plan}" }
                        }
                        for q in &qs.quotas {
                            {
                                let pct = format::quota_pct(q);
                                rsx! {
                                    div { class: "kimi-quota-row", key: "{q.window}",
                                        div { class: "kimi-quota-head",
                                            span { class: "kimi-quota-window", "{q.window}" }
                                            span { class: "kimi-quota-nums", "{q.used}/{q.limit} · {pct:.0}%" }
                                            if let Some(ref reset) = q.reset_at {
                                                span { class: "kimi-quota-reset", "↻ {format::reset_label(reset)}" }
                                            }
                                        }
                                        div { class: "provider-bar",
                                            div { class: format::bar_class(pct), style: "width: {pct:.0}%;" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Per-cycle usage history (5h / weekly peaks).
                    if !hist_bars.is_empty() {
                        div { class: "kimi-block",
                            div { class: "kimi-block-head",
                                span { class: "kimi-block-title", "Usage history" }
                                div { class: "kimi-toggle",
                                    button {
                                        class: if weekly_view { "" } else { "on" },
                                        onclick: move |_| hist_weekly.set(false),
                                        "5h"
                                    }
                                    button {
                                        class: if weekly_view { "on" } else { "" },
                                        onclick: move |_| hist_weekly.set(true),
                                        "7d"
                                    }
                                }
                            }
                            div { class: "kimi-chart",
                                for pct in &hist_bars {
                                    div {
                                        class: "kimi-chart-bar",
                                        style: "height: {pct:.0}%;",
                                        title: "{pct:.0}%",
                                    }
                                }
                            }
                        }
                    }
                    // Token usage & equivalent cost.
                    if let Some(report) = cost_opt {
                        div { class: "kimi-block",
                            div { class: "kimi-block-head",
                                span { class: "kimi-block-title", "{cost_title}" }
                            }
                            div { class: "kimi-cost-grid",
                                div { class: "kimi-cost-cell",
                                    span { class: "kimi-cost-value", "¥{report.today_cost():.2}" }
                                    span { class: "kimi-cost-label", "Today" }
                                }
                                div { class: "kimi-cost-cell",
                                    span { class: "kimi-cost-value", "¥{report.month_cost():.2}" }
                                    span { class: "kimi-cost-label", "30 days" }
                                }
                                div { class: "kimi-cost-cell",
                                    span { class: "kimi-cost-value", "{fmt_tokens(report.month_tokens().total())}" }
                                    span { class: "kimi-cost-label", "30d tokens" }
                                }
                                div { class: "kimi-cost-cell",
                                    if let Some((_, model, t)) = report.last_request() {
                                        span { class: "kimi-cost-value", "{fmt_tokens(t.total())}" }
                                        span { class: "kimi-cost-label", "{model}" }
                                    } else {
                                        span { class: "kimi-cost-value", "—" }
                                        span { class: "kimi-cost-label", "Last request" }
                                    }
                                }
                            }
                            span { class: "kimi-cost-today",
                                "In {fmt_tokens(report.today_tokens().input)} · Out {fmt_tokens(report.today_tokens().output)} · Cache {fmt_tokens(report.today_tokens().cache_read)} · {report.today_requests()} reqs today"
                            }
                            if !report.daily().is_empty() {
                                div { class: "kimi-chart cost",
                                    for (day, d) in report.daily() {
                                        {
                                            let h = (d.cost / cost_max * 100.0).round().max(2.0) as u64;
                                            rsx! {
                                                div {
                                                    class: "kimi-chart-bar cost",
                                                    key: "{day}",
                                                    style: "height: {h}%;",
                                                    title: "{day}: ¥{d.cost:.2} · {fmt_tokens(d.tokens.total())} tokens",
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Request source distribution (ZCode only).
                        if !report.by_source().is_empty() {
                            div { class: "kimi-block",
                                div { class: "kimi-block-head",
                                    span { class: "kimi-block-title", "By source" }
                                }
                                for s in report.by_source() {
                                    {
                                        let pct = if total_source_reqs > 0 {
                                            s.requests as f64 / total_source_reqs as f64 * 100.0
                                        } else { 0.0 };
                                        rsx! {
                                            div { class: "zcode-dim-row", key: "{s.name}",
                                                span { class: "zcode-dim-name", "{s.name}" }
                                                div { class: "zcode-dim-bar",
                                                    div { class: "zcode-dim-bar-fill glm", style: "width: {pct:.0}%;" }
                                                }
                                                span { class: "zcode-dim-meta", "{s.requests} · {pct:.0}% · {fmt_tokens(s.tokens.total())}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Inline key management for this instance. Cards only render for
                    // already-saved instances, so this is always the "configured" branch:
                    // masked key + Re-fetch + Remove. Adding a new instance (incl. a
                    // duplicate provider) happens via the Add row at the list bottom.
                    div { class: "card-key-row",
                        if let Some(m) = masked_key {
                            span { class: "card-key-masked", "{m}" }
                            button {
                                class: "card-key-btn",
                                title: "Re-fetch",
                                onclick: move |_| {
                                    spawn(async move {
                                        balance.refresh().await;
                                    });
                                },
                                "↻"
                            }
                            button {
                                class: "card-key-btn danger",
                                title: "Remove",
                                onclick: move |_| {
                                    storage::remove_monitor(&id);
                                    saved_keys_version.set(saved_keys_version() + 1);
                                    spawn(async move {
                                        balance.refresh().await;
                                    });
                                },
                                "✕"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Persist the add-row draft as a new monitor instance, bump the saved-keys
/// version, and kick off a fresh fetch so the instance appears as a card.
/// Shared by the Enter key and the Add button.
fn submit_monitor_key(
    add_provider: Signal<String>,
    mut add_key: Signal<String>,
    mut saved_keys_version: Signal<u32>,
    balance: BalanceState,
) {
    let k = add_key.read().trim().to_string();
    if k.is_empty() {
        return;
    }
    let prov = add_provider.read().clone();
    storage::save_monitor(&prov, &k);
    add_key.set(String::new());
    saved_keys_version.set(saved_keys_version() + 1);
    spawn(async move {
        balance.refresh().await;
    });
}

/// One-line "Add monitor" row: [provider select] [API key input] [Add].
/// Lives at the bottom of the provider list and inside the empty state. Always
/// visible — no toggle — so there's a permanent entry point for adding the
/// first, next, or a duplicate provider. On Add: `save_monitor` appends a new
/// instance (ids like glm-mon-1 / glm-mon-2, never dedupes), then a fresh
/// fetch makes it appear as its own card.
#[component]
fn AddMonitorRow(
    add_provider: Signal<String>,
    add_key: Signal<String>,
    saved_keys_version: Signal<u32>,
    balance: BalanceState,
) -> Element {
    let mut add_provider = add_provider;
    let mut add_key = add_key;
    let cur_provider = add_provider.read().clone();
    let cur_key = add_key.read().clone();
    rsx! {
        div {
            class: "card-key-row monitor-add-row",
            select {
                class: "card-key-select",
                value: "{cur_provider}",
                onchange: move |evt| {
                    add_provider.set(evt.value());
                },
                option { value: "Kimi",    "Kimi" }
                option { value: "GLM",     "GLM" }
                option { value: "DeepSeek","DeepSeek" }
                option { value: "MiniMax", "MiniMax" }
            }
            input {
                class: "card-key-input",
                r#type: "password",
                value: "{cur_key}",
                placeholder: "Paste API key...",
                oninput: move |evt| {
                    add_key.set(evt.value());
                },
                onkeydown: move |evt| {
                    if evt.key() == Key::Enter {
                        submit_monitor_key(add_provider, add_key, saved_keys_version, balance);
                    }
                    if evt.key() == Key::Escape {
                        add_key.set(String::new());
                        evt.stop_propagation();
                    }
                },
            }
            button {
                class: "card-key-btn primary",
                onclick: move |_| {
                    submit_monitor_key(add_provider, add_key, saved_keys_version, balance);
                },
                "Add"
            }
        }
    }
}
