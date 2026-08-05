//! The view layer. `App` assembles the shared state, wires the background
//! hooks, and lays out the three top-level pieces — island, tasks panel,
//! coding panel. Feature logic lives in `state` (actions) and `hooks`
//! (background tasks); the pieces live in their own modules.

mod coding_panel;
mod format;
mod hooks;
mod island;
mod memo_panel;
mod state;

use crate::windowing::{COLLAPSED_W, ISLAND_BLEED};
use crate::{balance, storage};
use coding_panel::CodingPanel;
use dioxus::prelude::*;
use island::Island;
use memo_panel::MemoPanel;
use state::{BalanceState, EditState, InputState, IslandState, MemoListState};
use std::collections::{HashMap, HashSet};

const CSS: &str = include_str!("../app.css");

#[component]
pub fn App() -> Element {
    let desktop = dioxus::desktop::window();

    // --- state assembly ---
    let memos = MemoListState {
        list: use_signal(storage::load_memos),
        search: use_signal(String::new),
        show_completed: use_signal(|| true),
        deleted: use_signal(|| None),
        toast_gen: use_signal(|| 0),
        alerted: use_signal(HashSet::new),
        flash: use_signal(HashSet::new),
    };
    let edit = EditState {
        id: use_signal(|| None),
        text: use_signal(String::new),
        priority: use_signal(|| None),
        due: use_signal(|| None),
    };
    let input = InputState {
        text: use_signal(String::new),
        priority: use_signal(|| None),
        due: use_signal(|| None),
        show_due_strip: use_signal(|| false),
    };
    let island = IslandState {
        expanded: use_signal(|| false),
        coding_expanded: use_signal(|| false),
        hovered: use_signal(|| false),
        hover_side: use_signal(|| 0),
        hover_gen: use_signal(|| 0),
        show_time: use_signal(|| false),
        face_expr: use_signal(|| 0),
        clock_text: use_signal(format::local_time_hm),
        suppress_click: use_signal(|| false),
        tip_index: use_signal(|| 0),
        wheel_accum: use_signal(|| 0.0),
        prev_tip_index: use_signal(|| None),
        tip_swap_dir: use_signal(|| 1),
        tip_swap_gen: use_signal(|| 0),
        coding_tip_index: use_signal(|| 0),
        coding_wheel_accum: use_signal(|| 0.0),
        prev_coding_index: use_signal(|| None),
        coding_swap_dir: use_signal(|| 1),
        coding_swap_gen: use_signal(|| 0),
        tip_step_at: use_signal(std::time::Instant::now),
        coding_step_at: use_signal(std::time::Instant::now),
    };
    let balance = BalanceState {
        data: use_signal(HashMap::new),
        errors: use_signal(HashMap::new),
        meta: use_signal(HashMap::new),
        last_error: use_signal(String::new),
        kimi_history: use_signal(balance::quota_history::load),
        kimi_cost: use_signal(|| None),
        glm_history: use_signal(|| balance::quota_history::load_named("glm-quota-history.json")),
        zcode_cost: use_signal(|| None),
    };

    // --- background hooks ---
    hooks::use_memo_persister(memos);
    hooks::use_click_through_poller(desktop.clone(), island);
    hooks::use_idle_clock(island);
    hooks::use_due_alerts(memos, island);
    hooks::use_autofocus_input(desktop.clone(), island);
    hooks::use_balance_poller(balance);

    let stage_class = if island.expanded() {
        "stage visual-expanded"
    } else if island.coding_expanded() {
        "stage coding-expanded"
    } else {
        "stage"
    };

    let stage_style = format!(
        "--collapsed-width: {}px; --island-bleed: {}px;",
        COLLAPSED_W, ISLAND_BLEED
    );

    rsx! {
        style { "{CSS}" }
        main {
            class: "{stage_class}",
            style: "{stage_style}",
            // While expanded the whole window is interactive: a click landing
            // on the transparent margin collapses the panel (popover-style).
            // The island and the panel stop propagation so their own clicks
            // never reach this handler.
            onclick: move |_| island.collapse(memos, edit),
            // Esc collapses the panel. Inputs handle their own Esc first
            // (clear draft / cancel edit) and stop propagation.
            onkeydown: move |evt| {
                if evt.key() == Key::Escape {
                    island.collapse(memos, edit);
                }
            },
            Island { island, memos, edit, balance }
            MemoPanel { memos, edit, input }
            CodingPanel { balance }
        }
    }
}
