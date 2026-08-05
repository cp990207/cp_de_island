//! The tasks panel: quick-capture input, search, the active/completed task
//! lists, and the undo toast. Also hosts the small widgets used inside it
//! (PriorityButton, DueChips, MemoRow) — they exist only to serve this panel.

use super::state::{EditState, InputState, MemoListState};
use crate::memo::{self, Memo};
use dioxus::prelude::*;

#[component]
pub fn MemoPanel(memos: MemoListState, edit: EditState, input: InputState) -> Element {
    let memo_count = memos.list.read().len();

    // Split into the two display groups. The stored vec order no longer
    // matters for display — sorting happens here, at render time.
    let query = memos.search.read().trim().to_lowercase();
    let matches = |m: &&Memo| query.is_empty() || m.content.to_lowercase().contains(query.as_str());

    let mut active: Vec<Memo> = memos
        .list
        .read()
        .iter()
        .filter(|m| !m.done && matches(m))
        .cloned()
        .collect();
    active.sort_by(memo::urgency_cmp);

    let mut completed: Vec<Memo> = memos
        .list
        .read()
        .iter()
        .filter(|m| m.done && matches(m))
        .cloned()
        .collect();
    completed.sort_by_key(|m| std::cmp::Reverse(m.completed_at.unwrap_or(0)));
    let completed_count = completed.len();
    let no_match = memo_count > 0 && active.is_empty() && completed.is_empty();

    rsx! {
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
                        value: "{input.text.read()}",
                        placeholder: "Type a task...",
                        oninput: move |evt| input.text.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter && !evt.is_composing() {
                                memos.add(input);
                            } else if evt.key() == Key::Escape {
                                // Esc throws away the whole draft,
                                // staged attributes included.
                                input.text.set(String::new());
                                input.priority.set(None);
                                input.due.set(None);
                                input.show_due_strip.set(false);
                                evt.stop_propagation();
                            }
                        },
                    }
                    button {
                        class: if input.show_due_strip() || input.due.read().is_some() {
                            "attr-btn active"
                        } else {
                            "attr-btn"
                        },
                        title: "Set due date",
                        onclick: move |_| input.show_due_strip.toggle(),
                        span { class: "due-icon" }
                    }
                    PriorityButton { priority: input.priority, with_label: false }
                    button {
                        class: "add-btn",
                        disabled: input.text.read().trim().is_empty(),
                        onclick: move |_| memos.add(input),
                        "Add"
                    }
                }
                if input.show_due_strip() || input.due.read().is_some() {
                    DueChips { due: input.due }
                }
                if memo_count >= 5 {
                    div {
                        class: "search-row",
                        input {
                            class: "search-input",
                            value: "{memos.search.read()}",
                            placeholder: "Search tasks...",
                            oninput: move |evt| memos.search.set(evt.value()),
                            onkeydown: move |evt| {
                                if evt.key() == Key::Escape {
                                    memos.search.set(String::new());
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
                        MemoRow {
                            key: "{item.id}",
                            item: item.clone(),
                            editing: edit.id.read().as_ref() == Some(&item.id),
                            flash: memos.flash.read().contains(&item.id),
                            edit_text: edit.text,
                            edit_priority: edit.priority,
                            edit_due: edit.due,
                            on_toggle: move |id: String| memos.toggle_done(id),
                            on_start_edit: move |id: String| edit.start(memos, id),
                            on_delete: move |id: String| memos.delete(edit, id),
                            on_commit: move |_| edit.commit(memos),
                            on_cancel_edit: move |_| edit.cancel(),
                        }
                    }
                    if !completed.is_empty() {
                        button {
                            class: "completed-header",
                            onclick: move |_| memos.show_completed.toggle(),
                            span { class: "completed-chevron",
                                if memos.show_completed() { "▾" } else { "▸" }
                            }
                            "Completed ({completed_count})"
                        }
                        if memos.show_completed() {
                            for item in completed {
                                MemoRow {
                                    key: "{item.id}",
                                    item: item.clone(),
                                    editing: edit.id.read().as_ref() == Some(&item.id),
                                    flash: false,
                                    edit_text: edit.text,
                                    edit_priority: edit.priority,
                                    edit_due: edit.due,
                                    on_toggle: move |id: String| memos.toggle_done(id),
                                    on_start_edit: move |id: String| edit.start(memos, id),
                                    on_delete: move |id: String| memos.delete(edit, id),
                                    on_commit: move |_| edit.commit(memos),
                                    on_cancel_edit: move |_| edit.cancel(),
                                }
                            }
                        }
                    }
                }
                if memos.deleted.read().is_some() {
                    div {
                        class: "undo-toast",
                        span { class: "undo-text", "Task deleted" }
                        button {
                            class: "undo-btn",
                            onclick: move |_| memos.undo_delete(),
                            "Undo"
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
