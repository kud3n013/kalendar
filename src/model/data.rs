//! Everything the app keeps, how it is saved and loaded, and the changes
//! that come in from servers.

use crate::strings::tr;
use super::*;
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

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
/// $XDG_DATA_HOME/katlendar/data.json, usually ~/.local/share/katlendar/.
pub fn data_path() -> PathBuf {
    let base = portable_home().map(|h| h.join(".local/share")).unwrap_or_else(|| {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."))
    });
    own_dir(&base).join("data.json")
}

/// The app's folder under `base`. The app used to be called kalendar, so a
/// folder left under that name is moved over the first time it is needed.
fn own_dir(base: &Path) -> PathBuf {
    let dir = base.join("katlendar");
    let old = base.join("kalendar");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

/// Where the settings are kept:
/// $XDG_CONFIG_HOME/katlendar/config.json, usually ~/.config/katlendar/.
pub fn config_path() -> PathBuf {
    let base = portable_home().map(|h| h.join(".config")).unwrap_or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
    });
    own_dir(&base).join("config.json")
}

#[cfg(test)]
#[test]
fn old_folder_moves_to_the_new_name() {
    let base = std::env::temp_dir().join(format!("katlendar-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("kalendar")).unwrap();
    std::fs::write(base.join("kalendar/data.json"), "{}").unwrap();
    let dir = own_dir(&base);
    assert_eq!(std::fs::read_to_string(dir.join("data.json")).unwrap(), "{}");
    assert!(!base.join("kalendar").exists());
    // a folder already under the new name is left alone
    std::fs::create_dir_all(base.join("kalendar")).unwrap();
    assert_eq!(own_dir(&base), dir);
    assert!(base.join("kalendar").exists());
    let _ = std::fs::remove_dir_all(&base);
}

pub(super) fn write_json(p: PathBuf, s: String) {
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
    /// An event's own colour, else for a task's block its list's colour,
    /// else its calendar's, else the main colour. The Inbox is no list
    /// here: a task in it takes its calendar's colour.
    pub fn event_color(&self, e: &Event) -> [u8; 3] {
        e.color.unwrap_or_else(|| self.block_color(e.task_id.and_then(|t| self.task(t)), e.calendar))
    }
    /// The colour of a task's block in a calendar: the task's own, else its
    /// list's, unless it is in the Inbox, else the calendar's.
    pub fn block_color(&self, task: Option<&Task>, cal: u64) -> [u8; 3] {
        task.and_then(|t| t.color).unwrap_or_else(|| self.list_color(task, cal))
    }
    /// A task's block colour without its own: its list's, else the calendar's.
    pub fn list_color(&self, task: Option<&Task>, cal: u64) -> [u8; 3] {
        task.and_then(|t| self.lists.iter().find(|l| l.id == t.list && t.list != 0)).map(|l| l.color).unwrap_or_else(|| self.calendar_color(cal))
    }
    /// The main colour picked under Appearance, else the app's own.
    pub fn main_color(&self) -> [u8; 3] {
        self.prefs.main_color.unwrap_or(MAIN_COLOR)
    }
    /// Reads a data file, bringing an older one up to date.
    pub fn from_json(text: &str) -> Option<Data> {
        let mut d: Data = serde_json::from_str(text).ok()?;
        d.migrate();
        d.fold_block_notes();
        Some(d)
    }
    /// A block shows its task's name and note. Older blocks could hold a
    /// note of their own: it is added to the task's, so nothing is lost
    /// when the blocks are brought in line.
    pub fn fold_block_notes(&mut self) {
        let mut extra: Vec<(u64, String)> = vec![];
        for e in &self.events {
            let (Some(tid), own) = (e.task_id, e.notes.trim()) else { continue };
            let Some(t) = self.task(tid) else { continue };
            let have = extra.iter().filter(|(id, _)| *id == tid).any(|(_, n)| n.contains(own));
            if !own.is_empty() && !t.notes.contains(own) && !have {
                extra.push((tid, own.to_string()));
            }
        }
        for (tid, own) in extra {
            if let Some(t) = self.task_mut(tid) {
                t.notes = if t.notes.trim().is_empty() { own } else { format!("{}\n\n{}", t.notes.trim_end(), own) };
            }
        }
        let ids: Vec<u64> = self.tasks.iter().map(|t| t.id).collect();
        for id in ids {
            self.sync_blocks(id);
        }
    }
    /// Gives every block of a task the task's name and note.
    pub fn sync_blocks(&mut self, task: u64) {
        let Some((title, notes)) = self.task(task).map(|t| (t.title.clone(), t.notes.clone())) else { return };
        for e in self.events.iter_mut().filter(|e| e.task_id == Some(task)) {
            if e.title != title {
                e.title = title.clone();
            }
            if e.notes != notes {
                e.notes = notes.clone();
            }
        }
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
            Some(tr("This event comes from a subscribed calendar and can't be changed here"))
        } else if c.account.is_some() && c.read_only {
            Some(tr("This calendar is read-only on its server, so the event can't be changed here"))
        } else if e.remote && e.recurrence_id.is_some() {
            Some(tr("Repeating events from an account can't be changed here yet"))
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
    /// The calendar new events start in: the one picked under Active
    /// calendars while it can take them, else the first of one's own.
    pub fn default_calendar(&self) -> u64 {
        self.prefs.default_calendar.filter(|id| self.calendar(*id).is_some_and(|c| c.writable())).unwrap_or_else(|| self.first_own_calendar())
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
            // a block's name and note are its task's
            if let Some(tid) = e.task_id {
                let (title, notes) = (e.title.clone(), e.notes.clone());
                if let Some(t) = self.task_mut(tid) {
                    if !title.trim().is_empty() {
                        t.title = title;
                    }
                    t.notes = notes;
                }
                self.sync_blocks(tid);
            }
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
            let id = t.id;
            self.sync_blocks(id);
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
    pub fn calendar_color(&self, id: u64) -> [u8; 3] {
        self.calendar(id).map(|c| c.color).unwrap_or_else(|| self.main_color())
    }
    /// Archives the completed tasks; returns how many.
    pub fn archive_done(&mut self) -> usize {
        let mut n = 0;
        for t in self.tasks.iter_mut().filter(|t| t.done && !t.archived) {
            t.archived = true;
            n += 1;
        }
        n
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

}
