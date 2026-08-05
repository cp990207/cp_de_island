use serde::{Deserialize, Serialize};

/// Three-level priority (TickTick-style). `None` on the memo means unset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
}

/// Cycle order for the flag button: None → Low → Medium → High → None.
pub fn next_priority(p: Option<Priority>) -> Option<Priority> {
    match p {
        None => Some(Priority::Low),
        Some(Priority::Low) => Some(Priority::Medium),
        Some(Priority::Medium) => Some(Priority::High),
        Some(Priority::High) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memo {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    // Task attributes — all optional so quick capture stays one Enter
    // keystroke. serde(default) lets v1 data files load unchanged.
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub due: Option<i64>,
}

/// Active-list ordering, shared by the panel list and the island tips:
/// due-soonest first (overdue floats to the top on its own), then priority
/// High → Low, then most recently touched.
pub fn urgency_cmp(a: &Memo, b: &Memo) -> std::cmp::Ordering {
    due_rank(a.due)
        .cmp(&due_rank(b.due))
        .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
        .then(b.updated_at.cmp(&a.updated_at))
}

fn due_rank(due: Option<i64>) -> (u8, i64) {
    match due {
        Some(t) => (0, t),
        None => (1, i64::MAX),
    }
}

fn priority_rank(p: Option<Priority>) -> u8 {
    match p {
        Some(Priority::High) => 0,
        Some(Priority::Medium) => 1,
        Some(Priority::Low) => 2,
        None => 3,
    }
}

impl Memo {
    pub fn new(content: String) -> Self {
        let now = unix_now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            created_at: now,
            updated_at: now,
            done: false,
            completed_at: None,
            priority: None,
            due: None,
        }
    }

    /// Past its due time and not done yet — displayed in red.
    pub fn is_overdue(&self) -> bool {
        !self.done && self.due.is_some_and(|d| d < unix_now())
    }
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_local() -> time::OffsetDateTime {
    time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc())
}

fn to_local(timestamp: i64) -> Option<time::OffsetDateTime> {
    let utc = time::OffsetDateTime::from_unix_timestamp(timestamp).ok()?;
    Some(utc.to_offset(now_local().offset()))
}

pub fn time_ago(timestamp: i64) -> String {
    let now = unix_now();
    let secs = now.saturating_sub(timestamp);
    match secs {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s if s < 604800 => format!("{}d ago", s / 86400),
        _ => format_date(timestamp),
    }
}

/// Older than a week: show an absolute date ("MM-DD" this year, full
/// "YYYY-MM-DD" across years) instead of an ever-growing "Nw ago".
fn format_date(timestamp: i64) -> String {
    let Some(local) = to_local(timestamp) else {
        return String::new();
    };
    let now_year = now_local().year();
    if local.year() == now_year {
        format!("{:02}-{:02}", local.month() as u8, local.day())
    } else {
        format!("{}-{:02}-{:02}", local.year(), local.month() as u8, local.day())
    }
}

/// Due time for list rows: "Today 18:00" / "Tomorrow 09:00" / "08-02 14:30"
/// (full year across years). Unlike `time_ago` this is a time the user chose.
pub fn due_label(timestamp: i64) -> String {
    let Some(local) = to_local(timestamp) else {
        return String::new();
    };
    let hm = format!("{:02}:{:02}", local.hour(), local.minute());
    let today = now_local().date();
    let date = local.date();
    if date == today {
        format!("Today {hm}")
    } else if date == today + time::Duration::days(1) {
        format!("Tomorrow {hm}")
    } else if date.year() == today.year() {
        format!("{:02}-{:02} {hm}", local.month() as u8, local.day())
    } else {
        format!("{}-{:02}-{:02} {hm}", local.year(), local.month() as u8, local.day())
    }
}

/// Compact due label for the island pill: "18:00" today, "Tomorrow", "08-02".
pub fn due_label_short(timestamp: i64) -> String {
    let Some(local) = to_local(timestamp) else {
        return String::new();
    };
    let today = now_local().date();
    let date = local.date();
    if date == today {
        format!("{:02}:{:02}", local.hour(), local.minute())
    } else if date == today + time::Duration::days(1) {
        "Tomorrow".to_string()
    } else if date.year() == today.year() {
        format!("{:02}-{:02}", local.month() as u8, local.day())
    } else {
        format!("{}-{:02}-{:02}", local.year(), local.month() as u8, local.day())
    }
}

/// Due-time presets: `days_ahead` days from today at a fixed local time.
pub fn preset_due(days_ahead: i64, hour: u8, minute: u8) -> Option<i64> {
    let now = now_local();
    let date = now.date() + time::Duration::days(days_ahead);
    let time = time::Time::from_hms(hour, minute, 0).ok()?;
    Some(
        time::PrimitiveDateTime::new(date, time)
            .assume_offset(now.offset())
            .unix_timestamp(),
    )
}

/// Parse the "YYYY-MM-DDTHH:MM" value of a datetime-local input as local time.
pub fn parse_local_datetime(s: &str) -> Option<i64> {
    let format =
        time::format_description::parse_borrowed::<2>("[year]-[month]-[day]T[hour]:[minute]")
            .ok()?;
    let parsed = time::PrimitiveDateTime::parse(s, &format).ok()?;
    Some(
        parsed
            .assume_offset(now_local().offset())
            .unix_timestamp(),
    )
}

/// Inverse of `parse_local_datetime`: prefill a datetime-local input.
pub fn to_local_input(timestamp: i64) -> String {
    let Some(local) = to_local(timestamp) else {
        return String::new();
    };
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}",
        local.year(),
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute()
    )
}
