//! Everything the user can ask for by key, menu or the command bar, and
//! the shortcuts bound to it.

use chrono::{Duration, Local, Timelike};
use eframe::egui::{self, Key};
use crate::model::*;
use crate::*;

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
    NewFrame,
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
    (Cmd::Today, later("Move to the current day"), "T", later("Calendar view")),
    (Cmd::Next, later("Move to the next period"), "Right", later("Calendar view")),
    (Cmd::Prev, later("Move to the previous period"), "Left", later("Calendar view")),
    (Cmd::DayFwd, later("Move one day forward"), "Shift+Right", later("Calendar view")),
    (Cmd::DayBack, later("Move one day back"), "Shift+Left", later("Calendar view")),
    (Cmd::ViewDay, later("Day"), "D", later("Calendar view")),
    (Cmd::ViewWeek, later("Week"), "W", later("Calendar view")),
    (Cmd::ViewMonth, later("Month"), "M", later("Calendar view")),
    (Cmd::ViewAgenda, later("Agenda"), "A", later("Calendar view")),
    (Cmd::ViewTwoWeeks, later("2 weeks"), "X", later("Calendar view")),
    (Cmd::Refresh, later("Refresh your calendars"), "R", later("Calendar view")),
    (Cmd::NewEvent, later("Create a new event"), "C", later("Events and tasks")),
    (Cmd::NewTask, later("Create a new task"), "Ctrl+T", later("Events and tasks")),
    (Cmd::NewFrame, later("Create a new frame"), "B", later("Events and tasks")),
    (Cmd::Edit, later("Edit an event"), "E", later("Events and tasks")),
    (Cmd::Delete, later("Delete selected event or task"), "Del / Backspace", later("Events and tasks")),
    (Cmd::Copy, later("Copy"), "Ctrl+C", later("Events and tasks")),
    (Cmd::Paste, later("Paste here"), "Right-click", later("Events and tasks")),
    (Cmd::Plan, later("Plan my tasks"), "", later("Events and tasks")),
    (Cmd::Reschedule, later("Reschedule session"), "Shift+R", later("Events and tasks")),
    (Cmd::FollowUp, later("Schedule follow-up"), "Shift+F", later("Events and tasks")),
    (Cmd::Split, later("Split and reschedule"), "Shift+S", later("Events and tasks")),
    (Cmd::Undo, later("Undo"), "Ctrl+Z", later("General")),
    (Cmd::CommandBar, later("Toggle command bar"), "Ctrl+K", later("General")),
    (Cmd::Search, later("Open search"), "Ctrl+F", later("General")),
    (Cmd::ToggleTasks, later("Collapse or expand the task panel"), "Ctrl+S", later("General")),
    (Cmd::ToggleTheme, later("Toggle dark/light themes"), "\\", later("General")),
    (Cmd::FinerGrid, later("Increase calendar time resolution"), "]", later("General")),
    (Cmd::CoarserGrid, later("Reduce calendar time resolution"), "[", later("General")),
    (Cmd::Prefs, later("Open settings"), "Ctrl+,", later("General")),
    (Cmd::Shortcuts, later("List available shortcuts"), ".", later("General")),
    (Cmd::Quit, later("Quit"), "Ctrl+Q", later("General")),
];

impl Cmd {
    /// A stable name each command's shortcut override is stored under.
    pub fn id(&self) -> &'static str {
        match self {
            Cmd::Today => "today",
            Cmd::Next => "next",
            Cmd::Prev => "prev",
            Cmd::DayFwd => "day-forward",
            Cmd::DayBack => "day-back",
            Cmd::ViewDay => "view-day",
            Cmd::ViewWeek => "view-week",
            Cmd::ViewMonth => "view-month",
            Cmd::ViewAgenda => "view-agenda",
            Cmd::ViewTwoWeeks => "view-two-weeks",
            Cmd::NewEvent => "new-event",
            Cmd::NewTask => "new-task",
            Cmd::NewFrame => "new-frame",
            Cmd::Edit => "edit",
            Cmd::Delete => "delete",
            Cmd::Copy => "copy",
            Cmd::Paste => "paste",
            Cmd::Undo => "undo",
            Cmd::Plan => "plan",
            Cmd::Reschedule => "reschedule",
            Cmd::FollowUp => "follow-up",
            Cmd::Split => "split",
            Cmd::ToggleTasks => "toggle-tasks",
            Cmd::ToggleTheme => "toggle-theme",
            Cmd::FinerGrid => "finer-grid",
            Cmd::CoarserGrid => "coarser-grid",
            Cmd::Search => "search",
            Cmd::CommandBar => "command-bar",
            Cmd::Prefs => "settings",
            Cmd::Shortcuts => "shortcuts",
            Cmd::Refresh => "refresh",
            Cmd::Quit => "quit",
        }
    }
}

/// One key with its modifiers.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KeyBind {
    pub ctrl: bool,
    pub shift: bool,
    pub key: Key,
}

pub fn key_from_name(s: &str) -> Option<Key> {
    Some(match s {
        "Left" => Key::ArrowLeft,
        "Right" => Key::ArrowRight,
        "Up" => Key::ArrowUp,
        "Down" => Key::ArrowDown,
        "Del" => Key::Delete,
        "," => Key::Comma,
        "." => Key::Period,
        "\\" => Key::Backslash,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        other => Key::from_name(other)?,
    })
}

/// The short name a key is written as in shortcut texts.
pub fn key_label(k: Key) -> &'static str {
    match k {
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Delete => "Del",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Backslash => "\\",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        other => other.name(),
    }
}

/// Writes a key combination the way `parse_bind` reads it.
pub fn bind_label(b: &KeyBind) -> String {
    let mut s = String::new();
    if b.ctrl {
        s += "Ctrl+";
    }
    if b.shift {
        s += "Shift+";
    }
    s + key_label(b.key)
}

/// Reads a shortcut text such as "Ctrl+Shift+K" or "Del / Backspace".
pub fn parse_bind(s: &str) -> Vec<KeyBind> {
    s.split(" / ")
        .filter_map(|alt| {
            let (mut ctrl, mut shift, mut key) = (false, false, None);
            for part in alt.trim().split('+') {
                match part {
                    "Ctrl" | "Cmd" => ctrl = true,
                    "Shift" => shift = true,
                    p => key = key_from_name(p),
                }
            }
            key.map(|key| KeyBind { ctrl, shift, key })
        })
        .collect()
}

impl App {
    /// The keys bound to a command: the user's own if set, else the default.
    pub fn binds(&self, cmd: Cmd) -> Vec<KeyBind> {
        parse_bind(&self.bind_text(cmd))
    }

    /// A tooltip naming a command and, when it has one, its shortcut.
    pub fn tip(&self, name: &str, cmd: Cmd) -> String {
        match self.bind_text(cmd) {
            k if k.is_empty() => name.to_string(),
            k => trf("{} ({})", &[&name, &k]),
        }
    }

    /// The shortcut text for a command, as shown to the user.
    pub fn bind_text(&self, cmd: Cmd) -> String {
        if let Some(s) = self.data.prefs.keybinds.get(cmd.id()) {
            return s.clone();
        }
        COMMANDS.iter().find(|c| c.0 == cmd).map(|c| c.2.to_string()).unwrap_or_default()
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
            Cmd::NewTask => self.open_new_task(0),
            Cmd::NewFrame => {
                // the viewed day, at the next full hour when that is today
                let now = Self::now();
                let start_h = if self.anchor == now.date() { (now.hour() as i64 + 1).min(22) } else { 9 };
                let day = self.anchor;
                self.open_new_frame(day, start_h * 60, (start_h + 2).min(24) * 60, self.data.default_calendar(), String::new());
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
                    self.say(ctx, tr("Copied. Right-click the calendar and choose Paste here."), false);
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
            Cmd::Plan => self.plan(ctx, None),
            Cmd::ToggleTasks => {
                self.data.prefs.sidebar_open = !self.data.prefs.sidebar_open;
                // kept for next time, unless a narrow window is what hid it
                if self.compact.is_none() {
                    self.save();
                }
            }
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
                    self.say(ctx, tr("No subscribed calendars or accounts to refresh"), false);
                } else {
                    self.say(ctx, tr("Refreshing calendars…"), false);
                }
            }
            Cmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// The mouse's back and forward buttons (4 and 5) move the view: a week
    /// or a month at a time, and a single day in the day views.
    pub fn mouse_nav(&mut self, ctx: &egui::Context) {
        let inline = !self.own_windows;
        let open = self.show_prefs
            || (inline && (self.editor.is_some() || self.task_editor.is_some()))
            || self.command_bar.is_some()
            || self.create.is_some()
            || self.frame_editor.is_some();
        if open {
            return;
        }
        let (back, fwd) = ctx.input(|i| {
            (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2))
        });
        let dir = match (back, fwd) {
            (true, false) => -1,
            (false, true) => 1,
            _ => return,
        };
        match self.view {
            View::Days(_) => self.anchor += Duration::days(dir),
            _ => self.step(dir),
        }
    }

    pub fn keys(&mut self, ctx: &egui::Context) {
        // editors in windows of their own take their own keys
        let inline = !self.own_windows;
        let dialogs = (inline && (self.editor.is_some() || self.task_editor.is_some()))
            || self.command_bar.is_some()
            || self.create.is_some()
            || self.frame_editor.is_some()
            || self.new_task_open;
        if self.recording.is_some() {
            return;
        }
        let (cmd_mod, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let hit = |b: &KeyBind| b.ctrl == cmd_mod && b.shift == shift && pressed(b.key);
        let bound: Vec<(Cmd, Vec<KeyBind>)> = COMMANDS.iter().map(|c| (c.0, self.binds(c.0))).collect();
        let fired = |global: bool| {
            bound.iter().find_map(|(c, bs)| {
                // Ctrl combos keep working while a window or text field is open
                bs.iter().any(|b| b.ctrl == global && hit(b)).then_some(*c)
            })
        };
        let busy = dialogs || ctx.egui_wants_keyboard_input();
        if let Some(c) = fired(true) {
            // a few commands work from anywhere; the rest would fight with
            // text fields (undo, copy) and wait until nothing is open
            let anywhere = matches!(c, Cmd::CommandBar | Cmd::Search | Cmd::NewTask | Cmd::Prefs | Cmd::Quit);
            if anywhere || !busy {
                return self.run(ctx, c);
            }
        }
        if pressed(Key::Escape) {
            if self.command_bar.take().is_some() {
            } else if self.new_task_open {
                self.new_task_open = false;
            } else if self.create.take().is_some() {
            } else if let Some(f) = self.frame_editor.as_ref() {
                // a kept frame changed in its window asks before it goes
                if !f.is_new && f.changed(self.data.frame(f.frame.id)) {
                    crate::dialogs::ask_discard(ctx, "frame_editor");
                } else {
                    self.frame_editor = None;
                }
            } else if inline && self.editor.as_ref().is_some_and(|e| e.changed()) {
                crate::dialogs::ask_discard(ctx, "event_editor");
            } else if inline && self.task_editor.as_ref().is_some_and(|e| e.changed()) {
                crate::dialogs::ask_discard(ctx, "task_editor");
            } else if inline && (self.editor.take().is_some() || self.task_editor.take().is_some()) {
            } else if self.show_prefs {
                self.show_prefs = false;
            } else {
                self.selected = None;
            }
            return;
        }
        if busy {
            return;
        }
        // egui turns Ctrl+C into a Copy event rather than a key press
        let copy_event = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if copy_event && self.binds(Cmd::Copy).iter().any(|b| b.ctrl && b.key == Key::C) {
            return self.run(ctx, Cmd::Copy);
        }
        if let Some(c) = fired(false) {
            self.run(ctx, c);
        }
    }
}

impl App {
    /// Plans open tasks, or only those of one list; says what came of it.
    pub fn plan(&mut self, ctx: &egui::Context, list: Option<u64>) {
        self.snapshot();
        let (made, left) = self.data.plan_tasks_in(Self::now(), list);
        self.commit();
        let msg = if made == 0 && left == 0 {
            tr("Nothing to plan").to_string()
        } else if left > 0 {
            trf("Planned {} session(s); {} task(s) did not fit in the next {} days", &[&made, &left, &PLAN_SPAN_DAYS])
        } else {
            trf("Planned {} session(s)", &[&made])
        };
        self.say(ctx, msg, made > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_shortcut_reads_back() {
        for (cmd, _, keys, _) in COMMANDS {
            if keys.is_empty() || *cmd == Cmd::Paste {
                continue;
            }
            let binds = parse_bind(keys);
            assert_eq!(binds.len(), keys.split(" / ").count(), "{keys}");
            let again: Vec<String> = binds.iter().map(bind_label).collect();
            assert_eq!(parse_bind(&again.join(" / ")), binds, "{keys}");
        }
    }

    #[test]
    fn settings_shortcut_is_ctrl_comma() {
        assert_eq!(parse_bind("Ctrl+,"), vec![KeyBind { ctrl: true, shift: false, key: Key::Comma }]);
        assert_eq!(parse_bind("Shift+Left")[0].key, Key::ArrowLeft);
    }

    #[test]
    fn command_ids_are_unique() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.0.id()).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
