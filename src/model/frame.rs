//! Frames: recurring stretches of time kept for certain kinds of task.

use super::*;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

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
        let list_ok = f.lists.contains(&t.list);
        let has = |g: &String| t.tags.iter().any(|x| x.to_lowercase() == g.to_lowercase());
        let tags_ok = if f.tags_all { f.tags.iter().all(has) } else { f.tags.iter().any(has) };
        let ok = match (f.lists.is_empty(), f.tags.is_empty()) {
            (true, true) => true,
            (false, true) => list_ok,
            (true, false) => tags_ok,
            // both picked: the task needs both
            (false, false) => list_ok && tags_ok,
        };
        if !ok {
            return false;
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

/// One day's stretch of a frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrameOcc {
    pub frame: u64,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl Data {
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
        !f.skip.contains(&day) && Self::frame_falls_on(f, day)
    }
    /// Whether a day is one of the frame's days, skipped or not.
    pub fn frame_falls_on(f: &Frame, day: NaiveDate) -> bool {
        if !f.enabled || day < f.from || f.until.is_some_and(|u| day > u) {
            return false;
        }
        if f.days & day_bit(day) == 0 {
            return false;
        }
        let anchor = f.from - Duration::days(f.from.weekday().num_days_from_monday() as i64);
        let weeks = (day - anchor).num_days() / 7;
        weeks % f.every_weeks.max(1) as i64 == 0
    }
    /// Frames on a day, on visible calendars of one's own, by start time.
    /// Frames that would run on a day had that day not been skipped.
    pub fn frames_skipped_on(&self, day: NaiveDate) -> Vec<&Frame> {
        self.frames
            .iter()
            .filter(|f| f.skip.contains(&day) && f.end_min > f.start_min && Self::frame_falls_on(f, day) && self.calendar(f.calendar).is_some_and(|c| c.visible && c.is_local()))
            .collect()
    }
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
}
