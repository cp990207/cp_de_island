//! The island itself: the always-visible circle/pill bar. Left side is the
//! tasks pill (wheel-cycled tips), right side the coding pill (balance
//! summary); the circle idles between a face and the clock.

use super::format;
use super::state::{BalanceState, EditState, IslandState, MemoListState};
use crate::{memo, storage};
use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use std::time::Duration;

/// Number of face expression assets (face-0 .. face-N-1 in the CSS).
pub const FACE_COUNT: u8 = 8;

#[component]
pub fn Island(
    island: IslandState,
    memos: MemoListState,
    edit: EditState,
    balance: BalanceState,
) -> Element {
    let desktop = dioxus::desktop::window();

    let memo_count = memos.list.read().len();
    // Pill fallback when there is no active task to show.
    let empty_text = if memo_count == 0 { "No tasks" } else { "All done" };

    let chevron_visible = island.expanded() && memo_count > 0;

    // Island mode: open panel takes priority, then hover, then idle.
    let is_circle = !island.expanded() && !island.coding_expanded() && island.hover_side() == 0;
    let is_side_left = island.expanded() || (!island.coding_expanded() && island.hover_side() == 1);

    let island_class = if is_circle {
        if island.show_time() {
            "island circle show-time".to_string()
        } else {
            "island circle".to_string()
        }
    } else if is_side_left {
        "island side-left-mode".to_string()
    } else {
        "island side-right-mode".to_string()
    };

    let face_class = format!("circle-face face-{}", island.face_expr());

    // Island pill tips: every active task in urgency order, ignoring the
    // search filter — the pill reflects global state. The most urgent task
    // sits on top, which is what the pill shows by default.
    let mut tips: Vec<memo::Memo> = memos
        .list
        .read()
        .iter()
        .filter(|m| !m.done)
        .cloned()
        .collect();
    tips.sort_by(memo::urgency_cmp);
    let tips_len = tips.len();
    // The modulo guards against the list shrinking (completions/deletes)
    // while the wheel index points past the new end.
    let current_tip = if tips_len == 0 {
        None
    } else {
        tips.get(*island.tip_index.read() % tips_len).cloned()
    };

    // Rolling swap: the outgoing line survives one animation cycle as an
    // overlay. The modulo guards against the list shrinking mid-animation.
    let prev_tip = island
        .prev_tip_index
        .read()
        .and_then(|i| tips.get(i % tips_len.max(1)).cloned());
    let tip_roll = if *island.tip_swap_dir.read() > 0 {
        "roll-up"
    } else {
        "roll-down"
    };
    let tip_out_gen = *island.tip_swap_gen.read();

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

    // Island coding pill: one summary line per saved monitor instance,
    // cycled with the mouse wheel (wrap-around), mirroring the task tips on
    // the left side. Ordered like the panel: provider type rank, then
    // instance id. Each item is (instance id, provider label, summary line);
    // the line is None when the last fetch errored.
    let coding_items: Vec<(String, String, Option<String>)> = {
        let data = balance.data.read();
        let errs = balance.errors.read();
        let meta = balance.meta.read();
        let mut ids: Vec<&String> = data.keys().collect();
        for k in errs.keys() {
            if !ids.contains(&k) {
                ids.push(k);
            }
        }
        ids.sort_by(|a, b| {
            let rank = |id: &str| meta.get(id).map_or(99, |s| format::provider_rank(s));
            rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
        });
        ids.iter()
            .map(|id| {
                let label = meta.get(*id).cloned().unwrap_or_else(|| (*id).to_string());
                let line = data.get(*id).map(|r| format::coding_pill_line(r).1);
                ((*id).clone(), label, line)
            })
            .collect()
    };
    let coding_len = coding_items.len();
    // The modulo guards against the list shrinking (removed monitors) while
    // the wheel index points past the new end — same guard as the tips.
    let current_coding = if coding_len == 0 {
        None
    } else {
        coding_items
            .get(*island.coding_tip_index.read() % coding_len)
            .cloned()
    };
    let prev_coding = island
        .prev_coding_index
        .read()
        .and_then(|i| coding_items.get(i % coding_len.max(1)).cloned());
    let coding_roll = if *island.coding_swap_dir.read() > 0 {
        "roll-up"
    } else {
        "roll-down"
    };
    let coding_out_gen = *island.coding_swap_gen.read();

    // One coding pill line (provider label + summary), shared by the
    // roll-in and roll-out layers of the swap animation.
    let coding_spans = |item: &(String, String, Option<String>)| {
        rsx! {
            span { class: "coding-provider-label", "{item.1}" }
            if let Some(amount) = &item.2 {
                span { class: "coding-amount", "{amount}" }
            }
        }
    };

    rsx! {
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
                        island.suppress_click.set(true);
                        d.drag();
                        if let Ok(pos) = d.window.outer_position() {
                            storage::save_window_pos(pos.x, pos.y);
                        }
                        spawn(async move {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            island.suppress_click.set(false);
                        });
                    }
                }
            },
            div {
                class: if island.hover_side() == 1 { "side-left hovered" } else { "side-left" },
                onmouseenter: move |_| island.enter_side(1),
                onmouseleave: move |_| island.leave_side(),
                onclick: move |evt: MouseEvent| {
                    evt.stop_propagation();
                    if *island.suppress_click.peek() {
                        island.suppress_click.set(false);
                        return;
                    }
                    island.coding_expanded.set(false);
                    if island.expanded() {
                        edit.commit(memos);
                    }
                    island.expanded.toggle();
                },
                onwheel: move |evt| {
                    let dy = match evt.delta() {
                        WheelDelta::Pixels(v) => v.y,
                        WheelDelta::Lines(v) => v.y * 16.0,
                        WheelDelta::Pages(v) => v.y * 160.0,
                    };
                    island.scroll_tips(dy, tips_len);
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
                span { class: "circle-clock", "{island.clock_text.read()}" }
                if chevron_visible {
                    span { class: "pill-chevron", "▾" }
                }
            }
            div { class: "pill-divider" }
            div {
                class: if island.hover_side() == 2 { "side-right hovered" } else { "side-right" },
                onmouseenter: move |_| island.enter_side(2),
                onmouseleave: move |_| island.leave_side(),
                onclick: move |evt: MouseEvent| {
                    evt.stop_propagation();
                    if *island.suppress_click.peek() {
                        island.suppress_click.set(false);
                        return;
                    }
                    island.expanded.set(false);
                    island.coding_expanded.toggle();
                },
                onwheel: move |evt| {
                    let dy = match evt.delta() {
                        WheelDelta::Pixels(v) => v.y,
                        WheelDelta::Lines(v) => v.y * 16.0,
                        WheelDelta::Pages(v) => v.y * 160.0,
                    };
                    island.scroll_coding(dy, coding_len);
                },
                div {
                    class: "coding-content",
                    span { class: "coding-icon" }
                    div {
                        class: "pill-swap {coding_roll}",
                        if let Some(prev) = prev_coding {
                            div {
                                class: "pill-swap-line swap-out",
                                key: "cout-{coding_out_gen}",
                                {coding_spans(&prev)}
                            }
                        }
                        if let Some(cur) = current_coding {
                            div {
                                class: "pill-swap-line swap-in",
                                key: "{cur.0}",
                                {coding_spans(&cur)}
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
    }
}
