//! Data, preferences, saving and the simple task planner.

mod buffer;
mod calendar;
mod data;
mod frame;
mod plan;
mod prefs;
mod quick;
mod sample;
mod tags;
mod task;
#[cfg(test)]
mod tests;

pub use buffer::*;
pub use calendar::*;
pub use data::*;
pub use frame::*;
pub use prefs::*;
pub use quick::*;
pub use task::*;

use chrono::{NaiveDateTime, Timelike};

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
pub const PLAN_OWN_FRAME_DAYS: i64 = 15; // how far ahead a task's own frame is looked for
/// Event colours, in menu order.
/// The app's own main colour, as theme::ACCENT.
pub const MAIN_COLOR: [u8; 3] = [0x62, 0x5e, 0xfc];
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
pub const SYNC_EVERY_S: f64 = 900.0; // subscribed and synced calendars refresh every 15 minutes
pub const PUSH_AFTER_S: f64 = 2.0; // a change made here is sent to its server this much later
pub const NEXTCLOUD_WAIT_S: u64 = 300; // how long a browser sign-in is waited for
pub const SYNC_ON_FOCUS_S: f64 = 60.0; // coming back to the window refreshes, at most this often
pub const NEW_EVENT_MIN: i64 = 60; // new events are one hour long
pub const NEW_EVENT_NOON_H: i64 = 12; // on a later day they start at 12:00

/// Minutes from midnight the grid opens at: 90 minutes before now, rounded
/// down to a quarter of an hour.
pub fn opening_scroll_min(now: NaiveDateTime) -> i64 {
    let e = (now.hour() as i64 * 60 + now.minute() as i64 - SCROLL_BACK_MIN).max(0);
    e - e % SCROLL_ROUND_MIN
}
