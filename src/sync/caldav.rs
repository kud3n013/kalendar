//! CalDAV: finding a person's calendars on a server and reading them.

use super::{Delta, Guard, Provider, RemoteCalendar, SyncError};
use crate::ics::{self, FeedEvent, FeedTask};
use crate::model::RemoteKey;
use chrono::{Duration, NaiveDate, NaiveDateTime};
use roxmltree::{Document, Node};

/// Everything is read again after this many days, so that repeating
/// events are written out further ahead as time passes.
const FULL_EVERY_DAYS: i64 = 30;
/// Events asked for in one request when reading changes.
const MULTIGET_BATCH: usize = 50;

pub struct CalDav {
    /// what the person typed: a server, or the address of their calendars
    pub server: String,
    pub username: String,
    pub password: String,
    /// an access token from a browser sign-in, used in place of the password
    pub bearer: Option<String>,
    /// the collection holding the calendars, once found
    pub home: Option<String>,
}

struct Reply {
    status: u16,
    body: String,
    /// where the answer came from, after redirects
    url: String,
    etag: Option<String>,
}

const NS: &str = r#"xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/""#;

fn xml_escape(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// "https://host:port" of an address.
fn origin(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => format!("{scheme}://{}", rest.split('/').next().unwrap_or("")),
        None => url.to_string(),
    }
}

fn host(url: &str) -> String {
    let o = origin(url);
    let h = o.split("://").nth(1).unwrap_or("").rsplit('@').next().unwrap_or("");
    let h = if h.starts_with('[') { h.split(']').next().unwrap_or("").trim_start_matches('[') } else { h.split(':').next().unwrap_or("") };
    h.to_lowercase()
}

/// An address the server gave, made whole against the one it came from.
fn join(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.contains("://") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{}{href}", origin(base))
    } else {
        let dir = match base.rfind('/') {
            Some(i) if i > base.find("://").map(|s| s + 2).unwrap_or(0) => &base[..=i],
            _ => return format!("{}/{href}", origin(base)),
        };
        format!("{dir}{href}")
    }
}

/// Passwords only travel encrypted, or to this computer itself.
fn safe_for_password(url: &str) -> bool {
    url.starts_with("https://") || matches!(host(url).as_str(), "localhost" | "127.0.0.1" | "::1")
}

/// Whether a password sent to `first` may follow a redirect to `next`:
/// the same server, or one beside it under the same name.
fn same_site(first: &str, next: &str) -> bool {
    let (a, b) = (host(first), host(next));
    let tail = |h: &str| h.rsplit('.').take(2).collect::<Vec<_>>().join(".");
    a == b || (a.contains('.') && a.parse::<std::net::IpAddr>().is_err() && tail(&a) == tail(&b))
}

fn child<'a>(n: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn text_of(n: Node) -> String {
    n.text().unwrap_or("").trim().to_string()
}

/// The parts of a multistatus answer.
fn responses<'a>(doc: &'a Document) -> Vec<Node<'a, 'a>> {
    doc.root_element().children().filter(|c| c.is_element() && c.tag_name().name() == "response").collect()
}

/// A property the server did give for this response.
fn prop<'a>(resp: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    resp.children()
        .filter(|c| c.is_element() && c.tag_name().name() == "propstat")
        .filter(|ps| child(*ps, "status").map(|s| text_of(s).contains(" 200")).unwrap_or(true))
        .filter_map(|ps| child(ps, "prop"))
        .find_map(|p| child(p, name))
}

fn href_in(n: Option<Node>) -> Option<String> {
    n.and_then(|n| child(n, "href")).map(text_of).filter(|h| !h.is_empty())
}

/// "#RRGGBB" or "#RRGGBBAA".
fn parse_color(v: &str) -> Option<[u8; 3]> {
    let v = v.trim().strip_prefix('#')?;
    if v.len() < 6 || !v.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

impl CalDav {
    pub fn new(server: &str, username: &str, password: &str, home: Option<String>) -> CalDav {
        let server = server.trim();
        let server = if server.contains("://") { server.to_string() } else { format!("https://{server}") };
        CalDav { server, username: username.trim().to_string(), password: password.to_string(), bearer: None, home }
    }

    /// For a server that takes an access token from a browser sign-in.
    pub fn with_bearer(server: &str, username: &str, token: &str, home: Option<String>) -> CalDav {
        CalDav { bearer: Some(token.to_string()), ..CalDav::new(server, username, "", home) }
    }

    /// One request; redirects are followed here so that the method and
    /// body are kept and the password never goes to another site.
    fn send(&self, method: &str, url: &str, depth: Option<&str>, body: &str) -> Result<Reply, SyncError> {
        self.send_with(method, url, depth, body, None)
    }

    /// `guard` is a condition header: the write only goes through while
    /// it holds on the server.
    fn send_with(&self, method: &str, url: &str, depth: Option<&str>, body: &str, guard: Option<(&str, &str)>) -> Result<Reply, SyncError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .allow_non_standard_methods(true)
            .build()
            .into();
        let mut url = url.to_string();
        for _ in 0..6 {
            if !safe_for_password(&url) {
                return Err(SyncError::Other("The server address must start with https://, so the password stays private.".into()));
            }
            let mut req = ureq::http::Request::builder().method(method).uri(&url).header("User-Agent", "kalendar");
            if same_site(&self.server, &url) {
                let auth = match &self.bearer {
                    Some(token) => format!("Bearer {token}"),
                    None => format!("Basic {}", base64(format!("{}:{}", self.username, self.password).as_bytes())),
                };
                req = req.header("Authorization", auth);
            }
            if let Some(d) = depth {
                req = req.header("Depth", d);
            }
            if let Some((name, value)) = guard {
                req = req.header(name, value);
            }
            if !body.is_empty() {
                req = req.header("Content-Type", if body.starts_with("BEGIN:") { "text/calendar; charset=utf-8" } else { "application/xml; charset=utf-8" });
            }
            let req = req.body(body.to_string()).map_err(|e| SyncError::Other(format!("The server address is not valid ({e}).")))?;
            let mut resp = agent.run(req).map_err(|e| SyncError::Network(e.to_string()))?;
            let status = resp.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if let Some(to) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                    url = join(&url, to);
                    continue;
                }
            }
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
            let body = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().unwrap_or_default();
            return Ok(Reply { status, body, url, etag });
        }
        Err(SyncError::Network("too many redirects".into()))
    }

    /// A request whose failure is an error.
    fn ask(&self, method: &str, url: &str, depth: Option<&str>, body: &str) -> Result<Reply, SyncError> {
        Self::checked(self.send(method, url, depth, body)?)
    }

    fn checked(r: Reply) -> Result<Reply, SyncError> {
        match r.status {
            200..=299 => Ok(r),
            401 | 403 => Err(SyncError::Auth),
            412 => Err(SyncError::Conflict),
            429 | 503 => Err(SyncError::RateLimited),
            404 | 410 => Err(SyncError::Gone),
            code => Err(SyncError::Other(format!("The server answered with error {code}."))),
        }
    }

    /// Nextcloud's call to make it forget the app password in use.
    pub fn revoke_app_password(&self, url: &str) {
        let _ = self.send_with("DELETE", url, None, "", Some(("OCS-APIRequest", "true")));
    }

    /// The collection holding the person's calendars: asked of the address
    /// as typed, then of the server's well-known address, then of its root.
    fn find_home(&self) -> Result<String, SyncError> {
        let body = format!("<d:propfind {NS}><d:prop><d:current-user-principal/><c:calendar-home-set/></d:prop></d:propfind>");
        let root = origin(&self.server);
        let mut tries = vec![self.server.clone(), format!("{root}/.well-known/caldav"), format!("{root}/")];
        tries.dedup();
        let mut last = SyncError::Other("No calendars were found at this address.".into());
        for t in tries {
            let r = match self.ask("PROPFIND", &t, Some("0"), &body) {
                Ok(r) => r,
                // a wrong password or an unreachable server will not get better
                Err(e @ (SyncError::Auth | SyncError::Network(_) | SyncError::RateLimited)) => return Err(e),
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            let Ok(doc) = Document::parse(&r.body) else { continue };
            let Some(resp) = responses(&doc).into_iter().next() else { continue };
            if let Some(h) = href_in(prop(resp, "calendar-home-set")) {
                return Ok(join(&r.url, &h));
            }
            let Some(principal) = href_in(prop(resp, "current-user-principal")) else { continue };
            let p = self.ask("PROPFIND", &join(&r.url, &principal), Some("0"), &body)?;
            if let Ok(doc) = Document::parse(&p.body) {
                if let Some(h) = responses(&doc).into_iter().next().and_then(|resp| href_in(prop(resp, "calendar-home-set"))) {
                    return Ok(join(&p.url, &h));
                }
            }
        }
        Err(last)
    }

    fn bad_answer() -> SyncError {
        SyncError::Other("The server's answer could not be read.".into())
    }

    /// A calendar's change tag and sync token.
    fn tags(&self, cal: &RemoteCalendar) -> Result<(Option<String>, Option<String>), SyncError> {
        let body = format!("<d:propfind {NS}><d:prop><cs:getctag/><d:sync-token/></d:prop></d:propfind>");
        let r = self.ask("PROPFIND", &cal.id, Some("0"), &body)?;
        let doc = Document::parse(&r.body).map_err(|_| Self::bad_answer())?;
        let resp = responses(&doc).into_iter().next().ok_or_else(Self::bad_answer)?;
        let get = |name: &str| prop(resp, name).map(text_of).filter(|t| !t.is_empty());
        Ok((get("getctag"), get("sync-token")))
    }

    /// The events inside an answer that carries calendar data.
    fn events_in(base: &str, body: &str, now: NaiveDateTime) -> Result<(Vec<FeedEvent>, Vec<FeedTask>), SyncError> {
        let doc = Document::parse(body).map_err(|_| Self::bad_answer())?;
        let mut out = vec![];
        let mut tasks = vec![];
        for resp in responses(&doc) {
            let (Some(href), Some(data)) = (child(resp, "href").map(text_of), prop(resp, "calendar-data")) else { continue };
            let etag = prop(resp, "getetag").map(text_of);
            let text: String = data.children().filter_map(|c| c.text()).collect();
            let Ok(feed) = ics::parse(&text, now) else { continue };
            let href = join(base, &href);
            tasks.extend(ics::parse_todos(&text).into_iter().map(|mut t| {
                t.href = Some(href.clone());
                t.etag = etag.clone();
                t
            }));
            out.extend(feed.events.into_iter().map(|mut e| {
                e.href = Some(href.clone());
                e.etag = etag.clone();
                e
            }));
        }
        Ok((out, tasks))
    }

    /// Everything the calendar holds: its events around `now`, or all of
    /// its tasks.
    fn pull_all(&self, cal: &RemoteCalendar, now: NaiveDateTime, tasks: bool) -> Result<Delta, SyncError> {
        // the token is read first: a change made while reading comes again
        let (ctag, token) = self.tags(cal)?;
        let stamp = |d: NaiveDateTime| d.format("%Y%m%dT000000Z").to_string();
        let (from, to) = (stamp(now - Duration::days(ics::PAST_DAYS + 1)), stamp(now + Duration::days(ics::AHEAD_DAYS + 1)));
        let filter = if tasks { "<c:comp-filter name=\"VTODO\"/>".to_string() } else { format!("<c:comp-filter name=\"VEVENT\"><c:time-range start=\"{from}\" end=\"{to}\"/></c:comp-filter>") };
        let body = format!("<c:calendar-query {NS}><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name=\"VCALENDAR\">{filter}</c:comp-filter></c:filter></c:calendar-query>");
        let r = self.ask("REPORT", &cal.id, Some("1"), &body)?;
        let (upserts, found) = Self::events_in(&r.url, &r.body, now)?;
        let state = format!("{}|{}", now.date().format("%Y-%m-%d"), token.unwrap_or_default());
        Ok(Delta { upserts, tasks: found, full: true, state: Some(state), ctag, ..Default::default() })
    }

    /// What changed since the token was given.
    fn pull_changes(&self, cal: &RemoteCalendar, since: &str, token: &str, now: NaiveDateTime) -> Result<Delta, SyncError> {
        let body = format!(
            "<d:sync-collection {NS}><d:sync-token>{}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>",
            xml_escape(token)
        );
        let r = self.send("REPORT", &cal.id, Some("0"), &body)?;
        match r.status {
            200..=299 => {}
            401 => return Err(SyncError::Auth),
            429 | 503 => return Err(SyncError::RateLimited),
            // a token the server has forgotten is refused in several ways
            _ => return Err(SyncError::TokenExpired),
        }
        let doc = Document::parse(&r.body).map_err(|_| Self::bad_answer())?;
        let new_token = child(doc.root_element(), "sync-token").map(text_of).filter(|t| !t.is_empty()).ok_or(SyncError::TokenExpired)?;
        let (mut changed, mut gone) = (vec![], vec![]);
        for resp in responses(&doc) {
            let Some(href) = child(resp, "href").map(text_of) else { continue };
            if child(resp, "status").map(|s| text_of(s).contains(" 404")).unwrap_or(false) {
                gone.push(href);
            } else if !href.ends_with('/') {
                changed.push(href);
            }
        }
        let (mut upserts, mut tasks) = (vec![], vec![]);
        for batch in changed.chunks(MULTIGET_BATCH) {
            let hrefs: String = batch.iter().map(|h| format!("<d:href>{}</d:href>", xml_escape(h))).collect();
            let body = format!("<c:calendar-multiget {NS}><d:prop><d:getetag/><c:calendar-data/></d:prop>{hrefs}</c:calendar-multiget>");
            let m = self.ask("REPORT", &cal.id, Some("1"), &body)?;
            let (events, found) = Self::events_in(&m.url, &m.body, now)?;
            upserts.extend(events);
            tasks.extend(found);
        }
        let ctag = if changed.is_empty() && gone.is_empty() { cal.ctag.clone() } else { None };
        Ok(Delta {
            upserts,
            tasks,
            changed: changed.iter().map(|h| join(&r.url, h)).collect(),
            deletes: gone.iter().map(|h| RemoteKey::Href(join(&r.url, h))).collect(),
            full: false,
            state: Some(format!("{since}|{new_token}")),
            ctag,
            name: None,
        })
    }
}

impl Provider for CalDav {
    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError> {
        let home = match &self.home {
            Some(h) => h.clone(),
            None => self.find_home()?,
        };
        let body = format!(
            "<d:propfind {NS}><d:prop><d:resourcetype/><d:displayname/><cs:getctag/><ic:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/></d:prop></d:propfind>"
        );
        let r = self.ask("PROPFIND", &home, Some("1"), &body)?;
        let doc = Document::parse(&r.body).map_err(|_| Self::bad_answer())?;
        let mut out = vec![];
        for resp in responses(&doc) {
            let Some(href) = child(resp, "href").map(text_of) else { continue };
            if !prop(resp, "resourcetype").is_some_and(|t| child(t, "calendar").is_some()) {
                continue;
            }
            // what it can hold: events, tasks or both; journals are left out
            let comps: Vec<&str> = prop(resp, "supported-calendar-component-set").map(|set| set.children().filter_map(|c| c.attribute("name")).collect()).unwrap_or_default();
            let holds = |kind: &str| comps.iter().any(|c| c.eq_ignore_ascii_case(kind));
            // a server that does not say is taken to hold events
            let (events, tasks) = (comps.is_empty() || holds("VEVENT"), holds("VTODO"));
            if !events && !tasks {
                continue;
            }
            let read_only = prop(resp, "current-user-privilege-set").is_some_and(|set| {
                !set.descendants().any(|n| matches!(n.tag_name().name(), "write" | "write-content" | "all"))
            });
            let fallback = href.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string();
            out.push(RemoteCalendar {
                id: join(&r.url, &href),
                name: prop(resp, "displayname").map(text_of).filter(|n| !n.is_empty()).unwrap_or(fallback),
                color: prop(resp, "calendar-color").and_then(|c| parse_color(&text_of(c))),
                ctag: prop(resp, "getctag").map(text_of).filter(|t| !t.is_empty()),
                read_only,
                events,
                tasks,
            });
        }
        self.home = Some(home);
        Ok(out)
    }

    fn home(&self) -> Option<String> {
        self.home.clone()
    }

    fn forget_home(&mut self) {
        self.home = None;
    }

    fn get(&mut self, href: &str) -> Result<(String, Option<String>), SyncError> {
        let r = self.ask("GET", href, None, "")?;
        Ok((r.body, r.etag))
    }

    fn put(&mut self, _cal: &RemoteCalendar, href: &str, ics: &str, guard: &Guard) -> Result<Option<String>, SyncError> {
        let header = match guard {
            Guard::New => Some(("If-None-Match", "*")),
            Guard::Match(etag) => Some(("If-Match", etag.as_str())),
            Guard::Any => None,
        };
        Ok(Self::checked(self.send_with("PUT", href, None, ics, header)?)?.etag)
    }

    fn delete(&mut self, _cal: &RemoteCalendar, href: &str, etag: Option<&str>) -> Result<(), SyncError> {
        Self::checked(self.send_with("DELETE", href, None, "", etag.map(|t| ("If-Match", t)))?).map(|_| ())
    }

    /// `state` is "day of the last full read|sync token".
    fn pull(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        self.pull_kind(cal, state, now, false)
    }

    fn pull_tasks(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        self.pull_kind(cal, state, now, true)
    }
}

impl CalDav {
    /// Events or tasks: what changed since `state`, or everything.
    fn pull_kind(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime, tasks: bool) -> Result<Delta, SyncError> {
        let known = state.and_then(|s| s.split_once('|')).and_then(|(day, token)| {
            let day = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
            ((now.date() - day).num_days().abs() < FULL_EVERY_DAYS).then_some((day, token))
        });
        if let Some((day, token)) = known {
            let since = day.format("%Y-%m-%d").to_string();
            if !token.is_empty() {
                match self.pull_changes(cal, &since, token, now) {
                    Err(SyncError::TokenExpired) => {}
                    other => return other,
                }
            } else if cal.ctag.is_some() {
                // no sync tokens here: the change tag says whether to read again
                let (ctag, _) = self.tags(cal)?;
                if ctag == cal.ctag {
                    return Ok(Delta { state: state.map(str::to_string), ctag, ..Default::default() });
                }
            }
        }
        self.pull_all(cal, now, tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(origin("https://example.org:8443/dav/x/"), "https://example.org:8443");
        assert_eq!(host("https://Example.org:8443/dav"), "example.org");
        assert_eq!(host("http://[::1]:5232/"), "::1");
        assert_eq!(join("https://example.org/dav/home/", "/dav/cal/1.ics"), "https://example.org/dav/cal/1.ics");
        assert_eq!(join("https://example.org/dav/home/", "work/"), "https://example.org/dav/home/work/");
        assert_eq!(join("https://example.org", "x"), "https://example.org/x");
        assert_eq!(join("https://a.example/", "https://b.example/c"), "https://b.example/c");
        assert!(safe_for_password("https://example.org/") && safe_for_password("http://127.0.0.1:5232/") && safe_for_password("http://localhost/"));
        assert!(!safe_for_password("http://example.org/"));
        assert!(same_site("https://caldav.example.com/", "https://p12-caldav.example.com/x"));
        assert!(!same_site("https://caldav.example.com/", "https://example.net/x"));
        assert!(!same_site("http://127.0.0.1:1/", "http://127.0.0.2:1/"));
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(parse_color("#FF8000FF"), Some([255, 128, 0]));
        assert_eq!(CalDav::new(" example.org/dav ", "u", "p", None).server, "https://example.org/dav");
    }

    #[test]
    fn reads_a_calendar_list() {
        let xml = r##"<?xml version="1.0"?><multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:CS="http://calendarserver.org/ns/" xmlns:IC="http://apple.com/ns/ical/">
<response><href>/u/</href><propstat><prop><resourcetype><collection/></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/u/work/</href><propstat><prop><resourcetype><collection/><C:calendar/></resourcetype><displayname>Work &amp; more</displayname><CS:getctag>"7"</CS:getctag><IC:calendar-color>#3585e1ff</IC:calendar-color><C:supported-calendar-component-set><C:comp name="VEVENT"/></C:supported-calendar-component-set><current-user-privilege-set><privilege><read/></privilege></current-user-privilege-set></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/u/todo/</href><propstat><prop><resourcetype><collection/><C:calendar/></resourcetype><C:supported-calendar-component-set><C:comp name="VTODO"/></C:supported-calendar-component-set></prop><status>HTTP/1.1 200 OK</status></propstat><propstat><prop><displayname/></prop><status>HTTP/1.1 404 Not Found</status></propstat></response>
</multistatus>"##;
        let doc = Document::parse(xml).unwrap();
        let rs = responses(&doc);
        assert_eq!(rs.len(), 3);
        assert_eq!(prop(rs[1], "displayname").map(text_of).as_deref(), Some("Work & more"));
        assert!(prop(rs[2], "displayname").is_none(), "a property the server did not have");
        assert!(child(prop(rs[1], "resourcetype").unwrap(), "calendar").is_some());
        assert!(child(prop(rs[0], "resourcetype").unwrap(), "calendar").is_none());
    }

    /// Against a real server. Set KALENDAR_TEST_CALDAV to its address
    /// (a Radicale on this computer will do), and KALENDAR_TEST_USER and
    /// KALENDAR_TEST_PASS; without them the test does nothing.
    #[test]
    fn against_a_server() {
        let Ok(server) = std::env::var("KALENDAR_TEST_CALDAV") else { return };
        let user = std::env::var("KALENDAR_TEST_USER").unwrap_or_else(|_| "test".into());
        let pass = std::env::var("KALENDAR_TEST_PASS").unwrap_or_else(|_| "test".into());
        let now = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut dav = CalDav::new(&server, &user, &pass, None);

        // a wrong password is told apart from other trouble
        let mut wrong = CalDav::new(&server, &user, "not the password", None);
        assert_eq!(wrong.list_calendars().unwrap_err(), SyncError::Auth);

        // finding the home, from the bare server address
        let home = dav.find_home().expect("home");
        assert!(home.ends_with(&format!("/{user}/")), "{home}");

        // a fresh calendar with one single and one repeating event
        let name = format!("kalendar-test-{}", std::process::id());
        let cal_url = format!("{home}{name}/");
        let mk = format!("<c:mkcalendar {NS}><d:set><d:prop><d:displayname>Test &amp; Co</d:displayname><ic:calendar-color>#ff8f1eff</ic:calendar-color><c:supported-calendar-component-set><c:comp name=\"VEVENT\"/></c:supported-calendar-component-set></d:prop></d:set></c:mkcalendar>");
        dav.ask("MKCALENDAR", &cal_url, None, &mk).expect("mkcalendar");
        let event = |uid: &str, title: &str, extra: &str| {
            format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//kalendar//test\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261101T000000Z\r\nSUMMARY:{title}\r\nDTSTART:20261116T150000Z\r\nDTEND:20261116T160000Z\r\n{extra}END:VEVENT\r\nEND:VCALENDAR\r\n")
        };
        dav.ask("PUT", &format!("{cal_url}one.ics"), None, &event("one", "One", "")).expect("put");
        dav.ask("PUT", &format!("{cal_url}many.ics"), None, &event("many", "Many", "RRULE:FREQ=DAILY;COUNT=3\r\n")).expect("put");

        // listing
        let cals = dav.list_calendars().expect("list");
        let cal = cals.iter().find(|c| c.id == cal_url).expect("the new calendar is listed").clone();
        assert_eq!((cal.name.as_str(), cal.color, cal.read_only, cal.events, cal.tasks), ("Test & Co", Some([0xff, 0x8f, 0x1e]), false, true, false));

        // the first pull reads everything
        let d = dav.pull(&cal, None, now).expect("full pull");
        assert!(d.full);
        let mut got: Vec<(String, Option<String>)> = d.upserts.iter().map(|e| (e.title.clone(), e.recurrence_id.clone())).collect();
        got.sort();
        assert_eq!(got.len(), 4, "{got:?}");
        assert_eq!(got[3], ("One".to_string(), None));
        assert!(d.upserts.iter().all(|e| e.href.as_deref().is_some_and(|h| h.starts_with(&cal_url)) && e.etag.is_some()));
        let state = d.state.clone().expect("state");
        assert!(state.starts_with("2026-11-13|") && state.len() > 11, "no sync token: {state}");

        // nothing changed: nothing comes back
        let cal = RemoteCalendar { ctag: d.ctag.clone(), ..cal };
        let d2 = dav.pull(&cal, Some(&state), now).expect("pull without changes");
        assert!(!d2.full && d2.upserts.is_empty() && d2.changed.is_empty() && d2.deletes.is_empty());

        // one changed, one removed, one added
        dav.ask("PUT", &format!("{cal_url}one.ics"), None, &event("one", "One later", "")).expect("put");
        dav.ask("DELETE", &format!("{cal_url}many.ics"), None, "").expect("delete");
        dav.ask("PUT", &format!("{cal_url}new.ics"), None, &event("new", "New", "")).expect("put");
        let d3 = dav.pull(&cal, d2.state.as_deref(), now).expect("pull of changes");
        assert!(!d3.full);
        let mut titles: Vec<&str> = d3.upserts.iter().map(|e| e.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["New", "One later"]);
        assert_eq!(d3.deletes, vec![RemoteKey::Href(format!("{cal_url}many.ics"))]);
        assert_eq!(d3.changed.len(), 2);

        // the model ends up with what the server holds
        let mut data = crate::model::Data::sample(now.date());
        data.events.clear();
        for delta in [d, d2, d3] {
            data.apply(1, delta);
        }
        let mut titles: Vec<&str> = data.events.iter().map(|e| e.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["New", "One later"]);

        // a token the server does not know leads to a full read
        let d4 = dav.pull(&cal, Some("2026-11-13|http://example.org/no-such-token"), now).expect("pull after a lost token");
        assert!(d4.full && d4.upserts.len() == 2);
        // and so does a state that has grown old
        let d5 = dav.pull(&cal, Some(&format!("2026-09-01|{}", state.split_once('|').unwrap().1)), now).expect("pull after a month");
        assert!(d5.full);

        // --- writing ---
        use crate::model::Event;
        use crate::sync::{push, PushJob, PushResult};
        let stamp = now;
        let at = |h: u32| NaiveDate::from_ymd_opt(2026, 11, 17).unwrap().and_hms_opt(h, 0, 0).unwrap();
        // a new event
        let mine = Event { id: 5, title: "Mine; with, signs".into(), start: at(9), end: at(10), notes: "Where: Room 4\nBring notes".into(), ..Default::default() };
        let r = push(&mut dav, &cal, &[PushJob::Save(mine.clone())], stamp);
        let PushResult::Saved { uid, href, etag, .. } = r[0].clone() else { panic!("not saved: {r:?}") };
        assert!(href.starts_with(&cal_url) && etag.is_some());
        let (raw, _) = dav.get(&href).expect("get");
        let (uid2, f) = ics::read_event(&raw).unwrap();
        assert_eq!((uid2, f.summary.as_str(), f.location.as_str(), f.description.as_str(), f.start), (uid.clone(), "Mine; with, signs", "Room 4", "Bring notes", at(9)));
        // creating it again must not overwrite: the address is taken
        assert_eq!(dav.put(&cal, &href, &raw, &Guard::New).unwrap_err(), SyncError::Conflict);

        // a change, with the version it was read at
        let mut changed = Event { href: Some(href.clone()), etag: etag.clone(), uid: Some(uid.clone()), remote: true, title: "Mine, moved".into(), start: at(11), end: at(12), ..mine.clone() };
        let r = push(&mut dav, &cal, &[PushJob::Save(changed.clone())], stamp);
        let PushResult::Saved { etag: etag2, .. } = r[0].clone() else { panic!("not saved: {r:?}") };
        assert_ne!(etag2, etag);
        let (raw2, _) = dav.get(&href).unwrap();
        assert!(raw2.contains("SEQUENCE:1"), "{raw2}");
        assert_eq!(ics::read_event(&raw2).unwrap().1.start, at(11));

        // a change made with an old version is a conflict, and the server's copy stays
        changed.title = "Mine, from a stale copy".into();
        let r = push(&mut dav, &cal, &[PushJob::Save(changed.clone())], stamp);
        assert!(matches!(r[0], PushResult::Conflict { event: 5 }), "{r:?}");
        assert_eq!(ics::read_event(&dav.get(&href).unwrap().0).unwrap().1.summary, "Mine, moved");

        // deleting with an old version is refused too; with the right one it goes
        let r = push(&mut dav, &cal, &[PushJob::Delete { href: href.clone(), etag: etag.clone() }], stamp);
        assert!(matches!(r[0], PushResult::DeleteConflict { .. }), "{r:?}");
        let r = push(&mut dav, &cal, &[PushJob::Delete { href: href.clone(), etag: etag2.clone() }], stamp);
        assert!(matches!(r[0], PushResult::Deleted { .. }), "{r:?}");
        assert_eq!(dav.get(&href).unwrap_err(), SyncError::Gone);
        // deleting what is gone is fine; changing it is a conflict
        let r = push(&mut dav, &cal, &[PushJob::Delete { href: href.clone(), etag: etag2 }, PushJob::Save(changed)], stamp);
        assert!(matches!(r[0], PushResult::Deleted { .. }) && matches!(r[1], PushResult::Conflict { .. }), "{r:?}");

        dav.ask("DELETE", &cal_url, None, "").expect("clean up");

        // --- tasks ---
        use crate::model::Task;
        let list_url = format!("{home}{name}-tasks/");
        let mk = format!("<c:mkcalendar {NS}><d:set><d:prop><d:displayname>To do</d:displayname><c:supported-calendar-component-set><c:comp name=\"VTODO\"/></c:supported-calendar-component-set></d:prop></d:set></c:mkcalendar>");
        dav.ask("MKCALENDAR", &list_url, None, &mk).expect("mkcalendar for tasks");
        let list = dav.list_calendars().unwrap().into_iter().find(|c| c.id == list_url).expect("the task list is listed");
        assert!(list.tasks && !list.events);
        // a new task, read back by a pull
        let task = Task { id: 7, title: "Buy milk".into(), notes: "Two".into(), due: NaiveDate::from_ymd_opt(2026, 11, 20), priority: 1, ..Default::default() };
        let r = push(&mut dav, &list, &[PushJob::SaveTask(task.clone())], stamp);
        let PushResult::TaskSaved { uid, href, etag, .. } = r[0].clone() else { panic!("not saved: {r:?}") };
        let d = dav.pull_tasks(&list, None, now).expect("pull of tasks");
        assert!(d.full && d.upserts.is_empty());
        assert_eq!(d.tasks.len(), 1);
        let t = &d.tasks[0];
        assert_eq!((t.uid.as_str(), t.title.as_str(), t.notes.as_str(), t.due, t.priority, t.done, t.href.as_deref()), (uid.as_str(), "Buy milk", "Two", task.due, 1, false, Some(href.as_str())));
        // ticked off here, then seen as a change
        let done = Task { done: true, uid: Some(uid.clone()), href: Some(href.clone()), etag: etag.clone(), ..task.clone() };
        let r = push(&mut dav, &list, &[PushJob::SaveTask(done.clone())], stamp);
        assert!(matches!(r[0], PushResult::TaskSaved { .. }), "{r:?}");
        let d2 = dav.pull_tasks(&list, d.state.as_deref(), now).expect("pull of task changes");
        assert!(!d2.full && d2.tasks.len() == 1 && d2.tasks[0].done);
        // a change from a stale copy is a conflict
        let r = push(&mut dav, &list, &[PushJob::SaveTask(Task { title: "Stale".into(), ..done })], stamp);
        assert!(matches!(r[0], PushResult::TaskConflict { task: 7 }), "{r:?}");
        dav.ask("DELETE", &list_url, None, "").expect("clean up");
    }
}
