//! Calendars kept in step with a server.

pub mod caldav;

use crate::ics::FeedEvent;
use crate::model::RemoteKey;
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
}

/// What a pull brought back.
#[derive(Clone, Debug, Default)]
pub struct Delta {
    /// new and changed events
    pub upserts: Vec<FeedEvent>,
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
    /// Writes an event; `etag` is the version it was read at. Returns the
    /// new version.
    // writing comes with the next step of the sync plan
    #[allow(dead_code)]
    fn put(&mut self, _cal: &RemoteCalendar, _href: &str, _ics: &str, _etag: Option<&str>) -> Result<Option<String>, SyncError> {
        Err(SyncError::Other("This calendar is read-only.".into()))
    }
    #[allow(dead_code)]
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
        Ok(vec![RemoteCalendar { id: self.url.clone(), name: crate::ics::default_name(&self.url), read_only: true, ..Default::default() }])
    }
    fn pull(&mut self, _cal: &RemoteCalendar, _state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        let feed = crate::ics::fetch(&self.url, now).map_err(SyncError::Other)?;
        Ok(Delta { upserts: feed.events, full: true, name: feed.name, ..Default::default() })
    }
}
