//! The little language of the quick task field, and durations as text.

use crate::strings::{tr, trf};
use chrono::{Duration, NaiveDate};

/// What typing in the quick task field means: "#tag", "!high", ">List",
/// "30m" / "1h30m" and ".today" / ".tomorrow" / ".2026-11-30" are picked out.
pub struct Quick {
    pub title: String,
    pub tags: Vec<String>,
    pub priority: u8,
    pub list_name: Option<String>,
    pub duration_min: Option<i64>,
    pub due: Option<NaiveDate>,
}

pub fn parse_quick(text: &str, today: NaiveDate) -> Quick {
    let mut q = Quick { title: String::new(), tags: vec![], priority: 0, list_name: None, duration_min: None, due: None };
    let mut words = vec![];
    for w in text.split_whitespace() {
        if let Some(t) = w.strip_prefix('#').filter(|t| !t.is_empty()) {
            q.tags.push(t.to_string());
        } else if let Some(p) = w.strip_prefix('!') {
            q.priority = match p.to_lowercase().as_str() {
                "" | "high" | "h" | "1" => 1,
                "medium" | "m" | "5" => 5,
                "low" | "l" | "9" => 9,
                _ => 1,
            };
        } else if let Some(l) = w.strip_prefix('>').filter(|t| !t.is_empty()) {
            q.list_name = Some(l.to_string());
        } else if let Some(d) = w.strip_prefix('.').filter(|t| !t.is_empty()) {
            q.due = match d.to_lowercase().as_str() {
                "today" => Some(today),
                "tomorrow" => Some(today + Duration::days(1)),
                other => NaiveDate::parse_from_str(other, "%Y-%m-%d").ok(),
            };
            if q.due.is_none() {
                words.push(w);
            }
        } else if let Some(m) = parse_duration(w) {
            q.duration_min = Some(m);
        } else {
            words.push(w);
        }
    }
    q.title = words.join(" ");
    q
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
