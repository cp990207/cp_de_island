//! Background tasks and effects, expressed as custom hooks so `App` reads as
//! a wiring table. Each is a plain function that calls Dioxus hooks; invoke
//! them unconditionally at the top of the component body.

use super::format;
use super::island::FACE_COUNT;
use super::state::{self, BalanceState, IslandState, MemoListState};
use crate::{balance, memo, storage, windowing};
use dioxus::desktop::DesktopContext;
use dioxus::prelude::*;
use std::time::Duration;

/// Persist on every change — but skip the first run: memos were just loaded
/// from disk, so an immediate rewrite is pure risk (e.g. after a failed
/// load) with zero benefit.
pub fn use_memo_persister(memos: MemoListState) {
    let mut first_run = true;
    use_effect(move || {
        let snapshot = memos.list.read();
        if first_run {
            first_run = false;
            return;
        }
        storage::save_memos(&snapshot);
    });
}

/// The window is fixed-size and click-through: poll the cursor against the
/// live hot regions and only make the window interactive when the cursor is
/// actually over the island or, while expanded, anywhere in the window.
pub fn use_click_through_poller(desktop: DesktopContext, island: IslandState) {
    use_future(move || {
        let d = desktop.clone();
        async move {
            let mut interactive = true;
            loop {
                tokio::time::sleep(Duration::from_millis(30)).await;
                let wide = *island.hovered.peek()
                    || *island.expanded.peek()
                    || *island.coding_expanded.peek();
                let any_open = *island.expanded.peek() || *island.coding_expanded.peek();
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

/// Alternate between the face and the clock while idle; keep the clock
/// fresh. The 5s tick also re-renders the component, which keeps the
/// relative "time ago" labels in the panel up to date.
pub fn use_idle_clock(mut island: IslandState) {
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            island.clock_text.set(format::local_time_hm());
            if *island.expanded.peek() {
                continue;
            }
            let next_show_time = !*island.show_time.peek();
            if !next_show_time {
                let current = *island.face_expr.peek();
                let mut next = uuid::Uuid::new_v4().as_bytes()[0] % FACE_COUNT;
                if next == current {
                    next = (next + 1) % FACE_COUNT;
                }
                island.face_expr.set(next);
            }
            island.show_time.set(next_show_time);
        }
    });
}

/// Due-soon alert: 10 minutes before a task's due time the island pops open
/// and the row flashes — a gentle visual reminder instead of an OS
/// notification. Once per due value (editing the due time re-arms it);
/// purely in-memory, so a restart just re-arms whatever is still upcoming.
pub fn use_due_alerts(mut memos: MemoListState, mut island: IslandState) {
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let now = memo::unix_now();
            let mut fresh: Vec<String> = Vec::new();
            for m in memos.list.peek().iter() {
                if m.done {
                    continue;
                }
                let Some(d) = m.due else {
                    continue;
                };
                if (0..=600).contains(&(d - now)) && !memos.alerted.peek().contains(&m.id) {
                    fresh.push(m.id.clone());
                }
            }
            if fresh.is_empty() {
                continue;
            }
            for id in &fresh {
                memos.alerted.write().insert(id.clone());
                memos.flash.write().insert(id.clone());
            }
            if !*island.expanded.peek() {
                island.expanded.set(true);
            }
            // The CSS flash runs a few pulses; drop the class afterwards so a
            // later re-alert can flash again.
            spawn(async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
                for id in fresh {
                    memos.flash.write().remove(&id);
                }
            });
        }
    });
}

/// Focus the add-input when the panel opens so typing works immediately.
pub fn use_autofocus_input(desktop: DesktopContext, island: IslandState) {
    use_effect(move || {
        if island.expanded() {
            let _ = desktop
                .webview
                .evaluate_script("document.getElementById('memo-input')?.focus();");
        }
    });
}

/// Periodically fetch balance data for all configured providers, sample
/// quota history, and refresh the local token/cost reports.
pub fn use_balance_poller(mut balance: BalanceState) {
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
                    || last_fetch.is_none_or(|t| t.elapsed() >= Duration::from_secs(300)));
            if should_fetch {
                let outcome = state::fetch_monitors().await;
                // Sample 5h/weekly windows into the per-cycle history on
                // every successful fetch.
                let (kimi_dirty, glm_dirty) =
                    state::record_quota_samples(&outcome.quota_samples, &mut history, &mut glm_hist);
                if kimi_dirty {
                    balance::quota_history::save(&history);
                    balance.kimi_history.set(history.clone());
                }
                if glm_dirty {
                    balance::quota_history::save_named("glm-quota-history.json", &glm_hist);
                    balance.glm_history.set(glm_hist.clone());
                }
                balance.apply(outcome);
                last_monitors = monitors;
                last_fetch = Some(std::time::Instant::now());
            } else {
                // Keep last_monitors in sync even when skipping fetch.
                last_monitors = monitors;
            }

            // Local CLI session-log scan for token/cost stats. Needs no API
            // key, so it runs even when the remote fetch above is skipped.
            // The scan cache makes repeat runs cheap.
            let should_scan = last_scan.is_none_or(|t| t.elapsed() >= Duration::from_secs(300));
            if should_scan {
                if let Some(dir) = storage::data_dir() {
                    let records =
                        balance::kimi_local::load_records(&dir.join("kimi-usage-cache.json"));
                    balance
                        .kimi_cost
                        .set(Some(balance::kimi_local::build_report(&records)));
                }
                // ZCode local usage from its SQLite db. Independent of Kimi:
                // ZCode not being installed is fine, build_report returns empty.
                balance
                    .zcode_cost
                    .set(Some(balance::zcode_local::build_report()));
                last_scan = Some(std::time::Instant::now());
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
