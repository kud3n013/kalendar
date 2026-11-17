//! The app's state: what is open, being edited or dragged, and the
//! actions on the calendar that the views and commands share.

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui;
use crate::model::*;
use crate::*;
use std::path::PathBuf;

pub enum Drag {
    Create { anchor: NaiveDateTime, draft: Event },
    Move { id: u64, grab_min: i64 },
    Resize { id: u64 },
}

pub struct EventEdit {
    pub ev: Event,
    pub is_new: bool,
    pub focus: bool,
    /// the block the editor was opened from, in screen points; the window
    /// sits beside it, and without one it keeps its usual corner
    pub anchor_rect: Option<egui::Rect>,
    /// the event as stored when the editor last looked; a change made
    /// elsewhere (a drag, undo) is told apart from the person's own edits
    /// and taken into the editor
    pub base: Option<Event>,
    /// for a task's block, its task, edited in the same window
    pub task: Option<TaskEdit>,
}

/// What a block drawn on the calendar becomes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CreateKind {
    Task,
    Event,
    Frame,
}

/// The small window that follows a block drawn on the calendar: it makes
/// the block a task session (the default), a plain event, or a frame.
pub struct CreateEdit {
    pub ev: Event,
    pub kind: CreateKind,
    /// where the block is, in screen points; the window sits beside it
    pub anchor_rect: egui::Rect,
    pub focus: bool,
    /// what a new task gets besides its name: list, importance, notes
    pub task: Task,
    pub due_text: String,
    /// how the block repeats, and for an event up to which day
    pub repeat: Repeat,
    pub until_text: String,
}

impl CreateEdit {
    pub fn new(ev: Event, kind: CreateKind, anchor_rect: egui::Rect) -> Self {
        CreateEdit { ev, kind, anchor_rect, focus: true, task: Task::default(), due_text: String::new(), repeat: Repeat::None, until_text: String::new() }
    }
}

/// A task list whose look is being changed from the task panel.
pub struct ListEdit {
    pub id: u64,
    /// the corner the little window hangs from
    pub at: egui::Pos2,
    pub fresh: bool,
    /// whether the state before the first change was kept for undo
    pub snapped: bool,
}

/// The frame editor: a frame being made or changed.
pub struct FrameEdit {
    pub frame: Frame,
    /// when the frame began as a block drawn on the calendar: that block and
    /// where it is on screen, so the window stays beside it
    pub from_block: Option<(Event, egui::Rect)>,
    pub is_new: bool,
    pub until_text: String,
    pub focus: bool,
}

pub struct TaskEdit {
    pub task: Task,
    pub due_text: String,
    pub tags_text: String,
    pub earliest_text: String,
    pub focus: bool,
    /// the task as stored when the editor last looked, as for [`EventEdit::base`]
    pub base: Task,
}

impl TaskEdit {
    /// An editor for a task as it is stored.
    pub fn of(t: &Task) -> Self {
        TaskEdit {
            task: t.clone(),
            due_text: t.due.map(|d| d.to_string()).unwrap_or_default(),
            tags_text: t.tags.join(" "),
            earliest_text: t.earliest.map(|d| d.to_string()).unwrap_or_default(),
            focus: false,
            base: t.clone(),
        }
    }

    /// Reads the typed dates and tags into the task; a due date that
    /// can't be read is refused.
    pub fn read_text(&mut self) -> Result<(), ()> {
        let text = self.due_text.trim();
        if text.is_empty() {
            self.task.due = None;
        } else {
            self.task.due = Some(NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| ())?);
        }
        self.task.tags = self.tags_text.split_whitespace().map(|t| t.trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect();
        self.task.earliest = NaiveDate::parse_from_str(self.earliest_text.trim(), "%Y-%m-%d").ok();
        Ok(())
    }
}

pub struct Toast {
    pub text: String,
    pub until: f64,
    pub undo: bool,
    pub move_due: Option<(u64, NaiveDate)>,
    /// when it appeared (set on first draw), for the fade
    pub born: f64,
}

pub struct App {
    pub data: Data,
    pub view: View,
    pub anchor: NaiveDate,
    pub selected: Option<u64>,
    pub editor: Option<EventEdit>,
    pub task_editor: Option<TaskEdit>,
    /// whether this system shows the editors in windows of their own, as
    /// last seen; without that they sit inside the main window
    pub own_windows: bool,
    pub create: Option<CreateEdit>,
    pub frame_editor: Option<FrameEdit>,
    pub drag: Option<Drag>,
    pub drag_task: Option<u64>,
    pub ctx_target: Option<u64>,
    /// the frame (and its day) under the last right-click, if no event was
    pub ctx_frame: Option<(u64, NaiveDate)>,
    /// the page sliding (or fading) when the view or its days change
    pub slide: motion::Slide,
    /// the calendar page as last painted, at rest
    pub page_shot: Vec<egui::epaint::ClippedShape>,
    pub last_shown: Option<(View, NaiveDate)>,
    /// test aid: show a right-click menu in a window ("event" or "task")
    pub demo_menu: Option<String>,
    pub ctx_time: Option<NaiveDateTime>,
    pub undo: Vec<Data>,
    pub toast: Option<Toast>,
    pub show_prefs: bool,
    pub settings_page: settings::Page,
    pub settings_query: String,
    pub shortcut_query: String,
    pub sub_kind: u8,
    pub new_cal_name: String,
    pub confirm_remove: Option<u64>,
    pub command_bar: Option<String>,
    pub command_focus: bool,
    pub new_task_text: String,
    pub new_task_open: bool,
    pub new_task_focus: bool,
    /// the list the new-task bar adds to (0 = Inbox)
    pub new_task_list: u64,
    pub quick_nav: bool,
    pub sub_url: String,
    pub sub_name: String,
    /// ids of the calendars and accounts being synced right now
    pub syncing: std::collections::HashSet<u64>,
    pub sync_tx: std::sync::mpsc::Sender<SyncDone>,
    pub sync_rx: std::sync::mpsc::Receiver<SyncDone>,
    pub synced_on_start: bool,
    pub last_auto_sync: f64,
    /// after failures: how many in a row, and when to try again
    pub backoff: std::collections::HashMap<u64, (u32, f64)>,
    pub was_focused: bool,
    /// the "connect an account" form
    pub acct_server: String,
    pub acct_user: String,
    pub acct_pass: String,
    pub acct_error: Option<String>,
    /// synced events as the server last knew them, to tell what was
    /// changed here since
    pub shadow: std::collections::HashMap<u64, Event>,
    /// the same for the tasks of synced lists
    pub task_shadow: std::collections::HashMap<u64, Task>,
    /// something was changed here that a server should hear of, and when
    pub outbox_changed: Option<f64>,
    /// a Nextcloud sign-in is waiting for the browser: its page, and the
    /// switch that calls it off
    pub nextcloud_wait: Option<(String, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
    pub tokens: TokenCache,
    /// the Google sign-in form
    pub g_email: String,
    pub g_client_id: String,
    pub g_secret: String,
    /// the Microsoft sign-in form
    pub ms_email: String,
    pub ms_client_id: String,
    /// the Proton sign-in form; the passwords are cleared once signed in
    pub pr_user: String,
    pub pr_pass: String,
    pub pr_code: String,
    pub pr_second: String,
    /// the token of a check Proton asked for, for the next try
    pub pr_human: Option<String>,
    pub pr_busy: bool,
    pub side_scroll: f32,
    /// how far the hours are scrolled, and where their top edge is on screen
    pub grid_scroll: (f32, f32),
    /// while a zoom is under way: the hour height it heads for, how far
    /// below the grid's top the mouse was, and the exact height reached so far
    pub zoom_goal: Option<(f32, f32, f32)>,
    /// set while the window is too narrow for the panel and several days:
    /// whether the panel was open, and the view, to put back afterwards
    pub compact: Option<(bool, View)>,
    /// the font family and text size now in use; None until first set
    pub font_applied: Option<(String, f32)>,
    /// installed font families, listed once the settings ask for them
    pub font_names: Option<Vec<String>>,
    pub sort_by: usize,
    pub filter_tag: Option<String>,
    /// the name of a list being added, and whether its field just appeared
    pub new_list_text: Option<(String, bool)>,
    pub list_edit: Option<ListEdit>,
    /// the name typed for a new list in the settings
    pub new_list_name: String,
    pub collapsed: std::collections::HashSet<String>,
    pub scroll_to: Option<f32>,
    pub clipboard: Option<Event>,
    /// the command whose new shortcut is being typed in settings
    pub recording: Option<Cmd>,
    pub shot: Option<PathBuf>,
    pub frames: u32,
    pub pal: Palette,
}

impl App {
    pub fn new(shot: Option<PathBuf>) -> Self {
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let today = Local::now().date_naive();
        let data = if shot.is_some() { Data::sample(today) } else { Data::load_or_sample(today) };
        let mut data = data;
        // for screenshots of the settings: a signed-in account to look at
        if shot.is_some() && testaid::get().demo_account {
            data.accounts.push(Account {
                id: 900,
                server: "https://cloud.example.org".into(),
                username: "sam".into(),
                home: Some("https://cloud.example.org/dav/calendars/sam/".into()),
                last_sync: Some(tr("Nov 11, 10:28").into()),
                error: None,
                ..Default::default()
            });
            for (i, (name, color, visible)) in [(tr("Home"), EVENT_COLORS[6].0, true), (tr("Team"), EVENT_COLORS[2].0, true), (tr("Holidays"), EVENT_COLORS[12].0, false)].into_iter().enumerate() {
                data.calendars.push(Calendar {
                    id: 901 + i as u64,
                    name: name.into(),
                    color,
                    visible,
                    account: Some(900),
                    remote_id: Some(format!("https://cloud.example.org/dav/calendars/sam/{i}/")),
                    read_only: i == 2,
                    holds_tasks: i < 2,
                    ..Default::default()
                });
            }
        }
        if shot.is_some() && testaid::get().demo_account {
            data.lists.push(TaskList { id: 910, name: tr("Shopping").into(), color: EVENT_COLORS[4].0, account: Some(900), remote_id: Some("https://cloud.example.org/dav/calendars/sam/shopping/".into()), ..Default::default() });
            data.lists.push(TaskList { id: 911, name: tr("Home").into(), color: EVENT_COLORS[6].0, account: Some(900), remote_id: Some("https://cloud.example.org/dav/calendars/sam/0/".into()), ..Default::default() });
        }
        let view = data.prefs.initial_view;
        let scroll = (opening_scroll_min(Self::now()) - data.prefs.grid_start_h * 60) as f32 / 60.0 * data.prefs.hour_height();
        let mut app = App {
            data,
            view,
            anchor: today,
            selected: None,
            editor: None,
            task_editor: None,
            own_windows: true,
            create: None,
            frame_editor: None,
            drag: None,
            drag_task: None,
            ctx_target: None,
            ctx_frame: None,
            slide: motion::Slide::default(),
            page_shot: vec![],
            last_shown: None,
            demo_menu: testaid::get().show_menu.clone(),
            ctx_time: None,
            undo: vec![],
            toast: None,
            show_prefs: false,
            settings_page: settings::Page::General,
            settings_query: String::new(),
            shortcut_query: String::new(),
            sub_kind: 0,
            new_cal_name: String::new(),
            confirm_remove: None,
            command_bar: None,
            command_focus: false,
            new_task_text: String::new(),
            new_task_open: false,
            new_task_focus: false,
            new_task_list: 0,
            quick_nav: false,
            sub_url: String::new(),
            sub_name: String::new(),
            syncing: Default::default(),
            sync_tx,
            sync_rx,
            synced_on_start: false,
            last_auto_sync: 0.0,
            backoff: Default::default(),
            was_focused: true,
            acct_server: String::new(),
            acct_user: String::new(),
            acct_pass: String::new(),
            acct_error: None,
            shadow: Default::default(),
            task_shadow: Default::default(),
            outbox_changed: None,
            nextcloud_wait: None,
            tokens: Default::default(),
            g_email: String::new(),
            g_client_id: String::new(),
            g_secret: String::new(),
            ms_email: String::new(),
            ms_client_id: String::new(),
            pr_user: String::new(),
            pr_pass: String::new(),
            pr_code: String::new(),
            pr_second: String::new(),
            pr_human: None,
            pr_busy: false,
            side_scroll: 0.0,
            grid_scroll: (0.0, 0.0),
            zoom_goal: None,
            compact: None,
            font_applied: None,
            font_names: None,
            sort_by: 0,
            filter_tag: None,
            new_list_text: None,
            list_edit: None,
            new_list_name: String::new(),
            collapsed: ["done".to_string()].into_iter().collect(),
            scroll_to: Some(scroll.max(0.0)),
            clipboard: None,
            recording: None,
            shot,
            frames: 0,
            pal: Palette::new(true),
        };
        // test aids for pictures of the new windows and layouts
        let aid = testaid::get();
        if aid.panel_right {
            app.data.prefs.sidebar_side = PanelSide::Right;
        }
        if app.shot.is_some() {
            match aid.theme.as_deref() {
                Some("light") => app.data.prefs.theme = Theme::Light,
                Some("dark") => app.data.prefs.theme = Theme::Dark,
                _ => {}
            }
            if let Some(m) = aid.slot {
                app.data.prefs.slot_min = m;
            }
        }
        if let Some(f) = &aid.font {
            app.data.prefs.font = f.clone();
        }
        if let Some(v) = &aid.show_newtask {
            let list = app.data.lists.iter().find(|l| l.name.eq_ignore_ascii_case(v)).map_or(0, |l| l.id);
            app.open_new_task(list);
        }
        if aid.grey {
            // scheduled tasks are done and every frame skips the day after today
            let ids: Vec<u64> = app.data.events.iter().filter_map(|e| e.task_id).collect();
            for t in app.data.tasks.iter_mut().filter(|t| ids.contains(&t.id)) {
                t.done = true;
            }
            for f in app.data.frames.iter_mut() {
                f.skip.push(today + Duration::days(1));
            }
        }
        if let Some(v) = &aid.list_edit {
            let id = app.data.lists.iter().find(|l| l.name.eq_ignore_ascii_case(v)).map_or(0, |l| l.id);
            if let Some(l) = app.data.lists.iter_mut().find(|l| l.id == id) {
                l.icon = icons::LIST_ICONS[1].to_string();
            }
            app.list_edit = Some(ListEdit { id, at: egui::pos2(400.0, 330.0), fresh: true, snapped: true });
        }
        if let Some(kind) = aid.show_create {
            let hour = aid.create_at;
            let start = today.and_hms_opt(hour, 0, 0).unwrap();
            app.open_create(start, 60, egui::Rect::from_min_size(egui::pos2(700.0, 300.0), egui::vec2(150.0, 100.0)));
            if kind == testaid::Create::Frame {
                if let Some(c) = app.create.take() {
                    app.open_new_frame(today, hour as i64 * 60, hour as i64 * 60 + 60, c.ev.calendar, String::new());
                    if let Some(fe) = &mut app.frame_editor {
                        fe.from_block = Some((c.ev.clone(), c.anchor_rect));
                    }
                }
            }
        }
        if let Some((title, _)) = app.shot.as_ref().and(aid.drag.as_ref()) {
            app.drag_task = app.data.tasks.iter().find(|t| t.title.contains(title.as_str())).map(|t| t.id);
        }
        if app.shot.is_some() && aid.show_frame {
            if let Some(id) = app.data.frames.first().map(|f| f.id) {
                app.edit_frame(id);
            }
        }
        app
    }

    /// The current time; KALENDAR_NOW=2026-11-11T10:30 overrides it for testing.
    pub fn now() -> NaiveDateTime {
        if let Some(t) = testaid::get().now {
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
        self.note_edits();
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
            self.say(ctx, tr("Undone"), false);
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

    pub fn step(&mut self, dir: i64) {
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

    /// Opens the small create window beside a block drawn on the calendar.
    /// Whether the window beside a drawn block is up, as a task, an event
    /// or a frame: there is only ever one.
    pub fn block_window_open(&self) -> bool {
        self.create.is_some() || self.frame_editor.as_ref().is_some_and(|f| f.from_block.is_some())
    }

    /// Closes the window beside a drawn block, whichever kind it shows.
    pub fn close_block_window(&mut self) {
        self.create = None;
        if self.frame_editor.as_ref().is_some_and(|f| f.from_block.is_some()) {
            self.frame_editor = None;
        }
    }

    pub fn open_create(&mut self, start: NaiveDateTime, mins: i64, anchor_rect: egui::Rect) {
        self.close_block_window();
        let id = self.data.new_id();
        self.create = Some(CreateEdit::new(
            Event {
                id,
                title: String::new(),
                start,
                end: start + Duration::minutes(mins),
                all_day: false,
                calendar: self.data.first_own_calendar(),
                notes: String::new(),
                task_id: None,
                color: None,
                ..Default::default()
            },
            CreateKind::Task,
            anchor_rect,
        ));
        self.close_inline_editor();
        self.selected = None;
    }

    /// Opens the frame editor on a new frame for `start_min..end_min` of a
    /// day: on weekdays it repeats Monday to Friday, on a weekend day just
    /// on that weekday. Subscribed calendars never get frames.
    pub fn open_new_frame(&mut self, day: NaiveDate, start_min: i64, end_min: i64, calendar: u64, name: String) {
        let calendar = if self.data.calendar(calendar).is_some_and(|c| c.is_local()) {
            calendar
        } else {
            self.data.first_own_calendar()
        };
        let days = if day.weekday().num_days_from_monday() < 5 { FRAME_WEEKDAYS } else { day_bit(day) };
        let id = self.data.new_id();
        self.frame_editor = Some(FrameEdit {
            frame: Frame { id, name, calendar, start_min, end_min: end_min.max(start_min + 15), days, from: day, ..Default::default() },
            from_block: None,
            is_new: true,
            until_text: String::new(),
            focus: true,
        });
        self.create = None;
        self.close_inline_editor();
        self.selected = None;
    }

    pub fn edit_frame(&mut self, id: u64) {
        if let Some(f) = self.data.frame(id).cloned() {
            self.frame_editor = Some(FrameEdit {
                until_text: f.until.map(|d| d.to_string()).unwrap_or_default(),
                frame: f,
                from_block: None,
                is_new: false,
                focus: false,
            });
            self.create = None;
            self.close_inline_editor();
        }
    }

    /// Takes one day out of a frame.
    pub fn skip_frame_day(&mut self, ctx: &egui::Context, id: u64, day: NaiveDate) {
        self.snapshot();
        if let Some(f) = self.data.frame_mut(id) {
            if !f.skip.contains(&day) {
                f.skip.push(day);
                f.skip.sort();
            }
        }
        self.commit();
        self.say(ctx, trf("Frame skipped on {}", &[&day.format("%a, %b %-d")]), true);
    }

    /// Puts a skipped day back into a frame.
    pub fn restore_frame_day(&mut self, id: u64, day: NaiveDate) {
        self.snapshot();
        if let Some(f) = self.data.frame_mut(id) {
            f.skip.retain(|d| *d != day);
        }
        self.commit();
    }

    /// Sets an event aside, or takes it back. A skipped event stays on the
    /// calendar, greyed, and other blocks may be put over it; for an event
    /// that can't be deleted here it is the way to leave it out.
    pub fn toggle_skip(&mut self, ctx: &egui::Context, id: u64) {
        self.snapshot();
        let Some(e) = self.data.event_mut(id) else { return };
        e.skipped = !e.skipped;
        let now = e.skipped;
        self.commit();
        self.say(ctx, if now { tr("Event skipped") } else { tr("Event no longer skipped") }, true);
    }

    pub fn delete_frame(&mut self, ctx: &egui::Context, id: u64) {
        self.snapshot();
        self.data.frames.retain(|f| f.id != id);
        self.commit();
        self.say(ctx, tr("Frame deleted"), true);
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
            anchor_rect: None,
            base: None,
            task: None,
        });
        self.selected = None;
    }

    pub fn edit_event(&mut self, id: u64) {
        self.edit_event_at(id, None);
    }

    /// Opens the editor for an event; with a block rect it sits beside it.
    /// Subscribed events open to be read; saving them is refused.
    pub fn edit_event_at(&mut self, id: u64, anchor_rect: Option<egui::Rect>) {
        if let Some(ev) = self.shown_event(id) {
            let task = ev.task_id.and_then(|t| self.data.task(t)).map(TaskEdit::of);
            self.editor = Some(EventEdit { base: Some(ev.clone()), ev, is_new: false, focus: false, anchor_rect, task });
            self.create = None;
            self.selected = Some(id);
        }
    }

    /// An event as its editor shows it: a block carries its task's name and note.
    pub fn shown_event(&self, id: u64) -> Option<Event> {
        let mut ev = self.data.event(id)?.clone();
        if let Some(t) = ev.task_id.and_then(|t| self.data.task(t)) {
            (ev.title, ev.notes) = (t.title.clone(), t.notes.clone());
        }
        Some(ev)
    }

    /// Closes the event editor only when it sits inside the main window;
    /// in a window of its own it stays open while the person does other things.
    pub fn close_inline_editor(&mut self) {
        if !self.own_windows {
            self.editor = None;
        }
    }

    pub fn edit_task(&mut self, id: u64) {
        if let Some(t) = self.data.task(id) {
            self.task_editor = Some(TaskEdit::of(t));
        }
    }

    /// True (with a message) when the event belongs to a subscribed calendar.
    pub fn refuse_remote(&mut self, ctx: &egui::Context, id: u64) -> bool {
        if let Some(why) = self.data.event(id).and_then(|e| self.data.locked(e)) {
            self.say(ctx, why, false);
            return true;
        }
        false
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
        self.say(ctx, if was_session { tr("Session unscheduled") } else { tr("Event deleted") }, true);
    }

    /// Ticks or unticks a task. A task whose due date repeats leaves a
    /// fresh copy behind with its dates moved on.
    pub fn toggle_task(&mut self, id: u64) {
        self.snapshot();
        let mut next: Option<Task> = None;
        if let Some(t) = self.data.task_mut(id) {
            t.done = !t.done;
            t.archived = false;
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
        if self.data.task(task_id).is_none() {
            return;
        }
        self.snapshot();
        self.place_task(ctx, task_id, start);
        self.commit();
    }

    /// Adds the calendar block for a task, without its own undo step.
    pub fn place_task(&mut self, ctx: &egui::Context, task_id: u64, start: NaiveDateTime) {
        let Some(t) = self.data.task(task_id).cloned() else { return };
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
            notes: t.notes.clone(),
            task_id: Some(task_id),
            color: None,
            ..Default::default()
        });
        self.selected = Some(id);
        self.after_task_scheduled(ctx, task_id, start.date());
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
                        text: tr("Scheduled after its due date.").into(),
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
        // a copy of a synced or subscribed event is a new event of one's own
        e.detach();
        e.skipped = false;
        e.calendar = self.data.writable_or_own(e.calendar);
        let len = src.end - src.start;
        let day_shift = Duration::days((at.date() - src.start.date()).num_days());
        let keep_clock = src.all_day || onto_all_day == Some(true);
        e.start = if keep_clock { src.start + day_shift } else { at };
        e.end = e.start + len;
        self.close_inline_editor();
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
        match self.data.next_free_slot(from, e.minutes(), e.task_id.and_then(|t| self.data.task(t))) {
            Some(start) => {
                let mut moved = e.clone();
                moved.start = start;
                moved.end = start + Duration::minutes(e.minutes());
                self.anchor_to(start.date());
                self.data.events.push(moved);
                self.commit();
                self.say(ctx, trf("Moved to {}", &[&start.format("%a %b %-d, %H:%M")]), true);
            }
            None => {
                self.data.events.push(e);
                self.undo.pop();
                self.say(ctx, tr("No free working time found in the next days"), false);
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
        match self.data.next_free_slot(from, e.minutes(), e.task_id.and_then(|t| self.data.task(t))) {
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
                self.say(ctx, trf("Follow-up on {}", &[&start.format("%a %b %-d, %H:%M")]), true);
            }
            None => self.say(ctx, tr("No free working time found in the next days"), false),
        }
    }

    /// Ends a running session now and books the rest of its time later.
    pub fn split_session(&mut self, ctx: &egui::Context, id: u64) {
        let Some(at) = self.split_time(id) else {
            self.say(ctx, tr("Only a session that is running now can be split"), false);
            return;
        };
        let Some(e) = self.data.event(id).cloned() else { return };
        let rest = (e.end - at).num_minutes();
        self.snapshot();
        if let Some(x) = self.data.event_mut(id) {
            x.end = at;
        }
        match self.data.next_free_slot(e.end.max(at), rest, e.task_id.and_then(|t| self.data.task(t))) {
            Some(start) => {
                let mut next = e.clone();
                next.id = self.data.new_id();
                next.start = start;
                next.end = start + Duration::minutes(rest);
                self.data.events.push(next);
                self.commit();
                self.say(ctx, trf("Remaining {} min moved to {}", &[&rest, &start.format("%a %b %-d, %H:%M")]), true);
            }
            None => {
                self.commit();
                self.say(ctx, tr("Session shortened; no free time found for the rest"), true);
            }
        }
    }

    pub fn delete_task(&mut self, ctx: &egui::Context, task_id: u64) {
        self.snapshot();
        self.data.tasks.retain(|t| t.id != task_id);
        self.data.events.retain(|e| e.task_id != Some(task_id));
        self.selected = None;
        self.commit();
        self.say(ctx, tr("Task deleted"), true);
    }

    /// Shows a day if the current view does not already.
    pub fn anchor_to(&mut self, day: NaiveDate) {
        if !self.visible_days().contains(&day) {
            self.anchor = day;
        }
    }
}
