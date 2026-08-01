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
        balance::ProviderResult::Quota(quotas) => match quotas.first() {
            Some(q) => (
                Some(q.provider.clone()),
                format!("{}/{}", q.remaining, q.limit),
            ),
            None => (None, "No data".to_string()),
        },
    }
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
    // Coding side: which provider the pill shows; cycled with the mouse wheel.
    let mut coding_index = use_signal(|| 0usize);
    let mut coding_wheel_accum = use_signal(|| 0.0f64);
    // Rolling-swap state for the wheel switch: the previous line keeps
    // rendering as an overlay that rolls out while the new line rolls in.
    // `dir`: 1 = wheel down (next, content rolls up), -1 = wheel up.
    // `gen` guards the delayed overlay cleanup (same pattern as hover_gen).
    let mut prev_tip_index = use_signal(|| None::<usize>);
    let mut tip_swap_dir = use_signal(|| 1i8);
    let mut tip_swap_gen = use_signal(|| 0u64);
    let mut prev_coding_index = use_signal(|| None::<usize>);
    let mut coding_swap_dir = use_signal(|| 1i8);
    let mut coding_swap_gen = use_signal(|| 0u64);
    // Which side the cursor is over: 0 = none, 1 = left (tasks), 2 = right (coding).
    let mut hover_side = use_signal(|| 0u8);
    // Coding panel open state (mutually exclusive with tasks panel).
    let mut coding_expanded = use_signal(|| false);
    // Cached balance data from the last fetch.
    let mut balance_data = use_signal(
        || std::collections::HashMap::<String, balance::ProviderResult>::new(),
    );
    // Config UI state.
    let mut config_provider = use_signal(|| String::from("Kimi"));
    let mut config_key = use_signal(String::new);
    // Last fetch error message (empty when no error or no fetch attempted).
    let mut last_fetch_error = use_signal(String::new);

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

    // Periodically fetch balance data for all configured providers.
    use_future(move || async move {
        let mut last_keys: std::collections::HashMap<String, String> =
            storage::load_all_provider_keys();
        let mut last_fetch: Option<std::time::Instant> = None;
        loop {
            let keys = storage::load_all_provider_keys();
            let keys_changed = keys != last_keys;
            // Fetch when keys changed, or every 5 minutes, or on first run.
            let should_fetch = !keys.is_empty()
                && (keys_changed
                    || last_fetch
                        .map_or(true, |t| t.elapsed() >= Duration::from_secs(300)));
            if should_fetch {
                let results = balance::fetch_all(&keys).await;
                let mut map = std::collections::HashMap::new();
                let mut first_err = String::new();
                for (name, result) in results {
                    match result {
                        Ok(data) => {
                            map.insert(name, data);
                        }
                        Err(e) => {
                            if first_err.is_empty() {
                                first_err = format!("{name}: {e}");
                            }
                        }
                    }
                }
                balance_data.set(map);
                last_fetch_error.set(first_err);
                last_keys = keys;
                last_fetch = Some(std::time::Instant::now());
            } else {
                // Keep last_keys in sync even when skipping fetch (e.g. keys empty).
                last_keys = keys;
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

    // Island coding line as (provider key, optional label, amount), resolved
    // the same way for the live line and the roll-out overlay.
    let coding_line_at = |idx: usize| -> (String, Option<String>, String) {
        let data = balance_data.read();
        let providers: Vec<&String> = data.keys().collect();
        match providers.get(idx.min(providers.len().saturating_sub(1))) {
            Some(name) => {
                let (label, amount) = data
                    .get(*name)
                    .map(coding_pill_line)
                    .unwrap_or_else(|| (None, "No keys".to_string()));
                (name.to_string(), label, amount)
            }
            None => ("none".to_string(), None, "No keys".to_string()),
        }
    };
    let (coding_key, coding_label, coding_amount) = coding_line_at(*coding_index.read());
    let prev_coding_line = (*prev_coding_index.read()).map(|i| coding_line_at(i));
    let coding_roll = if *coding_swap_dir.read() > 0 {
        "roll-up"
    } else {
        "roll-down"
    };
    let coding_out_gen = *coding_swap_gen.read();

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
                    onwheel: move |evt| {
                        let data = balance_data.read();
                        let providers: Vec<&String> = data.keys().collect();
                        let count = providers.len();
                        if count < 2 {
                            return;
                        }
                        let dy = match evt.delta() {
                            WheelDelta::Pixels(v) => v.y,
                            WheelDelta::Lines(v) => v.y * 16.0,
                            WheelDelta::Pages(v) => v.y * 160.0,
                        };
                        let mut accum = *coding_wheel_accum.read() + dy;
                        if accum.signum() != dy.signum() {
                            accum = dy;
                        }
                        const STEP: f64 = 48.0;
                        let old_idx = *coding_index.read();
                        let mut idx = old_idx;
                        while accum >= STEP {
                            idx = (idx + 1) % count;
                            accum -= STEP;
                        }
                        while accum <= -STEP {
                            idx = (idx + count - 1) % count;
                            accum += STEP;
                        }
                        coding_wheel_accum.set(accum);
                        // A full wrap lands back on the same provider — no swap.
                        if idx == old_idx {
                            return;
                        }
                        // Same rolling swap as the tips side.
                        prev_coding_index.set(Some(old_idx));
                        coding_swap_dir.set(if dy > 0.0 { 1 } else { -1 });
                        *coding_swap_gen.write() += 1;
                        let generation = *coding_swap_gen.read();
                        coding_index.set(idx);
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(400)).await;
                            if *coding_swap_gen.peek() == generation {
                                prev_coding_index.set(None);
                            }
                        });
                    },
                    div {
                        class: "coding-content",
                        span { class: "coding-icon" }
                        div {
                            class: "pill-swap {coding_roll}",
                            if let Some((_, prev_label, prev_amount)) = prev_coding_line {
                                div {
                                    class: "pill-swap-line swap-out",
                                    key: "out-{coding_out_gen}",
                                    if let Some(l) = prev_label {
                                        span { class: "coding-provider-label", "{l}" }
                                    }
                                    span { class: "coding-amount", "{prev_amount}" }
                                }
                            }
                            div {
                                class: "pill-swap-line swap-in",
                                key: "{coding_key}",
                                if let Some(l) = coding_label {
                                    span { class: "coding-provider-label", "{l}" }
                                }
                                span { class: "coding-amount", "{coding_amount}" }
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
                        let providers: Vec<&String> = data.keys().collect();
                        let idx = *coding_index.read();
                        if providers.is_empty() {
                            // Distinguish three cases when nothing is shown:
                            //  1. No key saved at all           -> "No API keys configured"
                            //  2. Key saved but fetch errored   -> show the error message
                            //  3. Key saved, fetch in-flight    -> "Fetching balance..."
                            let saved_keys = storage::load_all_provider_keys();
                            let err = last_fetch_error.read().clone();
                            if saved_keys.is_empty() {
                                rsx! {
                                    div {
                                        class: "coding-empty",
                                        span { class: "coding-empty-icon", "⚡" }
                                        p { "No API keys configured." }
                                        p { "Add one below to get started." }
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
                            let current_name = providers[idx.min(providers.len() - 1)];
                            let current_data = data.get(current_name);
                            rsx! {
                                div {
                                    class: "provider-summary",
                                    span { class: "provider-summary-name", "{current_name}" }
                                    match current_data {
                                        Some(balance::ProviderResult::Balance(b)) => rsx! {
                                            span { class: "provider-summary-amount", "{b.currency} {b.remaining:.2}" }
                                            if let Some(ref bd) = b.breakdown {
                                                span { class: "provider-summary-detail",
                                                    "Paid: {b.currency} {bd.paid:.2} · Granted: {b.currency} {bd.granted:.2}"
                                                }
                                            }
                                        },
                                        Some(balance::ProviderResult::Quota(quotas)) => rsx! {
                                            if let Some(q) = quotas.first() {
                                                span { class: "provider-summary-amount", "{q.remaining} / {q.limit}" }
                                                if let Some(ref reset) = q.reset_at {
                                                    span { class: "provider-summary-detail", "Resets: {reset}" }
                                                }
                                            }
                                        },
                                        Some(balance::ProviderResult::Both { balance: b, quotas }) => rsx! {
                                            span { class: "provider-summary-amount", "{b.currency} {b.remaining:.2}" }
                                            if !quotas.is_empty() {
                                                span { class: "provider-summary-detail",
                                                    "{quotas[0].remaining}/{quotas[0].limit} ({quotas[0].window})"
                                                }
                                            }
                                        },
                                        None => rsx! { span { class: "provider-summary-amount", "..." } },
                                    }
                                }
                                div {
                                    class: "provider-list",
                                    for (i, name) in providers.iter().enumerate() {
                                        {
                                            let is_current = i == idx.min(providers.len() - 1);
                                            let icon_class = match name.as_str() {
                                                "Kimi" => "provider-card-icon kimi",
                                                "DeepSeek" => "provider-card-icon deepseek",
                                                "MiniMax" => "provider-card-icon minimax",
                                                "GLM" => "provider-card-icon glm",
                                                _ => "provider-card-icon",
                                            };
                                            let result = data.get(*name);
                                            rsx! {
                                                div {
                                                    class: if is_current { "provider-card hovered" } else { "provider-card" },
                                                    key: "{name}",
                                                    div { class: "{icon_class}",
                                                        match name.as_str() {
                                                            "Kimi" => "K",
                                                            "DeepSeek" => "DS",
                                                            "MiniMax" => "MM",
                                                            "GLM" => "G",
                                                            _ => "?",
                                                        }
                                                    }
                                                    div {
                                                        class: "provider-card-info",
                                                        span { class: "provider-card-name", "{name}" }
                                                        match result {
                                                            Some(balance::ProviderResult::Quota(qs)) => rsx! {
                                                                if let Some(q) = qs.first() {
                                                                    span { class: "provider-card-detail", "{q.window}: {q.remaining}/{q.limit}" }
                                                                }
                                                            },
                                                            Some(balance::ProviderResult::Balance(b)) => rsx! {
                                                                if let Some(ref bd) = b.breakdown {
                                                                    span { class: "provider-card-detail", "P:{bd.paid:.0} G:{bd.granted:.0}" }
                                                                }
                                                            },
                                                            _ => rsx! {},
                                                        }
                                                    }
                                                    span { class: "provider-card-value",
                                                        match result {
                                                            Some(balance::ProviderResult::Balance(b)) => format!("{:.2}", b.remaining),
                                                            Some(balance::ProviderResult::Quota(qs)) => {
                                                                qs.first().map(|q| format!("{}/{}", q.remaining, q.limit)).unwrap_or_default()
                                                            }
                                                            Some(balance::ProviderResult::Both { balance: b, .. }) => format!("{:.2}", b.remaining),
                                                            None => "...".to_string(),
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
                    div {
                        class: "config-section",
                        div {
                            class: "config-row",
                            select {
                                class: "config-select",
                                value: "{config_provider.read()}",
                                onchange: move |evt| config_provider.set(evt.value()),
                                option { value: "Kimi", "Kimi" }
                                option { value: "DeepSeek", "DeepSeek" }
                                option { value: "MiniMax", "MiniMax" }
                                option { value: "GLM", "GLM" }
                            }
                            input {
                                class: "config-input",
                                r#type: "password",
                                value: "{config_key.read()}",
                                placeholder: "API Key...",
                                oninput: move |evt| config_key.set(evt.value()),
                                onkeydown: move |evt| {
                                    if evt.key() == Key::Escape {
                                        config_key.set(String::new());
                                        evt.stop_propagation();
                                    }
                                },
                            }
                        }
                        div {
                            class: "config-actions",
                            button {
                                class: "config-btn secondary",
                                onclick: move |_| {
                                    spawn(async move {
                                        let keys = storage::load_all_provider_keys();
                                        if keys.is_empty() {
                                            return;
                                        }
                                        let results = balance::fetch_all(&keys).await;
                                        let mut map = std::collections::HashMap::new();
                                        let mut first_err = String::new();
                                        for (name, result) in results {
                                            match result {
                                                Ok(data) => {
                                                    map.insert(name, data);
                                                }
                                                Err(e) => {
                                                    if first_err.is_empty() {
                                                        first_err = format!("{name}: {e}");
                                                    }
                                                }
                                            }
                                        }
                                        balance_data.set(map);
                                        last_fetch_error.set(first_err);
                                    });
                                },
                                "↻ Refresh"
                            }
                            button {
                                class: "config-btn primary",
                                onclick: move |_| {
                                    let provider = config_provider.read().clone();
                                    let key = config_key.read().trim().to_string();
                                    if !key.is_empty() {
                                        storage::save_provider_key(&provider, &key);
                                        config_key.set(String::new());
                                        // Fetch immediately so the user sees the new balance.
                                        spawn(async move {
                                            let keys = storage::load_all_provider_keys();
                                            let results = balance::fetch_all(&keys).await;
                                            let mut map = std::collections::HashMap::new();
                                            let mut first_err = String::new();
                                            for (name, result) in results {
                                                match result {
                                                    Ok(data) => {
                                                        map.insert(name, data);
                                                    }
                                                    Err(e) => {
                                                        if first_err.is_empty() {
                                                            first_err = format!("{name}: {e}");
                                                        }
                                                    }
                                                }
                                            }
                                            balance_data.set(map);
                                            last_fetch_error.set(first_err);
                                        });
                                    }
                                },
                                "Save"
                            }
                        }
                    }
                }
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
