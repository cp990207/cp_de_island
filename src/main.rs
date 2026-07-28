#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod memo;
mod storage;
mod windowing;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use std::time::Duration;
use windowing::{COLLAPSED_W, ISLAND_BLEED, WINDOW_H, WINDOW_W};

#[cfg(target_os = "windows")]
use dioxus::desktop::tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config())
        .launch(App);
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
            windowing::place_top_center(&handle, initial_w);
        })
}

fn local_time_hm() -> String {
    let now =
        time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:02}:{:02}", now.hour(), now.minute())
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
    let mut editing_id = use_signal(|| None::<String>);
    let mut edit_text = use_signal(String::new);

    use_effect(move || {
        storage::save_memos(&memos.read());
    });

    // The window is fixed-size and click-through: poll the cursor against the
    // live hot regions and only make the window interactive when the cursor is
    // actually over the island or the panel.
    {
        let d = desktop.clone();
        use_future(move || {
            let d = d.clone();
            async move {
                let mut interactive = true;
                loop {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let wide = *hovered.peek() || *expanded.peek();
                    let rects = windowing::hot_rects(&d.window, *expanded.peek(), wide);
                    let want = windowing::cursor_inside(&rects);
                    if want != interactive {
                        interactive = want;
                        windowing::set_click_through(&d.window, !want);
                    }
                }
            }
        });
    }

    // Alternate between the face and the clock while idle; keep the clock fresh.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            clock_text.set(local_time_hm());
            if *expanded.peek() || *hovered.peek() {
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

    let memo_count = memos.read().len();
    let latest_time = memos
        .read()
        .iter()
        .map(|m| m.updated_at)
        .max()
        .map(memo::time_ago)
        .unwrap_or_default();

    let count_text = match memo_count {
        0 => "No memos".to_string(),
        1 => "1 memo".to_string(),
        n => format!("{n} memos"),
    };

    let chevron_visible = expanded() && memo_count > 0;

    let island_class = if !expanded() && !hovered() {
        if show_time() {
            "island circle show-time"
        } else {
            "island circle"
        }
    } else {
        "island"
    };

    let face_class = format!("circle-face face-{}", face_expr());

    let stage_class = if expanded() {
        "stage visual-expanded"
    } else {
        "stage"
    };

    let stage_style = format!(
        "--collapsed-width: {}px; --island-bleed: {}px;",
        COLLAPSED_W, ISLAND_BLEED
    );

    let mut do_add = move || {
        let text = input_text.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut list = memos.read().clone();
        list.insert(0, memo::Memo::new(text));
        memos.set(list);
        input_text.set(String::new());
    };

    let mut do_delete = move |id: String| {
        if editing_id.read().as_ref() == Some(&id) {
            editing_id.set(None);
        }
        let mut list = memos.read().clone();
        list.retain(|m| m.id != id);
        memos.set(list);
    };

    let mut do_start_edit = move |id: String, content: String| {
        editing_id.set(Some(id));
        edit_text.set(content);
    };

    let mut do_save_edit = move |id: String| {
        let text = edit_text.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        if editing_id.read().as_ref() != Some(&id) {
            return;
        }
        let now = memo::unix_now();
        let mut list = memos.read().clone();
        if let Some(m) = list.iter_mut().find(|m| m.id == id) {
            m.content = text;
            m.updated_at = now;
        }
        memos.set(list);
        editing_id.set(None);
    };

    let mut do_cancel_edit = move |_: ()| {
        editing_id.set(None);
    };

    rsx! {
        style { "{CSS}" }
        main {
            class: "{stage_class}",
            style: "{stage_style}",
            oncontextmenu: {
                let d = desktop.clone();
                move |_| d.close()
            },
            section {
                class: "{island_class}",
                onmouseenter: move |_| {
                    *hover_gen.write() += 1;
                    hovered.set(true);
                },
                onmouseleave: move |_| {
                    if *expanded.peek() {
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
                    });
                },
                onclick: move |_| {
                    expanded.toggle();
                },
                onmousedown: {
                    let d = desktop.clone();
                    move |evt: MouseEvent| {
                        if evt.modifiers().shift()
                            && evt.trigger_button()
                                .is_some_and(|b| b == MouseButton::Primary)
                        {
                            d.drag();
                        }
                    }
                },
                div {
                    class: "pill-content",
                    span { class: "pill-icon", "📝" }
                    span { class: "pill-count", "{count_text}" }
                    if memo_count > 0 {
                        span { class: "pill-sep", "·" }
                        span { class: "pill-time", "{latest_time}" }
                    }
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

            div {
                class: "panel-shell",
                div {
                    class: "panel",
                    div {
                        class: "input-row",
                        input {
                            class: "memo-input",
                            value: "{input_text.read()}",
                            placeholder: "Type a memo...",
                            oninput: move |evt| input_text.set(evt.value()),
                            onkeydown: {
                                let mut do_add = do_add.clone();
                                move |evt| {
                                    if evt.key() == Key::Enter && !evt.is_composing() {
                                        do_add();
                                    }
                                }
                            },
                        }
                        button {
                            class: "add-btn",
                            onclick: move |_| do_add(),
                            "Add"
                        }
                    }
                    div {
                        class: "memo-list",
                        if memo_count == 0 {
                            div {
                                class: "empty-state",
                                span { class: "empty-icon", "✏️" }
                                p { "No memos yet. Type something above." }
                            }
                        }
                        for item in memos.read().iter().cloned() {
                            {
                                let is_editing = editing_id.read().as_ref() == Some(&item.id);
                                let item_class = if is_editing {
                                    "memo-item editing"
                                } else {
                                    "memo-item"
                                };
                                rsx! {
                                    div {
                                        class: "{item_class}",
                                        key: "{item.id}",
                                        if is_editing {
                                            input {
                                                class: "edit-input",
                                                value: "{edit_text.read()}",
                                                oninput: move |evt| edit_text.set(evt.value()),
                                                onkeydown: {
                                                    let id = item.id.clone();
                                                    let mut do_save = do_save_edit.clone();
                                                    let mut do_cancel = do_cancel_edit.clone();
                                                    move |evt| {
                                                        if evt.key() == Key::Enter && !evt.is_composing() {
                                                            do_save(id.clone());
                                                        } else if evt.key() == Key::Escape {
                                                            do_cancel(());
                                                        }
                                                    }
                                                },
                                            }
                                            div {
                                                class: "edit-actions",
                                                button {
                                                    class: "edit-save",
                                                    onclick: {
                                                        let id = item.id.clone();
                                                        let mut do_save = do_save_edit.clone();
                                                        move |_| do_save(id.clone())
                                                    },
                                                    "✓"
                                                }
                                                button {
                                                    class: "edit-cancel",
                                                    onclick: move |_| do_cancel_edit(()),
                                                    "✕"
                                                }
                                            }
                                        } else {
                                            span { class: "memo-text", "{item.content}" }
                                            span { class: "memo-meta", "{memo::time_ago(item.updated_at)}" }
                                            div {
                                                class: "memo-actions",
                                                button {
                                                    class: "icon-btn edit-btn-icon",
                                                    title: "Edit",
                                                    onclick: {
                                                        let id = item.id.clone();
                                                        let content = item.content.clone();
                                                        let mut start = do_start_edit.clone();
                                                        move |_| start(id.clone(), content.clone())
                                                    },
                                                    "✎"
                                                }
                                                button {
                                                    class: "icon-btn delete-btn-icon",
                                                    title: "Delete",
                                                    onclick: {
                                                        let id = item.id.clone();
                                                        let mut del = do_delete.clone();
                                                        move |_| del(id.clone())
                                                    },
                                                    "×"
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
    }
}

const CSS: &str = include_str!("app.css");

const FACE_COUNT: u8 = 8;
