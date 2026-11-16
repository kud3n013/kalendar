//! The little language of the quick task field, and durations as text.
//!
//! A line such as `Call Ana tomorrow at 5pm for 30m #work !high` becomes a
//! task: the title is what is left once the marked parts are picked out.
//! Which marks mean what comes from a preset, or the person's own set; plain
//! dates and times ("fri", "12/4", "5pm") are understood without any mark.
//! Words in "double quotes" are always kept in the title as they are.

use super::Repeat;
use crate::strings::{tr, trf};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

/// The marks of a well-known app, or the person's own.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub enum QuickPreset {
    #[default]
    Kalendar,
    Todoist,
    TickTick,
    Things,
    Custom,
}

impl QuickPreset {
    pub const ALL: [QuickPreset; 5] = [QuickPreset::Kalendar, QuickPreset::Todoist, QuickPreset::TickTick, QuickPreset::Things, QuickPreset::Custom];
    pub fn label(&self) -> &'static str {
        match self {
            QuickPreset::Kalendar => tr("Kalendar (like Morgen)"),
            QuickPreset::Todoist => tr("Todoist"),
            QuickPreset::TickTick => tr("TickTick"),
            QuickPreset::Things => tr("Plain words (like Things)"),
            QuickPreset::Custom => tr("My own"),
        }
    }
    /// The marks this preset uses; `Custom` has none of its own.
    pub fn tokens(&self) -> Option<Tokens> {
        let t = |tag: &str, list: &str, priority: &str, date: &str, time: &str, duration: &str| Tokens {
            tag: tag.into(),
            list: list.into(),
            priority: priority.into(),
            date: date.into(),
            time: time.into(),
            duration: duration.into(),
        };
        match self {
            QuickPreset::Kalendar => Some(t("#", ">", "!", ".", "@ at", "for ~")),
            QuickPreset::Todoist => Some(t("@ %", "#", "p", "", "at", "for")),
            QuickPreset::TickTick => Some(t("#", "~ ^", "!", "", "at", "for")),
            // Things reads no marks in the title, only dates and times
            QuickPreset::Things => Some(t("", "", "", "", "at", "for")),
            QuickPreset::Custom => None,
        }
    }
}

/// What marks each kind of information. A field holds one or more marks
/// split by spaces. A mark of two or more letters ("at", "for") is a word of
/// its own before the value; any other ("#", "@", "p") is written onto it.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Tokens {
    pub tag: String,
    pub list: String,
    pub priority: String,
    pub date: String,
    pub time: String,
    pub duration: String,
}

impl Default for Tokens {
    fn default() -> Self {
        QuickPreset::Kalendar.tokens().unwrap()
    }
}

/// The kinds of information a mark can stand for, for the settings page.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Tag,
    List,
    Priority,
    Date,
    Time,
    Duration,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Tag, Kind::List, Kind::Priority, Kind::Date, Kind::Time, Kind::Duration];
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Tag => tr("Tag"),
            Kind::List => tr("List"),
            Kind::Priority => tr("Priority"),
            Kind::Date => tr("Date"),
            Kind::Time => tr("Time"),
            Kind::Duration => tr("Duration"),
        }
    }
    fn example(&self, priority_mark: &str) -> &'static str {
        match self {
            Kind::Tag => "work",
            Kind::List => "Errands",
            Kind::Priority if priority_mark.chars().all(char::is_alphabetic) => "1",
            Kind::Priority => "high",
            Kind::Date => "fri",
            Kind::Time => "5pm",
            Kind::Duration => "30m",
        }
    }
}

impl Tokens {
    pub fn get(&self, k: Kind) -> &String {
        match k {
            Kind::Tag => &self.tag,
            Kind::List => &self.list,
            Kind::Priority => &self.priority,
            Kind::Date => &self.date,
            Kind::Time => &self.time,
            Kind::Duration => &self.duration,
        }
    }
    pub fn get_mut(&mut self, k: Kind) -> &mut String {
        match k {
            Kind::Tag => &mut self.tag,
            Kind::List => &mut self.list,
            Kind::Priority => &mut self.priority,
            Kind::Date => &mut self.date,
            Kind::Time => &mut self.time,
            Kind::Duration => &mut self.duration,
        }
    }
    /// How a kind is written with its first mark, e.g. "#work" or "at 5pm";
    /// empty when it has no mark.
    pub fn sample(&self, k: Kind) -> String {
        let Some(m) = self.get(k).split_whitespace().next() else { return String::new() };
        let ex = k.example(m);
        if is_word_mark(m) { format!("{m} {ex}") } else { format!("{m}{ex}") }
    }
    /// The hint for the quick task field.
    pub fn hint(&self) -> String {
        let parts: Vec<String> = [Kind::Tag, Kind::Priority, Kind::List].iter().map(|k| self.sample(*k)).filter(|s| !s.is_empty()).collect();
        trf("New task   tomorrow 5pm  30m  {}", &[&parts.join("  ")])
    }
}

/// How the quick task field reads what is typed.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct QuickSyntax {
    pub preset: QuickPreset,
    /// the marks used when the preset is `Custom`
    pub custom: Tokens,
    /// plain dates, times and repeats ("fri 5pm", "every monday") without a mark
    pub natural: bool,
    /// "12/4" is the 12th of April; else December 4th
    pub day_first: bool,
    /// the person's own words, each standing for others: "tmr" -> "tomorrow"
    pub abbrevs: Vec<(String, String)>,
}

impl Default for QuickSyntax {
    fn default() -> Self {
        QuickSyntax { preset: QuickPreset::Kalendar, custom: Tokens::default(), natural: true, day_first: true, abbrevs: vec![] }
    }
}

impl QuickSyntax {
    pub fn tokens(&self) -> Tokens {
        self.preset.tokens().unwrap_or_else(|| self.custom.clone())
    }
}

/// What a line typed in the quick task field means.
#[derive(Debug, Default, PartialEq)]
pub struct Quick {
    pub title: String,
    pub tags: Vec<String>,
    pub priority: u8,
    pub list_name: Option<String>,
    pub duration_min: Option<i64>,
    pub due: Option<NaiveDate>,
    /// a clock time puts the task straight on the calendar
    pub time: Option<NaiveTime>,
    /// "@24", "midnight": the midnight at the end of the day
    pub time_next_day: bool,
    pub repeat: Repeat,
}

impl Quick {
    /// When the task goes on the calendar, if a time was given: on its date,
    /// else today, or tomorrow once that time has passed today.
    pub fn start(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        let t = self.time?;
        let day = match self.due {
            Some(d) => d,
            None if !self.time_next_day && t <= now.time() => now.date() + Duration::days(1),
            None => now.date(),
        };
        let at = day.and_time(t);
        Some(if self.time_next_day { at + Duration::days(1) } else { at })
    }

    /// The parts picked out, as short labels to show under the field.
    pub fn chips(&self, now: NaiveDateTime, hour12: bool) -> Vec<String> {
        let mut v = vec![];
        match self.start(now) {
            Some(s) => {
                let t = if hour12 { s.format("%-I:%M %p").to_string() } else { s.format("%H:%M").to_string() };
                v.push(format!("{} {}  {t}", crate::icons::CALENDAR, day_label(s.date(), now.date())));
            }
            None => {
                if let Some(d) = self.due {
                    v.push(trf("Due {}", &[&day_label(d, now.date())]));
                }
            }
        }
        if let Some(m) = self.duration_min {
            v.push(fmt_duration(m));
        }
        if self.repeat != Repeat::None {
            v.push(self.repeat.label().into());
        }
        if self.priority != 0 {
            v.push(trf("{} priority", &[&priority_name(self.priority)]));
        }
        if let Some(l) = &self.list_name {
            v.push(format!("{}  {l}", crate::icons::LIST_TODO));
        }
        for t in &self.tags {
            v.push(format!("#{t}"));
        }
        v
    }
}

fn day_label(d: NaiveDate, today: NaiveDate) -> String {
    match (d - today).num_days() {
        0 => tr("Today").into(),
        1 => tr("Tomorrow").into(),
        2..=6 => d.format("%A").to_string(),
        _ if d.year() == today.year() => d.format("%a %b %-d").to_string(),
        _ => d.format("%a %b %-d %Y").to_string(),
    }
}

/// A mark that is a word of its own, written before its value.
fn is_word_mark(m: &str) -> bool {
    m.chars().count() >= 2 && m.chars().all(char::is_alphabetic)
}

struct Word {
    text: String,
    /// inside quotes: never read as anything but title
    literal: bool,
}

/// The words of a line, quoted parts kept apart, abbreviations spelled out.
fn words(text: &str, abbrevs: &[(String, String)]) -> Vec<Word> {
    let mut out = vec![];
    for (i, part) in text.split('"').enumerate() {
        let literal = i % 2 == 1;
        for w in part.split_whitespace() {
            let found = if literal { None } else { abbrevs.iter().find(|(k, _)| !k.trim().is_empty() && k.trim().eq_ignore_ascii_case(w)) };
            match found {
                Some((_, long)) => out.extend(long.split_whitespace().map(|x| Word { text: x.to_string(), literal: false })),
                None => out.push(Word { text: w.to_string(), literal }),
            }
        }
    }
    out
}

pub fn parse_quick(text: &str, today: NaiveDate, syntax: &QuickSyntax) -> Quick {
    let tok = syntax.tokens();
    let ws = words(text, &syntax.abbrevs);
    let low: Vec<String> = ws.iter().map(|w| w.text.to_lowercase().trim_end_matches(',').to_string()).collect();
    let mut q = Quick::default();
    let mut title: Vec<&str> = vec![];
    let mut i = 0;
    while i < ws.len() {
        if ws[i].literal {
            title.push(&ws[i].text);
            i += 1;
            continue;
        }
        // the words from here on that may be read together, up to a quoted one
        let end = ws[i..].iter().position(|w| w.literal).map(|p| i + p).unwrap_or(ws.len());
        let rest: Vec<&str> = low[i..end].iter().map(|s| s.as_str()).collect();
        match read_one(&ws[i].text, &rest, &tok, syntax, today, &mut q) {
            Some(n) => i += n,
            None => {
                title.push(&ws[i].text);
                i += 1;
            }
        }
    }
    q.title = title.join(" ");
    q
}

/// The end of `orig` (a word as typed) that matches `v` (taken from its
/// lower-cased form), so tags and lists keep the case they were typed in.
fn as_typed(orig: &str, v: &str) -> String {
    let o = orig.trim_end_matches(',');
    let at = o.len().saturating_sub(v.len());
    if o.is_char_boundary(at) && o[at..].to_lowercase() == v { o[at..].to_string() } else { v.to_string() }
}

/// Reads one piece of information at the start of `rest` (lower-cased
/// words; `orig` is the first as typed) into `q`; how many words it took.
fn read_one(orig: &str, rest: &[&str], tok: &Tokens, syn: &QuickSyntax, today: NaiveDate, q: &mut Quick) -> Option<usize> {
    let w = rest[0];
    let df = syn.day_first;
    // a marked value: the rest of this word, or for a word mark the next words
    let marked = |field: &str, read: &mut dyn FnMut(&[&str], bool) -> Option<usize>| -> Option<usize> {
        for m in field.split_whitespace() {
            let ml = m.to_lowercase();
            if is_word_mark(&ml) {
                if w == ml && rest.len() > 1 {
                    if let Some(n) = read(&rest[1..], true) {
                        return Some(n + 1);
                    }
                }
            } else if let Some(v) = w.strip_prefix(ml.as_str()) {
                let mut vs = vec![v];
                vs.extend_from_slice(&rest[1..]);
                if let Some(n) = read(&vs, false) {
                    return Some(n);
                }
            }
        }
        None
    };
    let tag = marked(&tok.tag, &mut |vs, own| {
        (!own && !vs[0].is_empty()).then(|| {
            q.tags.push(as_typed(orig, vs[0]));
            1
        })
    });
    if tag.is_some() {
        return tag;
    }
    let list = marked(&tok.list, &mut |vs, own| {
        (!own && !vs[0].is_empty()).then(|| {
            q.list_name = Some(as_typed(orig, vs[0]).replace('_', " "));
            1
        })
    });
    if list.is_some() {
        return list;
    }
    let prio = marked(&tok.priority, &mut |vs, _| {
        q.priority = priority_word(vs[0], &tok.priority)?;
        Some(1)
    });
    if prio.is_some() {
        return prio;
    }
    let date = marked(&tok.date, &mut |vs, _| {
        let (d, n) = date_at(vs, today, df)?;
        q.due = Some(d);
        Some(n)
    });
    if date.is_some() {
        return date;
    }
    let time = marked(&tok.time, &mut |vs, _| {
        let (t, next, len, n) = time_at(vs, true)?;
        set_time(q, t, next, len);
        Some(n)
    });
    if time.is_some() {
        return time;
    }
    let dur = marked(&tok.duration, &mut |vs, _| {
        let (m, n) = duration_at(vs)?;
        q.duration_min = Some(m);
        Some(n)
    });
    if dur.is_some() {
        return dur;
    }
    if !syn.natural {
        let m = parse_duration(w)?;
        q.duration_min = Some(m);
        return Some(1);
    }
    if let Some((r, d, n)) = repeat_at(rest, today) {
        q.repeat = r;
        if d.is_some() {
            q.due = d;
        }
        return Some(n);
    }
    // "on fri", "by 12/4"
    let skip = usize::from(matches!(w, "on" | "by") && rest.len() > 1);
    if let Some((d, n)) = date_at(&rest[skip..], today, df) {
        q.due = Some(d);
        return Some(n + skip);
    }
    if let Some((t, next, len, n)) = time_at(rest, false) {
        set_time(q, t, next, len);
        return Some(n);
    }
    let (m, n) = duration_at(rest)?;
    q.duration_min = Some(m);
    Some(n)
}

fn set_time(q: &mut Quick, t: NaiveTime, next: bool, len: Option<i64>) {
    q.time = Some(t);
    q.time_next_day = next;
    if len.is_some() {
        q.duration_min = len;
    }
}

/// "high", "h", "1" … after a priority mark; "" (the mark alone) is high,
/// and "!!!" high, "!!" medium. A mark of letters, like Todoist's "p",
/// needs a number: p1 high, p2 medium, p3 low, p4 none.
fn priority_word(v: &str, field: &str) -> Option<u8> {
    let by_number = |v: &str| match v {
        "1" => Some(1),
        "2" | "5" => Some(5),
        "3" | "9" => Some(9),
        "4" => Some(0),
        _ => None,
    };
    if field.split_whitespace().any(|m| m.chars().all(char::is_alphabetic)) {
        return by_number(v);
    }
    match v {
        "" | "high" | "hi" | "h" | "!!" => Some(1),
        "medium" | "med" | "m" | "!" => Some(5),
        "low" | "lo" | "l" => Some(9),
        "none" | "no" => Some(0),
        _ => by_number(v),
    }
}

fn weekday(w: &str) -> Option<Weekday> {
    use Weekday::*;
    Some(match w {
        "mon" | "monday" => Mon,
        "tue" | "tues" | "tuesday" => Tue,
        "wed" | "weds" | "wednesday" => Wed,
        "thu" | "thur" | "thurs" | "thursday" => Thu,
        "fri" | "friday" => Fri,
        "sat" | "saturday" => Sat,
        "sun" | "sunday" => Sun,
        _ => return None,
    })
}

fn month(w: &str) -> Option<u32> {
    let short = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let full = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    let w = w.trim_end_matches('.');
    short.iter().position(|x| *x == w).or_else(|| full.iter().position(|x| *x == w)).or_else(|| (w == "sept").then_some(8)).map(|i| i as u32 + 1)
}

/// "12", "12th", "1st" as a day of the month.
fn ordinal(w: &str) -> Option<u32> {
    let n = w.trim_end_matches(|c: char| c.is_alphabetic());
    if !matches!(&w[n.len()..], "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    n.parse().ok().filter(|d| (1..=31).contains(d))
}

fn small_number(w: &str) -> Option<i64> {
    Some(match w {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "ten" => 10,
        _ => w.parse().ok().filter(|n| (1..=999).contains(n))?,
    })
}

/// The first `wd` on or after `from`.
fn on_or_after(from: NaiveDate, wd: Weekday) -> NaiveDate {
    let ahead = (wd.num_days_from_monday() as i64 - from.weekday().num_days_from_monday() as i64).rem_euclid(7);
    from + Duration::days(ahead)
}

fn add_months(d: NaiveDate, n: i64) -> NaiveDate {
    d.checked_add_months(chrono::Months::new(n as u32)).unwrap_or(d + Duration::days(30 * n))
}

/// A day with no year given: this year's, or next year's once it has passed.
fn upcoming(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this < today { NaiveDate::from_ymd_opt(today.year() + 1, m, d) } else { Some(this) }
}

fn year_word(w: Option<&&str>) -> Option<i32> {
    w.and_then(|w| w.parse::<i32>().ok()).filter(|y| (2000..=2200).contains(y))
}

/// A date at the start of `ws`, and how many words it took.
pub fn date_at(ws: &[&str], today: NaiveDate, day_first: bool) -> Option<(NaiveDate, usize)> {
    let w0 = *ws.first()?;
    let w1 = ws.get(1).copied().unwrap_or("");
    match w0 {
        "today" | "tod" | "tdy" => return Some((today, 1)),
        "tomorrow" | "tmr" | "tmrw" | "tmw" => return Some((today + Duration::days(1), 1)),
        _ => {}
    }
    if let Some(wd) = weekday(w0) {
        return Some((on_or_after(today, wd), 1));
    }
    if w0 == "this" {
        return weekday(w1).map(|wd| (on_or_after(today, wd), 2));
    }
    if w0 == "next" {
        let next_monday = today + Duration::days(7 - today.weekday().num_days_from_monday() as i64);
        if let Some(wd) = weekday(w1) {
            return Some((on_or_after(next_monday, wd), 2));
        }
        return match w1 {
            "week" => Some((next_monday, 2)),
            "month" => Some((add_months(today.with_day(1)?, 1), 2)),
            _ => None,
        };
    }
    if w0 == "in" {
        // "in 3 days", "in a week", "in 2w"
        let (n, unit, used) = match small_number(w1) {
            Some(n) => (n, *ws.get(2)?, 3),
            None => {
                let num = w1.trim_end_matches(|c: char| c.is_alphabetic());
                (num.parse().ok()?, &w1[num.len()..], 2)
            }
        };
        let d = match unit {
            "d" | "day" | "days" => today + Duration::days(n),
            "w" | "wk" | "wks" | "week" | "weeks" => today + Duration::weeks(n),
            "mo" | "month" | "months" => add_months(today, n),
            _ => return None,
        };
        return Some((d, used));
    }
    if let Ok(d) = NaiveDate::parse_from_str(w0, "%Y-%m-%d") {
        return Some((d, 1));
    }
    // 12/4 and 12/4/2027
    let nums: Vec<&str> = w0.split('/').collect();
    if (2..=3).contains(&nums.len()) && nums.iter().all(|n| !n.is_empty() && n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit())) {
        let (a, b): (u32, u32) = (nums[0].parse().ok()?, nums[1].parse().ok()?);
        let (d, m) = if day_first { (a, b) } else { (b, a) };
        return match nums.get(2) {
            Some(y) => {
                let y: i32 = y.parse().ok()?;
                let y = if y < 100 { 2000 + y } else { y };
                NaiveDate::from_ymd_opt(y, m, d).map(|x| (x, 1))
            }
            None => upcoming(today, m, d).map(|x| (x, 1)),
        };
    }
    // "apr 12", "april 12th 2027", "12 apr", "12th of april"
    if let (Some(m), Some(d)) = (month(w0), ordinal(w1)) {
        return Some(match year_word(ws.get(2)) {
            Some(y) => (NaiveDate::from_ymd_opt(y, m, d)?, 3),
            None => (upcoming(today, m, d)?, 2),
        });
    }
    if let Some(d) = ordinal(w0) {
        let (m, used) = match month(w1) {
            Some(m) => (m, 2),
            None if w1 == "of" => (month(ws.get(2)?)?, 3),
            None => return None,
        };
        return Some(match year_word(ws.get(used)) {
            Some(y) => (NaiveDate::from_ymd_opt(y, m, d)?, used + 1),
            None => (upcoming(today, m, d)?, used),
        });
    }
    None
}

/// Hours and minutes from "5", "5pm", "5:30", "17:00", "9a"; with whether
/// pm (or am) was said.
fn clock(w: &str) -> Option<(u32, u32, Option<bool>)> {
    let w = w.replace('.', "");
    let (num, pm) = if let Some(n) = w.strip_suffix("am").or_else(|| w.strip_suffix('a')) {
        (n, Some(false))
    } else if let Some(n) = w.strip_suffix("pm").or_else(|| w.strip_suffix('p')) {
        (n, Some(true))
    } else {
        (w.as_str(), None)
    };
    if !num.chars().next()?.is_ascii_digit() {
        return None;
    }
    let (h, m) = match num.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse().ok()?, m.parse().ok()?),
        Some(_) => return None,
        None if num.len() <= 2 => (num.parse().ok()?, 0),
        None => return None,
    };
    (m <= 59).then_some((h, m, pm))
}

/// A clock reading made a time of day, with whether it is the midnight at
/// the end of the day ("24", "24:00").
fn to_time(h: u32, m: u32, pm: Option<bool>) -> Option<(NaiveTime, bool)> {
    match pm {
        Some(pm) if (1..=12).contains(&h) => Some((NaiveTime::from_hms_opt(h % 12 + if pm { 12 } else { 0 }, m, 0)?, false)),
        Some(_) => None,
        None if h == 24 && m == 0 => Some((NaiveTime::MIN, true)),
        None => Some((NaiveTime::from_hms_opt(h, m, 0)?, false)),
    }
}

/// A time at the start of `ws`, with whether it is the next midnight, the
/// length of a range ("5-6pm"), and how many words it took. Without a mark
/// a bare number is not taken: "5pm" and "17:00" are, "5" is not.
fn time_at(ws: &[&str], marked: bool) -> Option<(NaiveTime, bool, Option<i64>, usize)> {
    let w0 = *ws.first()?;
    match w0 {
        "noon" | "midday" => return Some((NaiveTime::from_hms_opt(12, 0, 0)?, false, None, 1)),
        "midnight" => return Some((NaiveTime::MIN, true, None, 1)),
        _ => {}
    }
    let sure = |h: u32, m: u32, pm: Option<bool>, raw: &str| -> Option<(NaiveTime, bool)> {
        if !marked && pm.is_none() && !raw.contains(':') {
            return None;
        }
        to_time(h, m, pm)
    };
    // "5-6pm", "9:00-10:30"
    if let Some((a, b)) = w0.split_once('-') {
        let (bh, bm, bpm) = clock(b)?;
        let (ah, am, apm) = clock(a)?;
        // "11-1pm" is 11am to 1pm; "5-6pm" 5pm to 6pm
        let apm = apm.or(bpm.map(|pm| if pm && ah > bh && ah != 12 { false } else { pm }));
        let (start, next) = sure(ah, am, apm, a).or_else(|| sure(ah, am, apm, b))?;
        let (end, _) = to_time(bh, bm, bpm)?;
        let mut len = (end - start).num_minutes();
        if len <= 0 {
            len += 24 * 60;
        }
        return Some((start, next, Some(len), 1));
    }
    // "5 pm"
    let pm_word = ws.get(1).and_then(|w| match *w {
        "am" | "a.m." => Some(false),
        "pm" | "p.m." => Some(true),
        _ => None,
    });
    if let (Some(pm), Some((h, m, None))) = (pm_word, clock(w0)) {
        let (t, next) = to_time(h, m, Some(pm))?;
        return Some((t, next, None, 2));
    }
    let (h, m, pm) = clock(w0)?;
    let (t, next) = sure(h, m, pm, w0)?;
    Some((t, next, None, 1))
}

/// "every day", "every weekday", "every week", "every month", "every fri":
/// the repeat and, for a day of the week, its first date.
fn repeat_at(ws: &[&str], today: NaiveDate) -> Option<(Repeat, Option<NaiveDate>, usize)> {
    let (w0, w1) = (*ws.first()?, *ws.get(1)?);
    if w0 != "every" {
        return None;
    }
    if let Some(wd) = weekday(w1) {
        return Some((Repeat::Weekly, Some(on_or_after(today, wd)), 2));
    }
    let r = match w1 {
        "day" => Repeat::Daily,
        "weekday" | "workday" => Repeat::Weekdays,
        "week" => Repeat::Weekly,
        "month" => Repeat::Monthly,
        _ => return None,
    };
    Some((r, None, 2))
}

/// A length at the start of `ws`: "30m", "1h30m", "1.5h", "30 min",
/// "an hour", "half an hour"; and how many words it took.
fn duration_at(ws: &[&str]) -> Option<(i64, usize)> {
    let w0 = *ws.first()?;
    if let Some(m) = parse_duration(w0) {
        return Some((m, 1));
    }
    let unit = |u: &str| -> Option<f64> {
        match u {
            "m" | "min" | "mins" | "minute" | "minutes" => Some(1.0),
            "h" | "hr" | "hrs" | "hour" | "hours" => Some(60.0),
            _ => None,
        }
    };
    let minutes = |n: f64, u: f64| -> Option<i64> {
        let m = (n * u).round() as i64;
        (m > 0).then_some(m)
    };
    // "1.5h"
    let num = w0.trim_end_matches(|c: char| c.is_alphabetic());
    if num.contains('.') {
        if let (Ok(n), Some(u)) = (num.parse::<f64>(), unit(&w0[num.len()..])) {
            return minutes(n, u).map(|m| (m, 1));
        }
    }
    let n: f64 = match w0 {
        "a" | "an" | "one" => 1.0,
        "half" => {
            let i = if ws.get(1) == Some(&"an") { 2 } else { 1 };
            return (ws.get(i) == Some(&"hour")).then_some((30, i + 1));
        }
        _ => w0.parse().ok().filter(|n: &f64| *n > 0.0 && *n < 10_000.0)?,
    };
    minutes(n, unit(ws.get(1)?)?).map(|m| (m, 2))
}

/// "45m", "2h", "1h30m" in minutes.
pub fn parse_duration(w: &str) -> Option<i64> {
    let w = w.to_lowercase();
    let (mut total, mut num, mut any) = (0i64, String::new(), false);
    for c in w.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else if c == 'h' && !num.is_empty() {
            total += num.parse::<i64>().ok()? * 60;
            num.clear();
            any = true;
        } else if c == 'm' && !num.is_empty() {
            total += num.parse::<i64>().ok()?;
            num.clear();
            any = true;
        } else {
            return None;
        }
    }
    (any && num.is_empty() && total > 0).then_some(total)
}

pub fn fmt_duration(m: i64) -> String {
    match (m / 60, m % 60) {
        (0, m) => trf("{}m", &[&m]),
        (h, 0) => trf("{}h", &[&h]),
        (h, m) => trf("{}h {}m", &[&h, &m]),
    }
}

pub fn priority_name(p: u8) -> &'static str {
    match p {
        0 => tr("None"),
        1..=3 => tr("High"),
        4..=5 => tr("Medium"),
        _ => tr("Low"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    // a Monday
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
    }
    fn now() -> NaiveDateTime {
        today().and_hms_opt(10, 0, 0).unwrap()
    }
    fn p(text: &str) -> Quick {
        parse_quick(text, today(), &QuickSyntax::default())
    }
    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }
    fn t(h: u32, m: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, m, 0)
    }

    #[test]
    fn times_with_marks() {
        let q = p("lunch @12pm");
        assert_eq!((q.title.as_str(), q.time), ("lunch", t(12, 0)));
        let q = p("gym at 5am");
        assert_eq!((q.title.as_str(), q.time), ("gym", t(5, 0)));
        let q = p("sleep @24");
        assert_eq!((q.time, q.time_next_day), (t(0, 0), true));
        assert_eq!(q.start(now()), Some(d(11, 17).and_hms_opt(0, 0, 0).unwrap()));
    }

    #[test]
    fn a_time_puts_it_on_the_calendar() {
        // 5am has passed at 10:00, so tomorrow
        assert_eq!(p("gym at 5am").start(now()), Some(d(11, 17).and_hms_opt(5, 0, 0).unwrap()));
        assert_eq!(p("call fri 5pm").start(now()), Some(d(11, 20).and_hms_opt(17, 0, 0).unwrap()));
        // a date alone is only the due date
        let q = p("report fri");
        assert_eq!((q.due, q.start(now())), (Some(d(11, 20)), None));
    }

    #[test]
    fn numeric_dates() {
        assert_eq!(p("pay rent 12/4").due, Some(NaiveDate::from_ymd_opt(2027, 4, 12).unwrap()));
        let us = QuickSyntax { day_first: false, ..Default::default() };
        assert_eq!(parse_quick("pay rent 12/4", today(), &us).due, Some(d(12, 4)));
        assert_eq!(p("x 2026-12-01").due, Some(d(12, 1)));
    }

    #[test]
    fn words_for_dates() {
        assert_eq!(p("a tomorrow").due, Some(d(11, 17)));
        assert_eq!(p("a in 3 days").due, Some(d(11, 19)));
        assert_eq!(p("a next fri").due, Some(d(11, 27)));
        assert_eq!(p("a dec 3").due, Some(d(12, 3)));
        assert_eq!(p("a on 3rd of december").due, Some(d(12, 3)));
    }

    #[test]
    fn ranges_repeats_and_durations() {
        let q = p("standup every monday at 9am");
        assert_eq!((q.title.as_str(), q.repeat, q.time), ("standup", Repeat::Weekly, t(9, 0)));
        let q = p("meet 5-6pm");
        assert_eq!((q.time, q.duration_min), (t(17, 0), Some(60)));
        assert_eq!(p("read for 30 min").duration_min, Some(30));
        assert_eq!(p("read 1h30m").duration_min, Some(90));
    }

    #[test]
    fn plain_words_stay_in_the_title() {
        assert_eq!(p("look at this").title, "look at this");
        assert_eq!(p("\"meet at 5pm\" notes").title, "meet at 5pm notes");
        assert_eq!(p("\"meet at 5pm\" notes").time, None);
    }

    #[test]
    fn kalendar_marks() {
        let q = p("buy milk #home !high >Errands");
        assert_eq!(q.title, "buy milk");
        assert_eq!(q.tags, vec!["home".to_string()]);
        assert_eq!(q.priority, 1);
        assert_eq!(q.list_name.as_deref(), Some("Errands"));
    }

    #[test]
    fn todoist_marks() {
        let s = QuickSyntax { preset: QuickPreset::Todoist, ..Default::default() };
        let q = parse_quick("email Ann p1 @work #Office tomorrow", today(), &s);
        assert_eq!(q.title, "email Ann");
        assert_eq!(q.priority, 1);
        assert_eq!(q.tags, vec!["work".to_string()]);
        assert_eq!(q.list_name.as_deref(), Some("Office"));
        assert_eq!(q.due, Some(d(11, 17)));
    }

    #[test]
    fn abbreviations_expand() {
        let s = QuickSyntax { abbrevs: vec![("eod".into(), "at 17:00".into()), ("w".into(), ">Work".into())], ..Default::default() };
        let q = parse_quick("ship it eod w", today(), &s);
        assert_eq!(q.title, "ship it");
        assert_eq!(q.time, t(17, 0));
        assert_eq!(q.list_name.as_deref(), Some("Work"));
    }

    #[test]
    fn natural_off_needs_marks() {
        let s = QuickSyntax { natural: false, ..Default::default() };
        let q = parse_quick("call fri 5pm", today(), &s);
        assert_eq!((q.title.as_str(), q.due, q.time), ("call fri 5pm", None, None));
        let q = parse_quick("call .fri @5pm", today(), &s);
        assert_eq!((q.due, q.time), (Some(d(11, 20)), t(17, 0)));
    }
}
