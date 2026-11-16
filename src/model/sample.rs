//! What a first start shows, and what screenshots and tests use.

use super::*;
use chrono::{Duration, NaiveDate};

impl Data {
    /// Neutral sample content for a first start.
    pub fn sample(today: NaiveDate) -> Data {
        let lists = vec![
            TaskList { id: 1, name: "Work".into(), icon: crate::icons::LIST_ICONS[1].to_string(), color: [0x77, 0x7b, 0xf0], ..Default::default() },
            TaskList { id: 2, name: "Personal".into(), icon: crate::icons::LIST_ICONS[3].to_string(), color: [0xf1, 0x76, 0xa8], ..Default::default() },
            TaskList { id: 3, name: "Study".into(), icon: crate::icons::LIST_ICONS[6].to_string(), color: [0xa2, 0x41, 0xf1], ..Default::default() },
        ];
        let mut d = Data {
            lists,
            events: vec![],
            tasks: vec![],
            calendars: vec![
                Calendar { id: 1, name: "Personal".into(), color: [0x35, 0x85, 0xe1], visible: true, ..Default::default() },
                Calendar { id: 2, name: "Work".into(), color: [0x77, 0x7b, 0xf0], visible: true, ..Default::default() },
                Calendar { id: 3, name: "Family".into(), color: [0xe8, 0xae, 0x26], visible: true, ..Default::default() },
                Calendar { id: 4, name: "Tasks".into(), color: [0x38, 0xc2, 0xc7], visible: true, ..Default::default() },
            ],
            frames: vec![],
            accounts: vec![],
            outbox: vec![],
            prefs: Prefs::default(),
            next_id: 10,
        };
        let ws = d.prefs.week_start(today);
        let at = |day: i64, h: u32, m: u32| (ws + Duration::days(day)).and_hms_opt(h, m, 0).unwrap();
        // sample calendars have the ids 1 to 4, in list order
        let ev = |d: &mut Data, title: &str, day: i64, h: u32, m: u32, mins: i64, cal: u64| {
            let id = d.new_id();
            let start = at(day, h, m);
            d.events.push(Event {
                id,
                title: title.into(),
                start,
                end: start + Duration::minutes(mins),
                all_day: false,
                calendar: cal + 1,
                notes: String::new(),
                task_id: None,
            color: None,
            ..Default::default()
            });
        };
        ev(&mut d, "Team sync", 1, 9, 0, 45, 1);
        ev(&mut d, "Lunch", 1, 12, 0, 60, 2);
        ev(&mut d, "Project review", 2, 10, 0, 90, 1);
        ev(&mut d, "Lunch", 2, 12, 0, 60, 2);
        ev(&mut d, "Gym", 2, 7, 0, 60, 0);
        ev(&mut d, "Design session", 3, 9, 30, 120, 1);
        ev(&mut d, "Call with Sam", 3, 10, 30, 30, 0);
        ev(&mut d, "Lunch", 3, 12, 0, 60, 2);
        ev(&mut d, "Planning", 4, 8, 30, 60, 1);
        ev(&mut d, "Dentist", 4, 14, 0, 45, 0);
        ev(&mut d, "Weekly wrap-up", 5, 11, 0, 60, 1);
        ev(&mut d, "Dinner with friends", 5, 9, 0, 30, 2);
        ev(&mut d, "Farmers market", 6, 9, 0, 90, 2);
        let id = d.new_id();
        d.events.push(Event {
            id,
            title: "Trip".into(),
            start: at(4, 0, 0),
            end: at(6, 0, 0),
            all_day: true,
            calendar: 3,
            notes: String::new(),
            task_id: None,
        color: None,
        ..Default::default()
        });
        let task = |d: &mut Data, title: &str, due: Option<i64>, pr: u8, dur: Option<i64>, list: u64, tag: &str| {
            let id = d.new_id();
            d.tasks.push(Task {
                id,
                title: title.into(),
                notes: String::new(),
                due: due.map(|x| today + Duration::days(x)),
                priority: pr,
                duration_min: dur,
                list,
                tags: if tag.is_empty() { vec![] } else { vec![tag.to_string()] },
                ..Default::default()
            });
            id
        };
        task(&mut d, "Send the invoice", Some(-1), 1, Some(30), 1, "admin");
        task(&mut d, "Write the monthly report", Some(0), 1, Some(60), 1, "deep-work");
        task(&mut d, "Reply to open emails", Some(0), 5, Some(30), 1, "email");
        task(&mut d, "Book train tickets", Some(1), 9, Some(15), 2, "");
        task(&mut d, "Prepare slides", Some(3), 5, Some(90), 1, "deep-work");
        task(&mut d, "Read chapter 4", Some(5), 5, Some(75), 3, "");
        task(&mut d, "Tidy up the notes", None, 0, None, 0, "");
        task(&mut d, "Renew library card", None, 9, Some(15), 2, "admin");
        let quick = FrameFilter { max_min: Some(30), ..Default::default() };
        for (name, cal, h, filter) in [("Deep work", 1, 9, FrameFilter::default()), ("Quick wins", 3, 16, quick)] {
            let id = d.new_id();
            d.frames.push(Frame {
                id,
                name: name.into(),
                calendar: cal + 1,
                start_min: h * 60,
                end_min: (h + if filter.is_empty() { 2 } else { 1 }) * 60,
                from: ws,
                filter,
                ..Default::default()
            });
        }
        let tid = task(&mut d, "Review the budget", Some(2), 1, Some(45), 1, "");
        let id = d.new_id();
        let dow = (today - ws).num_days();
        let start = at(dow, 13, 0);
        d.events.push(Event {
            id,
            title: "Review the budget".into(),
            start,
            end: start + Duration::minutes(45),
            all_day: false,
            calendar: 4,
            notes: String::new(),
            task_id: Some(tid),
        color: None,
        ..Default::default()
        });
        d
    }
}
