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
    /// the event's UID; never empty
    pub uid: String,
    /// names one occurrence of a repeating event: its original start, in
    /// UTC when the feed gives a zone
    pub recurrence_id: Option<String>,
    /// address and version on a server; None for a plain feed
    pub href: Option<String>,
    pub etag: Option<String>,
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
    let mut resp = ureq::get(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(25)))
        .build()
        .header("User-Agent", "kalendar")
        .header("Accept", "text/calendar, text/plain, */*")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(401 | 403) => {
                "The link was refused. Check that sharing is still on and the whole link was pasted.".to_string()
            }
            ureq::Error::StatusCode(404) => "Nothing was found at this link.".to_string(),
            ureq::Error::StatusCode(code) => format!("The server answered with error {code}."),
            other => format!("Could not reach the server ({other})."),
        })?;
    let text = resp
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_string()
        .map_err(|_| "The answer could not be read.".to_string())?;
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
    /// The moment as text that does not depend on this computer's zone.
    fn key(&self, all_day: bool) -> String {
        if all_day {
            return self.wall.format("%Y%m%d").to_string();
        }
        match &self.zone {
            Zone::Utc => self.wall.format("%Y%m%dT%H%M%SZ").to_string(),
            Zone::Named(tz) => match tz.from_local_datetime(&self.wall).earliest() {
                Some(t) => t.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string(),
                None => self.wall.format("%Y%m%dT%H%M%S").to_string(),
            },
            Zone::Floating => self.wall.format("%Y%m%dT%H%M%S").to_string(),
        }
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
            Some((_, id)) => crate::tz::resolve(id).map(Zone::Named).unwrap_or(Zone::Floating),
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
    let mut bymonth: Vec<u32> = vec![];
    let mut bysetpos: Vec<i32> = vec![];
    let mut wkst = Weekday::Mon;
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
            "BYMONTH" => bymonth = v.split(',').filter_map(|d| d.trim().parse().ok()).filter(|m| (1..=12).contains(m)).collect(),
            "BYSETPOS" => bysetpos = v.split(',').filter_map(|d| d.trim().parse().ok()).collect(),
            "WKST" => wkst = weekday(&v.trim().to_uppercase()).unwrap_or(Weekday::Mon),
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
    let in_months = |d: NaiveDate| bymonth.is_empty() || bymonth.contains(&d.month());
    match freq.to_uppercase().as_str() {
        "DAILY" => {
            let mut d = d0;
            for _ in 0..5000 {
                let ok = (byday.is_empty() || byday.iter().any(|(_, w)| *w == d.weekday())) && in_months(d);
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
            // weeks are counted from the day the rule names as their first
            let into_week = (d0.weekday().num_days_from_monday() + 7 - wkst.num_days_from_monday()) % 7;
            let mut week = d0 - Duration::days(into_week as i64);
            'w: for _ in 0..3000 {
                for i in 0..7 {
                    let d = week + Duration::days(i);
                    if days.contains(&d.weekday()) && in_months(d) && !push(d, &mut out) {
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
            let yearly = freq.eq_ignore_ascii_case("YEARLY");
            // the days of one month the rule picks
            let in_month = |y: i32, m: u32| -> Vec<NaiveDate> {
                let Some(first) = NaiveDate::from_ymd_opt(y, m, 1) else { return vec![] };
                let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
                let len = next.map(|n| (n - first).num_days()).unwrap_or(31) as i32;
                // a negative day counts from the end of the month
                let by_md = |d: &NaiveDate| {
                    bymonthday.is_empty() || bymonthday.iter().any(|md| d.day() as i32 == if *md > 0 { *md } else { len + 1 + *md })
                };
                let days = || first.iter_days().take(len as usize);
                let mut c: Vec<NaiveDate> = vec![];
                if !byday.is_empty() {
                    for (n, w) in &byday {
                        if *n != 0 {
                            c.extend(nth_weekday(y, m, *w, *n));
                        } else {
                            c.extend(days().filter(|d| d.weekday() == *w));
                        }
                    }
                    c.retain(by_md);
                } else if !bymonthday.is_empty() {
                    c.extend(days().filter(by_md));
                } else {
                    c.extend(NaiveDate::from_ymd_opt(y, m, d0.day()));
                }
                c
            };
            'm: for k in 0..2000i64 {
                // one round is a month, or the chosen months of a year
                let (y, months, first_m) = if yearly {
                    let y = d0.year() + (k * interval) as i32;
                    (y, if bymonth.is_empty() { vec![d0.month()] } else { bymonth.clone() }, 1)
                } else {
                    let m0 = d0.year() as i64 * 12 + d0.month0() as i64 + k * interval;
                    let (y, m) = (m0.div_euclid(12) as i32, m0.rem_euclid(12) as u32 + 1);
                    (y, if bymonth.is_empty() || bymonth.contains(&m) { vec![m] } else { vec![] }, m)
                };
                let mut cands: Vec<NaiveDate> = months.iter().flat_map(|m| in_month(y, *m)).collect();
                cands.sort();
                cands.dedup();
                if !bysetpos.is_empty() {
                    // "the n-th of those", counted from the end when negative
                    let n = cands.len() as i32;
                    let mut picked: Vec<NaiveDate> = bysetpos
                        .iter()
                        .filter_map(|p| if *p > 0 { Some(*p - 1) } else { Some(n + *p) })
                        .filter(|i| (0..n).contains(i))
                        .map(|i| cands[i as usize])
                        .collect();
                    picked.sort();
                    picked.dedup();
                    cands = picked;
                }
                for d in cands {
                    if !push(d, &mut out) {
                        break 'm;
                    }
                }
                if NaiveDate::from_ymd_opt(y, first_m, 1).map(|d| d.and_time(time) > limit).unwrap_or(true) {
                    break;
                }
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
        // a feed that leaves the UID out still needs a steady name
        let uid = if r.uid.is_empty() { format!("no-uid:{}:{}", start.key(all_day), title) } else { r.uid.clone() };
        // each start with the name of its occurrence
        let starts: Vec<(NaiveDateTime, Option<String>)> = match (&r.rrule, &r.recurrence_id) {
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
                .map(|w| (start.at(w), Some(When { wall: w, zone: start.zone.clone() }.key(all_day))))
                .filter(|(l, _)| !excluded.contains(l) && !overridden.contains(&(r.uid.clone(), *l)))
                .collect()
            }
            (_, rid) => vec![(start.local(), rid.as_ref().map(|w| w.key(all_day)))],
        };
        for (s, recurrence_id) in starts {
            let e = s + len;
            if e < from || s > to {
                continue;
            }
            events.push(FeedEvent {
                title: title.clone(),
                start: s,
                end: e,
                all_day,
                notes: notes.clone(),
                uid: uid.clone(),
                recurrence_id,
                href: None,
                etag: None,
            });
        }
    }
    events.sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
    Ok(Feed { name, events })
}

/// The parts of an event that can be changed here.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EventFields {
    pub summary: String,
    /// on this computer's clock
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub description: String,
    pub location: String,
    pub rrule: Option<String>,
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for l in text.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&l[1..]);
        } else if !l.is_empty() {
            lines.push(l.to_string());
        }
    }
    lines
}

/// Lines longer than 75 bytes go on with a space on the next line.
fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for l in lines {
        let mut used = 0;
        for c in l.chars() {
            if used + c.len_utf8() > 75 {
                out.push_str("\r\n ");
                used = 1;
            }
            out.push(c);
            used += c.len_utf8();
        }
        out.push_str("\r\n");
    }
    out
}

fn escape(v: &str) -> String {
    let mut out = String::new();
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str(r"\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Where the event to read or change sits among the lines: the main one
/// of a repeating event rather than a changed occurrence.
fn main_event(lines: &[String]) -> Option<(usize, usize)> {
    let is = |l: &String, what: &str| l.trim().eq_ignore_ascii_case(what);
    let mut found: Option<(usize, usize)> = None;
    let mut begin = None;
    for (i, l) in lines.iter().enumerate() {
        if is(l, "BEGIN:VEVENT") {
            begin = Some(i);
        } else if is(l, "END:VEVENT") {
            let Some(b) = begin.take() else { continue };
            let occurrence = lines[b..i].iter().any(|l| l.to_uppercase().starts_with("RECURRENCE-ID"));
            if !occurrence {
                return Some((b, i));
            }
            found = found.or(Some((b, i)));
        }
    }
    found
}

/// The event's own lines with how deep each sits: 0 for the event's own
/// properties, more inside an alarm.
fn own_lines(lines: &[String]) -> Vec<(usize, &String)> {
    let mut depth = 0usize;
    lines
        .iter()
        .map(|l| {
            let up = l.to_uppercase();
            if up.starts_with("END:") {
                depth = depth.saturating_sub(1);
                (depth, l)
            } else if up.starts_with("BEGIN:") {
                depth += 1;
                (depth - 1, l)
            } else {
                (depth, l)
            }
        })
        .collect()
}

/// Reads the changeable parts and the UID of an event as a server sent it.
// used from the CalDAV write path, which comes next
#[allow(dead_code)]
pub fn read_event(raw: &str) -> Option<(String, EventFields)> {
    let lines = unfold(raw);
    let (b, e) = main_event(&lines)?;
    let mut f = EventFields::default();
    let (mut uid, mut start, mut end, mut dur) = (String::new(), None, None, None);
    for (depth, line) in own_lines(&lines[b + 1..e]) {
        let Some((key, params, value)) = split_line(line) else { continue };
        if depth > 0 {
            continue;
        }
        match key.as_str() {
            "UID" => uid = value.trim().to_string(),
            "SUMMARY" => f.summary = unescape(value),
            "DESCRIPTION" => f.description = unescape(value),
            "LOCATION" => f.location = unescape(value),
            "RRULE" => f.rrule = Some(value.trim().to_string()),
            "DTSTART" => start = parse_when(value, &params),
            "DTEND" => end = parse_when(value, &params),
            "DURATION" => dur = parse_duration(value),
            _ => {}
        }
    }
    let (start, all_day) = start?;
    f.all_day = all_day;
    f.start = start.local();
    f.end = match (end, dur) {
        (Some((e, _)), _) => e.local(),
        (None, Some(d)) => f.start + d,
        (None, None) if all_day => f.start + Duration::days(1),
        (None, None) => f.start,
    };
    Some((uid, f))
}

/// Writes an event for a server. With `raw`, the event as the server sent
/// it, only the title, times, notes, place and repeat rule are changed,
/// and only where they differ: alarms, guests and everything this program
/// does not know stay as they were. `stamp` is the time now, in UTC.
// used from the CalDAV write path, which comes next
#[allow(dead_code)]
pub fn write_event(raw: Option<&str>, uid: &str, f: &EventFields, stamp: NaiveDateTime) -> String {
    let fresh = || -> Vec<String> {
        ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//kalendar//EN", "BEGIN:VEVENT", &format!("UID:{uid}"), "END:VEVENT", "END:VCALENDAR"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    let mut lines = raw.map(unfold).unwrap_or_else(fresh);
    let mut is_new = raw.is_none();
    let (b, e) = match main_event(&lines) {
        Some(x) => x,
        None => {
            // nothing usable was kept: start over
            lines = fresh();
            is_new = true;
            (3, 5)
        }
    };

    // what the event holds now
    let (mut old_start, mut old_end, mut old_dur) = (None, None, None);
    for (depth, line) in own_lines(&lines[b + 1..e]) {
        let Some((key, params, value)) = split_line(line) else { continue };
        if depth > 0 {
            continue;
        }
        match key.as_str() {
            "DTSTART" => old_start = parse_when(value, &params).map(|w| (w, params.clone())),
            "DTEND" => old_end = parse_when(value, &params),
            "DURATION" => old_dur = parse_duration(value),
            _ => {}
        }
    }
    // times are written in the zone the event already uses, else in UTC
    let zone: Option<(String, chrono_tz::Tz)> = old_start.as_ref().and_then(|((w, _), params)| match &w.zone {
        Zone::Named(tz) => params.iter().find(|(k, _)| k == "TZID").map(|(_, id)| (id.clone(), *tz)),
        _ => None,
    });
    let when = |name: &str, t: NaiveDateTime| -> String {
        if f.all_day {
            return format!("{name};VALUE=DATE:{}", t.format("%Y%m%d"));
        }
        let here = Local.from_local_datetime(&t).earliest();
        match (&zone, here) {
            (Some((id, tz)), Some(h)) => format!("{name};TZID={id}:{}", h.with_timezone(tz).naive_local().format("%Y%m%dT%H%M%S")),
            (_, Some(h)) => format!("{name}:{}Z", h.naive_utc().format("%Y%m%dT%H%M%S")),
            (_, None) => format!("{name}:{}", t.format("%Y%m%dT%H%M%S")),
        }
    };
    let same_when = |old: &Option<(When, bool)>, t: NaiveDateTime| old.as_ref().is_some_and(|(w, all_day)| *all_day == f.all_day && w.local() == t);
    let start_same = same_when(&old_start.as_ref().map(|(w, _)| w.clone()), f.start);
    // an event given as start and length keeps that form while both hold
    let keep_duration = old_end.is_none() && start_same && old_dur.is_some_and(|d| old_start.as_ref().map(|((w, _), _)| w.local() + d) == Some(f.end));
    let text = |name: &str, v: &str| (!v.trim().is_empty()).then(|| format!("{name}:{}", escape(v)));
    // the new line of each changeable property; None leaves it out
    let wanted: Vec<(&str, Option<String>)> = vec![
        ("DTSTART", Some(when("DTSTART", f.start))),
        ("DTEND", (!keep_duration).then(|| when("DTEND", f.end))),
        ("SUMMARY", text("SUMMARY", &f.summary)),
        ("DESCRIPTION", text("DESCRIPTION", &f.description)),
        ("LOCATION", text("LOCATION", &f.location)),
        ("RRULE", f.rrule.as_ref().filter(|r| !r.trim().is_empty()).map(|r| format!("RRULE:{}", r.trim()))),
    ];
    let stamp_text = stamp.format("%Y%m%dT%H%M%SZ").to_string();

    let mut out: Vec<String> = lines[..=b].to_vec();
    let mut seen: HashSet<String> = HashSet::new();
    for (depth, line) in own_lines(&lines[b + 1..e]) {
        let parts = split_line(line).filter(|_| depth == 0);
        let Some((key, params, value)) = parts else {
            out.push(line.clone());
            continue;
        };
        if let Some((_, new)) = wanted.iter().find(|(n, _)| *n == key) {
            if !seen.insert(key.clone()) {
                continue; // a second copy of a property that may appear once
            }
            let same = match key.as_str() {
                "DTSTART" => start_same,
                "DTEND" => new.is_some() && same_when(&parse_when(value, &params), f.end),
                "RRULE" => f.rrule.as_deref().map(str::trim) == Some(value.trim()),
                "SUMMARY" => unescape(value) == f.summary,
                "DESCRIPTION" => unescape(value) == f.description,
                _ => unescape(value) == f.location,
            };
            if same {
                out.push(line.clone());
            } else if let Some(n) = new {
                out.push(n.clone());
            }
            continue;
        }
        match key.as_str() {
            "DURATION" if !keep_duration => {}
            "SEQUENCE" => {
                seen.insert(key);
                out.push(format!("SEQUENCE:{}", value.trim().parse::<i64>().unwrap_or(0) + 1));
            }
            "DTSTAMP" | "LAST-MODIFIED" => {
                seen.insert(key.clone());
                out.push(format!("{key}:{stamp_text}"));
            }
            _ => out.push(line.clone()),
        }
    }
    for (name, new) in &wanted {
        if let Some(n) = new.as_ref().filter(|_| !seen.contains(*name)) {
            out.push(n.clone());
        }
    }
    if !seen.contains("SEQUENCE") {
        out.push(format!("SEQUENCE:{}", if is_new { 0 } else { 1 }));
    }
    for name in ["DTSTAMP", "LAST-MODIFIED"] {
        if !seen.contains(name) {
            out.push(format!("{name}:{stamp_text}"));
        }
    }
    out.extend_from_slice(&lines[e..]);
    fold(&out)
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
    fn uid_and_occurrence_are_kept() {
        let text = wrap("BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Zurich:20261102T090000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:w\r\nRECURRENCE-ID;TZID=Europe/Zurich:20261109T090000\r\nSUMMARY:Moved\r\nDTSTART;TZID=Europe/Zurich:20261109T110000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:No uid\r\nDTSTART;VALUE=DATE:20261120\r\nEND:VEVENT\r\n");
        let f = parse(&text, now()).unwrap();
        let keys: Vec<(&str, Option<&str>, &str)> = f.events.iter().map(|e| (e.uid.as_str(), e.recurrence_id.as_deref(), e.title.as_str())).collect();
        // 09:00 in Zurich is 08:00 UTC in winter; the moved one keeps the name of the one it replaces
        assert_eq!(
            keys,
            vec![
                ("w", Some("20261102T080000Z"), "Standup"),
                ("w", Some("20261109T080000Z"), "Moved"),
                ("w", Some("20261116T080000Z"), "Standup"),
                ("no-uid:20261120:No uid", None, "No uid"),
            ]
        );
        // read again, every event has the same name
        let again = parse(&text, now()).unwrap();
        assert_eq!(f.events, again.events);
    }

    fn starts(rule: &str, dtstart: &str) -> Vec<String> {
        let text = wrap(&format!("BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:X\r\nDTSTART:{dtstart}\r\nRRULE:{rule}\r\nEND:VEVENT\r\n"));
        parse(&text, now()).unwrap().events.iter().map(|e| e.start.format("%Y-%m-%d").to_string()).collect()
    }

    #[test]
    fn last_weekday_of_the_month() {
        // Oct 30 is a Friday, Nov 30 a Monday, Dec 31 a Thursday, Jan 29 a Friday
        assert_eq!(
            starts("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=4", "20261030T170000"),
            ["2026-10-30", "2026-11-30", "2026-12-31", "2027-01-29"]
        );
        // first and last working day of each month
        assert_eq!(
            starts("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1,-1;COUNT=4", "20261102T090000"),
            ["2026-11-02", "2026-11-30", "2026-12-01", "2026-12-31"]
        );
        // the last day of the month, whatever its length
        assert_eq!(starts("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=4", "20261130T090000"), ["2026-11-30", "2026-12-31", "2027-01-31", "2027-02-28"]);
        // every Tuesday of the month, without a number before the day
        assert_eq!(starts("FREQ=MONTHLY;BYDAY=TU;COUNT=5", "20261103T090000"), ["2026-11-03", "2026-11-10", "2026-11-17", "2026-11-24", "2026-12-01"]);
    }

    #[test]
    fn yearly_by_month() {
        // the fourth Thursday of November
        assert_eq!(starts("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;COUNT=2", "20261126T120000"), ["2026-11-26", "2027-11-25"]);
        // twice a year, on the day of the month it started on
        assert_eq!(starts("FREQ=YEARLY;BYMONTH=6,12;COUNT=3", "20261210T120000"), ["2026-12-10", "2027-06-10", "2027-12-10"]);
        // the 15th of the given months
        assert_eq!(starts("FREQ=YEARLY;BYMONTH=1,12;BYMONTHDAY=15;COUNT=3", "20261215T120000"), ["2026-12-15", "2027-01-15", "2027-12-15"]);
        // monthly, but only in some months
        assert_eq!(starts("FREQ=MONTHLY;BYMONTH=11,1;COUNT=3", "20261105T120000"), ["2026-11-05", "2027-01-05", "2027-11-05"]);
        // daily in December only
        assert_eq!(starts("FREQ=DAILY;BYMONTH=12;COUNT=2", "20261129T120000"), ["2026-12-01", "2026-12-02"]);
    }

    #[test]
    fn week_start_changes_every_other_week() {
        // the example of RFC 5545, moved to 2026: Tuesday Nov 3 is the start
        assert_eq!(starts("FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=MO", "20261103T090000"), ["2026-11-03", "2026-11-08", "2026-11-17", "2026-11-22"]);
        assert_eq!(starts("FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=SU", "20261103T090000"), ["2026-11-03", "2026-11-15", "2026-11-17", "2026-11-29"]);
    }

    #[test]
    fn windows_and_prefixed_zones() {
        let f = parse(
            &wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A\r\nDTSTART;TZID=Europe/Berlin:20261116T160000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:B\r\nDTSTART;TZID=\"W. Europe Standard Time\":20261116T160000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c\r\nSUMMARY:C\r\nDTSTART;TZID=/mozilla.org/20050126_1/Europe/Berlin:20261116T160000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:d\r\nSUMMARY:D\r\nDTSTART:20261116T150000Z\r\nEND:VEVENT\r\n"),
            now(),
        )
        .unwrap();
        assert_eq!(f.events.len(), 4);
        for e in &f.events {
            assert_eq!(e.start, f.events[3].start, "{} is at another time", e.title);
        }
    }

    const FULL: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Zurich\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:abc@example.org\r\nDTSTAMP:20260101T000000Z\r\nSEQUENCE:3\r\nSUMMARY;LANGUAGE=en:Review\r\nDTSTART;TZID=Europe/Zurich:20261116T160000\r\nDTEND;TZID=Europe/Zurich:20261116T170000\r\nLOCATION:Room 4\r\nORGANIZER;CN=Sam:mailto:sam@example.org\r\nATTENDEE;PARTSTAT=ACCEPTED;CN=A very long name that makes this line run past the limit:mailto:alex@example.org\r\nX-CUSTOM-THING:keep me\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Reminder\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn stamp() -> NaiveDateTime {
        at(2026, 11, 14, 20, 0)
    }

    #[test]
    fn writing_back_keeps_what_is_not_known() {
        let (uid, mut f) = read_event(FULL).unwrap();
        assert_eq!((uid.as_str(), f.summary.as_str(), f.location.as_str()), ("abc@example.org", "Review", "Room 4"));
        // nothing changed: only the stamp and the sequence number move
        let same = unfold(&write_event(Some(FULL), &uid, &f, stamp()));
        let before = unfold(FULL);
        assert_eq!(same.len(), before.len() + 1, "only LAST-MODIFIED is added");
        for l in &before {
            if l.starts_with("DTSTAMP") || l.starts_with("SEQUENCE") {
                continue;
            }
            assert!(same.contains(l), "lost: {l}");
        }
        assert!(same.contains(&"SEQUENCE:4".to_string()) && same.contains(&"DTSTAMP:20261114T200000Z".to_string()));

        // title, time and notes changed
        f.summary = "Review, part 2".into();
        f.start += Duration::hours(1);
        f.end += Duration::hours(2);
        f.description = "Bring\nnotes".into();
        f.location.clear();
        let text = write_event(Some(FULL), &uid, &f, stamp());
        assert!(text.lines().all(|l| l.len() <= 75), "a line is too long");
        let out = unfold(&text);
        for keep in ["X-CUSTOM-THING:keep me", "ORGANIZER;CN=Sam:mailto:sam@example.org", "TRIGGER:-PT10M", "DESCRIPTION:Reminder", "TZID:Europe/Zurich", "PRODID:-//Other//EN"] {
            assert!(out.contains(&keep.to_string()), "lost: {keep}");
        }
        assert!(out.iter().any(|l| l.starts_with("ATTENDEE;PARTSTAT=ACCEPTED;CN=A very long name") && l.ends_with("mailto:alex@example.org")));
        assert!(out.contains(&"SUMMARY:Review\\, part 2".to_string()));
        assert!(out.contains(&"DTSTART;TZID=Europe/Zurich:20261116T170000".to_string()), "the event's own zone is kept");
        assert!(out.contains(&"DTEND;TZID=Europe/Zurich:20261116T190000".to_string()));
        assert!(out.contains(&"DESCRIPTION:Bring\\nnotes".to_string()));
        assert!(!out.iter().any(|l| l.starts_with("LOCATION")));
        // and it reads back as what was written
        let (uid2, back) = read_event(&text).unwrap();
        assert_eq!((uid2, back), (uid, f));
    }

    #[test]
    fn writing_a_new_event() {
        let f = EventFields { summary: "Lunch; with Sam".into(), start: at(2026, 11, 20, 0, 0), end: at(2026, 11, 22, 0, 0), all_day: true, ..Default::default() };
        let text = write_event(None, "new-1", &f, stamp());
        let out = unfold(&text);
        assert_eq!(out.first().map(String::as_str), Some("BEGIN:VCALENDAR"));
        for want in ["UID:new-1", "DTSTART;VALUE=DATE:20261120", "DTEND;VALUE=DATE:20261122", r"SUMMARY:Lunch\; with Sam", "SEQUENCE:0", "DTSTAMP:20261114T200000Z"] {
            assert!(out.contains(&want.to_string()), "missing: {want}");
        }
        assert_eq!(read_event(&text).unwrap(), ("new-1".to_string(), f));
        // a timed one goes out in UTC and comes back at the same local time
        let t = EventFields { summary: "Call".into(), start: at(2026, 11, 20, 9, 0), end: at(2026, 11, 20, 9, 30), ..Default::default() };
        let text = write_event(None, "new-2", &t, stamp());
        assert!(unfold(&text).iter().any(|l| l.starts_with("DTSTART:2026") && l.ends_with('Z')));
        assert_eq!(read_event(&text).unwrap().1, t);
        // an event given as start and length keeps that form until its length changes
        let dur = "BEGIN:VEVENT\r\nUID:d\r\nDTSTART:20261120T090000\r\nDURATION:PT30M\r\nEND:VEVENT\r\n";
        let (_, mut d) = read_event(dur).unwrap();
        assert!(write_event(Some(dur), "d", &d, stamp()).contains("DURATION:PT30M"));
        d.end += Duration::minutes(15);
        let longer = write_event(Some(dur), "d", &d, stamp());
        assert!(!longer.contains("DURATION") && longer.contains("DTEND:"));
    }

    #[test]
    fn not_a_calendar() {
        assert!(parse("<html>hello</html>", now()).is_err());
    }
}
