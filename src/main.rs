//! Kalendar: a calendar and task planner.

mod ics;
mod icons;
mod model;
mod panel;
mod secrets;
mod settings;
mod sync;
mod tz;
mod dialogs;
mod fonts;
mod views;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{self, Color32, Key};
use model::*;
use std::path::PathBuf;
use sync::caldav::CalDav;
use sync::Provider;

/// What a background thread hands back to the window.
pub enum SyncDone {
    /// a calendar's events were read
    Pulled { cal: u64, res: Result<sync::Delta, sync::SyncError> },
    /// an account's calendars were listed; `first` on signing in
    Listed { account: u64, first: bool, home: Option<String>, res: Result<Vec<sync::RemoteCalendar>, sync::SyncError> },
    /// a private link was read from the keyring
    Link { cal: u64, res: Result<String, String> },
    /// a private link was put into the keyring
    LinkKept { cal: u64, res: Result<(), String> },
    /// a task list's tasks were read
    PulledTasks { list: u64, res: Result<sync::Delta, sync::SyncError> },
    /// changes made here were sent to the server
    Pushed { cal: u64, results: Vec<sync::PushResult> },
    /// Nextcloud's sign-in page is open in the browser
    NextcloudOpened { login: String },
    /// the person signed in on Nextcloud's page, or it failed
    NextcloudDone { res: Result<sync::nextcloud::Granted, sync::SyncError> },
    /// the person signed in on Google's page, or it failed
    GoogleDone { res: Result<GoogleGrant, sync::SyncError> },
    /// a sign-in to Proton went through, wants a check first, or failed
    ProtonDone { username: String, res: Result<sync::proton::SignIn, sync::SyncError> },
}

pub struct GoogleGrant {
    microsoft: bool,
    email: String,
    client_id: String,
    /// what goes into the keyring: refresh token and client secret
    secret: String,
    access: String,
    expires_in: u64,
}

/// Access tokens of signed-in accounts: in memory only, shared with the
/// background threads. Each is kept with the time it runs out.
pub type TokenCache = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u64, (String, std::time::Instant)>>>;

/// A connection to an account's server. `secret` is what the keyring
/// holds for it: an app password, or for Google the refresh token, from
/// which an access token is had when the last one has run out.
fn open_account(a: &Account, secret: &str, tokens: &TokenCache, allow_file: bool) -> Result<Box<dyn Provider>, sync::SyncError> {
    if a.proton {
        let (uid, refresh, key_pass) = sync::proton::unpack(secret).ok_or(sync::SyncError::Auth)?;
        let access = tokens.lock().ok().and_then(|t| t.get(&a.id).map(|(t, _)| t.clone())).unwrap_or_default();
        // Proton hands out a new refresh token with every access token;
        // the newest is the one to keep
        let (id, cache, kept_uid, kept_pass) = (a.id, tokens.clone(), uid.clone(), key_pass.clone());
        let renewed = move |access: &str, refresh: &str| {
            if let Ok(mut c) = cache.lock() {
                c.insert(id, (access.to_string(), std::time::Instant::now()));
            }
            let _ = secrets::set(&secrets::account_key(id), &sync::proton::pack(&kept_uid, refresh, &kept_pass), allow_file);
        };
        return Ok(Box::new(sync::proton::Proton::new(&uid, &access, &refresh, &key_pass, Some(Box::new(renewed)))));
    }
    if !a.google && !a.microsoft {
        return Ok(Box::new(CalDav::new(&a.server, &a.username, secret, a.home.clone())));
    }
    let now = std::time::Instant::now();
    let cached = tokens.lock().ok().and_then(|t| t.get(&a.id).filter(|(_, until)| *until > now).map(|(t, _)| t.clone()));
    let access = match cached {
        Some(t) => t,
        None => {
            let (refresh, client_secret) = sync::google::unpack(secret).ok_or(sync::SyncError::Auth)?;
            let client_id = a.client_id.as_deref().unwrap_or("");
            let app = if a.microsoft { sync::graph::app(client_id, &a.username) } else { sync::google::app(client_id, &client_secret, &a.username) };
            let t = sync::oauth::refresh(&app, &refresh)?;
            if let Ok(mut cache) = tokens.lock() {
                cache.insert(a.id, (t.access.clone(), now + std::time::Duration::from_secs(t.expires_in.saturating_sub(60))));
            }
            // Microsoft hands out a new refresh token each time; the
            // newest is the one to keep
            if let Some(newer) = t.refresh.filter(|r| *r != refresh) {
                let _ = secrets::set(&secrets::account_key(a.id), &sync::google::pack(&newer, &client_secret), allow_file);
            }
            t.access
        }
    };
    if a.microsoft {
        return Ok(Box::new(sync::graph::Graph::new(&access)));
    }
    Ok(Box::new(CalDav::with_bearer(&a.server, &a.username, &access, a.home.clone())))
}

/// Why an account's sync failed, in words that say what to do.
fn account_error(a: Option<&Account>, e: &sync::SyncError) -> String {
    match (a, e) {
        (Some(a), sync::SyncError::Auth) if a.proton => "Proton wants you to sign in again (the session ended or the password changed): sign out of this account here, then sign in once more.".into(),
        (Some(a), sync::SyncError::Auth) if a.microsoft => "Microsoft wants you to sign in again: sign out of this account here, then sign in once more.".into(),
        (Some(a), sync::SyncError::Auth) if a.google => "Google wants you to sign in again: sign out of this account here, then sign in once more. While a Google Cloud project is in Testing, a sign-in lasts 7 days.".into(),
        _ => e.text(),
    }
}

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

/// The outline of a squircle: a rectangle whose corners are superellipse
/// arcs (continuous curvature) instead of circular ones.
pub fn squircle_points(rect: egui::Rect, radius: f32) -> Vec<egui::Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if r <= 0.5 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    const STEPS: usize = 10;
    // |x|^4 + |y|^4 = r^4, so the offsets are cos(t)^0.5 and sin(t)^0.5
    let arc = |i: usize| {
        let t = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
        (t.cos().max(0.0).sqrt(), t.sin().max(0.0).sqrt())
    };
    let mut pts = Vec::with_capacity(4 * (STEPS + 1));
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(rt - r + r * s, t + r - r * c)); // top right
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(rt - r + r * c, b - r + r * s)); // bottom right
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(l + r - r * s, b - r + r * c)); // bottom left
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(l + r - r * c, t + r - r * s)); // top left
    }
    pts
}

pub fn fill_squircle(p: &egui::Painter, rect: egui::Rect, radius: f32, fill: Color32) {
    p.add(egui::Shape::convex_polygon(squircle_points(rect, radius), fill, egui::Stroke::NONE));
}

pub fn stroke_squircle(p: &egui::Painter, rect: egui::Rect, radius: f32, stroke: egui::Stroke) {
    p.add(egui::Shape::closed_line(squircle_points(rect, radius), stroke));
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
    /// the block the editor was opened from, in screen points; the window
    /// sits beside it, and without one it keeps its usual corner
    pub anchor_rect: Option<egui::Rect>,
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
}

/// The frame editor: a frame being made or changed.
pub struct FrameEdit {
    pub frame: Frame,
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
    (Cmd::NewTask, "Create a new task", "Ctrl+T", "Events and tasks"),
    (Cmd::NewFrame, "Create a new frame", "B", "Events and tasks"),
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
    (Cmd::ToggleTasks, "Collapse or expand the task panel", "Ctrl+S", "General"),
    (Cmd::ToggleTheme, "Toggle dark/light themes", "\\", "General"),
    (Cmd::FinerGrid, "Increase calendar time resolution", "]", "General"),
    (Cmd::CoarserGrid, "Reduce calendar time resolution", "[", "General"),
    (Cmd::Prefs, "Open settings", "Ctrl+,", "General"),
    (Cmd::Shortcuts, "List available shortcuts", ".", "General"),
    (Cmd::Quit, "Quit", "Ctrl+Q", "General"),
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

fn key_from_name(s: &str) -> Option<Key> {
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

pub struct App {
    pub data: Data,
    pub view: View,
    pub anchor: NaiveDate,
    pub selected: Option<u64>,
    pub editor: Option<EventEdit>,
    pub task_editor: Option<TaskEdit>,
    pub create: Option<CreateEdit>,
    pub frame_editor: Option<FrameEdit>,
    pub drag: Option<Drag>,
    pub drag_task: Option<u64>,
    pub ctx_target: Option<u64>,
    /// the frame (and its day) under the last right-click, if no event was
    pub ctx_frame: Option<(u64, NaiveDate)>,
    /// start time, direction (0 = fade) and reach (share of the width) of
    /// the slide when the view changes
    pub nav_anim: (f64, f32, f32),
    /// the calendar page as last painted, and the one sliding away
    pub page_shot: Vec<egui::epaint::ClippedShape>,
    pub page_old: Vec<egui::epaint::ClippedShape>,
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
    /// while a zoom is under way: the hour height it heads for, and how far
    /// below the grid's top the mouse was
    pub zoom_goal: Option<(f32, f32)>,
    /// set while the window is too narrow for the panel and several days:
    /// whether the panel was open, and the view, to put back afterwards
    pub compact: Option<(bool, View)>,
    /// the font family and text size now in use; None until first set
    pub font_applied: Option<(String, f32)>,
    /// installed font families, listed once the settings ask for them
    pub font_names: Option<Vec<String>>,
    pub sort_by: usize,
    pub filter_tag: Option<String>,
    pub new_list_text: Option<String>,
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
    fn new(shot: Option<PathBuf>) -> Self {
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let today = Local::now().date_naive();
        let data = if shot.is_some() { Data::sample(today) } else { Data::load_or_sample(today) };
        let mut data = data;
        // for screenshots of the settings: a signed-in account to look at
        if shot.is_some() && std::env::var_os("KALENDAR_DEMO_ACCOUNT").is_some() {
            data.accounts.push(Account {
                id: 900,
                server: "https://cloud.example.org".into(),
                username: "sam".into(),
                home: Some("https://cloud.example.org/dav/calendars/sam/".into()),
                last_sync: Some("Nov 11, 10:28".into()),
                error: None,
                ..Default::default()
            });
            for (i, (name, color, visible)) in [("Home", EVENT_COLORS[6].0, true), ("Team", EVENT_COLORS[2].0, true), ("Holidays", EVENT_COLORS[12].0, false)].into_iter().enumerate() {
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
        if shot.is_some() && std::env::var_os("KALENDAR_DEMO_ACCOUNT").is_some() {
            data.lists.push(TaskList { id: 910, name: "Shopping".into(), color: EVENT_COLORS[4].0, account: Some(900), remote_id: Some("https://cloud.example.org/dav/calendars/sam/shopping/".into()), ..Default::default() });
            data.lists.push(TaskList { id: 911, name: "Home".into(), color: EVENT_COLORS[6].0, account: Some(900), remote_id: Some("https://cloud.example.org/dav/calendars/sam/0/".into()), ..Default::default() });
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
            create: None,
            frame_editor: None,
            drag: None,
            drag_task: None,
            ctx_target: None,
            ctx_frame: None,
            nav_anim: (-10.0, 0.0, 1.0),
            page_shot: vec![],
            page_old: vec![],
            last_shown: None,
            demo_menu: std::env::var("KALENDAR_SHOW_MENU").ok(),
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
            collapsed: ["done".to_string()].into_iter().collect(),
            scroll_to: Some(scroll.max(0.0)),
            clipboard: None,
            recording: None,
            shot,
            frames: 0,
            pal: Palette::new(true),
        };
        // test aids for pictures of the new windows and layouts
        if std::env::var("KALENDAR_PANEL_RIGHT").is_ok() {
            app.data.prefs.sidebar_side = PanelSide::Right;
        }
        if app.shot.is_some() {
            match std::env::var("KALENDAR_THEME").as_deref() {
                Ok("light") => app.data.prefs.theme = Theme::Light,
                Ok("dark") => app.data.prefs.theme = Theme::Dark,
                _ => {}
            }
            if let Some(m) = std::env::var("KALENDAR_SLOT").ok().and_then(|v| v.parse().ok()) {
                app.data.prefs.slot_min = m;
            }
        }
        // KALENDAR_SHOW_NEWTASK=<list name> opens it as that list's + button does
        // for screenshots: a font family other than the system's
        if let Ok(f) = std::env::var("KALENDAR_FONT") {
            app.data.prefs.font = f;
        }
        if let Ok(v) = std::env::var("KALENDAR_SHOW_NEWTASK") {
            let list = app.data.lists.iter().find(|l| l.name.eq_ignore_ascii_case(&v)).map_or(0, |l| l.id);
            app.open_new_task(list);
        }
        if std::env::var("KALENDAR_SHOW_CREATE").is_ok() {
            let start = today.and_hms_opt(10, 0, 0).unwrap();
            app.open_create(start, 60, egui::Rect::from_min_size(egui::pos2(700.0, 300.0), egui::vec2(150.0, 100.0)));
        }
        // KALENDAR_DRAG="title@x,y": hold the task with that title as if it
        // were being dragged, with the pointer at x,y
        if let Some(title) = app.shot.as_ref().and(drag_aid()).map(|d| d.0) {
            app.drag_task = app.data.tasks.iter().find(|t| t.title.contains(&title)).map(|t| t.id);
        }
        if app.shot.is_some() && std::env::var("KALENDAR_SHOW_FRAME").is_ok() {
            if let Some(id) = app.data.frames.first().map(|f| f.id) {
                app.edit_frame(id);
            }
        }
        app
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

    /// Opens the small create window beside a block drawn on the calendar.
    pub fn open_create(&mut self, start: NaiveDateTime, mins: i64, anchor_rect: egui::Rect) {
        let id = self.data.new_id();
        self.create = Some(CreateEdit {
            ev: Event {
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
            kind: CreateKind::Task,
            anchor_rect,
            focus: true,
        });
        self.editor = None;
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
            is_new: true,
            until_text: String::new(),
            focus: true,
        });
        self.create = None;
        self.editor = None;
        self.selected = None;
    }

    pub fn edit_frame(&mut self, id: u64) {
        if let Some(f) = self.data.frame(id).cloned() {
            self.frame_editor = Some(FrameEdit {
                until_text: f.until.map(|d| d.to_string()).unwrap_or_default(),
                frame: f,
                is_new: false,
                focus: false,
            });
            self.create = None;
            self.editor = None;
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
        self.say(ctx, format!("Frame skipped on {}", day.format("%a, %b %-d")), true);
    }

    pub fn delete_frame(&mut self, ctx: &egui::Context, id: u64) {
        self.snapshot();
        self.data.frames.retain(|f| f.id != id);
        self.commit();
        self.say(ctx, "Frame deleted", true);
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
        });
        self.selected = None;
    }

    pub fn edit_event(&mut self, id: u64) {
        self.edit_event_at(id, None);
    }

    /// Opens the editor for an event; with a block rect it sits beside it.
    /// Subscribed events open to be read; saving them is refused.
    pub fn edit_event_at(&mut self, id: u64, anchor_rect: Option<egui::Rect>) {
        if let Some(e) = self.data.event(id) {
            self.editor = Some(EventEdit { ev: e.clone(), is_new: false, focus: false, anchor_rect });
            self.create = None;
            self.selected = Some(id);
        }
    }

    /// The keys bound to a command: the user's own if set, else the default.
    pub fn binds(&self, cmd: Cmd) -> Vec<KeyBind> {
        parse_bind(&self.bind_text(cmd))
    }

    /// A tooltip naming a command and, when it has one, its shortcut.
    pub fn tip(&self, name: &str, cmd: Cmd) -> String {
        match self.bind_text(cmd) {
            k if k.is_empty() => name.to_string(),
            k => format!("{name} ({k})"),
        }
    }

    /// The shortcut text for a command, as shown to the user.
    pub fn bind_text(&self, cmd: Cmd) -> String {
        if let Some(s) = self.data.prefs.keybinds.get(cmd.id()) {
            return s.clone();
        }
        COMMANDS.iter().find(|c| c.0 == cmd).map(|c| c.2.to_string()).unwrap_or_default()
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
        if let Some(why) = self.data.event(id).and_then(|e| self.data.locked(e)) {
            self.say(ctx, why, false);
            return true;
        }
        false
    }

    /// Remembers synced events as they are now; what differs from this
    /// later was changed here.
    pub fn reshadow(&mut self) {
        let writable: std::collections::HashSet<u64> = self.data.calendars.iter().filter(|c| c.is_synced_writable()).map(|c| c.id).collect();
        self.shadow = self.data.events.iter().filter(|e| writable.contains(&e.calendar)).map(|e| (e.id, e.clone())).collect();
        let lists: std::collections::HashSet<u64> = self.data.lists.iter().filter(|l| l.is_synced_writable()).map(|l| l.id).collect();
        self.task_shadow = self.data.tasks.iter().filter(|t| lists.contains(&t.list)).map(|t| (t.id, t.clone())).collect();
    }

    /// The same as `note_edits`, for tasks in lists that sync both ways.
    fn note_task_edits(&mut self) -> bool {
        let lists: std::collections::HashSet<u64> = self.data.lists.iter().filter(|l| l.is_synced_writable()).map(|l| l.id).collect();
        if lists.is_empty() && self.task_shadow.is_empty() {
            return false;
        }
        let mut touched = false;
        let mut here = std::collections::HashSet::new();
        for t in &mut self.data.tasks {
            here.insert(t.id);
            let old = self.task_shadow.get(&t.id);
            if let Some(o) = old.filter(|o| o.list != t.list) {
                // moved to another list: gone from the old one, new in this.
                // A list that was removed here keeps its tasks on the server.
                if let Some(href) = o.href.as_ref().filter(|_| lists.contains(&o.list)) {
                    self.data.outbox.push(PendingDelete { calendar: o.list, href: href.clone(), etag: o.etag.clone() });
                    touched = true;
                }
                t.detach();
            } else if old.is_none() && t.href.is_some() && lists.contains(&t.list) {
                // a copy of a synced task, or one brought back by undo
                t.detach();
            }
            if !lists.contains(&t.list) {
                continue;
            }
            let same = old.is_some_and(|o| o.list == t.list && o.same_content(t));
            if !same {
                t.dirty = true;
                touched = true;
            }
        }
        for (id, o) in &self.task_shadow {
            if here.contains(id) || !lists.contains(&o.list) {
                continue;
            }
            if let Some(href) = o.href.clone().filter(|h| !self.data.outbox.iter().any(|d| &d.href == h)) {
                self.data.outbox.push(PendingDelete { calendar: o.list, href, etag: o.etag.clone() });
                touched = true;
            }
        }
        touched
    }

    /// Finds what was changed here in calendars that sync both ways, and
    /// marks it to be sent: changed and new events are flagged, deleted
    /// ones are noted in the outbox. Runs on every save.
    fn note_edits(&mut self) {
        let writable: std::collections::HashSet<u64> = self.data.calendars.iter().filter(|c| c.is_synced_writable()).map(|c| c.id).collect();
        let mut touched = self.note_task_edits();
        if writable.is_empty() && self.shadow.is_empty() {
            self.reshadow();
            if touched {
                self.outbox_changed = Some(f64::NAN);
            }
            return;
        }
        let mut here = std::collections::HashSet::new();
        for e in &mut self.data.events {
            here.insert(e.id);
            let old = self.shadow.get(&e.id);
            if let Some(o) = old.filter(|o| o.calendar != e.calendar) {
                // moved to another calendar: gone from the old one, new in this
                if let Some(href) = &o.href {
                    self.data.outbox.push(PendingDelete { calendar: o.calendar, href: href.clone(), etag: o.etag.clone() });
                    touched = true;
                }
                e.detach();
            } else if old.is_none() && (e.remote || e.href.is_some()) && writable.contains(&e.calendar) {
                // a copy of a synced event, or one brought back by undo
                e.detach();
            }
            if !writable.contains(&e.calendar) {
                continue;
            }
            let same = old.is_some_and(|o| o.calendar == e.calendar && o.same_content(e));
            if !same && !e.dirty {
                e.dirty = true;
            }
            touched |= !same;
        }
        for (id, o) in &self.shadow {
            if here.contains(id) {
                continue;
            }
            if let Some(href) = o.href.clone().filter(|h| !self.data.outbox.iter().any(|d| &d.href == h)) {
                // a calendar that went away takes its events without a word to the server
                if writable.contains(&o.calendar) {
                    self.data.outbox.push(PendingDelete { calendar: o.calendar, href, etag: o.etag.clone() });
                    touched = true;
                }
            }
        }
        self.reshadow();
        if touched {
            self.outbox_changed = Some(f64::NAN);
        }
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
        let id = self.data.new_id();
        self.data.calendars.push(Calendar {
            id,
            read_only: true,
            name: if name.is_empty() { ics::default_name(&url) } else { name.to_string() },
            color: EVENT_COLORS[(6 + n) % EVENT_COLORS.len()].0,
            visible: true,
            url: Some(url.clone()),
            ..Default::default()
        });
        self.sub_url.clear();
        self.sub_name.clear();
        self.commit();
        self.keep_private_links(ctx);
        self.sync_feed(ctx, id);
    }

    /// Runs `work` on a background thread and hands its result to the
    /// window, which is woken to take it in.
    fn in_background(&self, ctx: &egui::Context, work: impl FnOnce(&dyn Fn(SyncDone)) + Send + 'static) {
        let (tx, ctx) = (self.sync_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            work(&|done| {
                let _ = tx.send(done);
                ctx.request_repaint();
            });
        });
    }

    /// A Proton link holds the keys to its calendar: it is moved into the
    /// keyring, and links already there are read back.
    pub fn keep_private_links(&mut self, ctx: &egui::Context) {
        if self.shot.is_some() {
            return;
        }
        let allow_file = self.data.prefs.plaintext_secrets;
        for c in &self.data.calendars {
            let (id, Some(url)) = (c.id, c.url.clone()) else { continue };
            if c.url_secret && url == SECRET_URL {
                self.in_background(ctx, move |send| send(SyncDone::Link { cal: id, res: secrets::get(&secrets::feed_key(id), allow_file) }));
            } else if !c.url_secret && ics::is_proton(&url) {
                self.in_background(ctx, move |send| send(SyncDone::LinkKept { cal: id, res: secrets::set(&secrets::feed_key(id), &url, allow_file) }));
            }
        }
    }

    /// Removes a calendar and forgets its private link.
    pub fn drop_calendar(&mut self, id: u64) {
        if self.data.calendar(id).is_some_and(|c| c.url_secret) && self.shot.is_none() {
            std::thread::spawn(move || secrets::delete(&secrets::feed_key(id)));
        }
        self.data.remove_calendar(id);
    }

    /// Fetches one subscribed calendar in the background.
    pub fn sync_feed(&mut self, ctx: &egui::Context, cal: u64) {
        let Some(url) = self.data.calendar(cal).and_then(|c| c.url.clone()).filter(|u| u != SECRET_URL) else { return };
        if self.shot.is_some() || !self.syncing.insert(cal) {
            return;
        }
        let now = Self::now();
        self.in_background(ctx, move |send| {
            let res = sync::IcsFeed { url }.pull(&sync::RemoteCalendar::default(), None, now);
            send(SyncDone::Pulled { cal, res });
        });
    }

    /// Signs in to the server typed in preferences.
    pub fn connect_account(&mut self, ctx: &egui::Context) {
        let (server, user) = (self.acct_server.trim().to_string(), self.acct_user.trim().to_string());
        if server.is_empty() || user.is_empty() || self.acct_pass.is_empty() {
            self.acct_error = Some("Fill in the server address, the user name and the app password.".into());
            return;
        }
        if self.data.accounts.iter().any(|a| a.server == server && a.username == user) {
            self.acct_error = Some("This account is already connected.".into());
            return;
        }
        self.acct_error = None;
        let id = self.data.new_id();
        self.data.accounts.push(Account { id, server, username: user, ..Default::default() });
        // the password leaves the form at once; it is kept only once it worked
        let pass = std::mem::take(&mut self.acct_pass);
        self.sync_account(ctx, id, Some(pass));
    }

    /// Signs out: the calendars go, and the password leaves the keyring.
    pub fn remove_account(&mut self, id: u64) {
        self.snapshot();
        let gone = self.data.account(id).cloned();
        self.data.remove_account(id);
        let allow_file = self.data.prefs.plaintext_secrets;
        if self.shot.is_none() {
            std::thread::spawn(move || {
                let key = secrets::account_key(id);
                // a Nextcloud app password is taken back at the server too
                // a Google sign-in likewise
                // a Proton session is ended there as well
                if let Some((uid, refresh, _)) = gone.as_ref().filter(|a| a.proton).and_then(|_| secrets::get(&key, allow_file).ok()).and_then(|s| sync::proton::unpack(&s)) {
                    if let Ok((access, _)) = sync::proton::refresh(sync::proton::API, &uid, &refresh) {
                        sync::proton::sign_out(sync::proton::API, &uid, &access);
                    }
                }
                if let Some(a) = gone.filter(|a| a.nextcloud || a.google) {
                    if let Ok(pass) = secrets::get(&key, allow_file) {
                        match sync::google::unpack(&pass).filter(|_| a.google) {
                            Some((refresh, _)) => sync::google::revoke(&refresh),
                            None => sync::nextcloud::revoke(&a.server, &a.username, &pass),
                        }
                    }
                }
                secrets::delete(&key);
            });
        }
        self.commit();
    }

    /// Asks an account's server which calendars it has; their events are
    /// pulled once the answer is in. `password` is given on first sign-in,
    /// and kept in the keyring if the server takes it.
    pub fn sync_account(&mut self, ctx: &egui::Context, account: u64, password: Option<String>) {
        let Some(a) = self.data.account(account).cloned() else { return };
        if self.shot.is_some() || !self.syncing.insert(account) {
            return;
        }
        let allow_file = self.data.prefs.plaintext_secrets;
        let first = password.is_some();
        let tokens = self.tokens.clone();
        self.in_background(ctx, move |send| {
            let key = secrets::account_key(account);
            let pass = match password {
                Some(p) => Ok(p),
                None => secrets::get(&key, allow_file),
            };
            let (home, res) = match pass {
                Err(e) => (None, Err(sync::SyncError::Other(e))),
                Ok(p) => match open_account(&a, &p, &tokens, allow_file) {
                  Err(e) => (None, Err(e)),
                  Ok(mut dav) => {
                    let mut res = dav.list_calendars();
                    // the calendars may have moved since they were found
                    if a.home.is_some() && matches!(res, Err(sync::SyncError::Other(_))) {
                        dav.forget_home();
                        res = dav.list_calendars();
                    }
                    if first && res.is_ok() {
                        if let Err(e) = secrets::set(&key, &p, allow_file) {
                            res = Err(sync::SyncError::Other(e));
                        }
                    }
                    (dav.home(), res)
                  }
                },
            };
            send(SyncDone::Listed { account, first, home, res });
        });
    }

    /// Pulls the shown calendars of an account, one after the other.
    fn pull_account(&mut self, ctx: &egui::Context, account: u64) {
        let Some(a) = self.data.account(account).cloned() else { return };
        if self.shot.is_some() {
            return;
        }
        // the last part says whether it is a task list
        let mut jobs: Vec<(u64, sync::RemoteCalendar, Option<String>, Vec<sync::PushJob>, bool)> = vec![];
        for c in self.data.calendars.iter().filter(|c| c.account == Some(account) && c.visible) {
            let Some(id) = c.remote_id.clone() else { continue };
            if !self.syncing.insert(c.id) {
                continue;
            }
            // what was changed here goes out first
            let mut out: Vec<sync::PushJob> = vec![];
            if c.is_synced_writable() {
                out.extend(self.data.events.iter().filter(|e| e.calendar == c.id && e.dirty).cloned().map(sync::PushJob::Save));
                out.extend(self.data.outbox.iter().filter(|d| d.calendar == c.id).map(|d| sync::PushJob::Delete { href: d.href.clone(), etag: d.etag.clone() }));
            }
            jobs.push((c.id, sync::RemoteCalendar { id, ctag: c.ctag.clone(), ..Default::default() }, c.sync_state.clone(), out, false));
        }
        // and its task lists
        for l in self.data.lists.iter().filter(|l| l.account == Some(account)) {
            let Some(id) = l.remote_id.clone() else { continue };
            if !self.syncing.insert(l.id) {
                continue;
            }
            let mut out: Vec<sync::PushJob> = vec![];
            if l.is_synced_writable() {
                out.extend(self.data.tasks.iter().filter(|t| t.list == l.id && t.dirty).cloned().map(sync::PushJob::SaveTask));
                out.extend(self.data.outbox.iter().filter(|d| d.calendar == l.id).map(|d| sync::PushJob::Delete { href: d.href.clone(), etag: d.etag.clone() }));
            }
            jobs.push((l.id, sync::RemoteCalendar { id, ctag: l.ctag.clone(), ..Default::default() }, l.sync_state.clone(), out, true));
        }
        if jobs.is_empty() {
            return;
        }
        let (allow_file, now) = (self.data.prefs.plaintext_secrets, Self::now());
        let tokens = self.tokens.clone();
        self.in_background(ctx, move |send| {
            // the keyring's answer and the sign-in both have to work out
            let pass = secrets::get(&secrets::account_key(account), allow_file).map_err(sync::SyncError::Other);
            let opened = pass.and_then(|p| open_account(&a, &p, &tokens, allow_file));
            let (mut dav, pass) = match opened {
                Ok(d) => (Some(d), Ok(())),
                Err(e) => (None, Err(e)),
            };
            for (cal, remote, mut state, out, tasks) in jobs {
                if let (Some(dav), false) = (&mut dav, out.is_empty()) {
                    let results = sync::push(dav.as_mut(), &remote, &out, chrono::Utc::now().naive_utc());
                    // after a conflict the server's copy is read afresh
                    if results.iter().any(|r| matches!(r, sync::PushResult::Conflict { .. } | sync::PushResult::TaskConflict { .. } | sync::PushResult::DeleteConflict { .. })) {
                        state = None;
                    }
                    send(SyncDone::Pushed { cal, results });
                }
                let res = match (&mut dav, &pass) {
                    (Some(dav), _) if tasks => dav.pull_tasks(&remote, state.as_deref(), now),
                    (Some(dav), _) => dav.pull(&remote, state.as_deref(), now),
                    (None, Err(e)) => Err(e.clone()),
                    (None, Ok(_)) => unreachable!(),
                };
                send(if tasks { SyncDone::PulledTasks { list: cal, res } } else { SyncDone::Pulled { cal, res } });
            }
        });
    }

    /// Refreshes everything now, whatever failed before.
    pub fn sync_all(&mut self, ctx: &egui::Context) -> usize {
        self.backoff.clear();
        self.sync_due(ctx, f64::MAX)
    }

    /// Refreshes what is not waiting after a failure.
    fn sync_due(&mut self, ctx: &egui::Context, time: f64) -> usize {
        let ready = |b: &std::collections::HashMap<u64, (u32, f64)>, id: u64| b.get(&id).is_none_or(|(_, next)| time >= *next);
        let feeds: Vec<u64> = self.data.calendars.iter().filter(|c| c.url.is_some() && ready(&self.backoff, c.id)).map(|c| c.id).collect();
        let accounts: Vec<u64> = self.data.accounts.iter().filter(|a| ready(&self.backoff, a.id)).map(|a| a.id).collect();
        for id in &feeds {
            self.sync_feed(ctx, *id);
        }
        for id in &accounts {
            self.sync_account(ctx, *id, None);
        }
        feeds.len() + accounts.len()
    }

    /// After a failure the next automatic try waits twice as long each
    /// time, up to four hours.
    fn back_off(&mut self, id: u64, time: f64) {
        let fails = self.backoff.get(&id).map(|b| b.0).unwrap_or(0) + 1;
        let wait = SYNC_EVERY_S * 2f64.powi(fails.min(4) as i32);
        self.backoff.insert(id, (fails, time + wait));
    }

    /// Takes in whatever the background work has finished.
    fn poll_sync(&mut self, ctx: &egui::Context) {
        let time = ctx.input(|i| i.time);
        let stamp = || Some(Local::now().format("%b %-d, %H:%M").to_string());
        while let Ok(done) = self.sync_rx.try_recv() {
            match done {
                SyncDone::Pulled { cal, res } => {
                    self.syncing.remove(&cal);
                    let Some(c) = self.data.calendar(cal) else { continue };
                    let (feed_url, account, name) = (c.url.clone(), c.account, c.name.clone());
                    match res {
                        Ok(delta) => {
                            let count = delta.upserts.len();
                            // a feed that names itself replaces the stand-in name
                            let own = delta.name.clone().filter(|_| feed_url.as_deref().map(ics::default_name).as_deref() == Some(name.as_str()));
                            self.data.apply(cal, delta);
                            let Some(c) = self.data.calendar_mut(cal) else { continue };
                            c.last_sync = stamp();
                            c.error = None;
                            if let Some(n) = own {
                                c.name = n;
                            }
                            let name = c.name.clone();
                            self.backoff.remove(&cal);
                            if feed_url.is_some() {
                                self.say(ctx, format!("{name}: {count} events"), false);
                            }
                        }
                        Err(e) => {
                            let text = e.text();
                            if feed_url.is_some() {
                                self.say(ctx, format!("{name}: {text}"), false);
                            }
                            if let Some(c) = self.data.calendar_mut(cal) {
                                c.error = Some(text.clone());
                            }
                            let why = account_error(account.and_then(|a| self.data.account(a)), &e);
                            if let Some(a) = account.and_then(|a| self.data.account_mut(a)) {
                                a.error = Some(why);
                            }
                            self.back_off(account.unwrap_or(cal), time);
                        }
                    }
                }
                SyncDone::Listed { account, first, home, res } => {
                    self.syncing.remove(&account);
                    if self.data.account(account).is_none() {
                        continue;
                    }
                    match res {
                        Ok(list) => {
                            self.take_calendar_list(account, home, &list);
                            self.backoff.remove(&account);
                            if first {
                                self.sub_kind = 0;
                                self.acct_server.clear();
                                self.acct_user.clear();
                                let n = list.len();
                                self.say(ctx, format!("Connected: {n} calendar{}", if n == 1 { "" } else { "s" }), false);
                            }
                            self.pull_account(ctx, account);
                        }
                        Err(e) if first => {
                            // nothing was kept: the form shows what went wrong
                            self.acct_server = self.data.account(account).map(|a| a.server.clone()).unwrap_or_default();
                            self.acct_user = self.data.account(account).map(|a| a.username.clone()).unwrap_or_default();
                            self.data.remove_account(account);
                            self.acct_error = Some(e.text());
                        }
                        Err(e) => {
                            let why = account_error(self.data.account(account), &e);
                            if let Some(a) = self.data.account_mut(account) {
                                a.error = Some(why);
                            }
                            self.back_off(account, time);
                        }
                    }
                }
                SyncDone::PulledTasks { list, res } => {
                    self.syncing.remove(&list);
                    let Some(account) = self.data.lists.iter().find(|l| l.id == list).and_then(|l| l.account) else { continue };
                    match res {
                        Ok(delta) => self.data.apply_tasks(list, delta),
                        Err(e) => {
                            let why = account_error(self.data.account(account), &e);
                            if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == list) {
                                l.error = Some(e.text());
                            }
                            if let Some(a) = self.data.account_mut(account) {
                                a.error = Some(why);
                            }
                            self.back_off(account, time);
                        }
                    }
                }
                SyncDone::Pushed { cal, results } => self.take_push_results(ctx, cal, results),
                SyncDone::NextcloudOpened { login } => {
                    if let Some(w) = &mut self.nextcloud_wait {
                        w.0 = login;
                    }
                }
                SyncDone::NextcloudDone { res } => {
                    if self.nextcloud_wait.take().is_none() {
                        continue; // called off meanwhile
                    }
                    match res {
                        Ok(g) => {
                            let (server, user) = (g.caldav(), g.username.clone());
                            if self.data.accounts.iter().any(|a| a.server == server && a.username == user) {
                                self.acct_error = Some("This account is already connected.".into());
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username: user, nextcloud: true, ..Default::default() });
                                self.sync_account(ctx, id, Some(g.app_password));
                            }
                        }
                        Err(e) => self.acct_error = Some(e.text()),
                    }
                }
                SyncDone::GoogleDone { res } => {
                    if self.nextcloud_wait.take().is_none() {
                        continue; // called off meanwhile
                    }
                    match res {
                        Ok(g) => {
                            let server = if g.microsoft { format!("microsoft:{}", g.email.to_lowercase()) } else { sync::google::server(&g.email) };
                            if self.data.accounts.iter().any(|a| a.server == server) {
                                self.acct_error = Some("This account is already connected.".into());
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username: g.email, google: !g.microsoft, microsoft: g.microsoft, client_id: Some(g.client_id), ..Default::default() });
                                if let Ok(mut cache) = self.tokens.lock() {
                                    cache.insert(id, (g.access, std::time::Instant::now() + std::time::Duration::from_secs(g.expires_in.saturating_sub(60))));
                                }
                                self.g_secret.clear();
                                self.sub_kind = 0;
                                self.sync_account(ctx, id, Some(g.secret));
                            }
                        }
                        Err(e) => self.acct_error = Some(e.text()),
                    }
                }
                SyncDone::ProtonDone { username, res } => {
                    self.pr_busy = false;
                    match res {
                        Ok(sync::proton::SignIn::Done(g)) => {
                            let server = format!("proton:{}", username.to_lowercase());
                            if self.data.accounts.iter().any(|a| a.server == server) {
                                self.acct_error = Some("This account is already connected.".into());
                                std::thread::spawn(move || sync::proton::sign_out(sync::proton::API, &g.uid, &g.access));
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username, proton: true, ..Default::default() });
                                if let Ok(mut cache) = self.tokens.lock() {
                                    cache.insert(id, (g.access, std::time::Instant::now()));
                                }
                                self.pr_pass.clear();
                                self.pr_code.clear();
                                self.pr_second.clear();
                                self.pr_human = None;
                                self.sub_kind = 0;
                                self.sync_account(ctx, id, Some(sync::proton::pack(&g.uid, &g.refresh, &g.key_pass)));
                            }
                        }
                        Ok(sync::proton::SignIn::Human { token, url }) => {
                            self.pr_human = Some(token);
                            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                            self.acct_error = Some("Proton asks for a check that you are human. Finish it on the page that opened in your browser, then press the sign-in button again.".into());
                        }
                        Err(e) => {
                            self.pr_human = None;
                            self.acct_error = Some(match e {
                                sync::SyncError::Auth => "Proton refused the user name or password.".into(),
                                e => e.text(),
                            });
                        }
                    }
                }
                SyncDone::Link { cal, res } => {
                    let Some(c) = self.data.calendar_mut(cal) else { continue };
                    match res {
                        Ok(url) => {
                            c.url = Some(url);
                            self.sync_feed(ctx, cal);
                        }
                        Err(e) => c.error = Some(format!("Its link is in the keyring and could not be read. {e}")),
                    }
                }
                SyncDone::LinkKept { cal, res } => {
                    // without a keyring the link stays in data.json, as before
                    if let (Ok(()), Some(c)) = (res, self.data.calendar_mut(cal)) {
                        c.url_secret = true;
                    }
                }
            }
            // what a server sent is not a change made here
            self.reshadow();
            self.commit();
        }
        // a change made here goes out a moment later, so a burst of edits
        // travels together
        if let Some(at) = self.outbox_changed {
            if at.is_nan() {
                self.outbox_changed = Some(time + PUSH_AFTER_S);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(PUSH_AFTER_S + 0.1));
            } else if time >= at {
                self.outbox_changed = None;
                let accounts: Vec<u64> = self.data.accounts.iter().map(|a| a.id).collect();
                for a in accounts {
                    self.pull_account(ctx, a);
                }
            }
        }
        // on start, every 15 minutes, and when the window is come back to;
        // never while it is minimised
        let (focused, minimized) = ctx.input(|i| (i.focused, i.viewport().minimized.unwrap_or(false)));
        let came_back = focused && !self.was_focused && time - self.last_auto_sync > SYNC_ON_FOCUS_S;
        self.was_focused = focused;
        if !self.synced_on_start {
            self.reshadow();
            self.keep_private_links(ctx);
        }
        if (!self.synced_on_start || time - self.last_auto_sync > SYNC_EVERY_S || came_back) && !minimized {
            self.synced_on_start = true;
            self.last_auto_sync = time;
            self.sync_due(ctx, time);
        }
    }

    /// Takes in how the changes sent to a server fared.
    /// Something was written to the server while it was removed or
    /// replaced here: the server's copy has to go too, or it would come
    /// back as a second one.
    fn forget_on_server(&mut self, cal: u64, href: String, etag: Option<String>) {
        match self.data.outbox.iter_mut().find(|d| d.calendar == cal && d.href == href) {
            // already to be deleted, but the write gave it a new version
            Some(d) => d.etag = etag,
            None => self.data.outbox.push(PendingDelete { calendar: cal, href, etag }),
        }
        self.outbox_changed = Some(f64::NAN);
    }

    fn take_push_results(&mut self, ctx: &egui::Context, cal: u64, results: Vec<sync::PushResult>) {
        use sync::PushResult::*;
        let (mut conflicts, mut task_conflicts) = (0, 0);
        let mut failed: Option<String> = None;
        for r in results {
            match r {
                Saved { sent, uid, href, etag } => {
                    let Some(e) = self.data.event_mut(sent.id).filter(|e| e.calendar == cal) else {
                        self.forget_on_server(cal, href, etag);
                        continue;
                    };
                    // changed again while it was on its way: it goes out once more
                    e.dirty = !e.same_content(&sent);
                    e.remote = true;
                    (e.uid, e.href, e.etag) = (Some(uid), Some(href), etag);
                }
                Conflict { event } => {
                    // the server's copy wins and comes in with the pull that
                    // follows; the edit made here is kept beside it
                    let own = self.data.first_own_calendar();
                    let Some(e) = self.data.event_mut(event) else { continue };
                    e.dirty = false;
                    let mut copy = e.clone();
                    copy.detach();
                    copy.calendar = own;
                    copy.id = self.data.new_id();
                    self.data.events.push(copy);
                    conflicts += 1;
                }
                TaskSaved { sent, uid, href, etag } => {
                    let Some(t) = self.data.task_mut(sent.id).filter(|t| t.list == cal) else {
                        self.forget_on_server(cal, href, etag);
                        continue;
                    };
                    t.dirty = !t.same_content(&sent);
                    (t.uid, t.href, t.etag) = (Some(uid), Some(href), etag);
                }
                TaskConflict { task } => {
                    // the server's copy wins; the edit made here is kept in the Inbox
                    let Some(t) = self.data.task_mut(task) else { continue };
                    t.dirty = false;
                    let mut copy = t.clone();
                    copy.detach();
                    copy.list = 0;
                    copy.id = self.data.new_id();
                    self.data.tasks.push(copy);
                    task_conflicts += 1;
                }
                Deleted { href } => self.data.outbox.retain(|d| d.href != href),
                DeleteConflict { href } => {
                    // changed on the server since: it stays, and comes back
                    self.data.outbox.retain(|d| d.href != href);
                    conflicts += 1;
                }
                Failed { error } => failed = Some(error.text()),
            }
        }
        let own = self.data.calendar(self.data.first_own_calendar()).map(|c| c.name.clone()).unwrap_or_default();
        if conflicts > 0 {
            self.say(ctx, format!("Conflict: the event was changed on the server, whose copy was kept. Your edit was saved as a copy in {own}."), false);
        } else if task_conflicts > 0 {
            self.say(ctx, "Conflict: the task was changed on the server, whose copy was kept. Your edit was saved as a copy in the Inbox.", false);
        } else if let Some(e) = &failed {
            self.say(ctx, format!("A change could not be sent and will be tried again. {e}"), false);
        }
        if let Some(c) = self.data.calendar_mut(cal) {
            c.error = failed;
        } else if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == cal) {
            l.error = failed;
        }
    }

    /// Signs in to a Nextcloud server through its own page in the
    /// browser. The page is opened, and the server is asked every two
    /// seconds whether the person is through, for up to five minutes.
    pub fn connect_nextcloud(&mut self, ctx: &egui::Context) {
        let server = self.acct_server.trim().to_string();
        if server.is_empty() {
            self.acct_error = Some("Fill in the server address first.".into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            use std::sync::atomic::Ordering;
            let res = sync::nextcloud::start(&server).and_then(|pending| {
                let _ = std::process::Command::new("xdg-open").arg(&pending.login).spawn();
                send(SyncDone::NextcloudOpened { login: pending.login.clone() });
                let began = std::time::Instant::now();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if stop.load(Ordering::Relaxed) {
                        return Err(sync::SyncError::Other("Sign-in was called off.".into()));
                    }
                    if began.elapsed().as_secs() > NEXTCLOUD_WAIT_S {
                        return Err(sync::SyncError::Other("Nobody signed in within five minutes. Try again.".into()));
                    }
                    if let Some(g) = sync::nextcloud::poll(&pending)? {
                        return Ok(g);
                    }
                }
            });
            send(SyncDone::NextcloudDone { res });
        });
    }

    /// Signs in to Google through its page in the browser, with the
    /// client ID and secret the person brought.
    pub fn connect_google(&mut self, ctx: &egui::Context) {
        let (email, client_id, secret) = (self.g_email.trim().to_string(), self.g_client_id.trim().to_string(), self.g_secret.trim().to_string());
        if !email.contains('@') || client_id.is_empty() {
            self.acct_error = Some("Fill in your Google address and the client ID of your Google Cloud project.".into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            let app = sync::google::app(&client_id, &secret, &email);
            let opened = |url: &str| {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                send(SyncDone::NextcloudOpened { login: url.to_string() });
            };
            let res = sync::oauth::authorize(&app, &opened, &stop, NEXTCLOUD_WAIT_S).and_then(|t| match t.refresh {
                Some(r) => Ok(GoogleGrant { microsoft: false, email, client_id, secret: sync::google::pack(&r, &secret), access: t.access, expires_in: t.expires_in }),
                None => Err(sync::SyncError::Other("Google signed you in but gave nothing to stay signed in with. Remove kalendar under your Google account's third-party access, then try again.".into())),
            });
            send(SyncDone::GoogleDone { res });
        });
    }

    /// Signs in to Microsoft through its page in the browser, with the
    /// client ID of the app registration the person brought.
    pub fn connect_microsoft(&mut self, ctx: &egui::Context) {
        let (email, client_id) = (self.ms_email.trim().to_string(), self.ms_client_id.trim().to_string());
        if !email.contains('@') || client_id.is_empty() {
            self.acct_error = Some("Fill in your Microsoft address and the client ID of your app registration.".into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            let app = sync::graph::app(&client_id, &email);
            let opened = |url: &str| {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                send(SyncDone::NextcloudOpened { login: url.to_string() });
            };
            let res = sync::oauth::authorize(&app, &opened, &stop, NEXTCLOUD_WAIT_S).and_then(|t| match t.refresh {
                Some(r) => Ok(GoogleGrant { microsoft: true, email, client_id, secret: sync::google::pack(&r, ""), access: t.access, expires_in: t.expires_in }),
                None => Err(sync::SyncError::Other("Microsoft signed you in but gave nothing to stay signed in with. Check that the app registration allows the offline_access permission.".into())),
            });
            send(SyncDone::GoogleDone { res });
        });
    }

    /// Signs in to Proton with what the form holds. The password is used
    /// for this one sign-in and not kept.
    pub fn connect_proton(&mut self, ctx: &egui::Context) {
        let username = self.pr_user.trim().to_string();
        if username.is_empty() || self.pr_pass.is_empty() {
            self.acct_error = Some("Fill in your Proton address and password.".into());
            return;
        }
        if self.shot.is_some() || self.pr_busy {
            return;
        }
        self.acct_error = None;
        self.pr_busy = true;
        let (pass, code, second, human) = (self.pr_pass.clone(), self.pr_code.clone(), self.pr_second.clone(), self.pr_human.clone());
        self.in_background(ctx, move |send| {
            let res = sync::proton::sign_in(sync::proton::API, &username, &pass, &code, &second, human.as_deref());
            send(SyncDone::ProtonDone { username, res });
        });
    }

    pub fn cancel_nextcloud(&mut self) {
        if let Some((_, stop)) = self.nextcloud_wait.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn add_synced_list(&mut self, account: u64, remote_id: String, name: &str, color: Option<[u8; 3]>, read_only: bool) -> u64 {
        let n = self.data.lists.len();
        let id = self.data.new_id();
        self.data.lists.push(TaskList {
            id,
            name: name.to_string(),
            color: color.unwrap_or(EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0),
            account: Some(account),
            remote_id: Some(remote_id),
            read_only,
            ..Default::default()
        });
        id
    }

    /// Whether the tasks kept in a synced calendar are brought in as a list.
    pub fn calendar_tasks_on(&self, cal: u64) -> bool {
        let Some(c) = self.data.calendar(cal) else { return false };
        self.data.lists.iter().any(|l| l.account == c.account && l.account.is_some() && l.remote_id == c.remote_id)
    }

    /// Brings the tasks kept in a synced calendar in as a task list, or
    /// takes that list away again. The server keeps its tasks either way.
    pub fn set_calendar_tasks(&mut self, ctx: &egui::Context, cal: u64, on: bool) {
        let Some(c) = self.data.calendar(cal).cloned() else { return };
        let (Some(account), Some(remote_id)) = (c.account, c.remote_id.clone()) else { return };
        if on && !self.calendar_tasks_on(cal) {
            self.add_synced_list(account, remote_id, &c.name, Some(c.color), c.read_only);
            self.reshadow();
            self.commit();
            self.pull_account(ctx, account);
        } else if !on {
            let lists: Vec<u64> = self.data.lists.iter().filter(|l| l.account == Some(account) && l.remote_id.as_ref() == Some(&remote_id)).map(|l| l.id).collect();
            for l in lists {
                self.data.remove_synced_list(l);
            }
            self.reshadow();
            self.commit();
        }
    }

    /// Brings the account's calendars in line with the server's list: new
    /// ones are added, ones that are gone leave with their events.
    fn take_calendar_list(&mut self, account: u64, home: Option<String>, list: &[sync::RemoteCalendar]) {
        if let Some(a) = self.data.account_mut(account) {
            a.home = home;
            a.error = None;
            a.last_sync = Some(Local::now().format("%b %-d, %H:%M").to_string());
        }
        let gone: Vec<u64> = self
            .data
            .calendars
            .iter()
            .filter(|c| c.account == Some(account) && !list.iter().any(|r| Some(&r.id) == c.remote_id.as_ref()))
            .map(|c| c.id)
            .collect();
        for id in gone {
            self.data.remove_calendar(id);
        }
        // task lists: one for each collection that holds only tasks; one
        // for a calendar that holds both only where the person asked for it
        let gone: Vec<u64> = self.data.lists.iter().filter(|l| l.account == Some(account) && !list.iter().any(|r| r.tasks && Some(&r.id) == l.remote_id.as_ref())).map(|l| l.id).collect();
        for id in gone {
            self.data.remove_synced_list(id);
        }
        for r in list.iter().filter(|r| r.tasks) {
            if let Some(l) = self.data.lists.iter_mut().find(|l| l.account == Some(account) && l.remote_id.as_ref() == Some(&r.id)) {
                l.read_only = r.read_only;
            } else if !r.events {
                self.add_synced_list(account, r.id.clone(), &r.name, r.color, r.read_only);
            }
            // whether it can hold tasks is kept with the calendar, for settings
            if let Some(c) = self.data.calendars.iter_mut().find(|c| c.account == Some(account) && c.remote_id.as_ref() == Some(&r.id)) {
                c.holds_tasks = true;
            }
        }
        for r in list.iter().filter(|r| r.events) {
            if let Some(c) = self.data.calendars.iter_mut().find(|c| c.account == Some(account) && c.remote_id.as_ref() == Some(&r.id)) {
                c.read_only = r.read_only;
                continue;
            }
            let n = self.data.calendars.len();
            let id = self.data.new_id();
            self.data.calendars.push(Calendar {
                id,
                name: r.name.clone(),
                color: r.color.unwrap_or(EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0),
                visible: true,
                account: Some(account),
                remote_id: Some(r.id.clone()),
                read_only: r.read_only,
                ..Default::default()
            });
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
        // a copy of a synced or subscribed event is a new event of one's own
        e.detach();
        e.calendar = self.data.writable_or_own(e.calendar);
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
        match self.data.next_free_slot(from, e.minutes(), e.task_id.and_then(|t| self.data.task(t))) {
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
        match self.data.next_free_slot(e.end.max(at), rest, e.task_id.and_then(|t| self.data.task(t))) {
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
            Cmd::NewTask => self.open_new_task(0),
            Cmd::NewFrame => {
                // the viewed day, at the next full hour when that is today
                let now = Self::now();
                let start_h = if self.anchor == now.date() { (now.hour() as i64 + 1).min(22) } else { 9 };
                let day = self.anchor;
                self.open_new_frame(day, start_h * 60, (start_h + 2).min(24) * 60, self.data.first_own_calendar(), String::new());
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
                    self.say(ctx, "No subscribed calendars or accounts to refresh", false);
                } else {
                    self.say(ctx, "Refreshing calendars…", false);
                }
            }
            Cmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// The mouse's back and forward buttons (4 and 5) move the view: a week
    /// or a month at a time, and a single day in the day views.
    fn mouse_nav(&mut self, ctx: &egui::Context) {
        let open = self.show_prefs
            || self.editor.is_some()
            || self.task_editor.is_some()
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

    fn keys(&mut self, ctx: &egui::Context) {
        let dialogs = self.editor.is_some()
            || self.task_editor.is_some()
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
            } else if self.frame_editor.take().is_some() {
            } else if self.editor.take().is_some() || self.task_editor.take().is_some() {
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

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        // for screenshots: step this many periods just before the picture,
        // to catch the slide half way
        if self.frames == 116 {
            if let Some(n) = std::env::var("KALENDAR_NAV").ok().and_then(|n| n.parse::<i64>().ok()) {
                self.step(n);
            }
        }
        // the picture arrives later, from the renderer, so it is passed back here
        static PICTURE: std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>> = std::sync::Mutex::new(None);
        if self.frames == 120 {
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                *PICTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(img);
                c.request_repaint();
            });
        }
        let img = PICTURE.lock().unwrap_or_else(|e| e.into_inner()).take();
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

/// The picture-mode drag aid, KALENDAR_DRAG="title@x,y".
fn drag_aid() -> Option<(String, egui::Pos2)> {
    let v = std::env::var("KALENDAR_DRAG").ok()?;
    let (title, at) = v.rsplit_once('@')?;
    let (x, y) = at.split_once(',')?;
    Some((title.to_string(), egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?)))
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some((_, pos)) = self.shot.as_ref().and(drag_aid()) {
            raw_input.events.push(egui::Event::PointerMoved(pos));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        ctx.set_theme(match self.data.prefs.theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
        // the font and the size of everything follow the settings
        let want = (self.data.prefs.font.clone(), self.data.prefs.ui_scale.clamp(0.8, 2.0));
        if self.font_applied.as_ref() != Some(&want) {
            if self.font_applied.as_ref().map(|a| &a.0) != Some(&want.0) {
                fonts::apply(ctx, &want.0);
            }
            ctx.set_zoom_factor(want.1);
            self.font_applied = Some(want);
        }
        self.pal = Palette::new(ctx.global_style().visuals.dark_mode);
        let (bg, side, text) = (self.pal.bg, self.pal.side, self.pal.text);
        ctx.global_style_mut(|s| {
            s.visuals.panel_fill = bg;
            s.visuals.window_fill = side;
            s.visuals.override_text_color = Some(text);
            s.visuals.selection.bg_fill = Color32::from_rgb(0x62, 0x5e, 0xfc);
            s.visuals.selection.stroke.color = Color32::WHITE;
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.animation_time = ANIM_S;
            // the one corner radius, everywhere
            let cr = egui::CornerRadius::same(RADIUS as u8);
            s.visuals.window_corner_radius = cr;
            s.visuals.menu_corner_radius = cr;
            for w in [
                &mut s.visuals.widgets.noninteractive,
                &mut s.visuals.widgets.inactive,
                &mut s.visuals.widgets.hovered,
                &mut s.visuals.widgets.active,
                &mut s.visuals.widgets.open,
            ] {
                w.corner_radius = cr;
            }
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
                "appearance" => Page::Appearance,
                "info" => Page::Info,
                _ => Page::General,
            };
            if let Ok(q) = std::env::var("KALENDAR_SETTINGS_QUERY") {
                self.settings_query = q;
            }
            if let Ok(q) = std::env::var("KALENDAR_SHORTCUT_QUERY") {
                self.shortcut_query = q;
            }
            // for screenshots: which kind of integration is being set up
            if let Some(k) = std::env::var("KALENDAR_SUB_KIND").ok().and_then(|k| k.parse().ok()) {
                self.sub_kind = k;
            }
            if let Ok(u) = std::env::var("KALENDAR_SUB_URL") {
                self.sub_kind = 1;
                self.sub_url = u;
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| self.settings_body(ui));
            self.dlg_toast_public(ctx);
            return self.screenshot(ctx);
        }
        self.keys(ctx);
        self.mouse_nav(ctx);
        // a narrow window shows one day and no task panel; widening it
        // brings back what was there
        // (for screenshots, KALENDAR_NARROW stands in for a narrow window)
        let narrow = ctx.content_rect().width() < COMPACT_W || (self.shot.is_some() && std::env::var("KALENDAR_NARROW").is_ok());
        match (narrow, self.compact) {
            (true, None) => {
                self.compact = Some((self.data.prefs.sidebar_open, self.view));
                self.data.prefs.sidebar_open = false;
                if !matches!(self.view, View::Agenda) {
                    self.view = View::Days(1);
                }
            }
            (false, Some((open, view))) => {
                self.compact = None;
                self.data.prefs.sidebar_open = open;
                self.view = view;
            }
            _ => {}
        }
        self.ui_sidebar(ui);
        self.ui_toolbar(ui);
        // moving in time slides the old page out and the new one in, the way
        // the days run; another view fades in
        let time = ctx.input(|i| i.time);
        let shown = (self.view, self.anchor);
        if let Some(last) = self.last_shown {
            if last != shown {
                let moved = (shown.1 - last.1).num_days();
                self.nav_anim = if last.0 != shown.0 || moved == 0 {
                    self.page_old.clear();
                    (time, 0.0, 1.0)
                } else {
                    // a step shorter than the days on screen moves just that far
                    let part = match shown.0 {
                        View::Days(n) => (moved.abs() as f32 / n.max(1) as f32).min(1.0),
                        _ => 1.0,
                    };
                    self.page_old = std::mem::take(&mut self.page_shot);
                    (time, moved.signum() as f32, part)
                };
            }
        }
        self.last_shown = Some(shown);
        let (began, dir, part) = self.nav_anim;
        let t = (((time - began) as f32) / if dir == 0.0 { ANIM_LONG_S } else { ANIM_SLIDE_S }).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);
        if t < 1.0 {
            ctx.request_repaint();
        } else {
            self.page_old.clear();
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| {
            let full = ui.max_rect();
            ui.set_clip_rect(full);
            let reach = full.width() * part;
            let dx = (dir * (1.0 - ease) * reach).round();
            // the page that is leaving, as it was last painted
            if t < 1.0 && dir != 0.0 {
                let off = egui::vec2(dx - dir * reach, 0.0);
                for cs in &self.page_old {
                    let mut shape = cs.shape.clone();
                    shape.translate(off);
                    ui.painter().with_clip_rect(cs.clip_rect.translate(off).intersect(full)).add(shape);
                }
            }
            let layer = ui.layer_id();
            let first = ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
            if t < 1.0 && dir != 0.0 {
                // the new page covers what it has reached of the old one
                let cover = if dir > 0.0 { full.with_min_x(full.left() + dx) } else { full.with_max_x(full.right() + dx) };
                ui.painter().rect_filled(cover, 0.0, self.pal.bg);
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(full.translate(egui::vec2(dx, 0.0))), |ui| {
                if dir == 0.0 {
                    ui.set_opacity(0.25 + 0.75 * ease);
                }
                match self.view {
                    View::Month => self.ui_month(ui),
                    View::Agenda => self.ui_agenda(ui),
                    _ => self.ui_time_grid(ui),
                }
            });
            // keep what was painted, in its resting place, for the next slide
            self.page_shot = ctx.graphics(|g| {
                g.get(layer).map_or(vec![], |l| {
                    l.all_entries()
                        .skip(first)
                        .map(|cs| {
                            let mut cs = cs.clone();
                            cs.shape.translate(egui::vec2(-dx, 0.0));
                            cs.clip_rect = cs.clip_rect.translate(egui::vec2(-dx, 0.0));
                            cs
                        })
                        .collect()
                })
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
        let drag_aid_on = self.shot.is_some() && drag_aid().is_some();
        if !ctx.input(|i| i.pointer.any_down()) && !drag_aid_on {
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
            // test aid: sign in to a CalDAV server and say what it holds.
            // The password is read from KALENDAR_PASS, so it stays out of
            // the command line; nothing is kept.
            "--check-caldav" => {
                let (Some(server), Some(user)) = (args.next(), args.next()) else {
                    eprintln!("--check-caldav needs a server address and a user name; the password is read from KALENDAR_PASS");
                    std::process::exit(2);
                };
                let pass = std::env::var("KALENDAR_PASS").unwrap_or_default();
                let mut dav = CalDav::new(&server, &user, &pass, None);
                let cals = dav.list_calendars().unwrap_or_else(|e| {
                    eprintln!("{}", e.text());
                    std::process::exit(1);
                });
                println!("{} calendars at {}", cals.len(), dav.home.clone().unwrap_or_default());
                for c in &cals {
                    match dav.pull(c, None, App::now()) {
                        Ok(d) => {
                            let tokens = d.state.as_deref().is_some_and(|s| !s.ends_with('|'));
                            println!("  {}: {} events{}{}", c.name, d.upserts.len(), if c.read_only { ", read-only" } else { "" }, if tokens { "" } else { ", no sync tokens" });
                        }
                        Err(e) => println!("  {}: {}", c.name, e.text()),
                    }
                }
                return;
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
        .with_min_inner_size([360.0, 480.0]);
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
            let _ = &cc;
            Ok(Box::new(App::new(shot)))
        }),
    );
    if let Err(e) = res {
        eprintln!("could not open the window: {e}");
        std::process::exit(127);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with one account whose first calendar takes changes and
    /// holds one synced event (id 500).
    fn synced_app() -> App {
        let mut app = App::new(Some(PathBuf::from("unused.png")));
        app.data.events.clear();
        app.data.accounts.push(Account { id: 900, home: Some("https://x.example/dav/".into()), ..Default::default() });
        app.data.calendars.push(Calendar { id: 901, name: "Synced".into(), visible: true, account: Some(900), remote_id: Some("https://x.example/dav/c/".into()), ..Default::default() });
        app.data.calendars.push(Calendar { id: 902, name: "Locked".into(), visible: true, account: Some(900), read_only: true, ..Default::default() });
        let start = App::now();
        app.data.events.push(Event {
            id: 500,
            title: "Synced".into(),
            start,
            end: start + Duration::hours(1),
            calendar: 901,
            remote: true,
            uid: Some("u1".into()),
            href: Some("https://x.example/dav/c/u1.ics".into()),
            etag: Some("\"1\"".into()),
            ..Default::default()
        });
        app.reshadow();
        app
    }

    #[test]
    fn edits_in_a_synced_calendar_are_marked_to_send() {
        let mut app = synced_app();
        app.commit();
        assert!(!app.data.event(500).unwrap().dirty, "nothing changed yet");
        assert!(app.outbox_changed.is_none());

        // a change
        app.data.event_mut(500).unwrap().title = "Renamed".into();
        app.commit();
        assert!(app.data.event(500).unwrap().dirty);
        assert!(app.outbox_changed.is_some());

        // a new event, and one in a local calendar
        let own = app.data.first_own_calendar();
        app.data.events.push(Event { id: 501, title: "New".into(), calendar: 901, ..Default::default() });
        app.data.events.push(Event { id: 502, title: "Own".into(), calendar: own, ..Default::default() });
        app.commit();
        assert!(app.data.event(501).unwrap().dirty && !app.data.event(502).unwrap().dirty);

        // a copy of a synced event is a new event, not a second name for the old one
        let mut copy = app.data.event(500).unwrap().clone();
        copy.id = 503;
        app.data.events.push(copy);
        app.commit();
        let c = app.data.event(503).unwrap();
        assert!(c.dirty && c.href.is_none() && c.uid.is_none() && !c.remote);

        // what a server sends is not an edit
        app.data.event_mut(500).unwrap().dirty = false;
        app.data.event_mut(500).unwrap().title = "From the server".into();
        app.reshadow();
        app.commit();
        assert!(!app.data.event(500).unwrap().dirty);

        // a deletion is noted with the version it was read at
        app.data.events.retain(|e| e.id != 500);
        app.commit();
        assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "https://x.example/dav/c/u1.ics".into(), etag: Some("\"1\"".into()) }]);
        app.commit();
        assert_eq!(app.data.outbox.len(), 1, "noted once");
    }

    #[test]
    fn moving_an_event_out_of_a_synced_calendar_deletes_it_there() {
        let mut app = synced_app();
        let own = app.data.first_own_calendar();
        app.data.event_mut(500).unwrap().calendar = own;
        app.commit();
        let e = app.data.event(500).unwrap();
        assert!(!e.dirty && !e.remote && e.href.is_none());
        assert_eq!(app.data.outbox.len(), 1);
        // and back in: it is a new event for the server
        app.data.event_mut(500).unwrap().calendar = 901;
        app.commit();
        let e = app.data.event(500).unwrap();
        assert!(e.dirty && e.href.is_none());
    }

    #[test]
    fn task_edits_in_a_synced_list_are_marked_to_send() {
        let ctx = egui::Context::default();
        let mut app = synced_app();
        app.data.tasks.clear();
        app.data.lists.push(TaskList { id: 950, name: "Synced".into(), account: Some(900), remote_id: Some("https://x.example/dav/t/".into()), ..Default::default() });
        // what the server sends comes in without being marked
        let ft = |uid: &str, title: &str, done: bool| ics::FeedTask { uid: uid.into(), title: title.into(), done, href: Some(format!("https://x.example/dav/t/{uid}.ics")), etag: Some("\"1\"".into()), ..Default::default() };
        app.data.apply_tasks(950, sync::Delta { tasks: vec![ft("a", "A", false), ft("b", "B", false)], full: true, state: Some("s".into()), ..Default::default() });
        app.reshadow();
        app.commit();
        assert_eq!(app.data.tasks.len(), 2);
        assert!(app.data.tasks.iter().all(|t| !t.dirty && t.list == 950));
        let (a, b) = (app.data.tasks[0].id, app.data.tasks[1].id);

        // ticking one off marks it; a new one is marked too; a task elsewhere is not
        app.data.task_mut(a).unwrap().done = true;
        app.data.tasks.push(Task { id: 960, title: "New".into(), list: 950, ..Default::default() });
        app.data.tasks.push(Task { id: 961, title: "Own".into(), list: 1, ..Default::default() });
        app.commit();
        assert!(app.data.task(a).unwrap().dirty && app.data.task(960).unwrap().dirty && !app.data.task(961).unwrap().dirty && !app.data.task(b).unwrap().dirty);
        assert!(app.outbox_changed.is_some());

        // a pull does not write over the edit, and brings the rest up to date
        app.data.apply_tasks(950, sync::Delta { tasks: vec![ft("a", "A from the server", false), ft("b", "B renamed", false)], changed: vec!["https://x.example/dav/t/a.ics".into(), "https://x.example/dav/t/b.ics".into()], state: Some("s2".into()), ..Default::default() });
        assert!(app.data.task(a).unwrap().done && app.data.task(a).unwrap().title == "A");
        assert_eq!(app.data.task(b).unwrap().title, "B renamed");
        assert!(app.data.task(960).is_some(), "a task not sent yet stays through a pull");

        // saved: the flag goes; a conflict keeps the edit in the Inbox
        let sent = app.data.task(a).unwrap().clone();
        app.take_push_results(&ctx, 950, vec![sync::PushResult::TaskSaved { sent, uid: "a".into(), href: "https://x.example/dav/t/a.ics".into(), etag: Some("\"2\"".into()) }]);
        assert!(!app.data.task(a).unwrap().dirty && app.data.task(a).unwrap().etag.as_deref() == Some("\"2\""));
        app.reshadow();
        app.data.task_mut(b).unwrap().title = "My B".into();
        app.commit();
        app.take_push_results(&ctx, 950, vec![sync::PushResult::TaskConflict { task: b }]);
        let copy = app.data.tasks.last().unwrap();
        assert_eq!((copy.title.as_str(), copy.list, copy.href.is_none()), ("My B", 0, true));
        assert!(!app.data.task(b).unwrap().dirty);
        app.reshadow();

        // deleting one is noted; moving one out deletes it on the server and makes it one's own
        app.data.tasks.retain(|t| t.id != a);
        app.data.task_mut(b).unwrap().list = 1;
        app.commit();
        let hrefs: Vec<&str> = app.data.outbox.iter().map(|d| d.href.as_str()).collect();
        assert!(hrefs.contains(&"https://x.example/dav/t/a.ics") && hrefs.contains(&"https://x.example/dav/t/b.ics"), "{hrefs:?}");
        assert!(app.data.task(b).unwrap().href.is_none() && !app.data.task(b).unwrap().dirty);

        // a full pull drops what the server no longer has, but not what was never sent
        app.data.outbox.clear();
        app.data.apply_tasks(950, sync::Delta { tasks: vec![], full: true, ..Default::default() });
        let left: Vec<u64> = app.data.tasks.iter().filter(|t| t.list == 950).map(|t| t.id).collect();
        assert_eq!(left, vec![960]);

        // taking the list away here says nothing to the server
        app.reshadow();
        app.data.tasks.push(Task { id: 970, title: "Kept on the server".into(), list: 950, href: Some("h".into()), uid: Some("k".into()), ..Default::default() });
        app.reshadow();
        app.data.remove_synced_list(950);
        app.commit();
        assert!(app.data.outbox.is_empty() && app.data.tasks.iter().all(|t| t.list != 950));
    }

    #[test]
    fn locked_events() {
        let mut app = synced_app();
        assert!(app.data.locked(app.data.event(500).unwrap()).is_none());
        app.data.event_mut(500).unwrap().recurrence_id = Some("20261116T080000Z".into());
        assert!(app.data.locked(app.data.event(500).unwrap()).is_some(), "an occurrence of a repeating event");
        app.data.event_mut(500).unwrap().recurrence_id = None;
        app.data.event_mut(500).unwrap().calendar = 902;
        assert!(app.data.locked(app.data.event(500).unwrap()).is_some(), "a calendar the server does not let one write");
        assert_eq!(app.data.writable_or_own(902), app.data.first_own_calendar());
        assert_eq!(app.data.writable_or_own(901), 901);
    }

    #[test]
    fn push_results_are_taken_in() {
        let ctx = egui::Context::default();
        let mut app = synced_app();
        // saved: the server's name and version are kept, and the flag goes
        app.data.events.push(Event { id: 501, title: "New".into(), calendar: 901, ..Default::default() });
        app.commit();
        let sent = app.data.event(501).unwrap().clone();
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: sent.clone(), uid: "n".into(), href: "h".into(), etag: Some("e".into()) }]);
        let e = app.data.event(501).unwrap();
        assert!(!e.dirty && e.remote && e.href.as_deref() == Some("h") && e.etag.as_deref() == Some("e"));
        // changed again while on its way: it stays marked
        app.data.event_mut(501).unwrap().title = "Newer".into();
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent, uid: "n".into(), href: "h".into(), etag: Some("e2".into()) }]);
        assert!(app.data.event(501).unwrap().dirty);

        // a conflict: the edit is kept as a copy in one's own calendar
        app.data.event_mut(500).unwrap().title = "My edit".into();
        app.commit();
        let before = app.data.events.len();
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Conflict { event: 500 }]);
        assert_eq!(app.data.events.len(), before + 1);
        let copy = app.data.events.last().unwrap();
        assert_eq!((copy.title.as_str(), copy.calendar, copy.href.is_none()), ("My edit", app.data.first_own_calendar(), true));
        assert!(!app.data.event(500).unwrap().dirty, "the server's copy may now come in");
        assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("Conflict")));
        // the server's copy then replaces the original
        let mut f = ics::FeedEvent { title: "Server's".into(), start: App::now(), end: App::now(), all_day: false, notes: String::new(), uid: "u1".into(), recurrence_id: None, href: Some("https://x.example/dav/c/u1.ics".into()), etag: Some("\"2\"".into()) };
        app.data.apply_delta(901, vec![f.clone()], &[]);
        assert_eq!(app.data.event(500).unwrap().title, "Server's");
        // but an edit not yet sent is not written over by a pull
        app.data.event_mut(500).unwrap().title = "Mine again".into();
        app.data.event_mut(500).unwrap().dirty = true;
        f.title = "Server's, later".into();
        app.data.apply_delta(901, vec![f], &[]);
        assert_eq!(app.data.event(500).unwrap().title, "Mine again");

        // an event that was removed here while it was being written: the
        // server's fresh copy is deleted too, at its new version
        let mut gone = app.data.event(501).unwrap().clone();
        gone.id = 777;
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: gone.clone(), uid: "g".into(), href: "gone".into(), etag: Some("1".into()) }]);
        assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "gone".into(), etag: Some("1".into()) }]);
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: gone, uid: "g".into(), href: "gone".into(), etag: Some("2".into()) }]);
        assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "gone".into(), etag: Some("2".into()) }]);
        assert!(app.outbox_changed.is_some());

        // deletions leave the outbox either way
        app.data.outbox = vec![PendingDelete { calendar: 901, href: "a".into(), etag: None }, PendingDelete { calendar: 901, href: "b".into(), etag: None }];
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Deleted { href: "a".into() }, sync::PushResult::DeleteConflict { href: "b".into() }]);
        assert!(app.data.outbox.is_empty());
        // a failure keeps the event marked and shows on the calendar
        app.take_push_results(&ctx, 901, vec![sync::PushResult::Failed { error: sync::SyncError::RateLimited }]);
        assert!(app.data.calendar(901).unwrap().error.is_some() && app.data.event(500).unwrap().dirty);
    }

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
