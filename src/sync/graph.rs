//! Microsoft 365 and Outlook.com calendars, through Microsoft Graph, with
//! a sign-in through the browser. kalendar ships no registration with
//! Microsoft: the person brings the client ID of their own app
//! registration (a public client, without a secret).

use crate::strings::{tr, trf};
use super::oauth::{url_encode, OAuthApp};
use super::{Delta, Provider, PushResult, RemoteCalendar, SyncError};
use crate::ics::{self, FeedEvent};
use crate::model::{Event, RemoteKey};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};

pub const GRAPH: &str = "https://graph.microsoft.com/v1.0";
/// Everything is read again after this many days, so that repeating
/// events are written out further ahead as time passes.
const FULL_EVERY_DAYS: i64 = 30;

/// The registration to sign in with. `hint` fills in the account on
/// Microsoft's page.
pub fn app(client_id: &str, hint: &str) -> OAuthApp {
    let mut extra = vec![];
    if !hint.trim().is_empty() {
        extra.push(("login_hint".to_string(), hint.trim().to_string()));
    }
    OAuthApp {
        // "common" takes work, school and personal accounts alike
        auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
        token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
        client_id: client_id.trim().to_string(),
        client_secret: None,
        // offline_access is what makes Microsoft hand out a refresh token
        scope: tr("Calendars.ReadWrite offline_access").into(),
        extra,
        // Microsoft takes "http://localhost" with any port for desktop programs
        localhost_redirect: true,
    }
}

pub struct Graph {
    /// where Graph is reached; another address only in tests
    pub base: String,
    pub token: String,
}

struct Reply {
    status: u16,
    body: Value,
}

/// A time as Graph writes it, "2026-11-16T15:00:00.0000000", in UTC
/// because that is asked for.
fn parse_time(v: &Value) -> Option<NaiveDateTime> {
    let s = v["dateTime"].as_str()?;
    NaiveDateTime::parse_from_str(s.get(..19)?, "%Y-%m-%dT%H:%M:%S").ok()
}

fn to_local(utc: NaiveDateTime) -> NaiveDateTime {
    Utc.from_utc_datetime(&utc).with_timezone(&Local).naive_local()
}

fn to_utc(local: NaiveDateTime) -> NaiveDateTime {
    Local.from_local_datetime(&local).earliest().map(|t| t.naive_utc()).unwrap_or(local)
}

/// An event out of Graph's JSON. None for what is no event to show.
fn event_of(v: &Value) -> Option<FeedEvent> {
    let id = v["id"].as_str()?.to_string();
    if v["isCancelled"].as_bool() == Some(true) {
        return None;
    }
    let all_day = v["isAllDay"].as_bool().unwrap_or(false);
    let (s, e) = (parse_time(&v["start"])?, parse_time(&v["end"])?);
    // whole days are dates, the same everywhere
    let (start, end) = if all_day { (s, e.max(s + Duration::days(1))) } else { (to_local(s), to_local(e).max(to_local(s) + Duration::minutes(15))) };
    let place = v["location"]["displayName"].as_str().unwrap_or("").trim();
    let text = v["body"]["content"].as_str().unwrap_or("").replace("\r\n", "\n");
    let text = text.trim();
    let notes = match (place.is_empty(), text.is_empty()) {
        (true, _) => text.to_string(),
        (false, true) => format!("Where: {place}"),
        (false, false) => format!("Where: {place}\n{text}"),
    };
    // one day of a repeating event; those cannot be changed here yet
    let repeats = matches!(v["type"].as_str(), Some("occurrence" | "exception" | "seriesMaster")) || v["seriesMasterId"].is_string();
    Some(FeedEvent {
        title: v["subject"].as_str().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Busy").to_string(),
        start,
        end,
        all_day,
        notes,
        uid: id.clone(),
        recurrence_id: repeats.then(|| s.format("%Y%m%dT%H%M%SZ").to_string()),
        href: Some(id),
        etag: v["@odata.etag"].as_str().map(str::to_string),
    })
}

/// An event as the JSON Graph takes. The notes travel only when asked:
/// writing them replaces a formatted text on the server with a plain one.
fn json_of(e: &Event, with_notes: bool) -> Value {
    let (place, text) = super::place_and_notes(&e.notes);
    let stamp = |t: NaiveDateTime| {
        let t = if e.all_day { t.date().and_hms_opt(0, 0, 0).unwrap_or(t) } else { to_utc(t) };
        json!({ "dateTime": t.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": "UTC" })
    };
    let mut v = json!({
        "subject": e.title,
        "start": stamp(e.start),
        "end": stamp(e.end),
        "isAllDay": e.all_day,
        "location": { "displayName": place },
    });
    if with_notes {
        v["body"] = json!({ "contentType": "text", "content": text });
    }
    v
}

impl Graph {
    pub fn new(token: &str) -> Graph {
        Graph { base: GRAPH.into(), token: token.to_string() }
    }

    fn call(&self, method: &str, url: &str, body: Option<&Value>, if_match: Option<&str>) -> Result<Reply, SyncError> {
        // the token only goes to Graph itself
        let local = self.base.starts_with("http://127.0.0.1:");
        if !url.starts_with(&self.base) || !(self.base.starts_with("https://") || local) {
            return Err(SyncError::Other(tr("The server sent an address that is not its own.").into()));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30))).http_status_as_error(false).max_redirects(0).build().into();
        let mut req = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("User-Agent", "kalendar")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Prefer", "outlook.timezone=\"UTC\", outlook.body-content-type=\"text\", odata.maxpagesize=100");
        if let Some(t) = if_match {
            req = req.header("If-Match", t);
        }
        let text = match body {
            Some(b) => {
                req = req.header("Content-Type", "application/json");
                b.to_string()
            }
            None => String::new(),
        };
        let req = req.body(text).map_err(|e| SyncError::Other(e.to_string()))?;
        let mut resp = agent.run(req).map_err(|e| SyncError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().unwrap_or_default();
        let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        match status {
            200..=299 => Ok(Reply { status, body }),
            401 => Err(SyncError::Auth),
            404 => Err(SyncError::Gone),
            // the sync state is no longer known
            410 => Err(SyncError::TokenExpired),
            412 => Err(SyncError::Conflict),
            429 | 503 | 504 => Err(SyncError::RateLimited),
            code => {
                let why = body["error"]["message"].as_str().unwrap_or("no reason given");
                Err(SyncError::Other(trf("Microsoft answered with error {}: {}", &[&code, &why])))
            }
        }
    }

    /// Reads a list that comes in pages, up to the link to go on from.
    fn pages(&self, first: &str) -> Result<(Vec<Value>, Option<String>), SyncError> {
        let (mut items, mut url) = (vec![], first.to_string());
        for _ in 0..500 {
            let r = self.call("GET", &url, None, None)?;
            if let Some(v) = r.body["value"].as_array() {
                items.extend(v.iter().cloned());
            }
            match r.body["@odata.nextLink"].as_str() {
                Some(next) => url = next.to_string(),
                None => return Ok((items, r.body["@odata.deltaLink"].as_str().map(str::to_string))),
            }
        }
        Err(SyncError::Other(tr("The server's list did not end.").into()))
    }

    fn pull_from(&self, url: &str, day: NaiveDate, full: bool) -> Result<Delta, SyncError> {
        let (items, link) = self.pages(url)?;
        let mut d = Delta { full, state: link.map(|l| format!("{}|{l}", day.format("%Y-%m-%d"))), ..Default::default() };
        for v in &items {
            let Some(id) = v["id"].as_str() else { continue };
            match event_of(v).filter(|_| v.get("@removed").is_none()) {
                Some(e) => {
                    d.changed.push(id.to_string());
                    d.upserts.push(e);
                }
                None => d.deletes.push(RemoteKey::Href(id.to_string())),
            }
        }
        Ok(d)
    }
}

impl Provider for Graph {
    fn home(&self) -> Option<String> {
        Some(self.base.clone())
    }

    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError> {
        let (items, _) = self.pages(&format!("{}/me/calendars?$select=id,name,hexColor,canEdit", self.base))?;
        Ok(items
            .iter()
            .filter_map(|v| {
                Some(RemoteCalendar {
                    id: v["id"].as_str()?.to_string(),
                    name: v["name"].as_str().unwrap_or("Calendar").to_string(),
                    color: v["hexColor"].as_str().and_then(|c| {
                        let c = c.trim().strip_prefix('#')?;
                        let p = |i: usize| u8::from_str_radix(c.get(i..i + 2)?, 16).ok();
                        Some([p(0)?, p(2)?, p(4)?])
                    }),
                    ctag: None,
                    events: true,
                    tasks: false,
                    read_only: v["canEdit"].as_bool() == Some(false),
                })
            })
            .collect())
    }

    /// `state` is "day of the last full read|link to go on from". Repeating
    /// events come written out day by day, for the days around `now`.
    fn pull(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        let known = state.and_then(|s| s.split_once('|')).and_then(|(day, link)| {
            let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
            ((now.date() - day).num_days().abs() < FULL_EVERY_DAYS && !link.is_empty()).then_some((day, link))
        });
        if let Some((day, link)) = known {
            match self.pull_from(link, day, false) {
                Err(SyncError::TokenExpired | SyncError::Gone) => {}
                other => return other,
            }
        }
        let stamp = |d: NaiveDateTime| d.format("%Y-%m-%dT00:00:00Z").to_string();
        let (from, to) = (stamp(now - Duration::days(ics::PAST_DAYS + 1)), stamp(now + Duration::days(ics::AHEAD_DAYS + 1)));
        let url = format!("{}/me/calendars/{}/calendarView/delta?startDateTime={from}&endDateTime={to}", self.base, url_encode(&cal.id));
        self.pull_from(&url, now.date(), true)
    }

    fn save_native(&mut self, cal: &RemoteCalendar, e: &Event) -> Option<PushResult> {
        let done = |r: Result<Reply, SyncError>| match r {
            Ok(r) => match r.body["id"].as_str() {
                Some(id) => PushResult::Saved { sent: e.clone(), uid: id.to_string(), href: id.to_string(), etag: r.body["@odata.etag"].as_str().map(str::to_string) },
                None => PushResult::Failed { error: SyncError::Other(trf("Microsoft's answer could not be read ({}).", &[&r.status])) },
            },
            Err(SyncError::Conflict | SyncError::Gone) => PushResult::Conflict { event: e.id },
            Err(error) => PushResult::Failed { error },
        };
        Some(match &e.href {
            None => done(self.call("POST", &format!("{}/me/calendars/{}/events", self.base, url_encode(&cal.id)), Some(&json_of(e, true)), None)),
            Some(id) => {
                let url = format!("{}/me/events/{}", self.base, url_encode(id));
                // the notes are only sent when they were changed here
                let theirs = match self.call("GET", &format!("{url}?$select=id,body"), None, None) {
                    Ok(r) => r.body["body"]["content"].as_str().unwrap_or("").replace("\r\n", "\n").trim().to_string(),
                    Err(SyncError::Gone) => return Some(PushResult::Conflict { event: e.id }),
                    Err(error) => return Some(PushResult::Failed { error }),
                };
                let ours = super::place_and_notes(&e.notes).1;
                done(self.call("PATCH", &url, Some(&json_of(e, ours.trim() != theirs)), e.etag.as_deref()))
            }
        })
    }

    fn delete(&mut self, _cal: &RemoteCalendar, href: &str, etag: Option<&str>) -> Result<(), SyncError> {
        self.call("DELETE", &format!("{}/me/events/{}", self.base, url_encode(href)), None, etag).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    /// A stand-in for Graph: answers the requests it is given in order,
    /// and keeps what it was sent ("METHOD path\nheaders\n\nbody").
    fn stand_in(replies: Vec<(&'static str, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}/v1.0", listener.local_addr().unwrap().port());
        let got = Arc::new(Mutex::new(vec![]));
        let (seen, b) = (got.clone(), base.clone());
        std::thread::spawn(move || {
            for (status, reply) in replies {
                let (mut s, _) = listener.accept().unwrap();
                let (head, body) = super::super::read_request(&mut s);
                seen.lock().unwrap().push(format!("{head}

{body}"));
                let reply = reply.replace("{BASE}", &b);
                let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            }
        });
        (base, got)
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 13).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    #[test]
    fn events_from_json() {
        let v: Value = serde_json::from_str(
            r#"{"id":"A1","@odata.etag":"W/\"x\"","subject":" Review ","isAllDay":false,"type":"singleInstance",
            "start":{"dateTime":"2026-11-16T15:00:00.0000000","timeZone":"UTC"},"end":{"dateTime":"2026-11-16T16:00:00.0000000","timeZone":"UTC"},
            "location":{"displayName":"Room 4"},"body":{"contentType":"text","content":"Bring notes\r\n"}}"#,
        )
        .unwrap();
        let e = event_of(&v).unwrap();
        assert_eq!((e.title.as_str(), e.notes.as_str(), e.uid.as_str(), e.href.as_deref(), e.etag.as_deref()), ("Review", "Where: Room 4\nBring notes", "A1", Some("A1"), Some("W/\"x\"")));
        assert_eq!(e.start, to_local(NaiveDate::from_ymd_opt(2026, 11, 16).unwrap().and_hms_opt(15, 0, 0).unwrap()));
        assert!(e.recurrence_id.is_none());

        // a whole day keeps its date, and one day of a series is marked
        let v: Value = serde_json::from_str(
            r#"{"id":"B","subject":"","isAllDay":true,"type":"occurrence","seriesMasterId":"M",
            "start":{"dateTime":"2026-11-20T00:00:00.0000000","timeZone":"UTC"},"end":{"dateTime":"2026-11-21T00:00:00.0000000","timeZone":"UTC"}}"#,
        )
        .unwrap();
        let e = event_of(&v).unwrap();
        assert_eq!((e.title.as_str(), e.all_day, e.start.date(), e.end.date()), ("Busy", true, NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(), NaiveDate::from_ymd_opt(2026, 11, 21).unwrap()));
        assert!(e.recurrence_id.is_some());
        assert!(event_of(&serde_json::json!({"id":"C","isCancelled":true})).is_none());

        // and back: what is written reads the same
        let ev = Event { title: "Review".into(), start: to_local(parse_time(&json!({"dateTime":"2026-11-16T15:00:00"})).unwrap()), end: to_local(parse_time(&json!({"dateTime":"2026-11-16T16:00:00"})).unwrap()), notes: "Where: Room 4\nBring notes".into(), ..Default::default() };
        let j = json_of(&ev, true);
        assert_eq!((j["start"]["dateTime"].as_str(), j["start"]["timeZone"].as_str(), j["location"]["displayName"].as_str(), j["body"]["content"].as_str()), (Some("2026-11-16T15:00:00"), Some("UTC"), Some("Room 4"), Some("Bring notes")));
        assert!(json_of(&ev, false).get("body").is_none());
    }

    #[test]
    fn lists_and_pulls() {
        let ev = |id: &str, title: &str| format!(r#"{{"id":"{id}","subject":"{title}","isAllDay":false,"start":{{"dateTime":"2026-11-16T15:00:00.0000000","timeZone":"UTC"}},"end":{{"dateTime":"2026-11-16T16:00:00.0000000","timeZone":"UTC"}}}}"#);
        let (base, got) = stand_in(vec![
            ("200 OK", r##"{"value":[{"id":"cal 1","name":"Calendar","hexColor":"#3585e1","canEdit":true},{"id":"c2","name":"Shared","hexColor":"","canEdit":false}]}"##.into()),
            // a first read in two pages
            ("200 OK", format!(r#"{{"value":[{}],"@odata.nextLink":"{{BASE}}/next?p=2"}}"#, ev("a", "A"))),
            ("200 OK", format!(r#"{{"value":[{}],"@odata.deltaLink":"{{BASE}}/delta?t=1"}}"#, ev("b", "B"))),
            // then the changes: one changed, one removed
            ("200 OK", format!(r#"{{"value":[{},{{"id":"b","@removed":{{"reason":"deleted"}}}}],"@odata.deltaLink":"{{BASE}}/delta?t=2"}}"#, ev("a", "A2"))),
            // a state the server forgot: everything is read again
            ("410 Gone", r#"{"error":{"code":"SyncStateNotFound","message":"gone"}}"#.into()),
            ("200 OK", format!(r#"{{"value":[{}],"@odata.deltaLink":"{{BASE}}/delta?t=3"}}"#, ev("a", "A2"))),
        ]);
        let mut g = Graph { base: base.clone(), token: "tok".into() };
        let cals = g.list_calendars().unwrap();
        assert_eq!(cals.len(), 2);
        assert_eq!((cals[0].id.as_str(), cals[0].color, cals[0].read_only, cals[1].color, cals[1].read_only), ("cal 1", Some([0x35, 0x85, 0xe1]), false, None, true));

        let d = g.pull(&cals[0], None, now()).unwrap();
        assert!(d.full);
        assert_eq!(d.upserts.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        let state = d.state.clone().unwrap();
        assert_eq!(state, format!("2026-11-13|{base}/delta?t=1"));

        let d2 = g.pull(&cals[0], Some(&state), now()).unwrap();
        assert!(!d2.full);
        assert_eq!(d2.upserts.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["A2"]);
        assert_eq!(d2.deletes, vec![RemoteKey::Href("b".into())]);

        let d3 = g.pull(&cals[0], d2.state.as_deref(), now()).unwrap();
        assert!(d3.full && d3.upserts.len() == 1);

        // the model follows
        let mut data = crate::model::Data::sample(now().date());
        data.events.clear();
        for delta in [d, d2, d3] {
            data.apply(1, delta);
        }
        assert_eq!(data.events.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["A2"]);

        let seen = got.lock().unwrap();
        assert!(seen[0].starts_with("GET /v1.0/me/calendars?$select="));
        assert!(seen[0].to_lowercase().contains("authorization: bearer tok"));
        assert!(seen[1].starts_with("GET /v1.0/me/calendars/cal%201/calendarView/delta?startDateTime=2026-09-13T00:00:00Z&endDateTime=2027-12-19T00:00:00Z"), "{}", seen[1]);
        assert!(seen[1].contains("outlook.timezone=\"UTC\""));
        assert!(seen[2].starts_with("GET /v1.0/next?p=2") && seen[3].starts_with("GET /v1.0/delta?t=1"));
        // a link that leads elsewhere is never followed with the token
        assert!(g.pull(&cals[0], Some("2026-11-13|https://elsewhere.example/delta"), now()).is_err());
    }

    #[test]
    fn saves_and_deletes() {
        let cal = RemoteCalendar { id: "c".into(), ..Default::default() };
        let start = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap().and_hms_opt(9, 0, 0).unwrap();
        let new = Event { id: 5, title: "Mine".into(), start, end: start + Duration::hours(1), notes: "Bring notes".into(), ..Default::default() };
        let (base, got) = stand_in(vec![
            ("201 Created", r#"{"id":"N1","@odata.etag":"W/\"1\""}"#.into()),
            // a change whose notes are as on the server
            ("200 OK", r#"{"id":"N1","body":{"contentType":"text","content":"Bring notes\r\n"}}"#.into()),
            ("200 OK", r#"{"id":"N1","@odata.etag":"W/\"2\""}"#.into()),
            // a change from a stale copy
            ("200 OK", r#"{"id":"N1","body":{"content":"Other notes"}}"#.into()),
            ("412 Precondition Failed", r#"{"error":{"code":"ErrorIrresolvableConflict","message":"changed"}}"#.into()),
            // a change to what is gone, and deletions
            ("404 Not Found", r#"{"error":{"code":"ErrorItemNotFound","message":"gone"}}"#.into()),
            ("204 No Content", String::new()),
            ("404 Not Found", "{}".into()),
            ("412 Precondition Failed", "{}".into()),
        ]);
        let mut g = Graph { base, token: "tok".into() };
        let stamp = now();
        let r = crate::sync::push(&mut g, &cal, &[crate::sync::PushJob::Save(new.clone())], stamp);
        let PushResult::Saved { href, etag, .. } = r[0].clone() else { panic!("{r:?}") };
        assert_eq!((href.as_str(), etag.as_deref()), ("N1", Some("W/\"1\"")));

        let kept = Event { href: Some(href.clone()), etag, remote: true, title: "Mine, moved".into(), ..new.clone() };
        let r = crate::sync::push(&mut g, &cal, &[crate::sync::PushJob::Save(kept.clone())], stamp);
        assert!(matches!(&r[0], PushResult::Saved { etag: Some(t), .. } if t == "W/\"2\""), "{r:?}");
        let r = crate::sync::push(&mut g, &cal, &[crate::sync::PushJob::Save(kept.clone())], stamp);
        assert!(matches!(r[0], PushResult::Conflict { event: 5 }), "{r:?}");
        let r = crate::sync::push(&mut g, &cal, &[crate::sync::PushJob::Save(kept.clone())], stamp);
        assert!(matches!(r[0], PushResult::Conflict { event: 5 }), "{r:?}");
        let del = |etag: &str| crate::sync::PushJob::Delete { href: "N1".into(), etag: Some(etag.into()) };
        let r = crate::sync::push(&mut g, &cal, &[del("W/\"2\""), del("W/\"2\""), del("old")], stamp);
        assert!(matches!(r[0], PushResult::Deleted { .. }) && matches!(r[1], PushResult::Deleted { .. }) && matches!(r[2], PushResult::DeleteConflict { .. }), "{r:?}");

        let seen = got.lock().unwrap();
        assert!(seen[0].starts_with("POST /v1.0/me/calendars/c/events") && seen[0].contains("\"subject\":\"Mine\"") && seen[0].contains("\"content\":\"Bring notes\""));
        assert!(seen[2].starts_with("PATCH /v1.0/me/events/N1") && seen[2].to_lowercase().contains("if-match: w/\"1\""));
        assert!(!seen[2].contains("\"body\""), "unchanged notes are not sent");
        assert!(seen[4].contains("\"body\""), "changed notes are");
        assert!(seen[6].starts_with("DELETE /v1.0/me/events/N1"));
    }
}
