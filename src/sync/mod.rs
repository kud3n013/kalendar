//! Calendars kept in step with a server.

pub mod caldav;
pub mod google;
pub mod graph;
pub mod nextcloud;
pub mod oauth;
pub mod proton;
pub mod proton_keys;
pub mod proton_srp;

use crate::ics::{FeedEvent, FeedTask};
use crate::model::{Event, RemoteKey, Task};
use chrono::NaiveDateTime;

/// Why a sync did not go through.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncError {
    /// the server refused the user name or password
    Auth,
    /// the event changed on the server since it was read
    Conflict,
    /// the server no longer knows the sync state; everything is read again
    TokenExpired,
    RateLimited,
    /// what was asked for is not on the server
    Gone,
    Network(String),
    Other(String),
}

impl SyncError {
    /// Words for the person.
    pub fn text(&self) -> String {
        match self {
            SyncError::Auth => "The server refused the user name or password.".into(),
            SyncError::Conflict => "The event was changed on the server in the meantime.".into(),
            SyncError::TokenExpired => "The server asked to read everything again.".into(),
            SyncError::Gone => "Nothing was found at this address.".into(),
            SyncError::RateLimited => "The server is busy; it will be tried again later.".into(),
            SyncError::Network(e) => format!("Could not reach the server ({e})."),
            SyncError::Other(e) => e.clone(),
        }
    }
}

/// A calendar as the server lists it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RemoteCalendar {
    /// its address or id on the server
    pub id: String,
    pub name: String,
    pub color: Option<[u8; 3]>,
    /// changes whenever anything in the calendar does
    pub ctag: Option<String>,
    pub read_only: bool,
    /// it can hold events
    pub events: bool,
    /// it can hold tasks
    pub tasks: bool,
}

/// What a pull brought back.
#[derive(Clone, Debug, Default)]
pub struct Delta {
    /// new and changed events
    pub upserts: Vec<FeedEvent>,
    /// new and changed tasks
    pub tasks: Vec<FeedTask>,
    /// addresses that changed: their events not among `upserts` are gone
    pub changed: Vec<String>,
    pub deletes: Vec<RemoteKey>,
    /// `upserts` is everything the calendar holds
    pub full: bool,
    /// to hand back on the next pull
    pub state: Option<String>,
    pub ctag: Option<String>,
    /// the calendar's own name, when the server tells it
    pub name: Option<String>,
}

/// One way of reaching calendars on a server. All calls block; they run on
/// a background thread.
pub trait Provider: Send {
    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError>;
    /// What changed since `state`, or everything without one. Repeating
    /// events are written out around `now`.
    fn pull(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError>;
    /// The same for the tasks of a list. Not every service has them.
    fn pull_tasks(&mut self, _cal: &RemoteCalendar, _state: Option<&str>, _now: NaiveDateTime) -> Result<Delta, SyncError> {
        Err(SyncError::Other("Tasks are not synced with this kind of account.".into()))
    }
    /// Where the account's calendars were found, to skip the search next time.
    fn home(&self) -> Option<String> {
        None
    }
    /// Forgets where the calendars were found, so they are looked for again.
    fn forget_home(&mut self) {}
    /// Writes an event in the service's own way. None where events travel
    /// as iCalendar text, through `get` and `put`.
    fn save_native(&mut self, _cal: &RemoteCalendar, _e: &Event) -> Option<PushResult> {
        None
    }
    /// An event as the server holds it, with its version.
    fn get(&mut self, _href: &str) -> Result<(String, Option<String>), SyncError> {
        Err(SyncError::Other("This calendar is read-only.".into()))
    }
    /// Writes an event, unless `guard` says the server moved on. Returns
    /// the new version when the server tells it.
    fn put(&mut self, _cal: &RemoteCalendar, _href: &str, _ics: &str, _guard: &Guard) -> Result<Option<String>, SyncError> {
        Err(SyncError::Other("This calendar is read-only.".into()))
    }
    /// Removes an event, unless it changed since `etag`.
    fn delete(&mut self, _cal: &RemoteCalendar, _href: &str, _etag: Option<&str>) -> Result<(), SyncError> {
        Err(SyncError::Other("This calendar is read-only.".into()))
    }
}

/// A calendar feed behind a link. It can only be read, and always whole.
pub struct IcsFeed {
    pub url: String,
}

impl Provider for IcsFeed {
    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError> {
        Ok(vec![RemoteCalendar { id: self.url.clone(), name: crate::ics::default_name(&self.url), read_only: true, events: true, ..Default::default() }])
    }
    fn pull(&mut self, _cal: &RemoteCalendar, _state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        let feed = crate::ics::fetch(&self.url, now).map_err(SyncError::Other)?;
        Ok(Delta { upserts: feed.events, full: true, name: feed.name, ..Default::default() })
    }
}

/// What must hold on the server for a write to go through.
#[derive(Clone, Debug, PartialEq)]
pub enum Guard {
    /// nothing is at the address yet
    New,
    /// the event is still at this version
    Match(String),
    /// the server gives no versions; the write goes through
    Any,
}

/// A change made here that the server has not seen.
#[derive(Clone, Debug)]
pub enum PushJob {
    /// a new or changed event
    Save(Event),
    /// a new or changed task
    SaveTask(Task),
    /// an event deleted here
    Delete { href: String, etag: Option<String> },
}

/// How one change fared.
#[derive(Clone, Debug)]
pub enum PushResult {
    /// `sent` is the event as it was written
    Saved { sent: Event, uid: String, href: String, etag: Option<String> },
    /// the server's copy changed or went in the meantime; it wins
    Conflict { event: u64 },
    TaskSaved { sent: Task, uid: String, href: String, etag: Option<String> },
    TaskConflict { task: u64 },
    Deleted { href: String },
    /// the event changed on the server before it could be deleted; it stays
    DeleteConflict { href: String },
    /// not written; it is tried again on the next sync
    Failed { error: SyncError },
}

/// An event's notes as the two fields a server keeps: the place, written
/// here as a first line "Where: …", and the description.
pub fn place_and_notes(notes: &str) -> (String, String) {
    match notes.strip_prefix("Where: ") {
        Some(rest) => match rest.split_once('\n') {
            Some((place, more)) => (place.trim().to_string(), more.to_string()),
            None => (rest.trim().to_string(), String::new()),
        },
        None => (String::new(), notes.to_string()),
    }
}

/// A name for a new event that no other event has.
fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    // only letters, digits and dashes: it is also the event's address, and
    // servers hand addresses back with other signs rewritten
    format!("{:x}-{:x}-{:x}-kalendar", nanos, std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

fn save(p: &mut dyn Provider, cal: &RemoteCalendar, e: &Event, stamp: NaiveDateTime) -> PushResult {
    use crate::ics::{read_event, write_event, EventFields};
    let (location, description) = place_and_notes(&e.notes);
    let fill = |f: &mut EventFields| {
        f.summary = e.title.clone();
        f.start = e.start;
        f.end = e.end;
        f.all_day = e.all_day;
        f.description = description.clone();
        f.location = location.clone();
    };
    let done = |res: Result<Option<String>, SyncError>, uid: String, href: String| match res {
        Ok(etag) => PushResult::Saved { sent: e.clone(), uid, href, etag },
        Err(SyncError::Conflict | SyncError::Gone) => PushResult::Conflict { event: e.id },
        Err(error) => PushResult::Failed { error },
    };
    match &e.href {
        // a change: only the parts that can be edited here are rewritten
        Some(href) => {
            let (raw, server_etag) = match p.get(href) {
                Ok(x) => x,
                Err(SyncError::Gone) => return PushResult::Conflict { event: e.id },
                Err(error) => return PushResult::Failed { error },
            };
            let Some((uid, mut f)) = read_event(&raw) else {
                return PushResult::Failed { error: SyncError::Other("The event on the server could not be read.".into()) };
            };
            if f.rrule.is_some() {
                return PushResult::Failed { error: SyncError::Other("Repeating events can't be changed here yet.".into()) };
            }
            fill(&mut f);
            let guard = match e.etag.clone().or(server_etag) {
                Some(t) => Guard::Match(t),
                None => Guard::Any,
            };
            let text = write_event(Some(&raw), &uid, &f, stamp);
            done(p.put(cal, href, &text, &guard), uid, href.clone())
        }
        None => {
            let uid = new_uid();
            let mut f = EventFields::default();
            fill(&mut f);
            let href = format!("{}/{}.ics", cal.id.trim_end_matches('/'), uid);
            let text = write_event(None, &uid, &f, stamp);
            done(p.put(cal, &href, &text, &Guard::New), uid, href)
        }
    }
}

fn save_task(p: &mut dyn Provider, cal: &RemoteCalendar, t: &Task, stamp: NaiveDateTime) -> PushResult {
    use crate::ics::{write_todo, TaskFields};
    let f = TaskFields { title: t.title.clone(), notes: t.notes.clone(), due: t.due, priority: t.priority, done: t.done };
    let done = |res: Result<Option<String>, SyncError>, uid: String, href: String| match res {
        Ok(etag) => PushResult::TaskSaved { sent: t.clone(), uid, href, etag },
        Err(SyncError::Conflict | SyncError::Gone) => PushResult::TaskConflict { task: t.id },
        Err(error) => PushResult::Failed { error },
    };
    match &t.href {
        Some(href) => {
            let (raw, server_etag) = match p.get(href) {
                Ok(x) => x,
                Err(SyncError::Gone) => return PushResult::TaskConflict { task: t.id },
                Err(error) => return PushResult::Failed { error },
            };
            let uid = t.uid.clone().or_else(|| crate::ics::parse_todos(&raw).into_iter().next().map(|x| x.uid)).unwrap_or_default();
            let guard = match t.etag.clone().or(server_etag) {
                Some(e) => Guard::Match(e),
                None => Guard::Any,
            };
            done(p.put(cal, href, &write_todo(Some(&raw), &uid, &f, stamp), &guard), uid, href.clone())
        }
        None => {
            let uid = new_uid();
            let href = format!("{}/{}.ics", cal.id.trim_end_matches('/'), uid);
            done(p.put(cal, &href, &write_todo(None, &uid, &f, stamp), &Guard::New), uid, href)
        }
    }
}

/// Sends the changes made here to the server, one by one. `stamp` is the
/// time now, in UTC.
pub fn push(p: &mut dyn Provider, cal: &RemoteCalendar, jobs: &[PushJob], stamp: NaiveDateTime) -> Vec<PushResult> {
    jobs.iter()
        .map(|job| match job {
            PushJob::Save(e) => match p.save_native(cal, e) {
                Some(r) => r,
                None => save(p, cal, e, stamp),
            },
            PushJob::SaveTask(t) => save_task(p, cal, t, stamp),
            PushJob::Delete { href, etag } => match p.delete(cal, href, etag.as_deref()) {
                // already gone is as good as deleted
                Ok(()) | Err(SyncError::Gone) => PushResult::Deleted { href: href.clone() },
                Err(SyncError::Conflict) => PushResult::DeleteConflict { href: href.clone() },
                Err(error) => PushResult::Failed { error },
            },
        })
        .collect()
}

/// For the tests' stand-in servers: reads one HTTP request and returns its
/// head and body, the body read as far as `Content-Length` says.
#[cfg(test)]
pub(crate) fn read_request(s: &mut impl std::io::Read) -> (String, String) {
    let mut all = vec![];
    let mut buf = [0u8; 8192];
    loop {
        let n = s.read(&mut buf).unwrap_or(0);
        all.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&all).to_string();
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            if n == 0 {
                return (text, String::new());
            }
            continue;
        };
        let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
        if body.len() >= len || n == 0 {
            return (head.to_string(), body.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_split_into_place_and_description() {
        assert_eq!(place_and_notes("Where: Room 4\nBring notes\nand pens"), ("Room 4".into(), "Bring notes\nand pens".into()));
        assert_eq!(place_and_notes("Where: Room 4"), ("Room 4".into(), String::new()));
        assert_eq!(place_and_notes("Just notes"), (String::new(), "Just notes".into()));
        assert_ne!(new_uid(), new_uid());
    }
}
