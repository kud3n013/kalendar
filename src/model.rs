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
pub const ZOOM_STEP: f32 = 1.1; // one wheel notch while zooming
pub const SYNC_EVERY_S: f64 = 900.0; // subscribed calendars refresh every 15 minutes
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
    pub show_due_today: bool,
    pub show_due_tomorrow: bool,
    pub show_due_soon: bool,
    pub due_on_calendar: bool,
    /// pixels per hour chosen by zooming; None follows the time resolution
    pub hour_height_px: Option<f32>,
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
            show_due_today: true,
            show_due_tomorrow: true,
            show_due_soon: true,
            due_on_calendar: true,
            hour_height_px: None,
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
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Event {
    pub id: u64,
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub calendar: usize,
    pub notes: String,
    pub task_id: Option<u64>,
    /// own colour, chosen from the right-click menu
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// came from a subscribed calendar
    #[serde(default)]
    pub remote: bool,
}

impl Event {
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
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

#[derive(Clone, Serialize, Deserialize)]
pub struct TaskList {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
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
        let stored: Option<Data> = std::fs::read_to_string(data_path()).ok().and_then(|s| serde_json::from_str(&s).ok());
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
        if let Ok(s) = serde_json::to_string_pretty(self) {
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
    /// The calendar task sessions go to: the one named "Tasks", else the
    /// first calendar of one's own.
    pub fn task_calendar(&self) -> usize {
        self.calendars
            .iter()
            .position(|c| c.url.is_none() && c.name == "Tasks")
            .or_else(|| self.calendars.iter().position(|c| c.url.is_none()))
            .unwrap_or(0)
    }
    /// The first calendar new events can be written to.
    pub fn first_own_calendar(&self) -> usize {
        self.calendars.iter().position(|c| c.url.is_none()).unwrap_or(0)
    }
    /// Replaces a subscribed calendar's events with freshly fetched ones.
    pub fn replace_remote(&mut self, cal: usize, feed: Vec<crate::ics::FeedEvent>) {
        self.events.retain(|e| !(e.remote && e.calendar == cal));
        for f in feed {
            let id = self.new_id();
            self.events.push(Event {
                id,
                title: f.title,
                start: f.start,
                end: f.end,
                all_day: f.all_day,
                calendar: cal,
                notes: f.notes,
                remote: true,
                ..Default::default()
            });
        }
    }
    /// Removes a calendar with its events and keeps the others pointing right.
    pub fn remove_calendar(&mut self, cal: usize) {
        if cal >= self.calendars.len() {
            return;
        }
        self.events.retain(|e| e.calendar != cal);
        for e in self.events.iter_mut().filter(|e| e.calendar > cal) {
            e.calendar -= 1;
        }
        self.calendars.remove(cal);
    }
    pub fn calendar_color(&self, i: usize) -> [u8; 3] {
        self.calendars.get(i).map(|c| c.color).unwrap_or([66, 133, 244])
    }
    pub fn event_visible(&self, e: &Event) -> bool {
        if !self.calendars.get(e.calendar).map(|c| c.visible).unwrap_or(true) {
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
            TaskList { id: 1, name: "Work".into(), color: [0x77, 0x7b, 0xf0] },
            TaskList { id: 2, name: "Personal".into(), color: [0xf1, 0x76, 0xa8] },
            TaskList { id: 3, name: "Study".into(), color: [0xa2, 0x41, 0xf1] },
        ];
        let mut d = Data {
            lists,
            events: vec![],
            tasks: vec![],
            calendars: vec![
                Calendar { name: "Personal".into(), color: [0x35, 0x85, 0xe1], visible: true, ..Default::default() },
                Calendar { name: "Work".into(), color: [0x77, 0x7b, 0xf0], visible: true, ..Default::default() },
                Calendar { name: "Family".into(), color: [0xe8, 0xae, 0x26], visible: true, ..Default::default() },
                Calendar { name: "Tasks".into(), color: [0x38, 0xc2, 0xc7], visible: true, ..Default::default() },
            ],
            prefs: Prefs::default(),
            next_id: 10,
        };
        let ws = d.prefs.week_start(today);
        let at = |day: i64, h: u32, m: u32| (ws + Duration::days(day)).and_hms_opt(h, m, 0).unwrap();
        let mut ev = |d: &mut Data, title: &str, day: i64, h: u32, m: u32, mins: i64, cal: usize| {
            let id = d.new_id();
            let start = at(day, h, m);
            d.events.push(Event {
                id,
                title: title.into(),
                start,
                end: start + Duration::minutes(mins),
                all_day: false,
                calendar: cal,
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
            calendar: 2,
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
            calendar: 3,
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
        let mut busy: Vec<(NaiveDateTime, NaiveDateTime)> = self
            .events
            .iter()
            .filter(|e| !e.all_day && e.start < we && e.end > ws)
            .map(|e| (e.start, e.end))
            .collect();
        busy.sort();
        let mut gaps = vec![];
        let mut cur = ws;
        for (s, e) in busy {
            if s > cur {
                gaps.push((cur, s));
            }
            cur = cur.max(e);
        }
        if cur < we {
            gaps.push((cur, we));
        }
        gaps
    }

    /// Fits open, unscheduled tasks into free working time over the next
    /// days. Returns (sessions made, tasks left behind).
    /// First free working-time gap of `mins` minutes at or after `from`,
    /// looking up to PLAN_MAX_SPAN_DAYS days ahead.
    pub fn next_free_slot(&self, from: NaiveDateTime, mins: i64) -> Option<NaiveDateTime> {
        let m = from.hour() as i64 * 60 + from.minute() as i64;
        let from = from.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes((m + 14) / 15 * 15);
        for d in 0..PLAN_MAX_SPAN_DAYS {
            for (s, e) in self.free_gaps(from.date() + Duration::days(d), from) {
                if (e - s).num_minutes() >= mins {
                    return Some(s);
                }
            }
        }
        None
    }

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
        let task_cal = self.task_calendar();
        let (mut made, mut left) = (0, 0);
        for t in todo {
            let mut need = t.duration_min.unwrap_or(self.prefs.default_task_min);
            let can_split = self.prefs.split_long_tasks && need >= PLAN_CHUNK_FROM;
            let mut placed: Vec<(NaiveDateTime, NaiveDateTime)> = vec![];
            'days: for d in 0..PLAN_SPAN_DAYS {
                let day = now.date() + Duration::days(d);
                if t.earliest.map(|e| day < e).unwrap_or(false) {
                    continue;
                }
                for (s, e) in self.free_gaps(day, from) {
                    let room = (e - s).num_minutes();
                    if room >= need {
                        placed.push((s, s + Duration::minutes(need)));
                        need = 0;
                        break 'days;
                    }
                }
            }
            if need > 0 && can_split {
                let saved = self.events.len();
                'days2: for d in 0..PLAN_SPAN_DAYS {
                    let day = now.date() + Duration::days(d);
                    if t.earliest.map(|e| day < e).unwrap_or(false) {
                        continue;
                    }
                if t.earliest.map(|e| day < e).unwrap_or(false) {
                    continue;
                }
                    for (s, e) in self.free_gaps(day, from) {
                        let room = (e - s).num_minutes();
                        let take = room.min(need);
                        if take < PLAN_MIN_CHUNK || (need - take != 0 && need - take < PLAN_MIN_CHUNK) {
                            continue;
                        }
                        placed.push((s, s + Duration::minutes(take)));
                        // hold the place so later gaps are computed right
                        self.events.push(Event {
                            id: 0,
                            title: String::new(),
                            start: s,
                            end: s + Duration::minutes(take),
                            all_day: false,
                            calendar: 0,
                            notes: String::new(),
                            task_id: None,
                        color: None,
                        ..Default::default()
                        });
                        need -= take;
                        if need == 0 {
                            break 'days2;
                        }
                    }
                }
                self.events.truncate(saved);
                if need > 0 {
                    placed.clear();
                }
            }
            if need > 0 || placed.is_empty() {
                left += 1;
                continue;
            }
            for (s, e) in placed {
                let id = self.new_id();
                self.events.push(Event {
                    id,
                    title: t.title.clone(),
                    start: s,
                    end: e,
                    all_day: false,
                    calendar: task_cal,
                    notes: String::new(),
                    task_id: Some(t.id),
                color: None,
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
}
