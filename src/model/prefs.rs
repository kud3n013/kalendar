//! The settings and the views they choose between.

use crate::strings::{tr, trf};
use super::*;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

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
            View::Agenda => tr("Agenda").into(),
            View::Days(1) => "1 day".into(),
            View::Days(n) => trf("{} days", &[&n]),
            View::Week => tr("Week").into(),
            View::TwoWeeks => "2 weeks".into(),
            View::Month => tr("Month").into(),
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
    /// font family for all text; empty follows the system
    pub font: String,
    /// size of everything on screen, 1.0 being the usual
    pub ui_scale: f32,
    /// slides, glides and fades; off, everything is just there
    pub animations: bool,
    /// how the Inbox looks: its name (empty for "Inbox"), colour and icon
    pub inbox_name: String,
    pub inbox_color: Option<[u8; 3]>,
    pub inbox_icon: String,
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
            font: String::new(),
            ui_scale: 1.0,
            animations: true,
            inbox_name: String::new(),
            inbox_color: None,
            // the inbox tray
            inbox_icon: crate::icons::LIST_ICONS[0].to_string(),
            show_scheduled_tasks: false,
            undo_grace_s: 10,
            auto_due: true,
            due_move: DueMove::Ask,
            default_task_min: 30,
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
    pub fn inbox_name(&self) -> String {
        let n = self.inbox_name.trim();
        if n.is_empty() { tr("Inbox").to_string() } else { n.to_string() }
    }
    pub fn hour_height(&self) -> f32 {
        if let Some(h) = self.hour_height_px {
            // not rounded: a zoom passes through the heights in between
            return h.clamp(HOUR_H_MIN, HOUR_H_MAX);
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
