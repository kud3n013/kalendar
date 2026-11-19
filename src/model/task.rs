//! Tasks and the lists they are kept in.

use crate::strings::tr;
use super::*;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

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
    /// Put away from the Completed list; still kept and synced.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
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
            Repeat::None => tr("No repeat"),
            Repeat::Daily => tr("Every day"),
            Repeat::Weekdays => tr("Every weekday"),
            Repeat::Weekly => tr("Every week"),
            Repeat::Monthly => tr("Every month"),
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
    /// A first last day for a repeating event that starts on `day`: four
    /// weeks on, twelve weeks for a weekly one, a year for a monthly one.
    pub fn first_until(&self, day: NaiveDate) -> NaiveDate {
        match self {
            Repeat::Weekly => day + Duration::weeks(12),
            Repeat::Monthly => day.checked_add_months(chrono::Months::new(12)).unwrap_or(day),
            _ => day + Duration::weeks(4),
        }
    }
    /// The days after `day`, up to and with `until`, on which an event
    /// that starts on `day` comes again. A monthly one keeps its day of the
    /// month where the month has it; at most 200 days.
    pub fn days_after(&self, day: NaiveDate, until: NaiveDate) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        if *self == Repeat::None {
            return out;
        }
        let mut d = day;
        for i in 1..200u32 {
            d = match self {
                Repeat::Monthly => match day.checked_add_months(chrono::Months::new(i)) {
                    Some(n) => n,
                    None => break,
                },
                r => r.next(d),
            };
            if d > until {
                break;
            }
            out.push(d);
        }
        out
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct TaskList {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    /// a Lucide glyph shown in place of the ring; empty for the ring
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
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

pub const INBOX_COLOR: [u8; 3] = [0x35, 0x85, 0xe1];

impl Data {
    /// A list's name, colour and icon as kept; list 0 is the Inbox, whose
    /// name is empty until it is renamed.
    pub fn list_look(&self, id: u64) -> (String, [u8; 3], String) {
        match self.lists.iter().find(|l| l.id == id) {
            Some(l) => (l.name.clone(), l.color, l.icon.clone()),
            None => (self.prefs.inbox_name.clone(), self.prefs.inbox_color.unwrap_or(INBOX_COLOR), self.prefs.inbox_icon.clone()),
        }
    }
    pub fn set_list_look(&mut self, id: u64, look: (String, [u8; 3], String)) {
        if let Some(l) = self.lists.iter_mut().find(|l| l.id == id) {
            (l.name, l.color, l.icon) = look;
        } else if id == 0 {
            self.prefs.inbox_name = look.0;
            self.prefs.inbox_color = Some(look.1).filter(|c| *c != INBOX_COLOR);
            self.prefs.inbox_icon = look.2;
        }
    }
    /// Removes one of one's own lists; its tasks go to the Inbox.
    pub fn remove_own_list(&mut self, id: u64) {
        self.lists.retain(|l| l.id != id);
        for t in self.tasks.iter_mut().filter(|t| t.list == id) {
            t.list = 0;
        }
        for f in self.frames.iter_mut() {
            f.filter.lists.retain(|l| *l != id);
        }
    }
}

impl TaskList {
    /// A synced list whose server takes changes.
    pub fn is_synced_writable(&self) -> bool {
        self.account.is_some() && !self.read_only
    }
}

impl Task {
    /// Forgets where the task lives on a server: it becomes a new one.
    /// Takes what the task window edits from `n`. What came in from the
    /// server while the window was open (its version) stays.
    pub fn take_edits(&mut self, n: Task) {
        self.title = n.title;
        self.notes = n.notes;
        self.due = n.due;
        self.priority = n.priority;
        self.duration_min = n.duration_min;
        self.done = n.done;
        self.flexible = n.flexible;
        self.list = n.list;
        self.tags = n.tags;
        self.earliest = n.earliest;
        self.repeat = n.repeat;
    }
    pub fn detach(&mut self) {
        self.dirty = false;
        (self.uid, self.href, self.etag, self.raw) = (None, None, None, None);
    }
    /// The parts a server keeps are the same in both.
    pub fn same_content(&self, o: &Task) -> bool {
        self.title == o.title && self.notes == o.notes && self.due == o.due && self.priority == o.priority && self.done == o.done
    }
}
