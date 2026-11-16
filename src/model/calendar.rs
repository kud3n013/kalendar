//! Calendars, the accounts they come from, and their events.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Calendar {
    /// stable identity; events and frames point at it. 0 only in data
    /// files written before ids existed, until `Data::migrate` runs
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    /// link of a subscribed calendar; its events are read-only
    #[serde(default)]
    pub url: Option<String>,
    /// when it was last fetched, as text
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// the synced account it belongs to; None for own and subscribed ones
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<u64>,
    /// its address or id on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_id: Option<String>,
    /// the server's sync token, to ask only for what changed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctag: Option<String>,
    /// its events cannot be changed here
    #[serde(default)]
    pub read_only: bool,
    /// on its server it can hold tasks as well as events
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub holds_tasks: bool,
    /// its link is private and kept in the keyring; data.json holds
    /// `SECRET_URL` in its place
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub url_secret: bool,
}

/// What data.json holds for a link that is kept in the keyring.
pub const SECRET_URL: &str = "keyring:";

/// A calendar server the person signed in to. Its password is in the
/// keyring, under the account's id.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Account {
    pub id: u64,
    /// the address as typed
    pub server: String,
    pub username: String,
    /// where the calendars were found
    pub home: Option<String>,
    pub last_sync: Option<String>,
    pub error: Option<String>,
    /// a Google account, signed in through the browser; the keyring holds
    /// its refresh token and client secret
    pub google: bool,
    /// a Microsoft account, reached through Microsoft Graph; the keyring
    /// holds its refresh token
    pub microsoft: bool,
    /// the client ID the person brought, for Google or Microsoft
    pub client_id: Option<String>,
    /// signed in through Nextcloud's own page; its app password can be
    /// taken back on signing out
    pub nextcloud: bool,
    /// a Proton account, read-only; the keyring holds its session and
    /// what opens its keys, never the password
    pub proton: bool,
}

/// An event deleted here that the server still has.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct PendingDelete {
    pub calendar: u64,
    pub href: String,
    pub etag: Option<String>,
}

impl Account {
    /// "name @ server" for lists.
    pub fn label(&self) -> String {
        if self.google || self.microsoft || self.proton {
            return self.username.clone();
        }
        let host = self.server.split("://").last().unwrap_or("").split('/').next().unwrap_or("");
        format!("{} @ {}", self.username, host)
    }
}

impl Calendar {
    /// Kept on this computer only: not subscribed and not synced.
    pub fn is_local(&self) -> bool {
        self.url.is_none() && self.account.is_none()
    }
    /// A synced calendar whose server takes changes.
    pub fn is_synced_writable(&self) -> bool {
        self.account.is_some() && !self.read_only
    }
    /// Events can be made and changed in it.
    pub fn writable(&self) -> bool {
        self.is_local() || self.is_synced_writable()
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Event {
    pub id: u64,
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    /// id of its calendar
    pub calendar: u64,
    pub notes: String,
    pub task_id: Option<u64>,
    /// own colour, chosen from the right-click menu
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// came from a subscribed calendar
    #[serde(default)]
    pub remote: bool,
    /// UID of the event on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    /// which occurrence of a repeating event this is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_id: Option<String>,
    /// where it lives on the server
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// the event as the server sent it, so nothing is lost when writing back
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// changed here and not yet sent to the server
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
}

impl Event {
    /// Forgets where the event lives on a server: it becomes a new event
    /// of one's own.
    /// Takes what the event window edits from `n`. What came in from the
    /// server while the window was open (its version) and what is set
    /// elsewhere (a colour) stay.
    pub fn take_edits(&mut self, n: Event) {
        self.title = n.title;
        self.start = n.start;
        self.end = n.end;
        self.all_day = n.all_day;
        self.calendar = n.calendar;
        self.notes = n.notes;
    }
    pub fn detach(&mut self) {
        self.remote = false;
        self.dirty = false;
        (self.uid, self.recurrence_id, self.href, self.etag, self.raw) = (None, None, None, None, None);
    }
    /// The parts a server keeps are the same in both.
    pub fn same_content(&self, o: &Event) -> bool {
        self.title == o.title && self.start == o.start && self.end == o.end && self.all_day == o.all_day && self.notes == o.notes
    }
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }
}

/// Names an event on a server.
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteKey {
    /// UID and, for one occurrence of a repeating event, which one
    Uid(String, Option<String>),
    /// address of the whole event, with all its occurrences
    Href(String),
}

impl RemoteKey {
    pub fn matches(&self, e: &Event) -> bool {
        match self {
            RemoteKey::Uid(uid, rid) => e.uid.as_ref() == Some(uid) && &e.recurrence_id == rid,
            RemoteKey::Href(h) => e.href.as_ref() == Some(h),
        }
    }
}
