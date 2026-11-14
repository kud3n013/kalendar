//! Kalendar: a calendar and task planner.

mod ics;
mod model;
mod panel;
mod settings;
mod dialogs;
mod views;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{self, Color32, Key};
use model::*;
use std::path::PathBuf;

pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub side: Color32,
    pub rail: Color32,
    pub border: Color32,
    pub border_minor: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub today: Color32,
    pub weekend: Color32,
    pub now: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Palette {
                dark,
                bg: Color32::from_rgb(0x22, 0x22, 0x22),
                side: Color32::from_rgb(0x1c, 0x1c, 0x1c),
                rail: Color32::from_rgb(0x1c, 0x1c, 0x1c),

                border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
                border_minor: Color32::from_rgb(0x30, 0x30, 0x30),
                text: Color32::from_rgb(0xf4, 0xf4, 0xf5),
                muted: Color32::from_rgb(0x96, 0x95, 0xa0),
                accent: Color32::from_rgb(0x62, 0x5e, 0xfc),
                today: Color32::from_rgb(0x2a, 0x2d, 0x35),
                weekend: Color32::from_rgb(0x29, 0x29, 0x29),
                now: Color32::from_rgb(0xea, 0x43, 0x35),
            }
        } else {
            Palette {
                dark,
                bg: Color32::WHITE,
                side: Color32::WHITE,
                rail: Color32::from_rgb(0xf7, 0xf7, 0xf7),

                border: Color32::from_rgb(0xe1, 0xe0, 0xe0),
                border_minor: Color32::from_rgb(0xec, 0xec, 0xec),
                text: Color32::from_rgb(0x1f, 0x23, 0x28),
                muted: Color32::from_rgb(0x7f, 0x82, 0x85),
                accent: Color32::from_rgb(0x62, 0x5e, 0xfc),
                today: Color32::from_rgb(0xf3, 0xf7, 0xfe),
                weekend: Color32::from_rgb(0xf9, 0xf9, 0xf9),
                now: Color32::from_rgb(0xea, 0x43, 0x35),
            }
        }
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}
pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub enum Drag {
    Create { anchor: NaiveDateTime, draft: Event },
    Move { id: u64, grab_min: i64 },
    Resize { id: u64 },
}

pub struct EventEdit {
    pub ev: Event,
    pub is_new: bool,
    pub focus: bool,
}

pub struct TaskEdit {
    pub task: Task,
    pub due_text: String,
    pub tags_text: String,
    pub earliest_text: String,
    pub focus: bool,
}

pub struct Toast {
    pub text: String,
    pub until: f64,
    pub undo: bool,
    pub move_due: Option<(u64, NaiveDate)>,
    /// when it appeared (set on first draw), for the fade
    pub born: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Cmd {
    Today,
    Next,
    Prev,
    DayFwd,
    DayBack,
    ViewDay,
    ViewWeek,
    ViewMonth,
    ViewAgenda,
    ViewTwoWeeks,
    NewEvent,
    NewTask,
    Edit,
    Delete,
    Copy,
    Paste,
    Undo,
    Plan,
    Reschedule,
    FollowUp,
    Split,
    ToggleTasks,
    ToggleTheme,
    FinerGrid,
    CoarserGrid,
    Search,
    CommandBar,
    Prefs,
    Shortcuts,
    Refresh,
    Quit,
}

/// (command, label, keys, section), as shown in the shortcut list.
pub const COMMANDS: &[(Cmd, &str, &str, &str)] = &[
    (Cmd::Today, "Move to the current day", "T", "Calendar view"),
    (Cmd::Next, "Move to the next period", "Right", "Calendar view"),
    (Cmd::Prev, "Move to the previous period", "Left", "Calendar view"),
    (Cmd::DayFwd, "Move one day forward", "Shift+Right", "Calendar view"),
    (Cmd::DayBack, "Move one day back", "Shift+Left", "Calendar view"),
    (Cmd::ViewDay, "Day", "D", "Calendar view"),
    (Cmd::ViewWeek, "Week", "W", "Calendar view"),
    (Cmd::ViewMonth, "Month", "M", "Calendar view"),
    (Cmd::ViewAgenda, "Agenda", "A", "Calendar view"),
    (Cmd::ViewTwoWeeks, "2 weeks", "X", "Calendar view"),
    (Cmd::Refresh, "Refresh your calendars", "R", "Calendar view"),
    (Cmd::NewEvent, "Create a new event", "C", "Events and tasks"),
    (Cmd::NewTask, "Create a new task", "N then T", "Events and tasks"),
    (Cmd::Edit, "Edit an event", "E", "Events and tasks"),
    (Cmd::Delete, "Delete selected event or task", "Del / Backspace", "Events and tasks"),
    (Cmd::Copy, "Copy", "Ctrl+C", "Events and tasks"),
    (Cmd::Paste, "Paste here", "Right-click", "Events and tasks"),
    (Cmd::Plan, "Plan my tasks", "", "Events and tasks"),
    (Cmd::Reschedule, "Reschedule session", "Shift+R", "Events and tasks"),
    (Cmd::FollowUp, "Schedule follow-up", "Shift+F", "Events and tasks"),
    (Cmd::Split, "Split and reschedule", "Shift+S", "Events and tasks"),
    (Cmd::Undo, "Undo", "Ctrl+Z", "General"),
    (Cmd::CommandBar, "Toggle command bar", "Ctrl+K", "General"),
    (Cmd::Search, "Open search", "Ctrl+F", "General"),
    (Cmd::ToggleTasks, "Toggle left drawer", "Space", "General"),
    (Cmd::ToggleTheme, "Toggle dark/light themes", "\\", "General"),
    (Cmd::FinerGrid, "Increase calendar time resolution", "]", "General"),
    (Cmd::CoarserGrid, "Reduce calendar time resolution", "[", "General"),
    (Cmd::Prefs, "Open general preferences", "P", "General"),
    (Cmd::Shortcuts, "List available shortcuts", ".", "General"),
    (Cmd::Quit, "Quit", "Ctrl+Q", "General"),
];

pub struct App {
    pub data: Data,
    pub view: View,
    pub anchor: NaiveDate,
    pub selected: Option<u64>,
    pub editor: Option<EventEdit>,
    pub task_editor: Option<TaskEdit>,
    pub drag: Option<Drag>,
    pub drag_task: Option<u64>,
    pub ctx_target: Option<u64>,
    /// start time and direction of the slide when the view changes
    pub nav_anim: (f64, f32),
    pub last_shown: Option<(View, NaiveDate)>,
    /// test aid: show a right-click menu in a window ("event" or "task")
    pub demo_menu: Option<String>,
    pub ctx_time: Option<NaiveDateTime>,
    pub undo: Vec<Data>,
    pub toast: Option<Toast>,
    pub show_prefs: bool,
    pub settings_page: settings::Page,
    pub sub_kind: u8,
    pub new_cal_name: String,
    pub confirm_remove: Option<usize>,
    pub command_bar: Option<String>,
    pub command_focus: bool,
    pub new_task_text: String,
    pub focus_new_task: bool,
    pub quick_nav: bool,
    pub sub_url: String,
    pub sub_name: String,
    pub syncing: std::collections::HashSet<String>,
    pub sync_tx: std::sync::mpsc::Sender<(String, Result<ics::Feed, String>)>,
    pub sync_rx: std::sync::mpsc::Receiver<(String, Result<ics::Feed, String>)>,
    pub synced_on_start: bool,
    pub last_auto_sync: f64,
    pub side_scroll: f32,
    pub sort_by: usize,
    pub filter_tag: Option<String>,
    pub new_list_text: Option<String>,
    pub adding_to: Option<(u64, String)>,
    pub collapsed: std::collections::HashSet<String>,
    pub scroll_to: Option<f32>,
    pub clipboard: Option<Event>,
    pub last_n: f64,
    pub shot: Option<PathBuf>,
    pub frames: u32,
    pub pal: Palette,
}

impl App {
    fn new(shot: Option<PathBuf>) -> Self {
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let today = Local::now().date_naive();
        let data = if shot.is_some() { Data::sample(today) } else { Data::load_or_sample(today) };
        let view = data.prefs.initial_view;
        let scroll = (opening_scroll_min(Self::now()) - data.prefs.grid_start_h * 60) as f32 / 60.0 * data.prefs.hour_height();
        App {
            data,
            view,
            anchor: today,
            selected: None,
            editor: None,
            task_editor: None,
            drag: None,
            drag_task: None,
            ctx_target: None,
            nav_anim: (-10.0, 0.0),
            last_shown: None,
            demo_menu: std::env::var("KALENDAR_SHOW_MENU").ok(),
            ctx_time: None,
            undo: vec![],
            toast: None,
            show_prefs: false,
            settings_page: settings::Page::General,
            sub_kind: 0,
            new_cal_name: String::new(),
            confirm_remove: None,
            command_bar: None,
            command_focus: false,
            new_task_text: String::new(),
            focus_new_task: false,
            quick_nav: false,
            sub_url: String::new(),
            sub_name: String::new(),
            syncing: Default::default(),
            sync_tx,
            sync_rx,
            synced_on_start: false,
            last_auto_sync: 0.0,
            side_scroll: 0.0,
            sort_by: 0,
            filter_tag: None,
            new_list_text: None,
            adding_to: None,
            collapsed: ["done".to_string()].into_iter().collect(),
            scroll_to: Some(scroll.max(0.0)),
            clipboard: None,
            last_n: -10.0,
            shot,
            frames: 0,
            pal: Palette::new(true),
        }
    }

    /// The current time; KALENDAR_NOW=2026-11-11T10:30 overrides it for testing.
    pub fn now() -> NaiveDateTime {
        if let Some(t) = std::env::var("KALENDAR_NOW").ok().and_then(|v| NaiveDateTime::parse_from_str(&v, "%Y-%m-%dT%H:%M").ok()) {
            return t;
        }
        Local::now().naive_local()
    }

    pub fn snapshot(&mut self) {
        self.undo.push(self.data.clone());
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
    }

    pub fn commit(&mut self) {
        if self.shot.is_none() {
            self.data.save();
        }
    }

    pub fn say(&mut self, ctx: &egui::Context, text: impl Into<String>, undo: bool) {
        let grace = self.data.prefs.undo_grace_s as f64;
        let undo = undo && grace > 0.0;
        self.toast = Some(Toast {
            text: text.into(),
            until: ctx.input(|i| i.time) + if undo { grace } else { 4.0 },
            undo,
            move_due: None,
            born: -1.0,
        });
    }

    pub fn do_undo(&mut self, ctx: &egui::Context) {
        if let Some(d) = self.undo.pop() {
            self.data = d;
            self.selected = None;
            self.commit();
            self.say(ctx, "Undone", false);
        }
    }

    /// The days shown by the current view.
    pub fn visible_days(&self) -> Vec<NaiveDate> {
        let p = &self.data.prefs;
        let weekend_ok = |d: &NaiveDate| p.weekends_visible || d.weekday().num_days_from_monday() < 5;
        match self.view {
            View::Days(n) => (0..n as i64).map(|i| self.anchor + Duration::days(i)).collect(),
            View::Week | View::Agenda => {
                let s = p.week_start(self.anchor);
                (0..7).map(|i| s + Duration::days(i)).filter(weekend_ok).collect()
            }
            View::TwoWeeks => {
                let s = p.week_start(self.anchor);
                (0..14).map(|i| s + Duration::days(i)).filter(weekend_ok).collect()
            }
            View::Month => {
                let first = self.anchor.with_day(1).unwrap();
                let s = p.week_start(first);
                (0..42).map(|i| s + Duration::days(i)).filter(weekend_ok).collect()
            }
        }
    }

    fn step(&mut self, dir: i64) {
        self.anchor = match self.view {
            View::Days(n) => self.anchor + Duration::days(dir * n as i64),
            View::Week | View::Agenda => self.anchor + Duration::days(7 * dir),
            View::TwoWeeks => self.anchor + Duration::days(14 * dir),
            View::Month => {
                let first = self.anchor.with_day(1).unwrap();
                if dir > 0 {
                    first.checked_add_months(Months::new(1)).unwrap_or(first)
                } else {
                    first.checked_sub_months(Months::new(1)).unwrap_or(first)
                }
            }
        };
    }

    pub fn open_new_event(&mut self, start: NaiveDateTime, mins: i64, all_day: bool) {
        let id = self.data.new_id();
        self.editor = Some(EventEdit {
            ev: Event {
                id,
                title: String::new(),
                start,
                end: start + Duration::minutes(mins),
                all_day,
                calendar: self.data.first_own_calendar(),
                notes: String::new(),
                task_id: None,
            color: None,
            ..Default::default()
            },
            is_new: true,
            focus: true,
        });
        self.selected = None;
    }

    pub fn edit_event(&mut self, id: u64) {
        if let Some(e) = self.data.event(id) {
            if e.remote {
                // subscribed events open to be read; saving is refused
            }
            self.editor = Some(EventEdit { ev: e.clone(), is_new: false, focus: false });
            self.selected = Some(id);
        }
    }

    pub fn edit_task(&mut self, id: u64) {
        if let Some(t) = self.data.task(id) {
            self.task_editor = Some(TaskEdit {
                task: t.clone(),
                due_text: t.due.map(|d| d.to_string()).unwrap_or_default(),
                tags_text: t.tags.join(" "),
                earliest_text: t.earliest.map(|d| d.to_string()).unwrap_or_default(),
                focus: false,
            });
        }
    }

    /// True (with a message) when the event belongs to a subscribed calendar.
    pub fn refuse_remote(&mut self, ctx: &egui::Context, id: u64) -> bool {
        if self.data.event(id).map(|e| e.remote).unwrap_or(false) {
            self.say(ctx, "This event comes from a subscribed calendar and can't be changed here", false);
            return true;
        }
        false
    }

    /// Starts subscribing to the link typed in preferences.
    pub fn subscribe(&mut self, ctx: &egui::Context) {
        let url = match ics::normalize_url(&self.sub_url) {
            Ok(u) => u,
            Err(e) => return self.say(ctx, e, false),
        };
        if self.data.calendars.iter().any(|c| c.url.as_deref() == Some(url.as_str())) {
            return self.say(ctx, "You are already subscribed to this calendar", false);
        }
        let name = self.sub_name.trim();
        let n = self.data.calendars.iter().filter(|c| c.url.is_some()).count();
        self.data.calendars.push(Calendar {
            name: if name.is_empty() { ics::default_name(&url) } else { name.to_string() },
            color: EVENT_COLORS[(6 + n) % EVENT_COLORS.len()].0,
            visible: true,
            url: Some(url.clone()),
            ..Default::default()
        });
        self.sub_url.clear();
        self.sub_name.clear();
        self.commit();
        self.sync_one(ctx, &url);
    }

    /// Fetches one subscribed calendar in the background.
    pub fn sync_one(&mut self, ctx: &egui::Context, url: &str) {
        if self.shot.is_some() || !self.syncing.insert(url.to_string()) {
            return;
        }
        let (tx, url, ctx, now) = (self.sync_tx.clone(), url.to_string(), ctx.clone(), Self::now());
        std::thread::spawn(move || {
            let res = ics::fetch(&url, now);
            let _ = tx.send((url, res));
            ctx.request_repaint();
        });
    }

    pub fn sync_all(&mut self, ctx: &egui::Context) -> usize {
        let urls: Vec<String> = self.data.calendars.iter().filter_map(|c| c.url.clone()).collect();
        for u in &urls {
            self.sync_one(ctx, u);
        }
        urls.len()
    }

    /// Takes in whatever the background fetches have finished.
    fn poll_sync(&mut self, ctx: &egui::Context) {
        while let Ok((url, res)) = self.sync_rx.try_recv() {
            self.syncing.remove(&url);
            let Some(i) = self.data.calendars.iter().position(|c| c.url.as_deref() == Some(url.as_str())) else { continue };
            match res {
                Ok(feed) => {
                    let count = feed.events.len();
                    let default = ics::default_name(&url);
                    if let Some(n) = feed.name.filter(|_| self.data.calendars[i].name == default) {
                        self.data.calendars[i].name = n;
                    }
                    self.data.replace_remote(i, feed.events);
                    self.data.calendars[i].last_sync = Some(Local::now().format("%b %-d, %H:%M").to_string());
                    self.data.calendars[i].error = None;
                    let name = self.data.calendars[i].name.clone();
                    self.say(ctx, format!("{name}: {count} events"), false);
                }
                Err(e) => {
                    let name = self.data.calendars[i].name.clone();
                    self.say(ctx, format!("{name}: {e}"), false);
                    self.data.calendars[i].error = Some(e);
                }
            }
            self.commit();
        }
        // on start, then every 15 minutes
        let time = ctx.input(|i| i.time);
        if !self.synced_on_start || time - self.last_auto_sync > SYNC_EVERY_S {
            self.synced_on_start = true;
            self.last_auto_sync = time;
            self.sync_all(ctx);
        }
    }

    pub fn delete_event(&mut self, ctx: &egui::Context, id: u64) {
        if self.refuse_remote(ctx, id) {
            return;
        }
        self.snapshot();
        let was_session = self.data.event(id).map(|e| e.task_id.is_some()).unwrap_or(false);
        self.data.events.retain(|e| e.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.commit();
        self.say(ctx, if was_session { "Session unscheduled" } else { "Event deleted" }, true);
    }

    /// Ticks or unticks a task. A task whose due date repeats leaves a
    /// fresh copy behind with its dates moved on.
    pub fn toggle_task(&mut self, id: u64) {
        self.snapshot();
        let mut next: Option<Task> = None;
        if let Some(t) = self.data.task_mut(id) {
            t.done = !t.done;
            if t.done && t.repeat != Repeat::None {
                if let Some(due) = t.due {
                    let new_due = t.repeat.next(due);
                    let mut n = t.clone();
                    n.done = false;
                    n.due = Some(new_due);
                    n.earliest = t.earliest.map(|e| e + (new_due - due));
                    t.repeat = Repeat::None;
                    next = Some(n);
                }
            }
        }
        if let Some(mut n) = next {
            n.id = self.data.new_id();
            self.data.tasks.insert(0, n);
        }
        self.commit();
    }

    /// Puts a task on the calendar and applies the due-date rules.
    pub fn schedule_task(&mut self, ctx: &egui::Context, task_id: u64, start: NaiveDateTime) {
        let Some(t) = self.data.task(task_id).cloned() else { return };
        self.snapshot();
        let mins = t.duration_min.unwrap_or(self.data.prefs.default_task_min);
        let id = self.data.new_id();
        let cal = self.data.task_calendar();
        self.data.events.push(Event {
            id,
            title: t.title.clone(),
            start,
            end: start + Duration::minutes(mins),
            all_day: false,
            calendar: cal,
            notes: String::new(),
            task_id: Some(task_id),
        color: None,
        ..Default::default()
        });
        self.selected = Some(id);
        self.after_task_scheduled(ctx, task_id, start.date());
        self.commit();
    }

    pub fn after_task_scheduled(&mut self, ctx: &egui::Context, task_id: u64, day: NaiveDate) {
        let p = self.data.prefs.clone();
        let Some(t) = self.data.task_mut(task_id) else { return };
        match t.due {
            None if p.auto_due => t.due = Some(day),
            Some(due) if day > due => match p.due_move {
                DueMove::Always => t.due = Some(day),
                DueMove::Never => {}
                DueMove::Ask => {
                    self.toast = Some(Toast {
                        text: "Scheduled after its due date.".into(),
                        until: ctx.input(|i| i.time) + 10.0,
                        undo: false,
                        move_due: Some((task_id, day)),
                        born: -1.0,
                    });
                }
            },
            _ => {}
        }
    }

    /// Clones the copied event onto a clicked spot. Onto a timed spot it
    /// keeps its length; onto an all-day spot it keeps its clock time and
    /// only changes day.
    pub fn paste_at(&mut self, at: NaiveDateTime, onto_all_day: Option<bool>) {
        let Some(src) = self.clipboard.clone() else { return };
        self.snapshot();
        let mut e = src.clone();
        e.id = self.data.new_id();
        if e.remote {
            // a copy of a subscribed event becomes one's own
            e.remote = false;
            e.calendar = self.data.first_own_calendar();
        }
        let len = src.end - src.start;
        let day_shift = Duration::days((at.date() - src.start.date()).num_days());
        let keep_clock = src.all_day || onto_all_day == Some(true);
        e.start = if keep_clock { src.start + day_shift } else { at };
        e.end = e.start + len;
        self.editor = None;
        self.selected = Some(e.id);
        self.data.events.push(e);
        self.commit();
    }

    /// The moment a running session can be split at: now, rounded to the
    /// dragging step, when that leaves time on both sides.
    pub fn split_time(&self, id: u64) -> Option<NaiveDateTime> {
        let e = self.data.event(id)?;
        e.task_id?;
        let snap = self.data.prefs.snap_min;
        let now = Self::now();
        let m = (now.hour() as i64 * 60 + now.minute() as i64 + snap / 2) / snap * snap;
        let at = now.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(m);
        (at > e.start && at < e.end).then_some(at)
    }

    /// Moves a task session to the next free working time.
    pub fn reschedule_session(&mut self, ctx: &egui::Context, id: u64) {
        let Some(e) = self.data.event(id).cloned() else { return };
        if e.task_id.is_none() {
            return;
        }
        self.snapshot();
        self.data.events.retain(|x| x.id != id);
        let from = Self::now().max(e.end);
        match self.data.next_free_slot(from, e.minutes()) {
            Some(start) => {
                let mut moved = e.clone();
                moved.start = start;
                moved.end = start + Duration::minutes(e.minutes());
                self.anchor_to(start.date());
                self.data.events.push(moved);
                self.commit();
                self.say(ctx, format!("Moved to {}", start.format("%a %b %-d, %H:%M")), true);
            }
            None => {
                self.data.events.push(e);
                self.undo.pop();
                self.say(ctx, "No free working time found in the next days", false);
            }
        }
    }

    /// Completes this session's time and books another one of the same length.
    pub fn follow_up(&mut self, ctx: &egui::Context, id: u64) {
        let Some(e) = self.data.event(id).cloned() else { return };
        if e.task_id.is_none() {
            return;
        }
        let from = Self::now().max(e.end);
        match self.data.next_free_slot(from, e.minutes()) {
            Some(start) => {
                self.snapshot();
                let mut next = e.clone();
                next.id = self.data.new_id();
                next.start = start;
                next.end = start + Duration::minutes(e.minutes());
                self.selected = Some(next.id);
                self.anchor_to(start.date());
                self.data.events.push(next);
                self.commit();
                self.say(ctx, format!("Follow-up on {}", start.format("%a %b %-d, %H:%M")), true);
            }
            None => self.say(ctx, "No free working time found in the next days", false),
        }
    }

    /// Ends a running session now and books the rest of its time later.
    pub fn split_session(&mut self, ctx: &egui::Context, id: u64) {
        let Some(at) = self.split_time(id) else {
            self.say(ctx, "Only a session that is running now can be split", false);
            return;
        };
        let Some(e) = self.data.event(id).cloned() else { return };
        let rest = (e.end - at).num_minutes();
        self.snapshot();
        if let Some(x) = self.data.event_mut(id) {
            x.end = at;
        }
        match self.data.next_free_slot(e.end.max(at), rest) {
            Some(start) => {
                let mut next = e.clone();
                next.id = self.data.new_id();
                next.start = start;
                next.end = start + Duration::minutes(rest);
                self.data.events.push(next);
                self.commit();
                self.say(ctx, format!("Remaining {rest} min moved to {}", start.format("%a %b %-d, %H:%M")), true);
            }
            None => {
                self.commit();
                self.say(ctx, "Session shortened; no free time found for the rest", true);
            }
        }
    }

    pub fn delete_task(&mut self, ctx: &egui::Context, task_id: u64) {
        self.snapshot();
        self.data.tasks.retain(|t| t.id != task_id);
        self.data.events.retain(|e| e.task_id != Some(task_id));
        self.selected = None;
        self.commit();
        self.say(ctx, "Task deleted", true);
    }

    /// Shows a day if the current view does not already.
    pub fn anchor_to(&mut self, day: NaiveDate) {
        if !self.visible_days().contains(&day) {
            self.anchor = day;
        }
    }

    pub fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        let today = Local::now().date_naive();
        match cmd {
            Cmd::Today => {
                self.anchor = today;
                let p = &self.data.prefs;
                self.scroll_to = Some(((opening_scroll_min(Self::now()) - p.grid_start_h * 60) as f32 / 60.0 * p.hour_height()).max(0.0));
            }
            Cmd::Next => self.step(1),
            Cmd::Prev => self.step(-1),
            Cmd::DayFwd => self.anchor += Duration::days(1),
            Cmd::DayBack => self.anchor -= Duration::days(1),
            Cmd::ViewDay => self.view = View::Days(1),
            Cmd::ViewWeek => self.view = View::Week,
            Cmd::ViewMonth => self.view = View::Month,
            Cmd::ViewAgenda => self.view = View::Agenda,
            Cmd::ViewTwoWeeks => self.view = View::TwoWeeks,
            Cmd::NewEvent => {
                // viewed day not after now: the next full hour; a later day: 12:00
                let now = Self::now();
                let viewed = self.anchor.and_hms_opt(0, 0, 0).unwrap();
                let start = if viewed < now {
                    (now + Duration::hours(1)).date().and_hms_opt((now + Duration::hours(1)).hour(), 0, 0).unwrap()
                } else {
                    viewed + Duration::hours(NEW_EVENT_NOON_H)
                };
                self.anchor = start.date();
                self.open_new_event(start, NEW_EVENT_MIN, false);
            }
            Cmd::NewTask => {
                self.data.prefs.sidebar_open = true;
                self.focus_new_task = true;
            }
            Cmd::Edit => {
                if let Some(id) = self.selected {
                    self.edit_event(id);
                }
            }
            Cmd::Delete => {
                if let Some(id) = self.selected {
                    self.delete_event(ctx, id);
                }
            }
            Cmd::Copy => {
                if let Some(e) = self.selected.and_then(|id| self.data.event(id)) {
                    self.clipboard = Some(e.clone());
                    self.selected = None;
                    self.say(ctx, "Copied. Right-click the calendar and choose Paste here.", false);
                }
            }
            Cmd::Paste => {
                if let Some(at) = self.clipboard.as_ref().map(|e| e.start) {
                    self.paste_at(at, None);
                }
            }
            Cmd::Undo => self.do_undo(ctx),
            Cmd::Reschedule => {
                if let Some(id) = self.selected {
                    self.reschedule_session(ctx, id);
                }
            }
            Cmd::FollowUp => {
                if let Some(id) = self.selected {
                    self.follow_up(ctx, id);
                }
            }
            Cmd::Split => {
                if let Some(id) = self.selected {
                    self.split_session(ctx, id);
                }
            }
            Cmd::Plan => {
                self.snapshot();
                let (made, left) = self.data.plan_tasks(Self::now());
                self.commit();
                let msg = if made == 0 && left == 0 {
                    "Nothing to plan".to_string()
                } else if left > 0 {
                    format!("Planned {made} session(s); {left} task(s) did not fit in the next {PLAN_SPAN_DAYS} days")
                } else {
                    format!("Planned {made} session(s)")
                };
                self.say(ctx, msg, made > 0);
            }
            Cmd::ToggleTasks => self.data.prefs.sidebar_open = !self.data.prefs.sidebar_open,
            Cmd::ToggleTheme => {
                let dark_now = self.pal.dark;
                self.data.prefs.theme = match self.data.prefs.theme {
                    Theme::Light => Theme::Dark,
                    Theme::Dark => Theme::Light,
                    Theme::System => {
                        if dark_now {
                            Theme::Light
                        } else {
                            Theme::Dark
                        }
                    }
                };
                self.commit();
            }
            Cmd::FinerGrid | Cmd::CoarserGrid => {
                let i = SLOT_CHOICES.iter().position(|&s| s == self.data.prefs.slot_min).unwrap_or(1);
                let j = if cmd == Cmd::FinerGrid { i.saturating_sub(1) } else { (i + 1).min(SLOT_CHOICES.len() - 1) };
                self.data.prefs.slot_min = SLOT_CHOICES[j];
                self.data.prefs.hour_height_px = None;
                self.commit();
            }
            Cmd::Search | Cmd::CommandBar => {
                if self.command_bar.is_some() && cmd == Cmd::CommandBar {
                    self.command_bar = None;
                } else {
                    self.command_bar = Some(String::new());
                    self.command_focus = true;
                }
            }
            Cmd::Prefs => self.open_settings(settings::Page::General),
            Cmd::Shortcuts => self.open_settings(settings::Page::Shortcuts),
            Cmd::Refresh => {
                if self.sync_all(ctx) == 0 {
                    self.say(ctx, "No subscribed calendars to refresh", false);
                } else {
                    self.say(ctx, "Refreshing calendars…", false);
                }
            }
            Cmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let dialogs = self.editor.is_some()
            || self.task_editor.is_some()
            || self.command_bar.is_some();
        let (cmd_mod, shift, time) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift, i.time));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        // always available
        if cmd_mod && pressed(Key::K) {
            return self.run(ctx, Cmd::CommandBar);
        }
        if cmd_mod && pressed(Key::F) {
            return self.run(ctx, Cmd::Search);
        }
        if cmd_mod && pressed(Key::Q) {
            return self.run(ctx, Cmd::Quit);
        }
        if pressed(Key::Escape) {
            if self.command_bar.take().is_some() {
            } else if self.editor.take().is_some() || self.task_editor.take().is_some() {
            } else if self.show_prefs {
                self.show_prefs = false;
            } else {
                self.selected = None;
            }
            return;
        }
        if dialogs || ctx.wants_keyboard_input() {
            return;
        }
        let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        let paste = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))));
        if copy || (cmd_mod && pressed(Key::C)) {
            return self.run(ctx, Cmd::Copy);
        }
        let _ = paste; // Ctrl+V is deliberately unbound; pasting is done by right-click
        if cmd_mod && pressed(Key::Z) {
            return self.run(ctx, Cmd::Undo);
        }
        if cmd_mod {
            return;
        }
        if shift {
            for (k, c) in [(Key::R, Cmd::Reschedule), (Key::F, Cmd::FollowUp), (Key::S, Cmd::Split)] {
                if pressed(k) {
                    return self.run(ctx, c);
                }
            }
        }
        if pressed(Key::N) {
            self.last_n = time;
            return;
        }
        if pressed(Key::T) {
            let c = if time - self.last_n < 1.0 { Cmd::NewTask } else { Cmd::Today };
            self.last_n = -10.0;
            return self.run(ctx, c);
        }
        let map: &[(Key, Cmd)] = &[
            (Key::D, Cmd::ViewDay),
            (Key::W, Cmd::ViewWeek),
            (Key::M, Cmd::ViewMonth),
            (Key::A, Cmd::ViewAgenda),
            (Key::X, Cmd::ViewTwoWeeks),
            (Key::C, Cmd::NewEvent),
            (Key::E, Cmd::Edit),
            (Key::R, Cmd::Refresh),
            (Key::P, Cmd::Prefs),
            (Key::Period, Cmd::Shortcuts),
            (Key::Delete, Cmd::Delete),
            (Key::Backspace, Cmd::Delete),
            (Key::Space, Cmd::ToggleTasks),
            (Key::Backslash, Cmd::ToggleTheme),
            (Key::CloseBracket, Cmd::FinerGrid),
            (Key::OpenBracket, Cmd::CoarserGrid),
            (Key::ArrowRight, if shift { Cmd::DayFwd } else { Cmd::Next }),
            (Key::ArrowLeft, if shift { Cmd::DayBack } else { Cmd::Prev }),
        ];
        for (k, c) in map {
            if pressed(*k) {
                return self.run(ctx, *c);
            }
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        if self.frames == 120 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let img = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(dir);
                }
            }
            let ok = image::save_buffer(
                &path,
                img.as_raw(),
                img.size[0] as u32,
                img.size[1] as u32,
                image::ColorType::Rgba8,
            );
            match ok {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("could not save the picture: {e}");
                    std::process::exit(1);
                }
            }
        }
        if self.frames > 1200 {
            eprintln!("no picture arrived");
            std::process::exit(1);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_theme(match self.data.prefs.theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
        self.pal = Palette::new(ctx.style().visuals.dark_mode);
        let (bg, side, text) = (self.pal.bg, self.pal.side, self.pal.text);
        ctx.style_mut(|s| {
            s.visuals.panel_fill = bg;
            s.visuals.window_fill = side;
            s.visuals.override_text_color = Some(text);
            s.visuals.selection.bg_fill = Color32::from_rgb(0x62, 0x5e, 0xfc);
            s.visuals.selection.stroke.color = Color32::WHITE;
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.animation_time = ANIM_S;
        });

        self.poll_sync(ctx);
        if let Ok(page) = std::env::var("KALENDAR_SHOW_SETTINGS") {
            // test aid: the settings fill the main window, so a picture can be taken
            use settings::Page;
            self.settings_page = match page.as_str() {
                "calendars" => Page::Calendars,
                "active" => Page::ActiveCalendars,
                "shortcuts" => Page::Shortcuts,
                "planner" => Page::Planner,
                "info" => Page::Info,
                _ => Page::General,
            };
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ctx, |ui| self.settings_body(ui));
            self.dlg_toast_public(ctx);
            return self.screenshot(ctx);
        }
        self.keys(ctx);
        self.ui_rail(ctx);
        self.ui_sidebar(ctx);
        self.ui_toolbar(ctx);
        // slide and fade when the period or the view changes
        let time = ctx.input(|i| i.time);
        let shown = (self.view, self.anchor);
        if let Some(last) = self.last_shown {
            if last != shown {
                let dir = (shown.1 - last.1).num_days().signum() as f32;
                self.nav_anim = (time, dir);
            }
        }
        self.last_shown = Some(shown);
        let t = (((time - self.nav_anim.0) as f32) / ANIM_LONG_S).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let dx = self.nav_anim.1 * (1.0 - ease) * 48.0;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.set_clip_rect(full);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full.translate(egui::vec2(dx, 0.0))), |ui| {
                ui.set_opacity(0.25 + 0.75 * ease);
                match self.view {
                    View::Month => self.ui_month(ui),
                    View::Agenda => self.ui_agenda(ui),
                    _ => self.ui_time_grid(ui),
                }
            });
        });
        self.ui_dialogs(ctx);
        if let Some(kind) = self.demo_menu.clone() {
            let want_task = kind == "task";
            if let Some(id) = self.data.events.iter().find(|e| e.task_id.is_some() == want_task && !e.all_day).map(|e| e.id) {
                egui::Window::new("menu").title_bar(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    self.block_menu(ui, id);
                });
            }
        }
        self.ui_drag_ghost(ctx);
        if !ctx.input(|i| i.pointer.any_down()) {
            self.drag_task = None;
        }
        // keep the clock line moving
        ctx.request_repaint_after(std::time::Duration::from_secs(CLOCK_REFRESH_S));
        self.screenshot(ctx);
    }
}

fn main() {
    let mut shot = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => match args.next() {
                Some(p) => shot = Some(PathBuf::from(p)),
                None => {
                    eprintln!("--screenshot needs a file name");
                    std::process::exit(2);
                }
            },
            // test aid: fetch a calendar link and say what was read
            "--check-feed" => {
                let Some(link) = args.next() else {
                    eprintln!("--check-feed needs a link");
                    std::process::exit(2);
                };
                let res = ics::normalize_url(&link).and_then(|u| ics::fetch(&u, App::now()));
                match res {
                    Ok(f) => {
                        println!("{}: {} events", f.name.unwrap_or_else(|| "calendar".into()), f.events.len());
                        for e in f.events.iter().filter(|e| e.start >= App::now()).take(5) {
                            println!("  {}  {}{}", e.start.format("%Y-%m-%d %H:%M"), e.title, if e.all_day { "  (all day)" } else { "" });
                        }
                        return;
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            }
            "--help" | "-h" => {
                println!("kalendar [--screenshot <file.png>]");
                return;
            }
            other => {
                eprintln!("unknown option: {other}");
                std::process::exit(1);
            }
        }
    }
    let mut vb = egui::ViewportBuilder::default()
        .with_title("Kalendar")
        .with_app_id("kalendar")
        .with_inner_size([1400.0, 860.0])
        .with_min_inner_size([720.0, 480.0]);
    if shot.is_some() {
        vb = vb.with_active(false);
        #[cfg(target_os = "windows")]
        {
            vb = vb.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let res = eframe::run_native(
        "Kalendar",
        options,
        Box::new(move |cc| {
            // a heavier face for titles: the regular face, drawn with a little extra weight
            let mut fonts = egui::FontDefinitions::default();
            let base = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
            if let Some(first) = base.first().and_then(|n| fonts.font_data.get(n)).cloned() {
                let mut heavy = (*first).clone();
                heavy.tweak.scale = 1.0;
                fonts.font_data.insert("heavy".into(), std::sync::Arc::new(heavy));
            }
            let mut fam = vec!["heavy".to_string()];
            fam.extend(base);
            fonts.families.insert(egui::FontFamily::Name("bold".into()), fam);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(App::new(shot)))
        }),
    );
    if let Err(e) = res {
        eprintln!("could not open the window: {e}");
        std::process::exit(127);
    }
}
