#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod balance;
mod memo;
mod storage;
mod windowing;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use std::collections::HashSet;
use std::time::Duration;
use windowing::{COLLAPSED_W, ISLAND_BLEED, WINDOW_H, WINDOW_W};

#[cfg(target_os = "windows")]
use dioxus::desktop::tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

fn main() {
    // Refuse to run alongside another instance: two processes would keep
    // overwriting each other's data file (last writer wins).
    #[cfg(target_os = "windows")]
    let _instance_guard = match single_instance::acquire() {
        Some(guard) => guard,
        None => return,
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config())
        .launch(App);
}

/// Single-instance guard backed by a named Win32 mutex. The returned handle
/// must stay alive for the whole process; the OS releases it on exit.
#[cfg(target_os = "windows")]
mod single_instance {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;

    pub fn acquire() -> Option<HANDLE> {
        let name: Vec<u16> = "Local\\MemoPillSingleInstance\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            // Query failed — do not block the app over a locking error.
            return Some(handle);
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }
        Some(handle)
    }
}

fn desktop_config() -> Config {
    let initial_w = WINDOW_W;
    let initial_h = WINDOW_H;

    let mut window = WindowBuilder::new()
        .with_title("Memo Pill")
        .with_inner_size(LogicalSize::new(initial_w, initial_h))
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(true);

    #[cfg(target_os = "windows")]
    {
        window = window
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false);
    }

    Config::new()
        .with_window(window)
        .with_background_color((0, 0, 0, 0))
        .with_disable_context_menu(true)
        .with_on_window(move |handle, _| {
            handle.set_always_on_top(true);
            #[cfg(target_os = "windows")]
            {
                let _ = handle.set_skip_taskbar(true);
                handle.set_undecorated_shadow(false);
            }
            // Keep the position from the last drag; first run (or a stale
            // position off every monitor) falls back to top-center.
            if !windowing::restore_position(&handle) {
                windowing::place_top_center(&handle, initial_w);
            }
        })
}

fn local_time_hm() -> String {
    let now =
        time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:02}:{:02}", now.hour(), now.minute())
}

/// Active-list ordering: due-soonest first (overdue floats to the top on its
/// own), then priority High → Low, then most recently touched.
fn due_rank(due: Option<i64>) -> (u8, i64) {
    match due {
        Some(t) => (0, t),
        None => (1, i64::MAX),
    }
}

fn priority_rank(p: Option<memo::Priority>) -> u8 {
    match p {
        Some(memo::Priority::High) => 0,
        Some(memo::Priority::Medium) => 1,
        Some(memo::Priority::Low) => 2,
        None => 3,
    }
}

/// Short balance line for the island pill: (optional provider label, amount).
/// Shared by the live line and the roll-out overlay of the swap animation.
fn coding_pill_line(result: &balance::ProviderResult) -> (Option<String>, String) {
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
fn reset_label(iso: &str) -> String {
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
fn fmt_tokens(n: u64) -> String {
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

fn quota_pct(q: &balance::QuotaInfo) -> f64 {
    if q.limit == 0 {
        return 0.0;
    }
    (q.used as f64 / q.limit as f64 * 100.0).clamp(0.0, 100.0)
}

fn bar_class(pct: f64) -> &'static str {
    if pct >= 95.0 {
        "provider-bar-fill danger"
    } else if pct >= 80.0 {
        "provider-bar-fill warn"
    } else {
        "provider-bar-fill"
    }
}

/// Re-fetch every saved monitor instance and write the results into the
/// balance data/error/meta signals. Centralised so Add / Remove / Re-fetch
/// all share one source of truth. `Signal<T>` is `Copy`, so callers just pass
/// their signals through. Runs the fetch then updates `balance_data` (id →
/// result), `balance_errors` (id → msg), `balance_meta` (id → provider type),
/// and `last_fetch_error` (first failure's message).
async fn refresh_balance(
    mut balance_data: Signal<std::collections::HashMap<String, balance::ProviderResult>>,
    mut balance_errors: Signal<std::collections::HashMap<String, String>>,
    mut balance_meta: Signal<std::collections::HashMap<String, String>>,
    mut last_fetch_error: Signal<String>,
) {
    let monitors = storage::load_monitors();
    if monitors.is_empty() {
        balance_data.set(std::collections::HashMap::new());
        balance_errors.set(std::collections::HashMap::new());
        balance_meta.set(std::collections::HashMap::new());
        last_fetch_error.set(String::new());
        return;
    }
    let keys: Vec<balance::MonitorKey> = monitors
        .iter()
        .map(|m| balance::MonitorKey {
            id: m.id.clone(),
            provider: m.provider.clone(),
            key: m.key.clone(),
        })
        .collect();
    let results = balance::fetch_all(&keys).await;
    let mut map = std::collections::HashMap::new();
    let mut errs = std::collections::HashMap::new();
    let mut meta = std::collections::HashMap::new();
    let mut first_err = String::new();
    for (id, provider, result) in results {
        meta.insert(id.clone(), provider.clone());
        match result {
            Ok(d) => {
                map.insert(id, d);
            }
            Err(e) => {
                if first_err.is_empty() {
                    first_err = format!("{provider}: {e}");
                }
                errs.insert(id, e.to_string());
            }
        }
    }
    balance_data.set(map);
    balance_errors.set(errs);
    balance_meta.set(meta);
    last_fetch_error.set(first_err);
}

#[component]
fn App() -> Element {
    let desktop = dioxus::desktop::window();
    let mut expanded = use_signal(|| false);
    let mut hovered = use_signal(|| false);
    let mut show_time = use_signal(|| false);
    let mut face_expr = use_signal(|| 0u8);
    let mut clock_text = use_signal(local_time_hm);
    let mut hover_gen = use_signal(|| 0u64);
    let mut memos = use_signal(storage::load_memos);
    let mut input_text = use_signal(String::new);
    // Attributes staged for the next Add (TickTick-style icons beside the
    // input; optional, so plain typing + Enter still captures instantly).
    let mut input_priority = use_signal(|| None::<memo::Priority>);
    let mut input_due = use_signal(|| None::<i64>);
    let mut show_due_strip = use_signal(|| false);
    let mut editing_id = use_signal(|| None::<String>);
    let mut edit_text = use_signal(String::new);
    let mut edit_priority = use_signal(|| None::<memo::Priority>);
    let mut edit_due = use_signal(|| None::<i64>);
    let mut search_text = use_signal(String::new);
    let mut show_completed = use_signal(|| true);
    // Single-level undo: the last deleted memo and its original position.
    let mut deleted = use_signal(|| None::<(memo::Memo, usize)>);
    let mut toast_gen = use_signal(|| 0u64);
    // Due-soon alerts (in-memory): ids already alerted, ids currently flashing.
    let mut alerted = use_signal(HashSet::<String>::new);
    let mut flash = use_signal(HashSet::<String>::new);
    // Set by a Shift+drag so the trailing click does not toggle the panel.
    let mut suppress_click = use_signal(|| false);
    // Which tip the island pill shows; cycled with the mouse wheel.
    let mut tip_index = use_signal(|| 0usize);
    // Wheel delta accumulator (px) so notched wheels and smooth touchpads
    // both step one tip at a time.
    let mut wheel_accum = use_signal(|| 0.0f64);
    // Coding side: per-provider card expand state. Each provider card can be
    // expanded independently (multiple open at once); collapsed cards show a
    // compact 5h/weekly summary, expanded cards show the full detail block.
    let mut coding_card_expanded =
        use_signal(|| std::collections::HashSet::<String>::new());
    // Always-on "Add monitor" row at the bottom of the provider list: a
    // one-line form (provider select + key input + Add button) so there's
    // always a visible entry point. Provider/key draft live here; the card
    // list only renders instances already saved, so without this row there'd
    // be no way to add the very first (or next, or duplicate) provider.
    let add_monitor_provider = use_signal(|| String::from("Kimi"));
    let add_monitor_key = use_signal(String::new);
    // Rolling-swap state for the wheel switch: the previous line keeps
    // rendering as an overlay that rolls out while the new line rolls in.
    // `dir`: 1 = wheel down (next, content rolls up), -1 = wheel up.
    // `gen` guards the delayed overlay cleanup (same pattern as hover_gen).
    let mut prev_tip_index = use_signal(|| None::<usize>);
    let mut tip_swap_dir = use_signal(|| 1i8);
    let mut tip_swap_gen = use_signal(|| 0u64);
    // Which side the cursor is over: 0 = none, 1 = left (tasks), 2 = right (coding).
    let mut hover_side = use_signal(|| 0u8);
    // Coding panel open state (mutually exclusive with tasks panel).
    let mut coding_expanded = use_signal(|| false);
    // Cached balance data from the last fetch. Keyed by monitor instance id
    // (e.g. "glm-mon-2"), NOT by provider name — that allows multiple
    // instances of the same provider type to coexist as separate cards.
    let mut balance_data = use_signal(
        || std::collections::HashMap::<String, balance::ProviderResult>::new(),
    );
    // Per-instance fetch errors. balance_data only holds successes, so this
    // map carries the failures — that way a single failed instance still
    // shows up as a card with its error instead of vanishing silently when
    // another instance succeeds.
    let mut balance_errors =
        use_signal(|| std::collections::HashMap::<String, String>::new());
    // Instance id -> provider type name (e.g. "glm-mon-2" -> "GLM"). The card
    // renderer needs the type to pick icons / cost views / history series,
    // but the data maps are keyed by id, so this side table bridges them.
    let mut balance_meta =
        use_signal(|| std::collections::HashMap::<String, String>::new());
    // Last fetch error message (empty when no error or no fetch attempted).
    let mut last_fetch_error = use_signal(String::new);
    // Bumped whenever a provider key is saved or removed, so the
    // "saved keys" list in the config section re-reads from disk.
    let mut saved_keys_version = use_signal(|| 0u32);
    // Kimi quota history (per-cycle peaks) and local token/cost report from
    // the CLI session logs. Both refresh on the same 5-minute cadence.
    let mut kimi_history = use_signal(balance::quota_history::load);
    let mut kimi_cost = use_signal(|| None::<balance::kimi_local::CostReport>);
    // GLM counterpart: per-cycle history (own file) + ZCode-local token/cost
    // report mined from ~/.zcode/cli/db/db.sqlite.
    let mut glm_history = use_signal(|| balance::quota_history::load_named("glm-quota-history.json"));
    let mut zcode_cost = use_signal(|| None::<balance::zcode_local::ZcodeCostReport>);
    // History chart view: false = 5h cycles, true = weekly cycles.
    let mut hist_weekly = use_signal(|| false);

    // Persist on every change — but skip the first run: memos were just
    // loaded from disk, so an immediate rewrite is pure risk (e.g. after a
    // failed load) with zero benefit.
    {
        let mut first_run = true;
        use_effect(move || {
            let snapshot = memos.read();
            if first_run {
                first_run = false;
                return;
            }
            storage::save_memos(&snapshot);
        });
    }

    // The window is fixed-size and click-through: poll the cursor against the
    // live hot regions and only make the window interactive when the cursor is
    // actually over the island or, while expanded, anywhere in the window.
    {
        let d = desktop.clone();
        use_future(move || {
            let d = d.clone();
            async move {
                let mut interactive = true;
                loop {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let wide = *hovered.peek() || *expanded.peek() || *coding_expanded.peek();
                    let any_open = *expanded.peek() || *coding_expanded.peek();
                    let rects = windowing::hot_rects(&d.window, any_open, wide);
                    let want = windowing::cursor_inside(&rects);
                    if want != interactive {
                        interactive = want;
                        windowing::set_click_through(&d.window, !want);
                    }
                }
            }
        });
    }

    // Alternate between the face and the clock while idle; keep the clock
    // fresh. The 5s tick also re-renders the component, which keeps the
    // relative "time ago" labels in the panel up to date.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            clock_text.set(local_time_hm());
            if *expanded.peek() {
                continue;
            }
            let next_show_time = !*show_time.peek();
            if !next_show_time {
                let current = *face_expr.peek();
                let mut next = uuid::Uuid::new_v4().as_bytes()[0] % FACE_COUNT;
                if next == current {
                    next = (next + 1) % FACE_COUNT;
                }
                face_expr.set(next);
            }
            show_time.set(next_show_time);
        }
    });

    // Due-soon alert: 10 minutes before a task's due time the island pops
    // open and the row flashes — a gentle visual reminder instead of an OS
    // notification. Once per due value (editing the due time re-arms it);
    // purely in-memory, so a restart just re-arms whatever is still upcoming.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let now = memo::unix_now();
            let mut fresh: Vec<String> = Vec::new();
            for m in memos.peek().iter() {
                if m.done {
                    continue;
                }
                let Some(d) = m.due else {
                    continue;
                };
                if (0..=600).contains(&(d - now)) && !alerted.peek().contains(&m.id) {
                    fresh.push(m.id.clone());
                }
            }
            if fresh.is_empty() {
                continue;
            }
            for id in &fresh {
                alerted.write().insert(id.clone());
                flash.write().insert(id.clone());
            }
            if !*expanded.peek() {
                expanded.set(true);
            }
            // The CSS flash runs a few pulses; drop the class afterwards so a
            // later re-alert can flash again.
            spawn(async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
                for id in fresh {
                    flash.write().remove(&id);
                }
            });
        }
    });

    // Focus the add-input when the panel opens so typing works immediately.
    {
        let d = desktop.clone();
        use_effect(move || {
            if expanded() {
                let _ = d
                    .webview
                    .evaluate_script("document.getElementById('memo-input')?.focus();");
            }
        });
    }

    // Periodically fetch balance data for all configured providers, sample
    // Kimi quota history, and refresh the local Kimi token/cost report.
    use_future(move || async move {
        let mut last_monitors: Vec<storage::MonitorEntry> = storage::load_monitors();
        let mut last_fetch: Option<std::time::Instant> = None;
        let mut last_scan: Option<std::time::Instant> = None;
        let mut history = balance::quota_history::load();
        let mut glm_hist = balance::quota_history::load_named("glm-quota-history.json");
        loop {
            let monitors = storage::load_monitors();
            let monitors_changed = monitors != last_monitors;
            // Fetch when monitors changed, or every 5 minutes, or on first run.
            let should_fetch = !monitors.is_empty()
                && (monitors_changed
                    || last_fetch
                        .map_or(true, |t| t.elapsed() >= Duration::from_secs(300)));
            if should_fetch {
                let keys: Vec<balance::MonitorKey> = monitors
                    .iter()
                    .map(|m| balance::MonitorKey {
                        id: m.id.clone(),
                        provider: m.provider.clone(),
                        key: m.key.clone(),
                    })
                    .collect();
                let results = balance::fetch_all(&keys).await;
                let mut map = std::collections::HashMap::new();
                let mut errs = std::collections::HashMap::new();
                let mut meta = std::collections::HashMap::new();
                let mut first_err = String::new();
                let mut history_dirty = false;
                let mut glm_history_dirty = false;
                for (id, provider, result) in results {
                    // Always record the instance -> type mapping so the card
                    // renderer can pick icons/cost views even on fetch error.
                    meta.insert(id.clone(), provider.clone());
                    match result {
                        Ok(data) => {
                            // Sample 5h/weekly windows into the per-cycle
                            // history on every successful fetch. History is
                            // sampled per provider TYPE (not per instance):
                            // Kimi type -> kimi_history, GLM type -> glm_history.
                            if let balance::ProviderResult::Quota(qs) = &data {
                                for q in &qs.quotas {
                                    let series = match q.window.as_str() {
                                        "weekly" => {
                                            Some(balance::quota_history::Series::Weekly)
                                        }
                                        "5h" => {
                                            Some(balance::quota_history::Series::Session)
                                        }
                                        _ => None,
                                    };
                                    if let Some(s) = series {
                                        if provider == "Kimi" {
                                            history_dirty |= history.record(
                                                s,
                                                q.reset_at.as_deref(),
                                                q.used,
                                                q.limit,
                                            );
                                        } else if provider == "GLM" {
                                            glm_history_dirty |= glm_hist.record(
                                                s,
                                                q.reset_at.as_deref(),
                                                q.used,
                                                q.limit,
                                            );
                                        }
                                    }
                                }
                            }
                            map.insert(id, data);
                        }
                        Err(e) => {
                            let msg = format!("{provider}: {e}");
                            if first_err.is_empty() {
                                first_err = msg.clone();
                            }
                            errs.insert(id, e.to_string());
                        }
                    }
                }
                if history_dirty {
                    balance::quota_history::save(&history);
                    kimi_history.set(history.clone());
                }
                if glm_history_dirty {
                    balance::quota_history::save_named("glm-quota-history.json", &glm_hist);
                    glm_history.set(glm_hist.clone());
                }
                balance_data.set(map);
                balance_errors.set(errs);
                balance_meta.set(meta);
                last_fetch_error.set(first_err);
                last_monitors = monitors;
                last_fetch = Some(std::time::Instant::now());
            } else {
                // Keep last_monitors in sync even when skipping fetch.
                last_monitors = monitors;
            }

            // Local CLI session-log scan for token/cost stats. Needs no API
            // key, so it runs even when the remote fetch above is skipped.
            // The scan cache makes repeat runs cheap.
            let should_scan = last_scan.map_or(true, |t| t.elapsed() >= Duration::from_secs(300));
            if should_scan {
                if let Some(dir) = storage::data_dir() {
                    let records =
                        balance::kimi_local::load_records(&dir.join("kimi-usage-cache.json"));
                    kimi_cost.set(Some(balance::kimi_local::build_report(&records)));
                }
                // ZCode local usage from its SQLite db. Independent of Kimi:
                // ZCode not being installed is fine, build_report returns empty.
                zcode_cost.set(Some(balance::zcode_local::build_report()));
                last_scan = Some(std::time::Instant::now());
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });

    let memo_count = memos.read().len();
    // Pill fallback when there is no active task to show.
    let empty_text = if memo_count == 0 { "No tasks" } else { "All done" };

    let chevron_visible = expanded() && memo_count > 0;

    // Island mode: open panel takes priority, then hover, then idle.
    let is_circle = !expanded() && !coding_expanded() && hover_side() == 0;
    let is_side_left = expanded() || (!coding_expanded() && hover_side() == 1);

    let island_class = if is_circle {
        if show_time() {
            "island circle show-time".to_string()
        } else {
            "island circle".to_string()
        }
    } else if is_side_left {
        "island side-left-mode".to_string()
    } else {
        "island side-right-mode".to_string()
    };

    let face_class = format!("circle-face face-{}", face_expr());

    let stage_class = if expanded() {
        "stage visual-expanded"
    } else if coding_expanded() {
        "stage coding-expanded"
    } else {
        "stage"
    };

    let stage_style = format!(
        "--collapsed-width: {}px; --island-bleed: {}px;",
        COLLAPSED_W, ISLAND_BLEED
    );

    // Split into the two display groups. The stored vec order no longer
    // matters for display — sorting happens here, at render time.
    let query = search_text.read().trim().to_lowercase();
    let matches = |m: &&memo::Memo| {
        query.is_empty() || m.content.to_lowercase().contains(query.as_str())
    };

    // Urgency order, shared by the panel list and the island tips.
    let by_urgency = |a: &memo::Memo, b: &memo::Memo| {
        due_rank(a.due)
            .cmp(&due_rank(b.due))
            .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
            .then(b.updated_at.cmp(&a.updated_at))
    };

    let mut active: Vec<memo::Memo> = memos
        .read()
        .iter()
        .filter(|m| !m.done && matches(m))
        .cloned()
        .collect();
    active.sort_by(by_urgency);

    // Island pill tips: every active task in urgency order, ignoring the
    // search filter — the pill reflects global state. The most urgent task
    // sits on top, which is what the pill shows by default.
    let mut tips: Vec<memo::Memo> = memos
        .read()
        .iter()
        .filter(|m| !m.done)
        .cloned()
        .collect();
    tips.sort_by(by_urgency);
    let tips_len = tips.len();
    // The modulo guards against the list shrinking (completions/deletes)
    // while the wheel index points past the new end.
    let current_tip = if tips_len == 0 {
        None
    } else {
        tips.get(*tip_index.read() % tips_len).cloned()
    };

    // Rolling swap: the outgoing line survives one animation cycle as an
    // overlay. The modulo guards against the list shrinking mid-animation.
    let prev_tip = prev_tip_index
        .read()
        .and_then(|i| tips.get(i % tips_len.max(1)).cloned());
    let tip_roll = if *tip_swap_dir.read() > 0 {
        "roll-up"
    } else {
        "roll-down"
    };
    let tip_out_gen = *tip_swap_gen.read();

    // One pill tip line (content + due chip), shared by the roll-in and
    // roll-out layers of the swap animation.
    let tip_spans = |t: &memo::Memo| {
        rsx! {
            span { class: "pill-tip", title: "{t.content}", "{t.content}" }
            if let Some(d) = t.due {
                span { class: "pill-sep", "·" }
                span {
                    class: if t.is_overdue() { "pill-time pill-overdue" } else { "pill-time" },
                    "due {memo::due_label_short(d)}"
                }
            }
        }
    };

    // Island coding pill: show the first provider that has data as a compact
    // summary line (no carousel — each provider has its own expandable card in
    // the panel now). Falls back to the first provider name with no amount.
    let coding_pill_summary = {
        let data = balance_data.read();
        let errs = balance_errors.read();
        let meta = balance_meta.read();
        // Instance ids with either data or an error.
        let mut ids: Vec<&String> = data.keys().collect();
        for k in errs.keys() {
            if !ids.contains(&k) {
                ids.push(k);
            }
        }
        // Prefer the first instance that actually has balance data. The pill
        // shows the provider TYPE name (e.g. "GLM"), resolved via balance_meta.
        let with_data = ids.iter().find_map(|id| {
            data.get(*id).map(|r| {
                let label = meta.get(*id).cloned().unwrap_or_else(|| (*id).to_string());
                (label, coding_pill_line(r).1)
            })
        });
        match with_data {
            Some((name, amount)) => (Some(name), Some(amount)),
            None => match ids.first() {
                Some(id) => {
                    let label = meta.get(*id).cloned().unwrap_or_else(|| (*id).to_string());
                    (label.into(), None)
                }
                None => (None, None),
            }
        }
    };


    let mut completed: Vec<memo::Memo> = memos
        .read()
        .iter()
        .filter(|m| m.done && matches(m))
        .cloned()
        .collect();
    completed.sort_by(|a, b| b.completed_at.unwrap_or(0).cmp(&a.completed_at.unwrap_or(0)));
    let completed_count = completed.len();
    let no_match = memo_count > 0 && active.is_empty() && completed.is_empty();

    // Commit the in-progress edit (if any): a non-empty change is saved;
    // list order is derived at render time, so nothing bubbles manually. An
    // emptied edit is discarded, restoring the old content instead of
    // failing silently.
    let commit_editing = move || {
        let Some(id) = editing_id.read().clone() else {
            return;
        };
        let text = edit_text.read().trim().to_string();
        if !text.is_empty() {
            let new_priority = *edit_priority.read();
            let new_due = *edit_due.read();
            let mut list = memos.read().clone();
            if let Some(pos) = list.iter().position(|m| m.id == id) {
                let m = &mut list[pos];
                if m.content != text || m.priority != new_priority || m.due != new_due {
                    if m.due != new_due {
                        // New due time — re-arm the due-soon alert.
                        alerted.write().remove(&id);
                    }
                    m.content = text;
                    m.priority = new_priority;
                    m.due = new_due;
                    m.updated_at = memo::unix_now();
                    memos.set(list);
                }
            }
        }
        editing_id.set(None);
        search_text.set(String::new());
    };

    // Collapse the panel: commit any pending edit and reset hover — after a
    // margin click the cursor is not on the island, so `hovered` must not
    // linger and keep the island wide.
    let collapse_panel = {
        let mut commit = commit_editing.clone();
        move || {
            if !expanded() && !coding_expanded() {
                return;
            }
            commit();
            expanded.set(false);
            coding_expanded.set(false);
            hovered.set(false);
            hover_side.set(0);
        }
    };

    let mut do_add = move || {
        let text = input_text.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut m = memo::Memo::new(text);
        m.priority = *input_priority.read();
        m.due = *input_due.read();
        let mut list = memos.read().clone();
        list.insert(0, m);
        memos.set(list);
        input_text.set(String::new());
        input_priority.set(None);
        input_due.set(None);
        show_due_strip.set(false);
        // Make sure the fresh task is visible even if a search was active.
        search_text.set(String::new());
    };

    // Completion is the primary positive action — distinct from delete. The
    // task strikes through and sinks into the Completed group; clicking the
    // checkbox again restores it.
    let do_toggle_done = move |id: String| {
        let mut list = memos.read().clone();
        let Some(pos) = list.iter().position(|m| m.id == id) else {
            return;
        };
        let m = &mut list[pos];
        m.done = !m.done;
        m.completed_at = if m.done { Some(memo::unix_now()) } else { None };
        memos.set(list);
    };

    // Soft delete: stash the memo for a few seconds so it can be restored
    // from the toast. Single-level undo — a new delete replaces the stash.
    let do_delete = move |id: String| {
        if editing_id.read().as_ref() == Some(&id) {
            editing_id.set(None);
        }
        let mut list = memos.read().clone();
        let Some(pos) = list.iter().position(|m| m.id == id) else {
            return;
        };
        let removed = list.remove(pos);
        memos.set(list);
        alerted.write().remove(&id);
        flash.write().remove(&id);
        deleted.set(Some((removed, pos)));
        *toast_gen.write() += 1;
        let generation = *toast_gen.read();
        spawn(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            if *toast_gen.peek() != generation {
                return;
            }
            deleted.set(None);
        });
    };

    let mut do_undo_delete = move || {
        if let Some((m, pos)) = deleted.read().clone() {
            let mut list = memos.read().clone();
            list.insert(pos.min(list.len()), m);
            memos.set(list);
        }
        // Cancel the auto-dismiss timer and close the toast.
        *toast_gen.write() += 1;
        deleted.set(None);
    };

    // Starting an edit commits the previous one first: switching targets must
    // never silently throw away typed text.
    let do_start_edit = {
        let mut commit = commit_editing.clone();
        move |id: String| {
            commit();
            let Some(m) = memos.read().iter().find(|m| m.id == id).cloned() else {
                return;
            };
            editing_id.set(Some(id));
            edit_text.set(m.content);
            edit_priority.set(m.priority);
            edit_due.set(m.due);
        }
    };

    let do_cancel_edit = move |_: ()| {
        editing_id.set(None);
    };

    rsx! {
        style { "{CSS}" }
        main {
            class: "{stage_class}",
            style: "{stage_style}",
            // While expanded the whole window is interactive: a click landing
            // on the transparent margin collapses the panel (popover-style).
            // The island and the panel stop propagation so their own clicks
            // never reach this handler.
            onclick: {
                let mut collapse = collapse_panel.clone();
                move |_| collapse()
            },
            // Esc collapses the panel. Inputs handle their own Esc first
            // (clear draft / cancel edit) and stop propagation.
            onkeydown: {
                let mut collapse = collapse_panel.clone();
                move |evt| {
                    if evt.key() == Key::Escape {
                        collapse();
                    }
                }
            },
            section {
                class: "{island_class}",
                title: "Click to open · Scroll to switch · Shift+drag to move · Right-click to quit",
                oncontextmenu: {
                    let d = desktop.clone();
                    move |_| d.close()
                },
                onmousedown: {
                    let d = desktop.clone();
                    move |evt: MouseEvent| {
                        if evt.modifiers().shift()
                            && evt.trigger_button()
                                .is_some_and(|b| b == MouseButton::Primary)
                        {
                            suppress_click.set(true);
                            d.drag();
                            if let Ok(pos) = d.window.outer_position() {
                                storage::save_window_pos(pos.x, pos.y);
                            }
                            spawn(async move {
                                tokio::time::sleep(Duration::from_millis(500)).await;
                                suppress_click.set(false);
                            });
                        }
                    }
                },
                div {
                    class: if hover_side() == 1 { "side-left hovered" } else { "side-left" },
                    onmouseenter: move |_| {
                        *hover_gen.write() += 1;
                        let generation = *hover_gen.read();
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            if *hover_gen.peek() != generation {
                                return;
                            }
                            hovered.set(true);
                            hover_side.set(1);
                        });
                    },
                    onmouseleave: move |_| {
                        if *expanded.peek() || *coding_expanded.peek() {
                            return;
                        }
                        *hover_gen.write() += 1;
                        let generation = *hover_gen.read();
                        spawn(async move {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            if *hover_gen.peek() != generation {
                                return;
                            }
                            hovered.set(false);
                            hover_side.set(0);
                        });
                    },
                    onclick: {
                        let mut commit = commit_editing.clone();
                        move |evt: MouseEvent| {
                            evt.stop_propagation();
                            if *suppress_click.peek() {
                                suppress_click.set(false);
                                return;
                            }
                            coding_expanded.set(false);
                            if expanded() {
                                commit();
                            }
                            expanded.toggle();
                        }
                    },
                    onwheel: move |evt| {
                        if tips_len < 2 {
                            return;
                        }
                        let dy = match evt.delta() {
                            WheelDelta::Pixels(v) => v.y,
                            WheelDelta::Lines(v) => v.y * 16.0,
                            WheelDelta::Pages(v) => v.y * 160.0,
                        };
                        let mut accum = *wheel_accum.read() + dy;
                        if accum.signum() != dy.signum() {
                            accum = dy;
                        }
                        const STEP: f64 = 48.0;
                        let old_idx = *tip_index.read();
                        let mut idx = old_idx;
                        while accum >= STEP {
                            idx = (idx + 1) % tips_len;
                            accum -= STEP;
                        }
                        while accum <= -STEP {
                            idx = (idx + tips_len - 1) % tips_len;
                            accum += STEP;
                        }
                        wheel_accum.set(accum);
                        // A full wrap lands back on the same tip — no swap.
                        if idx == old_idx {
                            return;
                        }
                        // Keep the outgoing line for one roll cycle; the
                        // generation guard drops it after the animation.
                        prev_tip_index.set(Some(old_idx));
                        tip_swap_dir.set(if dy > 0.0 { 1 } else { -1 });
                        *tip_swap_gen.write() += 1;
                        let generation = *tip_swap_gen.read();
                        tip_index.set(idx);
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(400)).await;
                            if *tip_swap_gen.peek() == generation {
                                prev_tip_index.set(None);
                            }
                        });
                    },
                    div {
                        class: "pill-content",
                        span { class: "pill-icon" }
                        if let Some(tip) = current_tip {
                            div {
                                class: "pill-swap {tip_roll}",
                                if let Some(prev) = prev_tip {
                                    div {
                                        class: "pill-swap-line swap-out",
                                        key: "out-{tip_out_gen}",
                                        {tip_spans(&prev)}
                                    }
                                }
                                div {
                                    class: "pill-swap-line swap-in",
                                    key: "{tip.id}",
                                    {tip_spans(&tip)}
                                }
                            }
                        } else {
                            span { class: "pill-count", "{empty_text}" }
                        }
                    }
                    div {
                        class: "tips-circle-icon",
                        span { class: "pill-icon" }
                    }
                    div {
                        class: "{face_class}",
                        span { class: "eye left" }
                        span { class: "eye right" }
                        span { class: "mouth" }
                    }
                    span { class: "circle-clock", "{clock_text.read()}" }
                    if chevron_visible {
                        span { class: "pill-chevron", "▾" }
                    }
                }
                div { class: "pill-divider" }
                div {
                    class: if hover_side() == 2 { "side-right hovered" } else { "side-right" },
                    onmouseenter: move |_| {
                        *hover_gen.write() += 1;
                        let generation = *hover_gen.read();
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            if *hover_gen.peek() != generation {
                                return;
                            }
                            hovered.set(true);
                            hover_side.set(2);
                        });
                    },
                    onmouseleave: move |_| {
                        if *expanded.peek() || *coding_expanded.peek() {
                            return;
                        }
                        *hover_gen.write() += 1;
                        let generation = *hover_gen.read();
                        spawn(async move {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            if *hover_gen.peek() != generation {
                                return;
                            }
                            hovered.set(false);
                            hover_side.set(0);
                        });
                    },
                    onclick: move |evt: MouseEvent| {
                        evt.stop_propagation();
                        if *suppress_click.peek() {
                            suppress_click.set(false);
                            return;
                        }
                        expanded.set(false);
                        coding_expanded.toggle();
                    },
                    div {
                        class: "coding-content",
                        span { class: "coding-icon" }
                        div {
                            class: "pill-swap",
                            div {
                                class: "pill-swap-line swap-in",
                                if let Some(label) = coding_pill_summary.0 {
                                    span { class: "coding-provider-label", "{label}" }
                                }
                                if let Some(amount) = coding_pill_summary.1 {
                                    span { class: "coding-amount", "{amount}" }
                                }
                            }
                        }
                    }
                    div {
                        class: "coding-circle-icon",
                        span { class: "coding-icon" }
                    }
                }
            }

            div {
                class: "panel-shell",
                div {
                    class: "panel",
                    onclick: move |evt: MouseEvent| evt.stop_propagation(),
                    div {
                        class: "input-row",
                        input {
                            id: "memo-input",
                            class: "memo-input",
                            value: "{input_text.read()}",
                            placeholder: "Type a task...",
                            oninput: move |evt| input_text.set(evt.value()),
                            onkeydown: {
                                let mut do_add = do_add.clone();
                                move |evt| {
                                    if evt.key() == Key::Enter && !evt.is_composing() {
                                        do_add();
                                    } else if evt.key() == Key::Escape {
                                        // Esc throws away the whole draft,
                                        // staged attributes included.
                                        input_text.set(String::new());
                                        input_priority.set(None);
                                        input_due.set(None);
                                        show_due_strip.set(false);
                                        evt.stop_propagation();
                                    }
                                }
                            },
                        }
                        button {
                            class: if show_due_strip() || input_due.read().is_some() {
                                "attr-btn active"
                            } else {
                                "attr-btn"
                            },
                            title: "Set due date",
                            onclick: move |_| show_due_strip.toggle(),
                            span { class: "due-icon" }
                        }
                        PriorityButton { priority: input_priority, with_label: false }
                        button {
                            class: "add-btn",
                            disabled: input_text.read().trim().is_empty(),
                            onclick: move |_| do_add(),
                            "Add"
                        }
                    }
                    if show_due_strip() || input_due.read().is_some() {
                        DueChips { due: input_due }
                    }
                    if memo_count >= 5 {
                        div {
                            class: "search-row",
                            input {
                                class: "search-input",
                                value: "{search_text.read()}",
                                placeholder: "Search tasks...",
                                oninput: move |evt| search_text.set(evt.value()),
                                onkeydown: move |evt| {
                                    if evt.key() == Key::Escape {
                                        search_text.set(String::new());
                                        evt.stop_propagation();
                                    }
                                },
                            }
                        }
                    }
                    div {
                        class: "memo-list",
                        if memo_count == 0 {
                            div {
                                class: "empty-state",
                                span { class: "empty-icon", "✏️" }
                                p { "No tasks yet. Type something above." }
                            }
                        } else if no_match {
                            div {
                                class: "no-match",
                                p { "No tasks match \"{query}\"." }
                            }
                        }
                        for item in active {
                            {
                                let is_editing = editing_id.read().as_ref() == Some(&item.id);
                                let item_flash = flash.read().contains(&item.id);
                                rsx! {
                                    MemoRow {
                                        key: "{item.id}",
                                        item: item.clone(),
                                        editing: is_editing,
                                        flash: item_flash,
                                        edit_text,
                                        edit_priority,
                                        edit_due,
                                        on_toggle: {
                                            let mut f = do_toggle_done.clone();
                                            move |id: String| f(id)
                                        },
                                        on_start_edit: {
                                            let mut f = do_start_edit.clone();
                                            move |id: String| f(id)
                                        },
                                        on_delete: {
                                            let mut f = do_delete.clone();
                                            move |id: String| f(id)
                                        },
                                        on_commit: {
                                            let mut f = commit_editing.clone();
                                            move |_| f()
                                        },
                                        on_cancel_edit: {
                                            let mut f = do_cancel_edit.clone();
                                            move |_| f(())
                                        },
                                    }
                                }
                            }
                        }
                        if !completed.is_empty() {
                            button {
                                class: "completed-header",
                                onclick: move |_| show_completed.toggle(),
                                span { class: "completed-chevron",
                                    if show_completed() { "▾" } else { "▸" }
                                }
                                "Completed ({completed_count})"
                            }
                            if show_completed() {
                                for item in completed {
                                    {
                                        let is_editing = editing_id.read().as_ref() == Some(&item.id);
                                        rsx! {
                                            MemoRow {
                                                key: "{item.id}",
                                                item: item.clone(),
                                                editing: is_editing,
                                                flash: false,
                                                edit_text,
                                                edit_priority,
                                                edit_due,
                                                on_toggle: {
                                                    let mut f = do_toggle_done.clone();
                                                    move |id: String| f(id)
                                                },
                                                on_start_edit: {
                                                    let mut f = do_start_edit.clone();
                                                    move |id: String| f(id)
                                                },
                                                on_delete: {
                                                    let mut f = do_delete.clone();
                                                    move |id: String| f(id)
                                                },
                                                on_commit: {
                                                    let mut f = commit_editing.clone();
                                                    move |_| f()
                                                },
                                                on_cancel_edit: {
                                                    let mut f = do_cancel_edit.clone();
                                                    move |_| f(())
                                                },
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if deleted.read().is_some() {
                        div {
                            class: "undo-toast",
                            span { class: "undo-text", "Task deleted" }
                            button {
                                class: "undo-btn",
                                onclick: move |_| do_undo_delete(),
                                "Undo"
                            }
                        }
                    }
                }
            }

            div {
                class: "coding-panel-shell",
                div {
                    class: "coding-panel",
                    onclick: move |evt: MouseEvent| evt.stop_propagation(),
                    {
                        let data = balance_data.read();
                        let errs = balance_errors.read();
                        let meta = balance_meta.read();
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
                            let err = last_fetch_error.read().clone();
                            if monitors.is_empty() {
                                rsx! {
                                    div {
                                        class: "coding-empty",
                                        span { class: "coding-empty-icon", "⚡" }
                                        p { "No API keys configured." }
                                        p { "Add one below to get started." }
                                        AddMonitorRow {
                                            add_monitor_provider,
                                            add_monitor_key,
                                            saved_keys_version,
                                            balance_data,
                                            balance_errors,
                                            balance_meta,
                                            last_fetch_error,
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
                            let expanded_set = coding_card_expanded.read();
                            let _v = *saved_keys_version.read();
                            let monitors = storage::load_monitors();
                            rsx! {
                                div {
                                    class: "provider-list",
                                    for id in sorted {
                                        {
                                            let result = data.get(&id).cloned();
                                            let err_msg = errs.get(&id).cloned();
                                            let is_open = expanded_set.contains(&id);
                                            // Provider TYPE for this instance, from the
                                            // meta side table. Falls back to "?" if absent.
                                            let pname = meta.get(&id).cloned().unwrap_or_default();
                                            let has_key = monitors.iter().any(|m| m.id == id);
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
                                            // Compact collapsed summary: first two quota
                                            // windows (5h + weekly) as "5h 20% · 7d 55%".
                                            let quick = result.as_ref().and_then(|r| match r {
                                                balance::ProviderResult::Quota(qs) => {
                                                    let line = qs.quotas.iter().take(2).map(|q| {
                                                        let w = if q.window == "weekly" { "7d" } else { q.window.as_str() };
                                                        format!("{w} {:.0}%", quota_pct(q))
                                                    }).collect::<Vec<_>>().join(" · ");
                                                    if line.is_empty() { None } else { Some(line) }
                                                }
                                                balance::ProviderResult::Balance(b) => {
                                                    Some(format!("{:.2}", b.remaining))
                                                }
                                                balance::ProviderResult::Both { balance: b, .. } => {
                                                    Some(format!("{:.2}", b.remaining))
                                                }
                                            });
                                            let id_for_toggle = id.clone();
                                            // Card title shows the provider TYPE name (e.g.
                                            // "GLM"); duplicate instances share the title and
                                            // are told apart by the masked key in the body.
                                            let title = pname.clone();
                                            // Pre-compute the cost view + history for this
                                            // provider so the detail block below stays uniform.
                                            let weekly_view = *hist_weekly.read();
                                            let kimi_hist = kimi_history.read();
                                            let glm_hist = glm_history.read();
                                            let kimi_cost_r = kimi_cost.read();
                                            let zcode_cost_r = zcode_cost.read();
                                            let is_kimi = pname == "Kimi";
                                            let is_glm = pname == "GLM";
                                            let hist_bars: Vec<f64> = if is_kimi {
                                                if weekly_view { kimi_hist.weekly.iter().map(|c| c.pct).collect() }
                                                else { kimi_hist.session.iter().map(|c| c.pct).collect() }
                                            } else if is_glm {
                                                if weekly_view { glm_hist.weekly.iter().map(|c| c.pct).collect() }
                                                else { glm_hist.session.iter().map(|c| c.pct).collect() }
                                            } else {
                                                Vec::new()
                                            };
                                            // Cost report as trait object for uniform render.
                                            let cost_opt: Option<&dyn balance::kimi_local::CostData> = if is_kimi {
                                                kimi_cost_r.as_ref().map(|r| r as &dyn balance::kimi_local::CostData)
                                            } else if is_glm {
                                                zcode_cost_r.as_ref().map(|r| r as &dyn balance::kimi_local::CostData)
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
                                                            let mut s = coding_card_expanded.write();
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
                                                            if let Some(balance::ProviderResult::Quota(qs)) = &result {
                                                                if let Some(ref plan) = qs.plan {
                                                                    span { class: "provider-summary-plan", "{plan}" }
                                                                }
                                                                for q in &qs.quotas {
                                                                    {
                                                                        let pct = quota_pct(q);
                                                                        rsx! {
                                                                            div { class: "kimi-quota-row", key: "{q.window}",
                                                                                div { class: "kimi-quota-head",
                                                                                    span { class: "kimi-quota-window", "{q.window}" }
                                                                                    span { class: "kimi-quota-nums", "{q.used}/{q.limit} · {pct:.0}%" }
                                                                                    if let Some(ref reset) = q.reset_at {
                                                                                        span { class: "kimi-quota-reset", "↻ {reset_label(reset)}" }
                                                                                    }
                                                                                }
                                                                                div { class: "provider-bar",
                                                                                    div { class: bar_class(pct), style: "width: {pct:.0}%;" }
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
                                                            {
                                                                let masked = monitors
                                                                    .iter()
                                                                    .find(|m| m.id == id)
                                                                    .map(|m| {
                                                                        let k = &m.key;
                                                                        if k.len() <= 8 {
                                                                            "••••".to_string()
                                                                        } else {
                                                                            format!("{}…{}", &k[..4], &k[k.len() - 3..])
                                                                        }
                                                                    });
                                                                let id_for_delete = id.clone();
                                                                rsx! {
                                                                    div { class: "card-key-row",
                                                                        if let Some(m) = masked {
                                                                            span { class: "card-key-masked", "{m}" }
                                                                            button {
                                                                                class: "card-key-btn",
                                                                                title: "Re-fetch",
                                                                                onclick: move |_| {
                                                                                    spawn(async move {
                                                                                        refresh_balance(
                                                                                            balance_data,
                                                                                            balance_errors,
                                                                                            balance_meta,
                                                                                            last_fetch_error,
                                                                                        ).await;
                                                                                    });
                                                                                },
                                                                                "↻"
                                                                            }
                                                                            button {
                                                                                class: "card-key-btn danger",
                                                                                title: "Remove",
                                                                                onclick: move |_| {
                                                                                    storage::remove_monitor(&id_for_delete);
                                                                                    saved_keys_version.set(saved_keys_version() + 1);
                                                                                    spawn(async move {
                                                                                        refresh_balance(
                                                                                            balance_data,
                                                                                            balance_errors,
                                                                                            balance_meta,
                                                                                            last_fetch_error,
                                                                                        ).await;
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
                                            }
                                        }
                                    }
                                    // Persistent one-line "Add monitor" entry at
                                    // the list bottom: [provider select] [key] [Add].
                                    // Always visible — no dedup, so adding a second
                                    // GLM (etc.) just works. The for-loop above only
                                    // renders cards for instances already saved, so
                                    // without this row there'd be no way to add one.
                                    AddMonitorRow {
                                        add_monitor_provider,
                                        add_monitor_key,
                                        saved_keys_version,
                                        balance_data,
                                        balance_errors,
                                        balance_meta,
                                        last_fetch_error,
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One-line "Add monitor" row: [provider select] [API key input] [Add].
/// Lives at the bottom of the provider list and inside the empty state. Always
/// visible — no toggle — so there's a permanent entry point for adding the
/// first, next, or a duplicate provider. On Add: `save_monitor` appends a new
/// instance (ids like glm-mon-1 / glm-mon-2, never dedupes), then a fresh
/// fetch makes it appear as its own card.
#[component]
fn AddMonitorRow(
    add_monitor_provider: Signal<String>,
    add_monitor_key: Signal<String>,
    saved_keys_version: Signal<u32>,
    balance_data: Signal<std::collections::HashMap<String, balance::ProviderResult>>,
    balance_errors: Signal<std::collections::HashMap<String, String>>,
    balance_meta: Signal<std::collections::HashMap<String, String>>,
    last_fetch_error: Signal<String>,
) -> Element {
    let mut add_monitor_provider = add_monitor_provider;
    let mut add_monitor_key = add_monitor_key;
    let mut saved_keys_version = saved_keys_version;
    let balance_data = balance_data;
    let balance_errors = balance_errors;
    let balance_meta = balance_meta;
    let last_fetch_error = last_fetch_error;
    let cur_provider = add_monitor_provider.read().clone();
    let cur_key = add_monitor_key.read().clone();
    // Persist the draft key and kick off a fetch for the new instance.
    // Shared closure body (inlined in both Enter and Add-button handlers).
    rsx! {
        div {
            class: "card-key-row monitor-add-row",
            select {
                class: "card-key-select",
                value: "{cur_provider}",
                onchange: move |evt| {
                    add_monitor_provider.set(evt.value());
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
                    add_monitor_key.set(evt.value());
                },
                onkeydown: move |evt| {
                    if evt.key() == Key::Enter {
                        let k = add_monitor_key.read().trim().to_string();
                        if !k.is_empty() {
                            let prov = add_monitor_provider.read().clone();
                            storage::save_monitor(&prov, &k);
                            add_monitor_key.set(String::new());
                            saved_keys_version.set(saved_keys_version() + 1);
                            spawn(async move {
                                refresh_balance(
                                    balance_data,
                                    balance_errors,
                                    balance_meta,
                                    last_fetch_error,
                                ).await;
                            });
                        }
                    }
                    if evt.key() == Key::Escape {
                        add_monitor_key.set(String::new());
                        evt.stop_propagation();
                    }
                },
            }
            button {
                class: "card-key-btn primary",
                onclick: move |_| {
                    let k = add_monitor_key.read().trim().to_string();
                    if !k.is_empty() {
                        let prov = add_monitor_provider.read().clone();
                        storage::save_monitor(&prov, &k);
                        add_monitor_key.set(String::new());
                        saved_keys_version.set(saved_keys_version() + 1);
                        spawn(async move {
                            refresh_balance(
                                balance_data,
                                balance_errors,
                                balance_meta,
                                last_fetch_error,
                            ).await;
                        });
                    }
                },
                "Add"
            }
        }
    }
}

/// Flag button that cycles None → Low → Medium → High → None. `with_label`
/// renders the wider chip used in edit mode; the default is the compact
/// square that sits beside the add-input.
#[component]
fn PriorityButton(priority: Signal<Option<memo::Priority>>, with_label: bool) -> Element {
    let mut priority = priority;
    let p = *priority.read();
    let (color, name) = match p {
        None => ("p-none", "None"),
        Some(memo::Priority::Low) => ("p-low", "Low"),
        Some(memo::Priority::Medium) => ("p-med", "Medium"),
        Some(memo::Priority::High) => ("p-high", "High"),
    };
    let class = if with_label {
        format!("prio-chip {color}")
    } else {
        format!("attr-btn {color}")
    };
    rsx! {
        button {
            class: "{class}",
            title: "Priority: {name} — click to cycle",
            onclick: move |_| {
                let next = memo::next_priority(*priority.read());
                priority.set(next);
            },
            if with_label {
                "⚑ {name}"
            } else {
                "⚑"
            }
        }
    }
}

/// Due-date editor row: quick presets plus a datetime-local picker, and the
/// current value with a clear button once set. Used under the add-input and
/// inside edit mode.
#[component]
fn DueChips(due: Signal<Option<i64>>) -> Element {
    let mut due = due;
    let mut picking = use_signal(|| false);
    rsx! {
        div {
            class: "due-chips",
            button {
                class: "chip",
                title: "Due today at 18:00",
                onclick: move |_| {
                    due.set(memo::preset_due(0, 18, 0));
                    picking.set(false);
                },
                "Today"
            }
            button {
                class: "chip",
                title: "Due tomorrow at 09:00",
                onclick: move |_| {
                    due.set(memo::preset_due(1, 9, 0));
                    picking.set(false);
                },
                "Tomorrow"
            }
            button {
                class: "chip",
                title: "Due in 7 days at 09:00",
                onclick: move |_| {
                    due.set(memo::preset_due(7, 9, 0));
                    picking.set(false);
                },
                "+7d"
            }
            button {
                class: "chip",
                title: "Pick a date and time",
                onclick: move |_| picking.toggle(),
                "Pick…"
            }
            if picking() {
                input {
                    class: "date-input",
                    r#type: "datetime-local",
                    value: "{due().map(memo::to_local_input).unwrap_or_default()}",
                    onchange: move |evt| {
                        if let Some(ts) = memo::parse_local_datetime(&evt.value()) {
                            due.set(Some(ts));
                        }
                        picking.set(false);
                    },
                }
            }
            if let Some(d) = *due.read() {
                span { class: "due-current", "{memo::due_label(d)}" }
                button {
                    class: "chip clear",
                    title: "Clear due date",
                    onclick: move |_| due.set(None),
                    "×"
                }
            }
        }
    }
}

/// One task row, in view or edit mode. View: checkbox, content with a flag /
/// due / age meta line, and hover actions. Edit: input plus attribute chips.
#[component]
fn MemoRow(
    item: memo::Memo,
    editing: bool,
    flash: bool,
    edit_text: Signal<String>,
    edit_priority: Signal<Option<memo::Priority>>,
    edit_due: Signal<Option<i64>>,
    on_toggle: EventHandler<String>,
    on_start_edit: EventHandler<String>,
    on_delete: EventHandler<String>,
    on_commit: EventHandler<()>,
    on_cancel_edit: EventHandler<()>,
) -> Element {
    let mut edit_text = edit_text;
    let mut item_class = String::from("memo-item");
    if item.done {
        item_class.push_str(" done");
    }
    if flash {
        item_class.push_str(" flash");
    }
    if editing {
        item_class.push_str(" editing");
    }

    rsx! {
        div {
            class: "{item_class}",
            key: "{item.id}",
            if editing {
                div {
                    class: "edit-body",
                    input {
                        class: "edit-input",
                        value: "{edit_text.read()}",
                        // Focus as soon as the input appears:
                        // one click on ✎, then type.
                        onmounted: move |evt| async move {
                            let _ = evt.set_focus(true).await;
                        },
                        oninput: move |evt| edit_text.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter && !evt.is_composing() {
                                on_commit.call(());
                            } else if evt.key() == Key::Escape {
                                on_cancel_edit.call(());
                                evt.stop_propagation();
                            }
                        },
                    }
                    div {
                        class: "edit-chips",
                        PriorityButton { priority: edit_priority, with_label: true }
                        DueChips { due: edit_due }
                    }
                }
                div {
                    class: "edit-actions",
                    button {
                        class: "edit-save",
                        title: "Save",
                        onclick: move |_| on_commit.call(()),
                        "✓"
                    }
                    button {
                        class: "edit-cancel",
                        title: "Cancel",
                        onclick: move |_| on_cancel_edit.call(()),
                        "✕"
                    }
                }
            } else {
                button {
                    class: if item.done { "check-btn checked" } else { "check-btn" },
                    title: if item.done { "Mark as not done" } else { "Mark as done" },
                    onclick: {
                        let id = item.id.clone();
                        move |_| on_toggle.call(id.clone())
                    },
                    "✓"
                }
                div {
                    class: "memo-main",
                    span {
                        class: "memo-text",
                        // Full text on hover — the row itself
                        // is single-line with an ellipsis.
                        title: "{item.content}",
                        "{item.content}"
                    }
                    div {
                        class: "memo-meta-row",
                        if let Some(p) = item.priority {
                            {
                                let (color, name) = match p {
                                    memo::Priority::Low => ("p-low", "Low"),
                                    memo::Priority::Medium => ("p-med", "Medium"),
                                    memo::Priority::High => ("p-high", "High"),
                                };
                                rsx! {
                                    span { class: "meta-flag {color}", "⚑ {name}" }
                                }
                            }
                        }
                        if item.done {
                            span { class: "memo-meta",
                                "Done {item.completed_at.map(memo::time_ago).unwrap_or_default()}"
                            }
                        } else if let Some(d) = item.due {
                            span {
                                class: if item.is_overdue() { "meta-due overdue" } else { "meta-due" },
                                title: "Due {memo::due_label(d)}",
                                "{memo::due_label(d)}"
                            }
                        } else {
                            span { class: "memo-meta", "{memo::time_ago(item.updated_at)}" }
                        }
                    }
                }
                div {
                    class: "memo-actions",
                    button {
                        class: "icon-btn edit-btn-icon",
                        title: "Edit",
                        onclick: {
                            let id = item.id.clone();
                            move |_| on_start_edit.call(id.clone())
                        },
                        "✎"
                    }
                    button {
                        class: "icon-btn delete-btn-icon",
                        title: "Delete (undo available)",
                        onclick: {
                            let id = item.id.clone();
                            move |_| on_delete.call(id.clone())
                        },
                        "×"
                    }
                }
            }
        }
    }
}

const CSS: &str = include_str!("app.css");

const FACE_COUNT: u8 = 8;
