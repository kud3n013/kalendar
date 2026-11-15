//! Data, preferences, saving and the simple task planner.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---- layout and behaviour constants ----
pub const GUTTER_W: f32 = 58.0; // width of the hour gutter
pub const EVENT_MIN_H: f32 = 12.0; // smallest height an event block is drawn at
pub const OVERLAP_INSET: f32 = 16.0; // indent of each overlap layer
pub const OVERLAP_CONTENT_H: f32 = 68.0; // part of an overlapped event kept readable
pub const EVENT_EDGE_GAP: f32 = 1.0; // 1 px trimmed from top and bottom of each event
pub const PX_PER_SLOT: f32 = 26.0; // hour height = round(60 / slot * 26)
pub const HOUR_H_DEFAULT: f32 = 104.0; // hour height at the default 15-minute resolution
pub const HOUR_H_MIN: f32 = 24.0;
pub const HOUR_H_MAX: f32 = 360.0;
pub const SCROLL_BACK_MIN: i64 = 90; // opens 90 minutes before now
pub const SCROLL_ROUND_MIN: i64 = 15; // rounded down to 15 minutes
pub const CLOCK_REFRESH_S: u64 = 15; // clock line refresh
pub const SLOT_CHOICES: [i64; 4] = [5, 15, 30, 60];
pub const SNAP_CHOICES: [i64; 3] = [5, 15, 30];
pub const UNDO_CHOICES: [u32; 5] = [0, 5, 10, 15, 30];
pub const PLAN_SPAN_DAYS: i64 = 3; // the planner fills this many days
pub const PLAN_MAX_SPAN_DAYS: i64 = 8; // longest look-ahead
pub const ANIM_FAST_S: f32 = 0.1;
pub const ANIM_S: f32 = 0.175;
pub const ANIM_LONG_S: f32 = 0.275;
/// one page of the calendar sliding over to the next
pub const ANIM_SLIDE_S: f32 = 0.36;
/// Event colours, in menu order.
pub const EVENT_COLORS: [([u8; 3], &str); 13] = [
    ([0xf1, 0x76, 0xa8], "Pink"),
    ([0xeb, 0x62, 0x62], "Red"),
    ([0xff, 0x8f, 0x1e], "Orange"),
    ([0xb8, 0x73, 0x33], "Brown"),
    ([0xe8, 0xae, 0x26], "Gold"),
    ([0x8c, 0xba, 0x43], "Lime"),
    ([0x5c, 0xb2, 0x81], "Green"),
    ([0x38, 0xc2, 0xc7], "Cyan"),
    ([0x35, 0x85, 0xe1], "Blue"),
    ([0x77, 0x7b, 0xf0], "Indigo"),
    ([0xa2, 0x41, 0xf1], "Purple"),
    ([0xc4, 0x59, 0xdf], "Magenta"),
    ([0xb0, 0xac, 0xb9], "Grey"),
];
pub const PLAN_MIN_CHUNK: i64 = 30; // shortest split chunk
pub const PLAN_CHUNK_FROM: i64 = 120; // tasks at least this long may be split
pub const WORK_START_MIN: i64 = 8 * 60; // working hours 08:00
pub const WORK_END_MIN: i64 = 18 * 60; // to 18:00, Monday to Friday
pub const DUE_SOON_DAYS: i64 = 7; // "Due soon" looks this many days ahead
/// below this window width: one day, no task panel
pub const COMPACT_W: f32 = 700.0;
pub const ZOOM_STEP: f32 = 1.1; // one wheel notch while zooming
pub const SYNC_EVERY_S: f64 = 900.0; // subscribed and synced calendars refresh every 15 minutes
pub const PUSH_AFTER_S: f64 = 2.0; // a change made here is sent to its server this much later
pub const NEXTCLOUD_WAIT_S: u64 = 300; // how long a browser sign-in is waited for
pub const SYNC_ON_FOCUS_S: f64 = 60.0; // coming back to the window refreshes, at most this often
pub const NEW_EVENT_MIN: i64 = 60; // new events are one hour long
pub const NEW_EVENT_NOON_H: i64 = 12; // on a later day they start at 12:00
pub const RADIUS: f32 = 8.0; // the one corner radius used across the UI

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum View {
    Agenda,
    Days(u8),
    Week,
    TwoWeeks,
    Month,
}

impl View {
    pub fn label(&self) -> String {
        match self {
            View::Agenda => "Agenda".into(),
            View::Days(1) => "1 day".into(),
            View::Days(n) => format!("{n} days"),
            View::Week => "Week".into(),
            View::TwoWeeks => "2 weeks".into(),
            View::Month => "Month".into(),
        }
    }
    pub fn all() -> Vec<View> {
        let mut v = vec![View::Agenda];
        for n in 1..=9 {
            v.push(View::Days(n));
        }
        v.extend([View::Week, View::TwoWeeks, View::Month]);
        v
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum DueMove {
    Always,
    Ask,
    Never,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug, Default)]
pub enum PanelSide {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub first_day: u8, // 0 = Sunday
    pub hour12: bool,
    pub initial_view: View,
    pub weekends_visible: bool,
    pub dim_past: bool,
    pub show_completed_tasks: bool,
    pub slot_min: i64,
    pub snap_min: i64,
    pub day_max_events: usize,
    pub theme: Theme,
    pub grid_start_h: i64,
    pub grid_end_h: i64,
    pub sidebar_open: bool,
    pub sidebar_w: f32,
    pub sidebar_side: PanelSide,
    pub show_scheduled_tasks: bool,
    pub undo_grace_s: u32,
    pub auto_due: bool,
    pub due_move: DueMove,
    pub default_task_min: i64,
    pub split_long_tasks: bool,
    pub plan_without_due: bool,
    /// the planner puts tasks into matching frames first
    pub use_frames: bool,
    /// tasks no frame fits go into free working time; else they are left
    pub plan_outside_frames: bool,
    pub show_due_today: bool,
    pub show_due_tomorrow: bool,
    pub show_due_soon: bool,
    pub due_on_calendar: bool,
    /// pixels per hour chosen by zooming; None follows the time resolution
    pub hour_height_px: Option<f32>,
    /// passwords may go into a file when no keyring is running
    pub plaintext_secrets: bool,
    /// shortcut overrides: command id -> keys, e.g. "plan" -> "Ctrl+Shift+P"
    pub keybinds: std::collections::HashMap<String, String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            first_day: 0,
            hour12: false,
            initial_view: View::Week,
            weekends_visible: true,
            dim_past: false,
            show_completed_tasks: true,
            slot_min: 15,
            snap_min: 15,
            day_max_events: 4,
            theme: Theme::System,
            grid_start_h: 0,
            grid_end_h: 24,
            sidebar_open: true,
            sidebar_w: 410.0,
            sidebar_side: PanelSide::Left,
            show_scheduled_tasks: false,
            undo_grace_s: 10,
            auto_due: true,
            due_move: DueMove::Ask,
            default_task_min: 60,
            split_long_tasks: true,
            plan_without_due: true,
            use_frames: true,
            plan_outside_frames: true,
            show_due_today: true,
            show_due_tomorrow: true,
            show_due_soon: true,
            due_on_calendar: true,
            hour_height_px: None,
            plaintext_secrets: false,
            keybinds: Default::default(),
        }
    }
}

impl Prefs {
    pub fn hour_height(&self) -> f32 {
        if let Some(h) = self.hour_height_px {
            return h.round().clamp(HOUR_H_MIN, HOUR_H_MAX);
        }
        if self.slot_min == 15 {
            return HOUR_H_DEFAULT;
        }
        (60.0 / self.slot_min as f32 * PX_PER_SLOT)
            .round()
            .clamp(HOUR_H_MIN, HOUR_H_MAX)
    }
    pub fn week_start(&self, d: NaiveDate) -> NaiveDate {
        let dow = d.weekday().num_days_from_sunday() as i64;
        let back = (dow - self.first_day as i64).rem_euclid(7);
        d - Duration::days(back)
    }
    /// Hour label of the time gutter.
    pub fn fmt_hour(&self, t: NaiveTime) -> String {
        if self.hour12 {
            self.fmt_time(t)
        } else {
            format!("{:02}", t.hour())
        }
    }
    pub fn fmt_time(&self, t: NaiveTime) -> String {
        if self.hour12 {
            let (pm, h) = t.hour12();
            let ap = if pm { "PM" } else { "AM" };
            if t.minute() == 0 {
                format!("{h} {ap}")
            } else {
                format!("{h}:{:02} {ap}", t.minute())
            }
        } else {
            format!("{:02}:{:02}", t.hour(), t.minute())
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Calendar {
    /// stable identity; events and frames point at it. 0 only in data
    /// files written before ids existed, until `Data::migrate` runs
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    /// link of a subscribed calendar; its events are read-only
    #[serde(default)]
    pub url: Option<String>,
    /// when it was last fetched, as text
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// the synced account it belongs to; None for own and subscribed ones
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<u64>,
    /// its address or id on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_id: Option<String>,
    /// the server's sync token, to ask only for what changed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctag: Option<String>,
    /// its events cannot be changed here
    #[serde(default)]
    pub read_only: bool,
    /// on its server it can hold tasks as well as events
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub holds_tasks: bool,
    /// its link is private and kept in the keyring; data.json holds
    /// `SECRET_URL` in its place
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub url_secret: bool,
}

/// What data.json holds for a link that is kept in the keyring.
pub const SECRET_URL: &str = "keyring:";

/// A calendar server the person signed in to. Its password is in the
/// keyring, under the account's id.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Account {
    pub id: u64,
    /// the address as typed
    pub server: String,
    pub username: String,
    /// where the calendars were found
    pub home: Option<String>,
    pub last_sync: Option<String>,
    pub error: Option<String>,
    /// a Google account, signed in through the browser; the keyring holds
    /// its refresh token and client secret
    pub google: bool,
    /// a Microsoft account, reached through Microsoft Graph; the keyring
    /// holds its refresh token
    pub microsoft: bool,
    /// the client ID the person brought, for Google or Microsoft
    pub client_id: Option<String>,
    /// signed in through Nextcloud's own page; its app password can be
    /// taken back on signing out
    pub nextcloud: bool,
    /// a Proton account, read-only; the keyring holds its session and
    /// what opens its keys, never the password
    pub proton: bool,
}

/// An event deleted here that the server still has.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PendingDelete {
    pub calendar: u64,
    pub href: String,
    pub etag: Option<String>,
}

impl Account {
    /// "name @ server" for lists.
    pub fn label(&self) -> String {
        if self.google || self.microsoft || self.proton {
            return self.username.clone();
        }
        let host = self.server.split("://").last().unwrap_or("").split('/').next().unwrap_or("");
        format!("{} @ {}", self.username, host)
    }
}

impl Calendar {
    /// Kept on this computer only: not subscribed and not synced.
    pub fn is_local(&self) -> bool {
        self.url.is_none() && self.account.is_none()
    }
    /// A synced calendar whose server takes changes.
    pub fn is_synced_writable(&self) -> bool {
        self.account.is_some() && !self.read_only
    }
    /// Events can be made and changed in it.
    pub fn writable(&self) -> bool {
        self.is_local() || self.is_synced_writable()
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Event {
    pub id: u64,
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    /// id of its calendar
    pub calendar: u64,
    pub notes: String,
    pub task_id: Option<u64>,
    /// own colour, chosen from the right-click menu
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// came from a subscribed calendar
    #[serde(default)]
    pub remote: bool,
    /// UID of the event on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// which occurrence of a repeating event this is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_id: Option<String>,
    /// where it lives on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// the event as the server sent it, so nothing is lost when writing back
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// changed here and not yet sent to the server
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
}

impl Event {
    /// Forgets where the event lives on a server: it becomes a new event
    /// of one's own.
    pub fn detach(&mut self) {
        self.remote = false;
        self.dirty = false;
        (self.uid, self.recurrence_id, self.href, self.etag, self.raw) = (None, None, None, None, None);
    }
    /// The parts a server keeps are the same in both.
    pub fn same_content(&self, o: &Event) -> bool {
        self.title == o.title && self.start == o.start && self.end == o.end && self.all_day == o.all_day && self.notes == o.notes
    }
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub notes: String,
    pub due: Option<NaiveDate>,
    /// 0 none, 1-3 high, 4-5 medium, 6-9 low
    pub priority: u8,
    pub duration_min: Option<i64>,
    pub done: bool,
    /// the planner may move its sessions
    #[serde(default)]
    pub flexible: bool,
    /// task list it belongs to; 0 is the Inbox
    #[serde(default)]
    pub list: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    /// not to be planned before this day
    #[serde(default)]
    pub earliest: Option<NaiveDate>,
    /// how its due date repeats once it is completed
    #[serde(default)]
    pub repeat: Repeat,
    /// the same server fields as on events, for synced task lists
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug, Default)]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
}

impl Repeat {
    pub const ALL: [Repeat; 5] = [Repeat::None, Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Monthly];
    pub fn label(&self) -> &'static str {
        match self {
            Repeat::None => "No repeat",
            Repeat::Daily => "Every day",
            Repeat::Weekdays => "Every weekday",
            Repeat::Weekly => "Every week",
            Repeat::Monthly => "Every month",
        }
    }
    /// The next date after `d` in this pattern.
    pub fn next(&self, d: NaiveDate) -> NaiveDate {
        match self {
            Repeat::None => d,
            Repeat::Daily => d + Duration::days(1),
            Repeat::Weekdays => {
                let mut n = d + Duration::days(1);
                while matches!(n.weekday(), Weekday::Sat | Weekday::Sun) {
                    n += Duration::days(1);
                }
                n
            }
            Repeat::Weekly => d + Duration::days(7),
            Repeat::Monthly => d.checked_add_months(chrono::Months::new(1)).unwrap_or(d + Duration::days(30)),
        }
    }
}

/// Which tasks a frame takes. Empty parts mean "any".
#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
#[serde(default)]
pub struct FrameFilter {
    /// task lists it takes; 0 is the Inbox
    pub lists: Vec<u64>,
    pub tags: Vec<String>,
    /// all tags must be on the task, not just one of them
    pub tags_all: bool,
    /// longest task it takes, in minutes
    pub max_min: Option<i64>,
}

impl FrameFilter {
    pub fn is_empty(&self) -> bool {
        self.lists.is_empty() && self.tags.is_empty() && self.max_min.is_none()
    }
    /// How many conditions it sets; the more, the more specific.
    pub fn specificity(&self) -> usize {
        usize::from(!self.lists.is_empty()) + usize::from(!self.tags.is_empty()) + usize::from(self.max_min.is_some())
    }
}

/// A recurring block of time reserved for a kind of work. Frames are never
/// busy time; the planner puts matching tasks inside them.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Frame {
    pub id: u64,
    pub name: String,
    /// id of one of one's own calendars, never a subscribed one
    pub calendar: u64,
    pub color: Option<[u8; 3]>,
    /// minutes from midnight; the frame ends on the same day
    pub start_min: i64,
    pub end_min: i64,
    /// bit 0 = Monday ... bit 6 = Sunday
    pub days: u8,
    pub every_weeks: u8,
    pub from: NaiveDate,
    pub until: Option<NaiveDate>,
    /// single days left out
    pub skip: Vec<NaiveDate>,
    pub filter: FrameFilter,
    /// tasks outside the filter are kept out of it
    pub exclusive: bool,
    pub enabled: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            id: 0,
            name: String::new(),
            calendar: 0,
            color: None,
            start_min: 9 * 60,
            end_min: 11 * 60,
            days: FRAME_WEEKDAYS,
            every_weeks: 1,
            from: NaiveDate::default(),
            until: None,
            skip: vec![],
            filter: FrameFilter::default(),
            exclusive: true,
            enabled: true,
        }
    }
}

/// Monday to Friday as frame day bits.
pub const FRAME_WEEKDAYS: u8 = 0b001_1111;

/// Day bit of a date: Monday is bit 0.
pub fn day_bit(d: NaiveDate) -> u8 {
    1 << d.weekday().num_days_from_monday()
}

impl Frame {
    /// Whether the planner may put this task here. Tags compare without case.
    pub fn matches(&self, t: &Task, default_min: i64) -> bool {
        let f = &self.filter;
        if !f.lists.is_empty() && !f.lists.contains(&t.list) {
            return false;
        }
        if !f.tags.is_empty() {
            let has = |g: &String| t.tags.iter().any(|x| x.to_lowercase() == g.to_lowercase());
            let ok = if f.tags_all { f.tags.iter().all(has) } else { f.tags.iter().any(has) };
            if !ok {
                return false;
            }
        }
        if let Some(max) = f.max_min {
            if t.duration_min.unwrap_or(default_min) > max {
                return false;
            }
        }
        true
    }
    pub fn minutes(&self) -> i64 {
        self.end_min - self.start_min
    }
}

/// Names an event on a server.
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteKey {
    /// UID and, for one occurrence of a repeating event, which one
    Uid(String, Option<String>),
    /// address of the whole event, with all its occurrences
    Href(String),
}

impl RemoteKey {
    pub fn matches(&self, e: &Event) -> bool {
        match self {
            RemoteKey::Uid(uid, rid) => e.uid.as_ref() == Some(uid) && &e.recurrence_id == rid,
            RemoteKey::Href(h) => e.href.as_ref() == Some(h),
        }
    }
}

/// One day's stretch of a frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameOcc {
    pub frame: u64,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct TaskList {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    /// the synced account it belongs to; None for one's own lists
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<u64>,
    /// its address on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctag: Option<String>,
    /// the server does not take changes to it
    #[serde(default)]
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TaskList {
    /// A synced list whose server takes changes.
    pub fn is_synced_writable(&self) -> bool {
        self.account.is_some() && !self.read_only
    }
}

impl Task {
    /// Forgets where the task lives on a server: it becomes a new one.
    pub fn detach(&mut self) {
        self.dirty = false;
        (self.uid, self.href, self.etag, self.raw) = (None, None, None, None);
    }
    /// The parts a server keeps are the same in both.
    pub fn same_content(&self, o: &Task) -> bool {
        self.title == o.title && self.notes == o.notes && self.due == o.due && self.priority == o.priority && self.done == o.done
    }
}

/// What typing in the quick task field means: "#tag", "!high", ">List",
/// "30m" / "1h30m" and ".today" / ".tomorrow" / ".2026-11-30" are picked out.
pub struct Quick {
    pub title: String,
    pub tags: Vec<String>,
    pub priority: u8,
    pub list_name: Option<String>,
    pub duration_min: Option<i64>,
    pub due: Option<NaiveDate>,
}

pub fn parse_quick(text: &str, today: NaiveDate) -> Quick {
    let mut q = Quick { title: String::new(), tags: vec![], priority: 0, list_name: None, duration_min: None, due: None };
    let mut words = vec![];
    for w in text.split_whitespace() {
        if let Some(t) = w.strip_prefix('#').filter(|t| !t.is_empty()) {
            q.tags.push(t.to_string());
        } else if let Some(p) = w.strip_prefix('!') {
            q.priority = match p.to_lowercase().as_str() {
                "" | "high" | "h" | "1" => 1,
                "medium" | "m" | "5" => 5,
                "low" | "l" | "9" => 9,
                _ => 1,
            };
        } else if let Some(l) = w.strip_prefix('>').filter(|t| !t.is_empty()) {
            q.list_name = Some(l.to_string());
        } else if let Some(d) = w.strip_prefix('.').filter(|t| !t.is_empty()) {
            q.due = match d.to_lowercase().as_str() {
                "today" => Some(today),
                "tomorrow" => Some(today + Duration::days(1)),
                other => NaiveDate::parse_from_str(other, "%Y-%m-%d").ok(),
            };
            if q.due.is_none() {
                words.push(w);
            }
        } else if let Some(m) = parse_duration(w) {
            q.duration_min = Some(m);
        } else {
            words.push(w);
        }
    }
    q.title = words.join(" ");
    q
}

/// "45m", "2h", "1h30m" in minutes.
pub fn parse_duration(w: &str) -> Option<i64> {
    let w = w.to_lowercase();
    let (mut total, mut num, mut any) = (0i64, String::new(), false);
    for c in w.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else if c == 'h' && !num.is_empty() {
            total += num.parse::<i64>().ok()? * 60;
            num.clear();
            any = true;
        } else if c == 'm' && !num.is_empty() {
            total += num.parse::<i64>().ok()?;
            num.clear();
            any = true;
        } else {
            return None;
        }
    }
    (any && num.is_empty() && total > 0).then_some(total)
}

pub fn fmt_duration(m: i64) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

pub fn priority_name(p: u8) -> &'static str {
    match p {
        0 => "None",
        1..=3 => "High",
        4..=5 => "Medium",
        _ => "Low",
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Data {
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
    pub calendars: Vec<Calendar>,
    #[serde(default)]
    pub lists: Vec<TaskList>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// deletions waiting to be sent to a server
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outbox: Vec<PendingDelete>,
    /// settings live in config.json; this stays readable for old data files
    #[serde(default, skip_serializing)]
    pub prefs: Prefs,
    pub next_id: u64,
}

/// A folder named "<program>.home" beside the program is used as the home
/// folder when it exists, so everything can travel with the program.
fn portable_home() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut home = exe.into_os_string();
    home.push(".home");
    let home = PathBuf::from(home);
    home.is_dir().then_some(home)
}

/// Where the calendar and task data is kept:
/// $XDG_DATA_HOME/kalendar/data.json, usually ~/.local/share/kalendar/.
pub fn data_path() -> PathBuf {
    let base = portable_home().map(|h| h.join(".local/share")).unwrap_or_else(|| {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."))
    });
    base.join("kalendar").join("data.json")
}

/// Where the settings are kept:
/// $XDG_CONFIG_HOME/kalendar/config.json, usually ~/.config/kalendar/.
pub fn config_path() -> PathBuf {
    let base = portable_home().map(|h| h.join(".config")).unwrap_or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
    });
    base.join("kalendar").join("config.json")
}

fn write_json(p: PathBuf, s: String) {
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, s);
}

impl Data {
    pub fn load_or_sample(today: NaiveDate) -> Data {
        let stored: Option<Data> = std::fs::read_to_string(data_path()).ok().and_then(|s| Data::from_json(&s));
        let had_data = stored.is_some();
        let mut data = stored.unwrap_or_else(|| Data::sample(today));
        // settings come from their own file; an older data.json carries
        // them inside, and they are moved out to config.json right away
        match std::fs::read_to_string(config_path()).ok().and_then(|s| serde_json::from_str(&s).ok()) {
            Some(p) => data.prefs = p,
            None if had_data && !config_path().exists() => data.save(),
            None => {}
        }
        data
    }

    pub fn save(&self) {
        // private links stay in the keyring
        let mut out = self.clone();
        for c in out.calendars.iter_mut().filter(|c| c.url_secret) {
            c.url = Some(SECRET_URL.into());
        }
        if let Ok(s) = serde_json::to_string_pretty(&out) {
            write_json(data_path(), s);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.prefs) {
            write_json(config_path(), s);
        }
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn task(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
    pub fn task_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }
    pub fn event(&self, id: u64) -> Option<&Event> {
        self.events.iter().find(|e| e.id == id)
    }
    pub fn event_mut(&mut self, id: u64) -> Option<&mut Event> {
        self.events.iter_mut().find(|e| e.id == id)
    }
    pub fn task_is_scheduled(&self, id: u64) -> bool {
        self.events.iter().any(|e| e.task_id == Some(id))
    }
    pub fn event_color(&self, e: &Event) -> [u8; 3] {
        e.color.unwrap_or_else(|| self.calendar_color(e.calendar))
    }
    /// Reads a data file, bringing an older one up to date.
    pub fn from_json(text: &str) -> Option<Data> {
        let mut d: Data = serde_json::from_str(text).ok()?;
        d.migrate();
        Some(d)
    }
    /// Older data files had no calendar ids: events and frames held the
    /// calendar's place in the list. Gives every calendar an id, in list
    /// order, and points events and frames at the ids.
    pub fn migrate(&mut self) {
        if self.calendars.iter().all(|c| c.id != 0) {
            return;
        }
        let old = self.calendars.iter().all(|c| c.id == 0);
        for i in 0..self.calendars.len() {
            if self.calendars[i].id == 0 {
                self.calendars[i].id = self.new_id();
            }
            if self.calendars[i].url.is_some() {
                self.calendars[i].read_only = true;
            }
        }
        if !old {
            return;
        }
        let ids: Vec<u64> = self.calendars.iter().map(|c| c.id).collect();
        let first = ids.first().copied().unwrap_or(0);
        let at = |i: u64| ids.get(i as usize).copied().unwrap_or(first);
        for e in &mut self.events {
            e.calendar = at(e.calendar);
        }
        for f in &mut self.frames {
            f.calendar = at(f.calendar);
        }
    }
    pub fn calendar(&self, id: u64) -> Option<&Calendar> {
        self.calendars.iter().find(|c| c.id == id)
    }
    /// Why an event cannot be changed here, if it cannot.
    pub fn locked(&self, e: &Event) -> Option<&'static str> {
        let c = self.calendar(e.calendar)?;
        if c.url.is_some() {
            Some("This event comes from a subscribed calendar and can't be changed here")
        } else if c.account.is_some() && c.read_only {
            Some("This calendar is read-only on its server, so the event can't be changed here")
        } else if e.remote && e.recurrence_id.is_some() {
            Some("Repeating events from an account can't be changed here yet")
        } else {
            None
        }
    }
    /// The calendar an event of one's own can go to when its own cannot
    /// take it.
    pub fn writable_or_own(&self, cal: u64) -> u64 {
        if self.calendar(cal).is_some_and(|c| c.writable()) {
            cal
        } else {
            self.first_own_calendar()
        }
    }
    pub fn calendar_mut(&mut self, id: u64) -> Option<&mut Calendar> {
        self.calendars.iter_mut().find(|c| c.id == id)
    }
    /// The calendar task sessions go to: the one named "Tasks", else the
    /// first calendar of one's own.
    pub fn task_calendar(&self) -> u64 {
        self.calendars
            .iter()
            .find(|c| c.is_local() && c.name == "Tasks")
            .map(|c| c.id)
            .unwrap_or_else(|| self.first_own_calendar())
    }
    /// The first calendar new events can be written to.
    pub fn first_own_calendar(&self) -> u64 {
        self.calendars.iter().find(|c| c.is_local()).or(self.calendars.first()).map(|c| c.id).unwrap_or(0)
    }
    /// Brings a calendar in line with what the server sent: `upserts` are
    /// new or changed events, `deletes` are gone. An event already here is
    /// matched by UID and occurrence, and keeps its id and own colour.
    pub fn apply_delta(&mut self, cal: u64, upserts: Vec<crate::ics::FeedEvent>, deletes: &[RemoteKey]) {
        use std::collections::{HashMap, VecDeque};
        if !deletes.is_empty() {
            self.events.retain(|e| {
                !(e.remote && e.calendar == cal && deletes.iter().any(|k| k.matches(e)))
            });
        }
        // a feed may repeat a key; each copy takes the next match
        let mut have: HashMap<(String, Option<String>), VecDeque<usize>> = HashMap::new();
        for (i, e) in self.events.iter().enumerate() {
            if e.remote && e.calendar == cal {
                if let Some(uid) = &e.uid {
                    have.entry((uid.clone(), e.recurrence_id.clone())).or_default().push_back(i);
                }
            }
        }
        for f in upserts {
            let found = have.get_mut(&(f.uid.clone(), f.recurrence_id.clone())).and_then(|q| q.pop_front());
            let i = match found {
                Some(i) => i,
                None => {
                    let id = self.new_id();
                    self.events.push(Event { id, calendar: cal, remote: true, ..Default::default() });
                    self.events.len() - 1
                }
            };
            let e = &mut self.events[i];
            if e.dirty {
                // changed here and not sent yet: the edit is not written
                // over; sending it settles which copy wins
                continue;
            }
            e.title = f.title;
            e.start = f.start;
            e.end = f.end;
            e.all_day = f.all_day;
            e.notes = f.notes;
            e.uid = Some(f.uid);
            e.recurrence_id = f.recurrence_id;
            e.href = f.href;
            e.etag = f.etag;
        }
    }
    /// Takes in what a pull brought back and remembers where it stopped.
    pub fn apply(&mut self, cal: u64, mut d: crate::sync::Delta) {
        // what is waiting to be deleted on the server does not come back
        d.upserts.retain(|f| !f.href.as_ref().is_some_and(|h| self.outbox.iter().any(|o| o.calendar == cal && &o.href == h)));
        if d.full {
            self.apply_full(cal, d.upserts);
        } else {
            if !d.changed.is_empty() {
                // of a changed address, only what it still holds stays
                let keep: std::collections::HashSet<(Option<&str>, &str, Option<&str>)> =
                    d.upserts.iter().map(|f| (f.href.as_deref(), f.uid.as_str(), f.recurrence_id.as_deref())).collect();
                self.events.retain(|e| {
                    let changed = e.remote && e.calendar == cal && e.href.as_ref().is_some_and(|h| d.changed.contains(h));
                    !changed || keep.contains(&(e.href.as_deref(), e.uid.as_deref().unwrap_or(""), e.recurrence_id.as_deref()))
                });
            }
            self.apply_delta(cal, d.upserts, &d.deletes);
        }
        if let Some(c) = self.calendar_mut(cal) {
            c.sync_state = d.state;
            c.ctag = d.ctag;
        }
    }
    /// Takes in the tasks a pull brought back for a synced list. A task
    /// already here is matched by its UID; one changed here and not yet
    /// sent is left alone.
    pub fn apply_tasks(&mut self, list: u64, d: crate::sync::Delta) {
        use std::collections::HashSet;
        let got: HashSet<&str> = d.tasks.iter().map(|t| t.uid.as_str()).collect();
        self.tasks.retain(|t| {
            if t.list != list || t.dirty {
                return true;
            }
            let Some(href) = &t.href else { return true }; // not sent yet
            let named = t.uid.as_deref().is_some_and(|u| got.contains(u));
            if d.full {
                named
            } else {
                // gone, or its address changed and no longer holds it
                !(d.deletes.contains(&RemoteKey::Href(href.clone())) || (d.changed.contains(href) && !named))
            }
        });
        for f in d.tasks {
            let i = match self.tasks.iter().position(|t| t.list == list && t.uid.as_deref() == Some(f.uid.as_str())) {
                Some(i) => i,
                None => {
                    let id = self.new_id();
                    self.tasks.push(Task { id, list, ..Default::default() });
                    self.tasks.len() - 1
                }
            };
            let t = &mut self.tasks[i];
            if t.dirty {
                continue;
            }
            t.title = f.title;
            t.notes = f.notes;
            t.due = f.due;
            t.priority = f.priority;
            t.done = f.done;
            t.uid = Some(f.uid);
            t.href = f.href;
            t.etag = f.etag;
        }
        if let Some(l) = self.lists.iter_mut().find(|l| l.id == list) {
            l.sync_state = d.state;
            l.ctag = d.ctag;
            l.error = None;
        }
    }
    /// Removes a synced task list with its tasks. The server keeps them.
    pub fn remove_synced_list(&mut self, list: u64) {
        self.outbox.retain(|d| d.calendar != list);
        self.tasks.retain(|t| t.list != list);
        let sessions: std::collections::HashSet<u64> = self.tasks.iter().map(|t| t.id).collect();
        self.events.retain(|e| e.task_id.is_none_or(|t| sessions.contains(&t)));
        self.lists.retain(|l| l.id != list);
    }
    /// Removes an account with its calendars.
    pub fn remove_account(&mut self, id: u64) {
        let lists: Vec<u64> = self.lists.iter().filter(|l| l.account == Some(id)).map(|l| l.id).collect();
        for l in lists {
            self.remove_synced_list(l);
        }
        let cals: Vec<u64> = self.calendars.iter().filter(|c| c.account == Some(id)).map(|c| c.id).collect();
        for c in cals {
            self.remove_calendar(c);
        }
        self.accounts.retain(|a| a.id != id);
    }
    pub fn account(&self, id: u64) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }
    pub fn account_mut(&mut self, id: u64) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }
    /// Makes a calendar hold exactly the events of a freshly fetched feed.
    pub fn apply_full(&mut self, cal: u64, feed: Vec<crate::ics::FeedEvent>) {
        use std::collections::HashMap;
        // how many of each key the feed holds; extra copies here go
        let mut want: HashMap<(String, Option<String>), usize> = HashMap::new();
        for f in &feed {
            *want.entry((f.uid.clone(), f.recurrence_id.clone())).or_default() += 1;
        }
        self.events.retain(|e| {
            if !(e.remote && e.calendar == cal) {
                return true;
            }
            let Some(uid) = &e.uid else { return false };
            match want.get_mut(&(uid.clone(), e.recurrence_id.clone())) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    true
                }
                _ => false,
            }
        });
        self.apply_delta(cal, feed, &[]);
    }
    /// Removes a calendar with its events and frames.
    pub fn remove_calendar(&mut self, cal: u64) {
        self.outbox.retain(|d| d.calendar != cal);
        self.events.retain(|e| e.calendar != cal);
        self.frames.retain(|f| f.calendar != cal);
        self.calendars.retain(|c| c.id != cal);
    }
    pub fn frame(&self, id: u64) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }
    pub fn frame_mut(&mut self, id: u64) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }
    pub fn frame_color(&self, f: &Frame) -> [u8; 3] {
        f.color.unwrap_or_else(|| self.calendar_color(f.calendar))
    }
    /// Whether a frame runs on a day, ignoring calendar visibility.
    pub fn frame_occurs_on(f: &Frame, day: NaiveDate) -> bool {
        if !f.enabled || day < f.from || f.until.is_some_and(|u| day > u) {
            return false;
        }
        if f.days & day_bit(day) == 0 || f.skip.contains(&day) {
            return false;
        }
        let anchor = f.from - Duration::days(f.from.weekday().num_days_from_monday() as i64);
        let weeks = (day - anchor).num_days() / 7;
        weeks % f.every_weeks.max(1) as i64 == 0
    }
    /// Frames on a day, on visible calendars of one's own, by start time.
    pub fn frames_on(&self, day: NaiveDate) -> Vec<FrameOcc> {
        let midnight = day.and_hms_opt(0, 0, 0).unwrap();
        let mut v: Vec<FrameOcc> = self
            .frames
            .iter()
            .filter(|f| {
                self.calendar(f.calendar).is_some_and(|c| c.visible && c.is_local())
                    && f.end_min > f.start_min
                    && Self::frame_occurs_on(f, day)
            })
            .map(|f| FrameOcc {
                frame: f.id,
                start: midnight + Duration::minutes(f.start_min.clamp(0, 24 * 60)),
                end: midnight + Duration::minutes(f.end_min.clamp(0, 24 * 60)),
            })
            .collect();
        v.sort_by_key(|o| (o.start, o.end));
        v
    }
    pub fn calendar_color(&self, id: u64) -> [u8; 3] {
        self.calendar(id).map(|c| c.color).unwrap_or([66, 133, 244])
    }
    pub fn event_visible(&self, e: &Event) -> bool {
        if !self.calendar(e.calendar).map(|c| c.visible).unwrap_or(true) {
            return false;
        }
        if let Some(t) = e.task_id.and_then(|id| self.task(id)) {
            if t.done && !self.prefs.show_completed_tasks {
                return false;
            }
        }
        true
    }

    /// Neutral sample content for a first start.
    pub fn sample(today: NaiveDate) -> Data {
        let lists = vec![
            TaskList { id: 1, name: "Work".into(), color: [0x77, 0x7b, 0xf0], ..Default::default() },
            TaskList { id: 2, name: "Personal".into(), color: [0xf1, 0x76, 0xa8], ..Default::default() },
            TaskList { id: 3, name: "Study".into(), color: [0xa2, 0x41, 0xf1], ..Default::default() },
        ];
        let mut d = Data {
            lists,
            events: vec![],
            tasks: vec![],
            calendars: vec![
                Calendar { id: 1, name: "Personal".into(), color: [0x35, 0x85, 0xe1], visible: true, ..Default::default() },
                Calendar { id: 2, name: "Work".into(), color: [0x77, 0x7b, 0xf0], visible: true, ..Default::default() },
                Calendar { id: 3, name: "Family".into(), color: [0xe8, 0xae, 0x26], visible: true, ..Default::default() },
                Calendar { id: 4, name: "Tasks".into(), color: [0x38, 0xc2, 0xc7], visible: true, ..Default::default() },
            ],
            frames: vec![],
            accounts: vec![],
            outbox: vec![],
            prefs: Prefs::default(),
            next_id: 10,
        };
        let ws = d.prefs.week_start(today);
        let at = |day: i64, h: u32, m: u32| (ws + Duration::days(day)).and_hms_opt(h, m, 0).unwrap();
        // sample calendars have the ids 1 to 4, in list order
        let mut ev = |d: &mut Data, title: &str, day: i64, h: u32, m: u32, mins: i64, cal: u64| {
            let id = d.new_id();
            let start = at(day, h, m);
            d.events.push(Event {
                id,
                title: title.into(),
                start,
                end: start + Duration::minutes(mins),
                all_day: false,
                calendar: cal + 1,
                notes: String::new(),
                task_id: None,
            color: None,
            ..Default::default()
            });
        };
        ev(&mut d, "Team sync", 1, 9, 0, 45, 1);
        ev(&mut d, "Lunch", 1, 12, 0, 60, 2);
        ev(&mut d, "Project review", 2, 10, 0, 90, 1);
        ev(&mut d, "Lunch", 2, 12, 0, 60, 2);
        ev(&mut d, "Gym", 2, 7, 0, 60, 0);
        ev(&mut d, "Design session", 3, 9, 30, 120, 1);
        ev(&mut d, "Call with Sam", 3, 10, 30, 30, 0);
        ev(&mut d, "Lunch", 3, 12, 0, 60, 2);
        ev(&mut d, "Planning", 4, 8, 30, 60, 1);
        ev(&mut d, "Dentist", 4, 14, 0, 45, 0);
        ev(&mut d, "Weekly wrap-up", 5, 11, 0, 60, 1);
        ev(&mut d, "Dinner with friends", 5, 9, 0, 30, 2);
        ev(&mut d, "Farmers market", 6, 9, 0, 90, 2);
        let id = d.new_id();
        d.events.push(Event {
            id,
            title: "Trip".into(),
            start: at(4, 0, 0),
            end: at(6, 0, 0),
            all_day: true,
            calendar: 3,
            notes: String::new(),
            task_id: None,
        color: None,
        ..Default::default()
        });
        let mut task = |d: &mut Data, title: &str, due: Option<i64>, pr: u8, dur: Option<i64>, list: u64, tag: &str| {
            let id = d.new_id();
            d.tasks.push(Task {
                id,
                title: title.into(),
                notes: String::new(),
                due: due.map(|x| today + Duration::days(x)),
                priority: pr,
                duration_min: dur,
                list,
                tags: if tag.is_empty() { vec![] } else { vec![tag.to_string()] },
                ..Default::default()
            });
            id
        };
        task(&mut d, "Send the invoice", Some(-1), 1, Some(30), 1, "admin");
        task(&mut d, "Write the monthly report", Some(0), 1, Some(60), 1, "deep-work");
        task(&mut d, "Reply to open emails", Some(0), 5, Some(30), 1, "email");
        task(&mut d, "Book train tickets", Some(1), 9, Some(15), 2, "");
        task(&mut d, "Prepare slides", Some(3), 5, Some(90), 1, "deep-work");
        task(&mut d, "Read chapter 4", Some(5), 5, Some(75), 3, "");
        task(&mut d, "Tidy up the notes", None, 0, None, 0, "");
        task(&mut d, "Renew library card", None, 9, Some(15), 2, "admin");
        let quick = FrameFilter { max_min: Some(30), ..Default::default() };
        for (name, cal, h, filter) in [("Deep work", 1, 9, FrameFilter::default()), ("Quick wins", 3, 16, quick)] {
            let id = d.new_id();
            d.frames.push(Frame {
                id,
                name: name.into(),
                calendar: cal + 1,
                start_min: h * 60,
                end_min: (h + if filter.is_empty() { 2 } else { 1 }) * 60,
                from: ws,
                filter,
                ..Default::default()
            });
        }
        let tid = task(&mut d, "Review the budget", Some(2), 1, Some(45), 1, "");
        let id = d.new_id();
        let dow = (today - ws).num_days();
        let start = at(dow, 13, 0);
        d.events.push(Event {
            id,
            title: "Review the budget".into(),
            start,
            end: start + Duration::minutes(45),
            all_day: false,
            calendar: 4,
            notes: String::new(),
            task_id: Some(tid),
        color: None,
        ..Default::default()
        });
        d
    }

    /// Timed events that touch a day, sorted "start, -duration, title".
    pub fn timed_on(&self, day: NaiveDate) -> Vec<&Event> {
        let a = day.and_hms_opt(0, 0, 0).unwrap();
        let b = a + Duration::days(1);
        let mut v: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| !e.all_day && e.start < b && e.end > a && self.event_visible(e))
            .collect();
        v.sort_by(|x, y| {
            x.start
                .cmp(&y.start)
                .then(y.minutes().cmp(&x.minutes()))
                .then(x.calendar.cmp(&y.calendar))
                .then(y.title.cmp(&x.title))
        });
        v
    }

    pub fn all_day_on(&self, day: NaiveDate) -> Vec<&Event> {
        let mut v: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| {
                e.all_day && e.start.date() <= day && day < e.end.date().max(e.start.date() + Duration::days(1))
                    && self.event_visible(e)
            })
            .collect();
        v.sort_by(|x, y| x.start.cmp(&y.start).then(x.title.cmp(&y.title)));
        v
    }

    /// Free gaps inside working hours of one day, from `from` on.
    fn free_gaps(&self, day: NaiveDate, from: NaiveDateTime) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            return vec![];
        }
        let midnight = day.and_hms_opt(0, 0, 0).unwrap();
        let ws = (midnight + Duration::minutes(WORK_START_MIN)).max(from);
        let we = midnight + Duration::minutes(WORK_END_MIN);
        self.free_gaps_in(ws, we)
    }

    /// Stretches of [a, b) not covered by any timed event, on any calendar.
    /// Frames are not events and never count as busy.
    fn free_gaps_in(&self, a: NaiveDateTime, b: NaiveDateTime) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        if a >= b {
            return vec![];
        }
        let mut busy: Vec<(NaiveDateTime, NaiveDateTime)> = self
            .events
            .iter()
            .filter(|e| !e.all_day && e.start < b && e.end > a)
            .map(|e| (e.start, e.end))
            .collect();
        busy.sort();
        let mut gaps = vec![];
        let mut cur = a;
        for (s, e) in busy {
            if s > cur {
                gaps.push((cur, s));
            }
            cur = cur.max(e);
        }
        if cur < b {
            gaps.push((cur, b));
        }
        gaps
    }

    /// Frames are in use: switched on and at least one of them enabled.
    fn frames_active(&self) -> bool {
        self.prefs.use_frames && self.frames.iter().any(|f| f.enabled)
    }

    /// Where a task may go over `days` days from `day0`, at or after `from`:
    /// free time inside matching frames, and free working time outside the
    /// exclusive frames it does not match (empty when that is not allowed).
    /// Without a task, plain free working time.
    fn task_windows(&self, t: Option<&Task>, day0: NaiveDate, from: NaiveDateTime, days: i64) -> (Vec<Window>, Vec<Window>) {
        let task_cal = self.task_calendar();
        let frames = self.frames_active() && t.is_some();
        let (mut inside, mut outside) = (vec![], vec![]);
        for d in 0..days {
            let day = day0 + Duration::days(d);
            if t.and_then(|t| t.earliest).is_some_and(|e| day < e) {
                continue;
            }
            let mut cuts = vec![];
            if let Some(t) = t.filter(|_| frames) {
                for o in self.frames_on(day) {
                    let Some(f) = self.frame(o.frame) else { continue };
                    if f.matches(t, self.prefs.default_task_min) {
                        for (s, e) in self.free_gaps_in(o.start.max(from), o.end) {
                            inside.push(Window { start: s, end: e, calendar: f.calendar, spec: f.filter.specificity() });
                        }
                    } else if f.exclusive {
                        cuts.push((o.start, o.end));
                    }
                }
            }
            if !frames || self.prefs.plan_outside_frames {
                for (s, e) in subtract(self.free_gaps(day, from), &cuts) {
                    outside.push(Window { start: s, end: e, calendar: task_cal, spec: 0 });
                }
            }
        }
        // earliest first; where two frames start together the more specific wins
        inside.sort_by_key(|w| (w.start, std::cmp::Reverse(w.spec), w.end));
        (inside, outside)
    }

    /// First place `mins` minutes long at or after `from`, looking up to
    /// PLAN_MAX_SPAN_DAYS days ahead. With a task, its frames come first.
    pub fn next_free_slot(&self, from: NaiveDateTime, mins: i64, task: Option<&Task>) -> Option<NaiveDateTime> {
        let m = from.hour() as i64 * 60 + from.minute() as i64;
        let from = from.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes((m + 14) / 15 * 15);
        let (inside, outside) = self.task_windows(task, from.date(), from, PLAN_MAX_SPAN_DAYS);
        [inside, outside]
            .iter()
            .find_map(|ws| ws.iter().find(|w| (w.end - w.start).num_minutes() >= mins).map(|w| w.start))
    }

    /// Fits open, unscheduled tasks into matching frames, else into free
    /// working time, over the next days. Returns (sessions made, tasks left
    /// behind).

    pub fn plan_tasks(&mut self, now: NaiveDateTime) -> (usize, usize) {
        // sessions of flexible tasks that have not started may be moved
        let movable: Vec<u64> = self.tasks.iter().filter(|t| t.flexible && !t.done).map(|t| t.id).collect();
        self.events.retain(|e| !(e.start > now && e.task_id.map(|t| movable.contains(&t)).unwrap_or(false)));
        let snap = 15;
        let mins = now.hour() as i64 * 60 + now.minute() as i64;
        let from = now.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes((mins + snap - 1) / snap * snap);
        let mut todo: Vec<Task> = self
            .tasks
            .iter()
            .filter(|t| !t.done && !self.task_is_scheduled(t.id))
            .filter(|t| t.due.is_some() || self.prefs.plan_without_due)
            .cloned()
            .collect();
        todo.sort_by_key(|t| {
            (
                t.due.unwrap_or(NaiveDate::MAX),
                if t.priority == 0 { 10 } else { t.priority },
            )
        });
        let (mut made, mut left) = (0, 0);
        for t in todo {
            let need = t.duration_min.unwrap_or(self.prefs.default_task_min);
            let can_split = self.prefs.split_long_tasks && need >= PLAN_CHUNK_FROM;
            let (inside, outside) = self.task_windows(Some(&t), now.date(), from, PLAN_SPAN_DAYS);
            let Some(placed) = fit(&inside, need, can_split).or_else(|| fit(&outside, need, can_split)) else {
                left += 1;
                continue;
            };
            for (s, e, cal) in placed {
                let id = self.new_id();
                self.events.push(Event {
                    id,
                    title: t.title.clone(),
                    start: s,
                    end: e,
                    calendar: cal,
                    task_id: Some(t.id),
                    ..Default::default()
                });
                made += 1;
                if self.prefs.auto_due {
                    if let Some(tt) = self.task_mut(t.id) {
                        if tt.due.is_none() {
                            tt.due = Some(s.date());
                        }
                    }
                }
            }
        }
        (made, left)
    }
}

/// A stretch of time a task may be placed in, and the calendar it goes to.
#[derive(Clone, Copy, Debug)]
struct Window {
    start: NaiveDateTime,
    end: NaiveDateTime,
    calendar: u64,
    /// how specific the frame's filter is; 0 outside frames
    spec: usize,
}

type Span = (NaiveDateTime, NaiveDateTime);

/// `gaps` with the `cuts` taken out.
fn subtract(gaps: Vec<Span>, cuts: &[Span]) -> Vec<Span> {
    let mut out = gaps;
    for &(cs, ce) in cuts {
        out = out
            .into_iter()
            .flat_map(|(s, e)| {
                if ce <= s || cs >= e {
                    return vec![(s, e)];
                }
                let mut v = vec![];
                if cs > s {
                    v.push((s, cs));
                }
                if ce < e {
                    v.push((ce, e));
                }
                v
            })
            .collect();
    }
    out
}

/// Places `need` minutes in the windows: the first one with room for all of
/// it, else (when splitting is allowed) chunks of at least PLAN_MIN_CHUNK
/// across the windows in order, never leaving a smaller remainder.
fn fit(windows: &[Window], need: i64, can_split: bool) -> Option<Vec<(NaiveDateTime, NaiveDateTime, u64)>> {
    if let Some(w) = windows.iter().find(|w| (w.end - w.start).num_minutes() >= need) {
        return Some(vec![(w.start, w.start + Duration::minutes(need), w.calendar)]);
    }
    if !can_split {
        return None;
    }
    let mut left = need;
    let mut placed: Vec<(NaiveDateTime, NaiveDateTime, u64)> = vec![];
    for w in windows {
        // windows of overlapping frames may share time already taken
        let taken: Vec<Span> = placed.iter().map(|p| (p.0, p.1)).collect();
        for (s, e) in subtract(vec![(w.start, w.end)], &taken) {
            let take = (e - s).num_minutes().min(left);
            if take < PLAN_MIN_CHUNK || (left - take != 0 && left - take < PLAN_MIN_CHUNK) {
                continue;
            }
            placed.push((s, s + Duration::minutes(take), w.calendar));
            left -= take;
            if left == 0 {
                return Some(placed);
            }
        }
    }
    None
}

/// Minutes from midnight the grid opens at: 90 minutes before now, rounded
/// down to a quarter of an hour.
pub fn opening_scroll_min(now: NaiveDateTime) -> i64 {
    let e = (now.hour() as i64 * 60 + now.minute() as i64 - SCROLL_BACK_MIN).max(0);
    e - e % SCROLL_ROUND_MIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_data_file_moves_settings_to_config() {
        let dir = std::env::temp_dir().join(format!("kalendar-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();

        // an older data.json, with the settings inside it
        let mut old = Data::sample(today);
        old.prefs.sidebar_side = PanelSide::Right;
        old.prefs.keybinds.insert("settings".into(), "Ctrl+P".into());
        let mut v = serde_json::to_value(&old).unwrap();
        v["prefs"] = serde_json::to_value(&old.prefs).unwrap();
        write_json(data_path(), v.to_string());

        let d = Data::load_or_sample(today);
        assert_eq!(d.prefs.sidebar_side, PanelSide::Right);
        assert!(config_path().exists());
        let data_text = std::fs::read_to_string(data_path()).unwrap();
        assert!(!data_text.contains("\"prefs\""), "settings left in data.json");

        // and they come back from config.json alone
        let again = Data::load_or_sample(today);
        assert_eq!(again.prefs.keybinds.get("settings").map(|s| s.as_str()), Some("Ctrl+P"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn ymd(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    /// Two own calendars, no events; 2026-11-16 is a Monday.
    fn empty() -> Data {
        let mut d = Data::sample(ymd(11, 16));
        d.events.clear();
        d.tasks.clear();
        d.frames.clear();
        d
    }

    /// `cal` is the calendar's place in the sample list; its id is one more.
    fn frame(id: u64, cal: u64) -> Frame {
        Frame { id, name: format!("F{id}"), calendar: cal + 1, from: ymd(11, 16), ..Default::default() }
    }

    #[test]
    fn old_data_loads_and_saves_frames() {
        let mut v = serde_json::to_value(Data::sample(ymd(11, 16))).unwrap();
        v.as_object_mut().unwrap().remove("frames");
        let d: Data = serde_json::from_value(v).expect("old data.json without frames");
        assert!(d.frames.is_empty());
        let out = serde_json::to_value(&d).unwrap();
        assert_eq!(out["frames"], serde_json::json!([]));
        // a frame written with only a few fields still loads
        let f: Frame = serde_json::from_str(r#"{"id":3,"name":"Deep work"}"#).unwrap();
        assert_eq!((f.every_weeks, f.days, f.enabled, f.exclusive), (1, FRAME_WEEKDAYS, true, true));
    }

    #[test]
    fn frame_weekday_bits() {
        let mut f = frame(1, 0);
        f.days = 0b000_0101; // Monday and Wednesday
        assert!(Data::frame_occurs_on(&f, ymd(11, 16)));
        assert!(!Data::frame_occurs_on(&f, ymd(11, 17)));
        assert!(Data::frame_occurs_on(&f, ymd(11, 18)));
        f.days = 1 << 6; // Sunday
        assert!(Data::frame_occurs_on(&f, ymd(11, 22)));
        assert!(!Data::frame_occurs_on(&f, ymd(11, 21)));
    }

    #[test]
    fn frame_every_two_weeks_from_until_skip() {
        let mut f = frame(1, 0);
        f.from = ymd(11, 18); // a Wednesday; its week starts on 11-16
        f.every_weeks = 2;
        assert!(!Data::frame_occurs_on(&f, ymd(11, 16)), "before from");
        assert!(Data::frame_occurs_on(&f, ymd(11, 18)));
        assert!(!Data::frame_occurs_on(&f, ymd(11, 23)), "odd week");
        assert!(Data::frame_occurs_on(&f, ymd(11, 30)));
        f.until = Some(ymd(11, 30));
        assert!(Data::frame_occurs_on(&f, ymd(11, 30)), "until is inclusive");
        assert!(!Data::frame_occurs_on(&f, ymd(12, 14)));
        f.skip.push(ymd(11, 30));
        assert!(!Data::frame_occurs_on(&f, ymd(11, 30)));
        f.enabled = false;
        assert!(!Data::frame_occurs_on(&f, ymd(11, 18)));
    }

    #[test]
    fn frames_on_hides_hidden_calendars_and_sorts() {
        let mut d = empty();
        let mut a = frame(1, 0);
        a.start_min = 14 * 60;
        a.end_min = 15 * 60;
        let b = frame(2, 1);
        d.frames = vec![a, b];
        let on: Vec<u64> = d.frames_on(ymd(11, 16)).iter().map(|o| o.frame).collect();
        assert_eq!(on, vec![2, 1]);
        let o = d.frames_on(ymd(11, 16))[0];
        assert_eq!((o.start, o.end), (ymd(11, 16).and_hms_opt(9, 0, 0).unwrap(), ymd(11, 16).and_hms_opt(11, 0, 0).unwrap()));
        d.calendars[1].visible = false;
        assert_eq!(d.frames_on(ymd(11, 16)).len(), 1);
        assert!(d.frames_on(ymd(11, 21)).is_empty(), "Saturday");
    }

    #[test]
    fn frame_filter_matching() {
        let mut f = frame(1, 0);
        let t = Task { list: 1, tags: vec!["Deep-Work".into(), "x".into()], duration_min: Some(45), ..Default::default() };
        assert!(f.matches(&t, 60), "empty filter takes all");
        f.filter.lists = vec![0];
        assert!(!f.matches(&t, 60));
        f.filter.lists = vec![0, 1];
        assert!(f.matches(&t, 60));
        f.filter.tags = vec!["deep-work".into(), "y".into()];
        assert!(f.matches(&t, 60), "any tag, case does not matter");
        f.filter.tags_all = true;
        assert!(!f.matches(&t, 60));
        f.filter.tags = vec!["DEEP-WORK".into(), "x".into()];
        assert!(f.matches(&t, 60));
        f.filter.max_min = Some(30);
        assert!(!f.matches(&t, 60));
        f.filter.max_min = Some(45);
        assert!(f.matches(&t, 60));
        let no_len = Task { duration_min: None, ..t.clone() };
        assert!(!f.matches(&no_len, 60), "default length counts");
        assert!(f.matches(&no_len, 30));
        assert_eq!(f.filter.specificity(), 3);
    }

    fn sessions(d: &Data) -> Vec<String> {
        let mut v: Vec<String> = d
            .events
            .iter()
            .filter(|e| e.task_id.is_some())
            .map(|e| format!("{} {} {} c{}", e.start.format("%a %H:%M"), e.end.format("%H:%M"), e.title, d.calendars.iter().position(|c| c.id == e.calendar).unwrap()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn plan_without_frames_is_unchanged() {
        let now = ymd(11, 18).and_hms_opt(8, 0, 0).unwrap();
        let mut d = Data::sample(now.date());
        d.frames.clear();
        for (title, mins) in [("Big job", 240), ("Huge job", 400), ("Too big", 2000)] {
            let id = d.new_id();
            d.tasks.push(Task { id, title: title.into(), duration_min: Some(mins), due: Some(ymd(11, 30)), ..Default::default() });
        }
        // recorded from the planner before frames existed
        assert_eq!(d.plan_tasks(now), (14, 1));
        let want = [
            "Fri 08:00 08:15 Renew library card c3",
            "Fri 09:30 10:40 Huge job c3",
            "Fri 12:00 13:00 Tidy up the notes c3",
            "Thu 08:00 08:30 Huge job c3",
            "Thu 09:30 13:30 Big job c3",
            "Thu 13:30 14:00 Huge job c3",
            "Thu 14:45 18:00 Huge job c3",
            "Wed 08:00 08:30 Send the invoice c3",
            "Wed 08:30 09:30 Write the monthly report c3",
            "Wed 11:30 12:00 Reply to open emails c3",
            "Wed 13:00 13:45 Review the budget c3",
            "Wed 13:45 14:00 Book train tickets c3",
            "Wed 14:00 15:30 Prepare slides c3",
            "Wed 15:30 16:45 Read chapter 4 c3",
            "Wed 16:45 18:00 Huge job c3",
        ];
        assert_eq!(sessions(&d), want);
    }

    fn add_task(d: &mut Data, title: &str, mins: i64, tags: &[&str]) -> u64 {
        let id = d.new_id();
        d.tasks.push(Task {
            id,
            title: title.into(),
            duration_min: Some(mins),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        id
    }

    fn deep_frame(id: u64, cal: u64, start_h: i64, end_h: i64) -> Frame {
        let mut f = frame(id, cal);
        f.start_min = start_h * 60;
        f.end_min = end_h * 60;
        f.filter.tags = vec!["deep".into()];
        f
    }

    fn monday_8() -> NaiveDateTime {
        ymd(11, 16).and_hms_opt(8, 0, 0).unwrap()
    }

    #[test]
    fn matching_task_goes_into_its_frame() {
        let mut d = empty();
        d.frames.push(deep_frame(1, 1, 14, 16));
        add_task(&mut d, "Write", 60, &["Deep"]);
        assert_eq!(d.plan_tasks(monday_8()), (1, 0));
        assert_eq!(sessions(&d), ["Mon 14:00 15:00 Write c1"]);
    }

    #[test]
    fn other_tasks_keep_out_of_exclusive_frames() {
        let mut d = empty();
        d.frames.push(deep_frame(1, 1, 8, 12));
        add_task(&mut d, "Email", 60, &[]);
        d.plan_tasks(monday_8());
        assert_eq!(sessions(&d), ["Mon 12:00 13:00 Email c3"]);

        let mut d = empty();
        let mut f = deep_frame(1, 1, 8, 12);
        f.exclusive = false;
        d.frames.push(f);
        add_task(&mut d, "Email", 60, &[]);
        d.plan_tasks(monday_8());
        assert_eq!(sessions(&d), ["Mon 08:00 09:00 Email c3"]);
    }

    #[test]
    fn no_frame_fits_and_leave_unplanned() {
        let mut d = empty();
        d.prefs.plan_outside_frames = false;
        d.frames.push(deep_frame(1, 1, 8, 12));
        add_task(&mut d, "Email", 60, &[]);
        add_task(&mut d, "Write", 60, &["deep"]);
        assert_eq!(d.plan_tasks(monday_8()), (1, 1));
        assert_eq!(sessions(&d), ["Mon 08:00 09:00 Write c1"]);

        // switching frames off plans as before ("Write" now has a due date)
        d.events.clear();
        d.prefs.use_frames = false;
        assert_eq!(d.plan_tasks(monday_8()), (2, 0));
        assert_eq!(sessions(&d), ["Mon 08:00 09:00 Write c3", "Mon 09:00 10:00 Email c3"]);
    }

    #[test]
    fn long_task_splits_across_frames() {
        let mut d = empty();
        let mut a = deep_frame(1, 1, 9, 11);
        a.filter = FrameFilter::default();
        let mut b = deep_frame(2, 1, 14, 16);
        b.filter = FrameFilter::default();
        d.frames = vec![a, b];
        add_task(&mut d, "Thesis", 180, &[]);
        assert_eq!(d.plan_tasks(monday_8()), (2, 0));
        assert_eq!(sessions(&d), ["Mon 09:00 11:00 Thesis c1", "Mon 14:00 15:00 Thesis c1"]);
    }

    #[test]
    fn earliest_day_holds_in_frames() {
        let mut d = empty();
        d.frames.push(deep_frame(1, 1, 9, 11));
        let id = add_task(&mut d, "Write", 60, &["deep"]);
        d.task_mut(id).unwrap().earliest = Some(ymd(11, 18));
        d.plan_tasks(monday_8());
        assert_eq!(sessions(&d), ["Wed 09:00 10:00 Write c1"]);
    }

    #[test]
    fn events_take_room_from_frames() {
        let mut d = empty();
        d.frames.push(deep_frame(1, 1, 9, 11));
        d.events.push(Event {
            id: 99,
            title: "Call".into(),
            start: ymd(11, 16).and_hms_opt(9, 0, 0).unwrap(),
            end: ymd(11, 16).and_hms_opt(10, 30, 0).unwrap(),
            ..Default::default()
        });
        add_task(&mut d, "Write", 60, &["deep"]);
        add_task(&mut d, "Sketch", 30, &["deep"]);
        assert_eq!(d.plan_tasks(monday_8()), (2, 0));
        assert_eq!(sessions(&d), ["Mon 10:30 11:00 Sketch c1", "Tue 09:00 10:00 Write c1"]);
    }

    #[test]
    fn more_specific_frame_wins_a_tie() {
        let mut d = empty();
        let mut any = deep_frame(1, 0, 9, 11);
        any.filter = FrameFilter::default();
        let deep = deep_frame(2, 1, 9, 11);
        d.frames = vec![any, deep];
        add_task(&mut d, "Write", 60, &["deep"]);
        d.plan_tasks(monday_8());
        assert_eq!(sessions(&d), ["Mon 09:00 10:00 Write c1"]);
    }

    #[test]
    fn next_free_slot_prefers_frames() {
        let mut d = empty();
        d.frames.push(deep_frame(1, 1, 14, 16));
        let id = add_task(&mut d, "Write", 60, &["deep"]);
        let t = d.task(id).cloned();
        let at = |h| Some(ymd(11, 16).and_hms_opt(h, 0, 0).unwrap());
        assert_eq!(d.next_free_slot(monday_8(), 60, t.as_ref()), at(14));
        assert_eq!(d.next_free_slot(monday_8(), 60, None), at(8));
    }

    #[test]
    fn removing_a_calendar_keeps_the_others() {
        let mut d = Data::sample(ymd(11, 16));
        d.frames = vec![frame(1, 0), frame(2, 1), frame(3, 3)];
        let before: Vec<(u64, String)> =
            d.events.iter().filter(|e| e.calendar != 2).map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
        d.remove_calendar(2);
        let v: Vec<(u64, u64)> = d.frames.iter().map(|f| (f.id, f.calendar)).collect();
        assert_eq!(v, vec![(1, 1), (3, 4)]);
        let after: Vec<(u64, String)> = d.events.iter().map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
        assert_eq!(before, after, "an event moved to another calendar");
        // putting the calendars in another order moves nothing either
        d.calendars.reverse();
        let again: Vec<(u64, String)> = d.events.iter().map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
        assert_eq!(before, again);
    }

    /// A data file as written before calendars had ids.
    fn old_format(d: &Data) -> String {
        let mut v = serde_json::to_value(d).unwrap();
        let ids: Vec<u64> = d.calendars.iter().map(|c| c.id).collect();
        let place = |id: &serde_json::Value| ids.iter().position(|x| Some(*x) == id.as_u64()).unwrap();
        for c in v["calendars"].as_array_mut().unwrap() {
            let c = c.as_object_mut().unwrap();
            c.remove("id");
            c.remove("read_only");
        }
        for key in ["events", "frames"] {
            for e in v[key].as_array_mut().unwrap() {
                e["calendar"] = place(&e["calendar"]).into();
            }
        }
        v.to_string()
    }

    #[test]
    fn old_data_file_gets_calendar_ids() {
        let mut sample = Data::sample(ymd(11, 16));
        sample.calendars.push(Calendar { id: 5, name: "Feed".into(), url: Some("https://example.org/a.ics".into()), visible: true, ..Default::default() });
        let names = |d: &Data| -> Vec<(u64, String, String)> {
            let mut v: Vec<_> = d.events.iter().map(|e| (e.id, e.title.clone(), d.calendar(e.calendar).expect("event without calendar").name.clone())).collect();
            v.extend(d.frames.iter().map(|f| (f.id, f.name.clone(), d.calendar(f.calendar).expect("frame without calendar").name.clone())));
            v
        };
        let want = names(&sample);
        let loaded = Data::from_json(&old_format(&sample)).expect("old data.json");
        assert!(loaded.calendars.iter().all(|c| c.id != 0));
        let mut ids: Vec<u64> = loaded.calendars.iter().map(|c| c.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 5, "calendar ids must differ");
        assert!(loaded.calendars[4].read_only && !loaded.calendars[0].read_only);
        assert_eq!(names(&loaded), want);
        // saved and read again, nothing moves and the ids stay
        let again = Data::from_json(&serde_json::to_string(&loaded).unwrap()).unwrap();
        assert_eq!(names(&again), want);
        assert_eq!(again.calendars.iter().map(|c| c.id).collect::<Vec<_>>(), loaded.calendars.iter().map(|c| c.id).collect::<Vec<_>>());
        assert_eq!(again.next_id, loaded.next_id);
    }

    fn feed_event(uid: &str, rid: Option<&str>, title: &str, h: u32) -> crate::ics::FeedEvent {
        let start = ymd(11, 16).and_hms_opt(h, 0, 0).unwrap();
        crate::ics::FeedEvent {
            title: title.into(),
            start,
            end: start + Duration::hours(1),
            all_day: false,
            notes: String::new(),
            uid: uid.into(),
            recurrence_id: rid.map(|s| s.to_string()),
            href: None,
            etag: None,
        }
    }

    #[test]
    fn refreshing_a_feed_keeps_event_ids() {
        let mut d = empty();
        let own = d.events.len();
        let feed = || vec![feed_event("a", None, "A", 9), feed_event("r", Some("1"), "R", 10), feed_event("r", Some("2"), "R", 11)];
        d.apply_full(2, feed());
        let ids: Vec<u64> = d.events.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), own + 3);
        d.event_mut(ids[0]).unwrap().color = Some([1, 2, 3]);
        d.apply_full(2, feed());
        assert_eq!(d.events.iter().map(|e| e.id).collect::<Vec<_>>(), ids);
        assert_eq!(d.event(ids[0]).unwrap().color, Some([1, 2, 3]), "own colour lost");

        // one changed, one gone, one new
        d.apply_full(2, vec![feed_event("a", None, "A moved", 14), feed_event("r", Some("2"), "R", 11), feed_event("n", None, "N", 12)]);
        let a = d.event(ids[0]).expect("changed event kept its id");
        assert_eq!((a.title.as_str(), a.start.hour()), ("A moved", 14));
        assert!(d.event(ids[1]).is_none(), "event gone from the feed stays");
        assert!(d.event(ids[2]).is_some());
        assert_eq!(d.events.len(), 3);

        // a change on its own, and deletes by key
        d.apply_delta(2, vec![feed_event("n", None, "N2", 13)], &[RemoteKey::Uid("r".into(), Some("2".into()))]);
        assert_eq!(d.events.len(), 2);
        assert!(d.events.iter().any(|e| e.title == "N2"));
        // a changed address keeps only what it still holds
        let at = |uid: &str, rid: &str, href: &str| {
            let mut f = feed_event(uid, Some(rid), "S", 9);
            f.href = Some(href.into());
            f
        };
        let mut s = empty();
        s.apply(3, crate::sync::Delta { upserts: vec![at("s", "1", "/a.ics"), at("s", "2", "/a.ics"), at("t", "1", "/b.ics")], full: true, state: Some("x".into()), ..Default::default() });
        let kept = s.events.iter().find(|e| e.recurrence_id.as_deref() == Some("2")).unwrap().id;
        s.apply(3, crate::sync::Delta { upserts: vec![at("s", "2", "/a.ics")], changed: vec!["/a.ics".into()], state: Some("y".into()), ..Default::default() });
        assert_eq!(s.events.len(), 2, "the occurrence taken out of the series goes");
        assert!(s.event(kept).is_some());
        assert_eq!(s.calendar(3).unwrap().sync_state.as_deref(), Some("y"));
        s.apply(3, crate::sync::Delta { deletes: vec![RemoteKey::Href("/b.ics".into())], ..Default::default() });
        assert_eq!(s.events.len(), 1);
        // another calendar's events are never touched
        d.apply_full(1, vec![]);
        assert_eq!(d.events.len(), 2);
    }
}
