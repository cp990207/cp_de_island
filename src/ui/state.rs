//! Shared application state: plain `Copy` groups of Dioxus signals, one per
//! feature area. Grouping keeps component props narrow and avoids the
//! prop-drilling a flat list of ~40 signals would cause. Feature actions
//! (add/toggle/delete/edit, hover, collapse, balance fetch) live here as
//! methods so components stay pure view code.
//!
//! Field-like accessors (`expanded()`, `show_time()`, …) read the underlying
//! signal and subscribe the calling component, exactly like calling a bare
//! `Signal` — they exist so call sites read the same as before the grouping.

use crate::balance::quota_history::QuotaHistory;
use crate::balance::{self, ProviderResult, quota_history};
use crate::memo::{self, Memo, Priority};
use crate::storage;
use dioxus::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// The task list plus its UI adjuncts (search, undo toast, due-soon flash).
#[derive(Clone, Copy, PartialEq)]
pub struct MemoListState {
    pub list: Signal<Vec<Memo>>,
    pub search: Signal<String>,
    pub show_completed: Signal<bool>,
    /// Single-level undo: the last deleted memo and its original position.
    pub deleted: Signal<Option<(Memo, usize)>>,
    pub toast_gen: Signal<u64>,
    /// Due-soon alerts (in-memory): ids already alerted, ids flashing now.
    pub alerted: Signal<HashSet<String>>,
    pub flash: Signal<HashSet<String>>,
}

/// The in-progress edit of a single row (at most one row edits at a time).
#[derive(Clone, Copy, PartialEq)]
pub struct EditState {
    pub id: Signal<Option<String>>,
    pub text: Signal<String>,
    pub priority: Signal<Option<Priority>>,
    pub due: Signal<Option<i64>>,
}

/// The add-input draft and its staged attributes (TickTick-style icons
/// beside the input; optional, so typing + Enter still captures instantly).
#[derive(Clone, Copy, PartialEq)]
pub struct InputState {
    pub text: Signal<String>,
    pub priority: Signal<Option<Priority>>,
    pub due: Signal<Option<i64>>,
    pub show_due_strip: Signal<bool>,
}

/// Island chrome: panel open state, hover, idle face/clock, tip carousel.
#[derive(Clone, Copy, PartialEq)]
pub struct IslandState {
    /// Tasks panel open (mutually exclusive with `coding_expanded`).
    pub expanded: Signal<bool>,
    /// Coding panel open (mutually exclusive with `expanded`).
    pub coding_expanded: Signal<bool>,
    pub hovered: Signal<bool>,
    /// Which side the cursor is over: 0 = none, 1 = left (tasks), 2 = right.
    pub hover_side: Signal<u8>,
    pub hover_gen: Signal<u64>,
    pub show_time: Signal<bool>,
    pub face_expr: Signal<u8>,
    pub clock_text: Signal<String>,
    /// Set by a Shift+drag so the trailing click does not toggle the panel.
    pub suppress_click: Signal<bool>,
    /// Which tip the island pill shows; cycled with the mouse wheel.
    pub tip_index: Signal<usize>,
    /// Wheel delta accumulator (px) so notched wheels and smooth touchpads
    /// both step one tip at a time.
    pub wheel_accum: Signal<f64>,
    /// Rolling-swap state for the wheel switch: the previous line keeps
    /// rendering as an overlay that rolls out while the new line rolls in.
    /// `dir`: 1 = wheel down (next, content rolls up), -1 = wheel up.
    /// `gen` guards the delayed overlay cleanup (same pattern as hover_gen).
    pub prev_tip_index: Signal<Option<usize>>,
    pub tip_swap_dir: Signal<i8>,
    pub tip_swap_gen: Signal<u64>,
}

/// Coding-side data from the periodic fetch: remote balances, local cost
/// reports, and per-provider quota histories.
#[derive(Clone, Copy, PartialEq)]
pub struct BalanceState {
    /// Cached balance data from the last fetch. Keyed by monitor instance id
    /// (e.g. "glm-mon-2"), NOT by provider name — that allows multiple
    /// instances of the same provider type to coexist as separate cards.
    pub data: Signal<HashMap<String, ProviderResult>>,
    /// Per-instance fetch errors. `data` only holds successes, so this map
    /// carries the failures — that way a single failed instance still shows
    /// up as a card with its error instead of vanishing silently.
    pub errors: Signal<HashMap<String, String>>,
    /// Instance id → provider type name (e.g. "glm-mon-2" → "GLM"). The card
    /// renderer needs the type to pick icons / cost views / history series,
    /// but the data maps are keyed by id, so this side table bridges them.
    pub meta: Signal<HashMap<String, String>>,
    /// Last fetch error message (empty when no error or no fetch attempted).
    pub last_error: Signal<String>,
    /// Kimi quota history (per-cycle peaks) and local token/cost report from
    /// the CLI session logs. Both refresh on the same 5-minute cadence.
    pub kimi_history: Signal<QuotaHistory>,
    pub kimi_cost: Signal<Option<balance::kimi_local::CostReport>>,
    /// GLM counterpart: per-cycle history (own file) + ZCode-local token/cost
    /// report mined from ~/.zcode/cli/db/db.sqlite.
    pub glm_history: Signal<QuotaHistory>,
    pub zcode_cost: Signal<Option<balance::zcode_local::ZcodeCostReport>>,
}

impl MemoListState {
    pub fn show_completed(&self) -> bool {
        *self.show_completed.read()
    }

    /// Quick capture: insert the draft as a new task at the top, then reset
    /// the input draft. List order is derived at render time.
    pub fn add(mut self, mut input: InputState) {
        let text = input.text.read().trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut m = Memo::new(text);
        m.priority = *input.priority.read();
        m.due = *input.due.read();
        let mut list = self.list.read().clone();
        list.insert(0, m);
        self.list.set(list);
        input.text.set(String::new());
        input.priority.set(None);
        input.due.set(None);
        input.show_due_strip.set(false);
        // Make sure the fresh task is visible even if a search was active.
        self.search.set(String::new());
    }

    /// Completion is the primary positive action — distinct from delete. The
    /// task strikes through and sinks into the Completed group; clicking the
    /// checkbox again restores it.
    pub fn toggle_done(mut self, id: String) {
        let mut list = self.list.read().clone();
        let Some(pos) = list.iter().position(|m| m.id == id) else {
            return;
        };
        let m = &mut list[pos];
        m.done = !m.done;
        m.completed_at = if m.done { Some(memo::unix_now()) } else { None };
        self.list.set(list);
    }

    /// Soft delete: stash the memo for a few seconds so it can be restored
    /// from the toast. Single-level undo — a new delete replaces the stash.
    pub fn delete(mut self, mut edit: EditState, id: String) {
        if edit.id.read().as_ref() == Some(&id) {
            edit.id.set(None);
        }
        let mut list = self.list.read().clone();
        let Some(pos) = list.iter().position(|m| m.id == id) else {
            return;
        };
        let removed = list.remove(pos);
        self.list.set(list);
        self.alerted.write().remove(&id);
        self.flash.write().remove(&id);
        self.deleted.set(Some((removed, pos)));
        *self.toast_gen.write() += 1;
        let generation = *self.toast_gen.read();
        spawn(async move {
            tokio::time::sleep(Duration::from_secs(6)).await;
            if *self.toast_gen.peek() != generation {
                return;
            }
            self.deleted.set(None);
        });
    }

    pub fn undo_delete(mut self) {
        if let Some((m, pos)) = self.deleted.read().clone() {
            let mut list = self.list.read().clone();
            list.insert(pos.min(list.len()), m);
            self.list.set(list);
        }
        // Cancel the auto-dismiss timer and close the toast.
        *self.toast_gen.write() += 1;
        self.deleted.set(None);
    }
}

impl EditState {
    /// Commit the in-progress edit (if any): a non-empty change is saved;
    /// list order is derived at render time, so nothing bubbles manually. An
    /// emptied edit is discarded, restoring the old content instead of
    /// failing silently.
    pub fn commit(mut self, mut memos: MemoListState) {
        let Some(id) = self.id.read().clone() else {
            return;
        };
        let text = self.text.read().trim().to_string();
        if !text.is_empty() {
            let new_priority = *self.priority.read();
            let new_due = *self.due.read();
            let mut list = memos.list.read().clone();
            if let Some(pos) = list.iter().position(|m| m.id == id) {
                let m = &mut list[pos];
                if m.content != text || m.priority != new_priority || m.due != new_due {
                    if m.due != new_due {
                        // New due time — re-arm the due-soon alert.
                        memos.alerted.write().remove(&id);
                    }
                    m.content = text;
                    m.priority = new_priority;
                    m.due = new_due;
                    m.updated_at = memo::unix_now();
                    memos.list.set(list);
                }
            }
        }
        self.id.set(None);
        memos.search.set(String::new());
    }

    /// Starting an edit commits the previous one first: switching targets
    /// must never silently throw away typed text.
    pub fn start(mut self, memos: MemoListState, id: String) {
        self.commit(memos);
        let Some(m) = memos.list.read().iter().find(|m| m.id == id).cloned() else {
            return;
        };
        self.id.set(Some(id));
        self.text.set(m.content);
        self.priority.set(m.priority);
        self.due.set(m.due);
    }

    pub fn cancel(mut self) {
        self.id.set(None);
    }
}

impl InputState {
    pub fn show_due_strip(&self) -> bool {
        *self.show_due_strip.read()
    }
}

impl IslandState {
    pub fn expanded(&self) -> bool {
        *self.expanded.read()
    }

    pub fn coding_expanded(&self) -> bool {
        *self.coding_expanded.read()
    }

    pub fn hover_side(&self) -> u8 {
        *self.hover_side.read()
    }

    pub fn show_time(&self) -> bool {
        *self.show_time.read()
    }

    pub fn face_expr(&self) -> u8 {
        *self.face_expr.read()
    }

    /// Debounced hover-in: 100 ms grace so a passing cursor doesn't pop the
    /// island open.
    pub fn enter_side(mut self, side: u8) {
        *self.hover_gen.write() += 1;
        let generation = *self.hover_gen.read();
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if *self.hover_gen.peek() != generation {
                return;
            }
            self.hovered.set(true);
            self.hover_side.set(side);
        });
    }

    /// Debounced hover-out: 1 s grace; suppressed while a panel is open.
    pub fn leave_side(mut self) {
        if *self.expanded.peek() || *self.coding_expanded.peek() {
            return;
        }
        *self.hover_gen.write() += 1;
        let generation = *self.hover_gen.read();
        spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if *self.hover_gen.peek() != generation {
                return;
            }
            self.hovered.set(false);
            self.hover_side.set(0);
        });
    }

    /// Collapse whichever panel is open: commit any pending edit and reset
    /// hover — after a margin click the cursor is not on the island, so
    /// `hovered` must not linger and keep the island wide.
    pub fn collapse(mut self, memos: MemoListState, edit: EditState) {
        if !self.expanded() && !self.coding_expanded() {
            return;
        }
        edit.commit(memos);
        self.expanded.set(false);
        self.coding_expanded.set(false);
        self.hovered.set(false);
        self.hover_side.set(0);
    }

    /// Wheel-step the tip carousel by the accumulated pixel delta. A full
    /// wrap lands back on the same tip — no swap. Otherwise the outgoing
    /// line survives one roll cycle; the generation guard drops it after the
    /// animation.
    pub fn scroll_tips(mut self, dy: f64, tips_len: usize) {
        if tips_len < 2 {
            return;
        }
        let mut accum = *self.wheel_accum.read() + dy;
        if accum.signum() != dy.signum() {
            accum = dy;
        }
        const STEP: f64 = 48.0;
        let old_idx = *self.tip_index.read();
        let mut idx = old_idx;
        while accum >= STEP {
            idx = (idx + 1) % tips_len;
            accum -= STEP;
        }
        while accum <= -STEP {
            idx = (idx + tips_len - 1) % tips_len;
            accum += STEP;
        }
        self.wheel_accum.set(accum);
        if idx == old_idx {
            return;
        }
        // Keep the outgoing line for one roll cycle; the generation guard
        // drops it after the animation.
        self.prev_tip_index.set(Some(old_idx));
        self.tip_swap_dir.set(if dy > 0.0 { 1 } else { -1 });
        *self.tip_swap_gen.write() += 1;
        let generation = *self.tip_swap_gen.read();
        self.tip_index.set(idx);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            if *self.tip_swap_gen.peek() == generation {
                self.prev_tip_index.set(None);
            }
        });
    }
}

/// One successful 5h/weekly quota window, queued for history sampling.
pub struct QuotaSample {
    pub provider: String,
    pub series: quota_history::Series,
    pub reset_at: Option<String>,
    pub used: u64,
    pub limit: u64,
}

/// Everything a fetch round produced, folded once and shared by the periodic
/// poller and the manual Add / Remove / Re-fetch paths.
pub struct FetchOutcome {
    pub data: HashMap<String, ProviderResult>,
    pub errors: HashMap<String, String>,
    pub meta: HashMap<String, String>,
    pub first_error: String,
    pub quota_samples: Vec<QuotaSample>,
}

/// Re-fetch every saved monitor instance and fold the results into per-id
/// maps. Centralised so Add / Remove / Re-fetch / poller all share one
/// source of truth. `data` is keyed by instance id, `meta` always records
/// the instance → type mapping (even on fetch error, so the card renderer
/// can still pick icons/cost views).
pub async fn fetch_monitors() -> FetchOutcome {
    let monitors = storage::load_monitors();
    let keys: Vec<balance::MonitorKey> = monitors
        .iter()
        .map(|m| balance::MonitorKey {
            id: m.id.clone(),
            provider: m.provider.clone(),
            key: m.key.clone(),
        })
        .collect();
    let results = balance::fetch_all(&keys).await;
    let mut outcome = FetchOutcome {
        data: HashMap::new(),
        errors: HashMap::new(),
        meta: HashMap::new(),
        first_error: String::new(),
        quota_samples: Vec::new(),
    };
    for (id, provider, result) in results {
        outcome.meta.insert(id.clone(), provider.clone());
        match result {
            Ok(data) => {
                // Queue 5h/weekly windows for per-cycle history sampling.
                if let ProviderResult::Quota(qs) = &data {
                    for q in &qs.quotas {
                        let series = match q.window.as_str() {
                            "weekly" => Some(quota_history::Series::Weekly),
                            "5h" => Some(quota_history::Series::Session),
                            _ => None,
                        };
                        if let Some(series) = series {
                            outcome.quota_samples.push(QuotaSample {
                                provider: provider.clone(),
                                series,
                                reset_at: q.reset_at.clone(),
                                used: q.used,
                                limit: q.limit,
                            });
                        }
                    }
                }
                outcome.data.insert(id, data);
            }
            Err(e) => {
                if outcome.first_error.is_empty() {
                    outcome.first_error = format!("{provider}: {e}");
                }
                outcome.errors.insert(id, e.to_string());
            }
        }
    }
    outcome
}

/// Fold a fetch round's quota samples into the per-provider histories.
/// History is sampled per provider TYPE (not per instance): Kimi type →
/// `kimi`, GLM type → `glm`. Returns `(kimi_dirty, glm_dirty)`.
pub fn record_quota_samples(
    samples: &[QuotaSample],
    kimi: &mut QuotaHistory,
    glm: &mut QuotaHistory,
) -> (bool, bool) {
    let mut kimi_dirty = false;
    let mut glm_dirty = false;
    for s in samples {
        match s.provider.as_str() {
            "Kimi" => kimi_dirty |= kimi.record(s.series, s.reset_at.as_deref(), s.used, s.limit),
            "GLM" => glm_dirty |= glm.record(s.series, s.reset_at.as_deref(), s.used, s.limit),
            _ => {}
        }
    }
    (kimi_dirty, glm_dirty)
}

impl BalanceState {
    /// Push a fetch outcome into the signals.
    pub fn apply(mut self, outcome: FetchOutcome) {
        self.data.set(outcome.data);
        self.errors.set(outcome.errors);
        self.meta.set(outcome.meta);
        self.last_error.set(outcome.first_error);
    }

    /// Manual refresh shared by Add / Remove / Re-fetch: re-fetch all saved
    /// monitor instances and update the data/error/meta signals. Does not
    /// sample history — that is the poller's job.
    pub async fn refresh(self) {
        let outcome = fetch_monitors().await;
        self.apply(outcome);
    }
}
