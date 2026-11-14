//! Subscribed calendars: fetching a calendar feed (.ics) and reading it.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};
use std::collections::HashSet;

/// One event read from a feed, already in this computer's local time.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedEvent {
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub notes: String,
}

pub struct Feed {
    pub name: Option<String>,
    pub events: Vec<FeedEvent>,
}

/// How far repeating events are written out.
pub const PAST_DAYS: i64 = 60;
pub const AHEAD_DAYS: i64 = 400;

/// Turns what the person pasted into an address that can be fetched.
/// `webcal://` links are https links under another name.
pub fn normalize_url(input: &str) -> Result<String, String> {
    let u = input.trim();
    if u.is_empty() {
        return Err("Paste the calendar's link first".into());
    }
    let u = if let Some(rest) = u.strip_prefix("webcal://") {
        format!("https://{rest}")
    } else if let Some(rest) = u.strip_prefix("webcals://") {
        format!("https://{rest}")
    } else if u.starts_with("https://") || u.starts_with("http://") {
        u.to_string()
    } else if u.contains("://") {
        return Err("This kind of link is not supported; use an https or webcal link".into());
    } else {
        format!("https://{u}")
    };
    Ok(u)
}

/// True for a Proton Calendar "share with anyone" link.
pub fn is_proton(url: &str) -> bool {
    let host = url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_lowercase();
    host == "calendar.proton.me" || host.ends_with(".proton.me") || host == "calendar.protonmail.com"
}

/// A name to show before the feed has told us its own.
pub fn default_name(url: &str) -> String {
    if is_proton(url) {
        return "Proton Calendar".into();
    }
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("Calendar").to_string()
}

/// Downloads a feed and reads it. Runs on a background thread.
pub fn fetch(url: &str, now: NaiveDateTime) -> Result<Feed, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(25))
        .set("User-Agent", "kalendar")
        .set("Accept", "text/calendar, text/plain, */*")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403, _) | ureq::Error::Status(401, _) => {
                "The link was refused. Check that sharing is still on and the whole link was pasted.".to_string()
            }
            ureq::Error::Status(404, _) => "Nothing was found at this link.".to_string(),
            ureq::Error::Status(code, _) => format!("The server answered with error {code}."),
            ureq::Error::Transport(t) => format!("Could not reach the server ({}).", t.kind()),
        })?;
    let mut text = String::new();
    use std::io::Read;
    resp.into_reader().take(20 * 1024 * 1024).read_to_string(&mut text).map_err(|_| "The answer could not be read.".to_string())?;
    parse(&text, now)
}

#[derive(Default, Clone)]
struct Raw {
    uid: String,
    summary: String,
    description: String,
    location: String,
    start: Option<(When, bool)>,
    end: Option<(When, bool)>,
    duration: Option<Duration>,
    rrule: Option<String>,
    exdates: Vec<When>,
    recurrence_id: Option<When>,
    cancelled: bool,
}

/// A moment as written in the feed: wall time plus the zone it is in.
#[derive(Clone, Debug)]
struct When {
    wall: NaiveDateTime,
    zone: Zone,
}

#[derive(Clone, Debug)]
enum Zone {
    Utc,
    Named(chrono_tz::Tz),
    Floating,
}

impl When {
    /// The same moment on this computer's clock.
    fn local(&self) -> NaiveDateTime {
        self.at(self.wall)
    }
    /// A wall time in this value's zone, shown on this computer's clock.
    fn at(&self, wall: NaiveDateTime) -> NaiveDateTime {
        match &self.zone {
            Zone::Utc => Utc.from_utc_datetime(&wall).with_timezone(&Local).naive_local(),
            Zone::Named(tz) => match tz.from_local_datetime(&wall).earliest() {
                Some(t) => t.with_timezone(&Local).naive_local(),
                // a time skipped by the clocks going forward
                None => tz.from_utc_datetime(&wall).with_timezone(&Local).naive_local(),
            },
            Zone::Floating => wall,
        }
    }
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits "NAME;PARAM=x:value" into name, parameters and value.
fn split_line(line: &str) -> Option<(String, Vec<(String, String)>, &str)> {
    // the first colon outside quotes ends the name part
    let mut in_q = false;
    let mut cut = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                cut = Some(i);
                break;
            }
            _ => {}
        }
    }
    let cut = cut?;
    let (head, value) = (&line[..cut], &line[cut + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some((name, params, value))
}

/// Reads a date or date-time value. Returns the moment and whether it is a whole day.
fn parse_when(value: &str, params: &[(String, String)]) -> Option<(When, bool)> {
    let v = value.trim();
    if v.len() == 8 && !v.contains('T') {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        return Some((When { wall: d.and_hms_opt(0, 0, 0)?, zone: Zone::Floating }, true));
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let wall = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M"))
        .ok()?;
    let zone = if utc {
        Zone::Utc
    } else {
        match params.iter().find(|(k, _)| k == "TZID") {
            Some((_, id)) => id.trim_start_matches('/').parse::<chrono_tz::Tz>().map(Zone::Named).unwrap_or(Zone::Floating),
            None => Zone::Floating,
        }
    };
    Some((When { wall, zone }, false))
}

/// "P1DT2H30M", "PT45M", "P1W".
fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (neg, v) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let (mut total, mut num, mut time) = (Duration::zero(), String::new(), false);
    for c in v.chars() {
        match c {
            'T' => time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (c, time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// The n-th (or, for negative n, n-th from the end) given weekday of a month.
fn nth_weekday(year: i32, month: u32, wd: Weekday, n: i32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1)? } else { NaiveDate::from_ymd_opt(year, month + 1, 1)? };
    let days: Vec<NaiveDate> = first.iter_days().take_while(|d| *d < next).filter(|d| d.weekday() == wd).collect();
    if n > 0 {
        days.get(n as usize - 1).copied()
    } else {
        days.len().checked_sub((-n) as usize).and_then(|i| days.get(i).copied())
    }
}

/// Writes out the start times (as written in the feed's own zone) of a
/// repeating event, up to `until_wall`.
fn expand(start: NaiveDateTime, rule: &str, until_wall: NaiveDateTime, until_is_utc_hint: impl Fn(NaiveDateTime) -> NaiveDateTime) -> Vec<NaiveDateTime> {
    let mut freq = "";
    let mut interval = 1i64;
    let mut count: Option<usize> = None;
    let mut until: Option<NaiveDateTime> = None;
    let mut byday: Vec<(i32, Weekday)> = vec![];
    let mut bymonthday: Vec<i32> = vec![];
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.to_uppercase().as_str() {
            "FREQ" => freq = v,
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => {
                until = parse_when(v, &[]).map(|(w, all_day)| {
                    if all_day {
                        w.wall + Duration::days(1) - Duration::seconds(1)
                    } else if matches!(w.zone, Zone::Utc) {
                        until_is_utc_hint(w.wall)
                    } else {
                        w.wall
                    }
                })
            }
            "BYDAY" => {
                byday = v
                    .split(',')
                    .filter_map(|d| {
                        let d = d.trim();
                        let split = d.len().checked_sub(2)?;
                        let wd = weekday(&d[split..].to_uppercase())?;
                        Some((d[..split].parse().unwrap_or(0), wd))
                    })
                    .collect()
            }
            "BYMONTHDAY" => bymonthday = v.split(',').filter_map(|d| d.trim().parse().ok()).collect(),
            _ => {}
        }
    }
    let limit = until.map(|u| u.min(until_wall)).unwrap_or(until_wall);
    let time = start.time();
    let mut out: Vec<NaiveDateTime> = vec![];
    let mut push = |d: NaiveDate, out: &mut Vec<NaiveDateTime>| -> bool {
        let t = d.and_time(time);
        if t < start {
            return true;
        }
        if t > limit || count.map(|c| out.len() >= c).unwrap_or(false) {
            return false;
        }
        out.push(t);
        true
    };
    let d0 = start.date();
    match freq.to_uppercase().as_str() {
        "DAILY" => {
            let mut d = d0;
            for _ in 0..5000 {
                let ok = byday.is_empty() || byday.iter().any(|(_, w)| *w == d.weekday());
                if ok && !push(d, &mut out) {
                    break;
                }
                d += Duration::days(interval);
                if d.and_time(time) > limit {
                    break;
                }
            }
        }
        "WEEKLY" => {
            let days: Vec<Weekday> = if byday.is_empty() { vec![d0.weekday()] } else { byday.iter().map(|x| x.1).collect() };
            let mut week = d0 - Duration::days(d0.weekday().num_days_from_monday() as i64);
            'w: for _ in 0..3000 {
                for i in 0..7 {
                    let d = week + Duration::days(i);
                    if days.contains(&d.weekday()) && !push(d, &mut out) {
                        break 'w;
                    }
                }
                week += Duration::weeks(interval);
                if week.and_time(time) > limit {
                    break;
                }
            }
        }
        "MONTHLY" | "YEARLY" => {
            let step = if freq.eq_ignore_ascii_case("YEARLY") { 12 * interval } else { interval };
            let mut k = 0i64;
            'm: for _ in 0..2000 {
                let m0 = d0.year() as i64 * 12 + d0.month0() as i64 + k * step;
                let (y, m) = ((m0.div_euclid(12)) as i32, m0.rem_euclid(12) as u32 + 1);
                let mut cands: Vec<NaiveDate> = vec![];
                if !byday.is_empty() && byday.iter().all(|(n, _)| *n != 0) {
                    cands.extend(byday.iter().filter_map(|(n, w)| nth_weekday(y, m, *w, *n)));
                } else if !bymonthday.is_empty() {
                    for md in &bymonthday {
                        if *md > 0 {
                            cands.extend(NaiveDate::from_ymd_opt(y, m, *md as u32));
                        }
                    }
                } else {
                    cands.extend(NaiveDate::from_ymd_opt(y, m, d0.day()));
                }
                cands.sort();
                for d in cands {
                    if !push(d, &mut out) {
                        break 'm;
                    }
                }
                if NaiveDate::from_ymd_opt(y, m, 1).map(|d| d.and_time(time) > limit).unwrap_or(true) {
                    break;
                }
                k += 1;
            }
        }
        _ => {
            push(d0, &mut out);
        }
    }
    out
}

/// Reads the text of a calendar feed.
pub fn parse(text: &str, now: NaiveDateTime) -> Result<Feed, String> {
    if !text.to_uppercase().contains("BEGIN:VCALENDAR") {
        let hint = if text.trim_start().starts_with('<') || text.trim_start().starts_with('{') {
            " It looks like a web page: use the calendar's sharing link (it usually ends in .ics)."
        } else {
            ""
        };
        return Err(format!("This link does not lead to a calendar.{hint}"));
    }
    // long lines are folded; a line starting with a space continues the last
    let mut lines: Vec<String> = vec![];
    for l in text.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&l[1..]);
        } else {
            lines.push(l.to_string());
        }
    }
    let mut name = None;
    let mut raws: Vec<Raw> = vec![];
    let mut cur: Option<Raw> = None;
    let mut depth_other = 0; // inside VALARM and the like
    for line in &lines {
        let Some((key, params, value)) = split_line(line) else { continue };
        match (key.as_str(), value.trim().to_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => cur = Some(Raw::default()),
            ("END", "VEVENT") => {
                if let Some(r) = cur.take() {
                    raws.push(r);
                }
                depth_other = 0;
            }
            ("BEGIN", _) if cur.is_some() => depth_other += 1,
            ("END", _) if cur.is_some() => depth_other = (depth_other - 1).max(0),
            _ => {}
        }
        if key == "X-WR-CALNAME" && cur.is_none() {
            name = Some(unescape(value).trim().to_string()).filter(|n| !n.is_empty());
        }
        let Some(r) = cur.as_mut() else { continue };
        if depth_other > 0 {
            continue;
        }
        match key.as_str() {
            "UID" => r.uid = value.trim().to_string(),
            "SUMMARY" => r.summary = unescape(value),
            "DESCRIPTION" => r.description = unescape(value),
            "LOCATION" => r.location = unescape(value),
            "DTSTART" => r.start = parse_when(value, &params),
            "DTEND" => r.end = parse_when(value, &params),
            "DURATION" => r.duration = parse_duration(value),
            "RRULE" => r.rrule = Some(value.trim().to_string()),
            "EXDATE" => r.exdates.extend(value.split(',').filter_map(|v| parse_when(v, &params)).map(|x| x.0)),
            "RECURRENCE-ID" => r.recurrence_id = parse_when(value, &params).map(|x| x.0),
            "STATUS" => r.cancelled = value.trim().eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    // occurrences that were moved or changed replace the repeating one
    let overridden: HashSet<(String, NaiveDateTime)> =
        raws.iter().filter_map(|r| r.recurrence_id.as_ref().map(|w| (r.uid.clone(), w.local()))).collect();
    let (from, to) = (now - Duration::days(PAST_DAYS), now + Duration::days(AHEAD_DAYS));
    let mut events = vec![];
    for r in &raws {
        let Some((start, all_day)) = r.start.clone() else { continue };
        if r.cancelled {
            continue;
        }
        let len = match (&r.end, r.duration) {
            (Some((e, _)), _) => e.local() - start.local(),
            (None, Some(d)) => d,
            (None, None) if all_day => Duration::days(1),
            (None, None) => Duration::zero(),
        };
        let len = if all_day { len.max(Duration::days(1)) } else { len.max(Duration::minutes(15)) };
        let mut notes = r.description.trim().to_string();
        if !r.location.trim().is_empty() {
            notes = if notes.is_empty() { format!("Where: {}", r.location.trim()) } else { format!("Where: {}\n{}", r.location.trim(), notes) };
        }
        let title = if r.summary.trim().is_empty() { "Busy".to_string() } else { r.summary.trim().to_string() };
        let starts: Vec<NaiveDateTime> = match (&r.rrule, &r.recurrence_id) {
            (Some(rule), None) => {
                let excluded: HashSet<NaiveDateTime> = r.exdates.iter().map(|w| w.local()).collect();
                let s = start.clone();
                expand(start.wall, rule, to + Duration::days(2), |utc_wall| {
                    // an UNTIL given in UTC, compared in the event's own zone
                    match &s.zone {
                        Zone::Named(tz) => Utc.from_utc_datetime(&utc_wall).with_timezone(tz).naive_local(),
                        Zone::Utc => utc_wall,
                        Zone::Floating => DateTime::<Utc>::from_naive_utc_and_offset(utc_wall, Utc).with_timezone(&Local).naive_local(),
                    }
                })
                .into_iter()
                .map(|w| start.at(w))
                .filter(|l| !excluded.contains(l) && !overridden.contains(&(r.uid.clone(), *l)))
                .collect()
            }
            _ => vec![start.local()],
        };
        for s in starts {
            let e = s + len;
            if e < from || s > to {
                continue;
            }
            events.push(FeedEvent { title: title.clone(), start: s, end: e, all_day, notes: notes.clone() });
        }
    }
    events.sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
    Ok(Feed { name, events })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }
    fn now() -> NaiveDateTime {
        at(2026, 11, 13, 12, 0)
    }
    fn wrap(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:My Cal\r\n{body}END:VCALENDAR\r\n")
    }

    #[test]
    fn links() {
        assert_eq!(normalize_url(" webcal://example.org/a.ics ").unwrap(), "https://example.org/a.ics");
        assert_eq!(normalize_url("example.org/a.ics").unwrap(), "https://example.org/a.ics");
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("  ").is_err());
        assert!(is_proton("https://calendar.proton.me/api/calendar/v1/url/abc/calendar.ics?CacheKey=1&PassphraseKey=2"));
        assert!(!is_proton("https://example.org/proton.me/a.ics"));
        assert_eq!(default_name("https://calendar.proton.me/api/x"), "Proton Calendar");
    }

    #[test]
    fn simple_floating_and_all_day() {
        let f = parse(
            &wrap("BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Lunch\\, long\r\n  title\r\nDTSTART:20261114T120000\r\nDTEND:20261114T130000\r\nLOCATION:Cafe\r\nBEGIN:VALARM\r\nDESCRIPTION:ring\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:2\r\nSUMMARY:Trip\r\nDTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261123\r\nEND:VEVENT\r\n"),
            now(),
        )
        .unwrap();
        assert_eq!(f.name.as_deref(), Some("My Cal"));
        assert_eq!(f.events.len(), 2);
        assert_eq!(f.events[0].title, "Lunch, long title");
        assert_eq!(f.events[0].start, at(2026, 11, 14, 12, 0));
        assert_eq!(f.events[0].end, at(2026, 11, 14, 13, 0));
        assert_eq!(f.events[0].notes, "Where: Cafe");
        assert!(f.events[1].all_day);
        assert_eq!(f.events[1].end, at(2026, 11, 23, 0, 0));
    }

    #[test]
    fn utc_and_zone_agree() {
        // 15:00 UTC is 16:00 in Zurich in winter: both must land on the same local time
        let f = parse(
            &wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A\r\nDTSTART:20261116T150000Z\r\nDURATION:PT45M\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:B\r\nDTSTART;TZID=Europe/Zurich:20261116T160000\r\nDTEND;TZID=Europe/Zurich:20261116T164500\r\nEND:VEVENT\r\n"),
            now(),
        )
        .unwrap();
        assert_eq!(f.events.len(), 2);
        assert_eq!(f.events[0].start, f.events[1].start);
        assert_eq!(f.events[0].end, f.events[1].end);
        assert_eq!((f.events[0].end - f.events[0].start).num_minutes(), 45);
    }

    #[test]
    fn weekly_with_exception_and_moved_one() {
        let f = parse(
            &wrap("BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Standup\r\nDTSTART:20261102T090000\r\nDTEND:20261102T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\r\nEXDATE:20261104T090000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:w\r\nRECURRENCE-ID:20261109T090000\r\nSUMMARY:Standup (moved)\r\nDTSTART:20261109T110000\r\nDTEND:20261109T111500\r\nEND:VEVENT\r\n"),
            now(),
        )
        .unwrap();
        let starts: Vec<NaiveDateTime> = f.events.iter().map(|e| e.start).collect();
        // six in the series: Nov 2, 4, 9, 11, 16, 18; the 4th is removed, the 9th moved to 11:00
        assert_eq!(starts, vec![at(2026, 11, 2, 9, 0), at(2026, 11, 9, 11, 0), at(2026, 11, 11, 9, 0), at(2026, 11, 16, 9, 0), at(2026, 11, 18, 9, 0)]);
        assert_eq!(f.events[1].title, "Standup (moved)");
    }

    #[test]
    fn daily_until_and_monthly_nth() {
        let f = parse(
            &wrap("BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:D\r\nDTSTART:20261110T080000\r\nRRULE:FREQ=DAILY;INTERVAL=2;UNTIL=20261116\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:m\r\nSUMMARY:M\r\nDTSTART:20261112T100000\r\nRRULE:FREQ=MONTHLY;BYDAY=2TH;COUNT=3\r\nEND:VEVENT\r\n"),
            now(),
        )
        .unwrap();
        let d: Vec<u32> = f.events.iter().filter(|e| e.title == "D").map(|e| e.start.day()).collect();
        assert_eq!(d, vec![10, 12, 14, 16]);
        let m: Vec<NaiveDate> = f.events.iter().filter(|e| e.title == "M").map(|e| e.start.date()).collect();
        assert_eq!(m, vec![NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 10).unwrap(), NaiveDate::from_ymd_opt(2027, 1, 14).unwrap()]);
    }

    #[test]
    fn not_a_calendar() {
        assert!(parse("<html>hello</html>", now()).is_err());
    }
}
