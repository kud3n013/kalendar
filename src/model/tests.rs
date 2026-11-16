use super::*;
use super::data::write_json;
use chrono::{Duration, NaiveDate, NaiveDateTime};

#[test]
fn old_data_file_moves_settings_to_config() {
    let dir = std::env::temp_dir().join(format!("kalendar-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("XDG_DATA_HOME", dir.join("data"));
    std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
    let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();

    // an older data.json, with the settings inside it
    let mut old = Data::sample(today);
    old.prefs.sidebar_side = PanelSide::Right;
    old.prefs.keybinds.insert("settings".into(), "Ctrl+P".into());
    let mut v = serde_json::to_value(&old).unwrap();
    v["prefs"] = serde_json::to_value(&old.prefs).unwrap();
    write_json(data_path(), v.to_string());

    let d = Data::load_or_sample(today);
    assert_eq!(d.prefs.sidebar_side, PanelSide::Right);
    assert!(config_path().exists());
    let data_text = std::fs::read_to_string(data_path()).unwrap();
    assert!(!data_text.contains("\"prefs\""), "settings left in data.json");

    // and they come back from config.json alone
    let again = Data::load_or_sample(today);
    assert_eq!(again.prefs.keybinds.get("settings").map(|s| s.as_str()), Some("Ctrl+P"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn ymd(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

/// Two own calendars, no events; 2026-11-16 is a Monday.
fn empty() -> Data {
    let mut d = Data::sample(ymd(11, 16));
    d.events.clear();
    d.tasks.clear();
    d.frames.clear();
    d
}

/// `cal` is the calendar's place in the sample list; its id is one more.
fn frame(id: u64, cal: u64) -> Frame {
    Frame { id, name: format!("F{id}"), calendar: cal + 1, from: ymd(11, 16), ..Default::default() }
}

#[test]
fn old_data_loads_and_saves_frames() {
    let mut v = serde_json::to_value(Data::sample(ymd(11, 16))).unwrap();
    v.as_object_mut().unwrap().remove("frames");
    let d: Data = serde_json::from_value(v).expect("old data.json without frames");
    assert!(d.frames.is_empty());
    let out = serde_json::to_value(&d).unwrap();
    assert_eq!(out["frames"], serde_json::json!([]));
    // a frame written with only a few fields still loads
    let f: Frame = serde_json::from_str(r#"{"id":3,"name":"Deep work"}"#).unwrap();
    assert_eq!((f.every_weeks, f.days, f.enabled, f.exclusive), (1, FRAME_WEEKDAYS, true, true));
}

#[test]
fn frame_weekday_bits() {
    let mut f = frame(1, 0);
    f.days = 0b000_0101; // Monday and Wednesday
    assert!(Data::frame_occurs_on(&f, ymd(11, 16)));
    assert!(!Data::frame_occurs_on(&f, ymd(11, 17)));
    assert!(Data::frame_occurs_on(&f, ymd(11, 18)));
    f.days = 1 << 6; // Sunday
    assert!(Data::frame_occurs_on(&f, ymd(11, 22)));
    assert!(!Data::frame_occurs_on(&f, ymd(11, 21)));
}

#[test]
fn frame_every_two_weeks_from_until_skip() {
    let mut f = frame(1, 0);
    f.from = ymd(11, 18); // a Wednesday; its week starts on 11-16
    f.every_weeks = 2;
    assert!(!Data::frame_occurs_on(&f, ymd(11, 16)), "before from");
    assert!(Data::frame_occurs_on(&f, ymd(11, 18)));
    assert!(!Data::frame_occurs_on(&f, ymd(11, 23)), "odd week");
    assert!(Data::frame_occurs_on(&f, ymd(11, 30)));
    f.until = Some(ymd(11, 30));
    assert!(Data::frame_occurs_on(&f, ymd(11, 30)), "until is inclusive");
    assert!(!Data::frame_occurs_on(&f, ymd(12, 14)));
    f.skip.push(ymd(11, 30));
    assert!(!Data::frame_occurs_on(&f, ymd(11, 30)));
    f.enabled = false;
    assert!(!Data::frame_occurs_on(&f, ymd(11, 18)));
}

#[test]
fn frames_on_hides_hidden_calendars_and_sorts() {
    let mut d = empty();
    let mut a = frame(1, 0);
    a.start_min = 14 * 60;
    a.end_min = 15 * 60;
    let b = frame(2, 1);
    d.frames = vec![a, b];
    let on: Vec<u64> = d.frames_on(ymd(11, 16)).iter().map(|o| o.frame).collect();
    assert_eq!(on, vec![2, 1]);
    let o = d.frames_on(ymd(11, 16))[0];
    assert_eq!((o.start, o.end), (ymd(11, 16).and_hms_opt(9, 0, 0).unwrap(), ymd(11, 16).and_hms_opt(11, 0, 0).unwrap()));
    d.calendars[1].visible = false;
    assert_eq!(d.frames_on(ymd(11, 16)).len(), 1);
    assert!(d.frames_on(ymd(11, 21)).is_empty(), "Saturday");
}

#[test]
fn frame_filter_matching() {
    let mut f = frame(1, 0);
    let t = Task { list: 1, tags: vec!["Deep-Work".into(), "x".into()], duration_min: Some(45), ..Default::default() };
    assert!(f.matches(&t, 60), "empty filter takes all");
    f.filter.lists = vec![0];
    assert!(!f.matches(&t, 60));
    f.filter.lists = vec![0, 1];
    assert!(f.matches(&t, 60));
    f.filter.tags = vec!["nope".into()];
    assert!(!f.matches(&t, 60), "list and tags picked: both are needed");
    f.filter.lists = vec![0];
    f.filter.tags = vec!["x".into()];
    assert!(!f.matches(&t, 60));
    f.filter.lists = vec![0, 1];
    f.filter.tags = vec!["deep-work".into(), "y".into()];
    assert!(f.matches(&t, 60), "any tag, case does not matter");
    f.filter.tags_all = true;
    assert!(!f.matches(&t, 60));
    f.filter.tags = vec!["DEEP-WORK".into(), "x".into()];
    assert!(f.matches(&t, 60));
    f.filter.max_min = Some(30);
    assert!(!f.matches(&t, 60));
    f.filter.max_min = Some(45);
    assert!(f.matches(&t, 60));
    let no_len = Task { duration_min: None, ..t.clone() };
    assert!(!f.matches(&no_len, 60), "default length counts");
    assert!(f.matches(&no_len, 30));
    assert_eq!(f.filter.specificity(), 3);
}

fn sessions(d: &Data) -> Vec<String> {
    let mut v: Vec<String> = d
        .events
        .iter()
        .filter(|e| e.task_id.is_some())
        .map(|e| format!("{} {} {} c{}", e.start.format("%a %H:%M"), e.end.format("%H:%M"), e.title, d.calendars.iter().position(|c| c.id == e.calendar).unwrap()))
        .collect();
    v.sort();
    v
}

#[test]
fn plan_without_frames_is_unchanged() {
    let now = ymd(11, 18).and_hms_opt(8, 0, 0).unwrap();
    let mut d = Data::sample(now.date());
    d.frames.clear();
    // recorded when tasks without a time took an hour
    d.prefs.default_task_min = 60;
    for (title, mins) in [("Big job", 240), ("Huge job", 400), ("Too big", 2000)] {
        let id = d.new_id();
        d.tasks.push(Task { id, title: title.into(), duration_min: Some(mins), due: Some(ymd(11, 30)), ..Default::default() });
    }
    // recorded from the planner before frames existed
    assert_eq!(d.plan_tasks(now), (14, 1));
    let want = [
        "Fri 08:00 08:15 Renew library card c3",
        "Fri 09:30 10:40 Huge job c3",
        "Fri 12:00 13:00 Tidy up the notes c3",
        "Thu 08:00 08:30 Huge job c3",
        "Thu 09:30 13:30 Big job c3",
        "Thu 13:30 14:00 Huge job c3",
        "Thu 14:45 18:00 Huge job c3",
        "Wed 08:00 08:30 Send the invoice c3",
        "Wed 08:30 09:30 Write the monthly report c3",
        "Wed 11:30 12:00 Reply to open emails c3",
        "Wed 13:00 13:45 Review the budget c3",
        "Wed 13:45 14:00 Book train tickets c3",
        "Wed 14:00 15:30 Prepare slides c3",
        "Wed 15:30 16:45 Read chapter 4 c3",
        "Wed 16:45 18:00 Huge job c3",
    ];
    assert_eq!(sessions(&d), want);
}

fn add_task(d: &mut Data, title: &str, mins: i64, tags: &[&str]) -> u64 {
    let id = d.new_id();
    d.tasks.push(Task {
        id,
        title: title.into(),
        duration_min: Some(mins),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    });
    id
}

fn deep_frame(id: u64, cal: u64, start_h: i64, end_h: i64) -> Frame {
    let mut f = frame(id, cal);
    f.start_min = start_h * 60;
    f.end_min = end_h * 60;
    f.filter.tags = vec!["deep".into()];
    f
}

fn monday_8() -> NaiveDateTime {
    ymd(11, 16).and_hms_opt(8, 0, 0).unwrap()
}

#[test]
fn matching_task_goes_into_its_frame() {
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 14, 16));
    add_task(&mut d, "Write", 60, &["Deep"]);
    assert_eq!(d.plan_tasks(monday_8()), (1, 0));
    assert_eq!(sessions(&d), ["Mon 14:00 15:00 Write c1"]);
}

#[test]
fn a_task_waits_for_its_own_frame() {
    // its frame runs on Fridays only: neither an open frame nor free
    // time earlier in the week takes it
    let mut d = empty();
    let mut fri = deep_frame(1, 1, 9, 11);
    fri.days = 1 << 4;
    let mut any = deep_frame(2, 0, 9, 11);
    any.filter = FrameFilter::default();
    d.frames = vec![fri, any];
    add_task(&mut d, "Write", 60, &["deep"]);
    let other = add_task(&mut d, "Email", 60, &[]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Fri 09:00 10:00 Write c1", "Mon 09:00 10:00 Email c0"]);
    // and the frame that keeps others out says so
    let t = d.task(other).unwrap().clone();
    let fri9 = ymd(11, 20).and_hms_opt(9, 30, 0).unwrap();
    assert!(d.frame_keeping_out(&t, fri9, fri9 + Duration::minutes(30)).is_some());
    assert!(d.frame_keeping_out(&t, fri9 + Duration::hours(3), fri9 + Duration::hours(4)).is_none());
}

#[test]
fn a_closed_frame_cuts_into_an_open_one() {
    let mut d = empty();
    let deep = deep_frame(1, 1, 9, 10);
    let mut any = deep_frame(2, 0, 9, 11);
    any.filter = FrameFilter::default();
    d.frames = vec![deep, any];
    add_task(&mut d, "Email", 60, &[]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 10:00 11:00 Email c0"]);
}

#[test]
fn other_tasks_keep_out_of_exclusive_frames() {
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 8, 12));
    add_task(&mut d, "Email", 60, &[]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 12:00 13:00 Email c3"]);

    let mut d = empty();
    let mut f = deep_frame(1, 1, 8, 12);
    f.exclusive = false;
    d.frames.push(f);
    add_task(&mut d, "Email", 60, &[]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 08:00 09:00 Email c3"]);
}

#[test]
fn no_frame_fits_and_leave_unplanned() {
    let mut d = empty();
    d.prefs.plan_outside_frames = false;
    d.frames.push(deep_frame(1, 1, 8, 12));
    add_task(&mut d, "Email", 60, &[]);
    add_task(&mut d, "Write", 60, &["deep"]);
    assert_eq!(d.plan_tasks(monday_8()), (1, 1));
    assert_eq!(sessions(&d), ["Mon 08:00 09:00 Write c1"]);

    // switching frames off plans as before ("Write" now has a due date)
    d.events.clear();
    d.prefs.use_frames = false;
    assert_eq!(d.plan_tasks(monday_8()), (2, 0));
    assert_eq!(sessions(&d), ["Mon 08:00 09:00 Write c3", "Mon 09:00 10:00 Email c3"]);
}

#[test]
fn long_task_splits_across_frames() {
    let mut d = empty();
    let mut a = deep_frame(1, 1, 9, 11);
    a.filter = FrameFilter::default();
    let mut b = deep_frame(2, 1, 14, 16);
    b.filter = FrameFilter::default();
    d.frames = vec![a, b];
    add_task(&mut d, "Thesis", 180, &[]);
    assert_eq!(d.plan_tasks(monday_8()), (2, 0));
    assert_eq!(sessions(&d), ["Mon 09:00 11:00 Thesis c1", "Mon 14:00 15:00 Thesis c1"]);
}

#[test]
fn earliest_day_holds_in_frames() {
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 9, 11));
    let id = add_task(&mut d, "Write", 60, &["deep"]);
    d.task_mut(id).unwrap().earliest = Some(ymd(11, 18));
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Wed 09:00 10:00 Write c1"]);
}

#[test]
fn events_take_room_from_frames() {
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 9, 11));
    d.events.push(Event {
        id: 99,
        title: "Call".into(),
        start: ymd(11, 16).and_hms_opt(9, 0, 0).unwrap(),
        end: ymd(11, 16).and_hms_opt(10, 30, 0).unwrap(),
        ..Default::default()
    });
    add_task(&mut d, "Write", 60, &["deep"]);
    add_task(&mut d, "Sketch", 30, &["deep"]);
    assert_eq!(d.plan_tasks(monday_8()), (2, 0));
    assert_eq!(sessions(&d), ["Mon 10:30 11:00 Sketch c1", "Tue 09:00 10:00 Write c1"]);
}

#[test]
fn more_specific_frame_wins_a_tie() {
    let mut d = empty();
    let mut any = deep_frame(1, 0, 9, 11);
    any.filter = FrameFilter::default();
    let deep = deep_frame(2, 1, 9, 11);
    d.frames = vec![any, deep];
    add_task(&mut d, "Write", 60, &["deep"]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 09:00 10:00 Write c1"]);
}

#[test]
fn next_free_slot_prefers_frames() {
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 14, 16));
    let id = add_task(&mut d, "Write", 60, &["deep"]);
    let t = d.task(id).cloned();
    let at = |h| Some(ymd(11, 16).and_hms_opt(h, 0, 0).unwrap());
    assert_eq!(d.next_free_slot(monday_8(), 60, t.as_ref()), at(14));
    assert_eq!(d.next_free_slot(monday_8(), 60, None), at(8));
}

#[test]
fn removing_a_calendar_keeps_the_others() {
    let mut d = Data::sample(ymd(11, 16));
    d.frames = vec![frame(1, 0), frame(2, 1), frame(3, 3)];
    let before: Vec<(u64, String)> =
        d.events.iter().filter(|e| e.calendar != 2).map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
    d.remove_calendar(2);
    let v: Vec<(u64, u64)> = d.frames.iter().map(|f| (f.id, f.calendar)).collect();
    assert_eq!(v, vec![(1, 1), (3, 4)]);
    let after: Vec<(u64, String)> = d.events.iter().map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
    assert_eq!(before, after, "an event moved to another calendar");
    // putting the calendars in another order moves nothing either
    d.calendars.reverse();
    let again: Vec<(u64, String)> = d.events.iter().map(|e| (e.id, d.calendar(e.calendar).unwrap().name.clone())).collect();
    assert_eq!(before, again);
}

/// A data file as written before calendars had ids.
fn old_format(d: &Data) -> String {
    let mut v = serde_json::to_value(d).unwrap();
    let ids: Vec<u64> = d.calendars.iter().map(|c| c.id).collect();
    let place = |id: &serde_json::Value| ids.iter().position(|x| Some(*x) == id.as_u64()).unwrap();
    for c in v["calendars"].as_array_mut().unwrap() {
        let c = c.as_object_mut().unwrap();
        c.remove("id");
        c.remove("read_only");
    }
    for key in ["events", "frames"] {
        for e in v[key].as_array_mut().unwrap() {
            e["calendar"] = place(&e["calendar"]).into();
        }
    }
    v.to_string()
}

#[test]
fn old_data_file_gets_calendar_ids() {
    let mut sample = Data::sample(ymd(11, 16));
    sample.calendars.push(Calendar { id: 5, name: "Feed".into(), url: Some("https://example.org/a.ics".into()), visible: true, ..Default::default() });
    let names = |d: &Data| -> Vec<(u64, String, String)> {
        let mut v: Vec<_> = d.events.iter().map(|e| (e.id, e.title.clone(), d.calendar(e.calendar).expect("event without calendar").name.clone())).collect();
        v.extend(d.frames.iter().map(|f| (f.id, f.name.clone(), d.calendar(f.calendar).expect("frame without calendar").name.clone())));
        v
    };
    let want = names(&sample);
    let loaded = Data::from_json(&old_format(&sample)).expect("old data.json");
    assert!(loaded.calendars.iter().all(|c| c.id != 0));
    let mut ids: Vec<u64> = loaded.calendars.iter().map(|c| c.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 5, "calendar ids must differ");
    assert!(loaded.calendars[4].read_only && !loaded.calendars[0].read_only);
    assert_eq!(names(&loaded), want);
    // saved and read again, nothing moves and the ids stay
    let again = Data::from_json(&serde_json::to_string(&loaded).unwrap()).unwrap();
    assert_eq!(names(&again), want);
    assert_eq!(again.calendars.iter().map(|c| c.id).collect::<Vec<_>>(), loaded.calendars.iter().map(|c| c.id).collect::<Vec<_>>());
    assert_eq!(again.next_id, loaded.next_id);
}

fn feed_event(uid: &str, rid: Option<&str>, title: &str, h: u32) -> crate::ics::FeedEvent {
    let start = ymd(11, 16).and_hms_opt(h, 0, 0).unwrap();
    crate::ics::FeedEvent {
        title: title.into(),
        start,
        end: start + Duration::hours(1),
        all_day: false,
        notes: String::new(),
        uid: uid.into(),
        recurrence_id: rid.map(|s| s.to_string()),
        href: None,
        etag: None,
    }
}

#[test]
fn refreshing_a_feed_keeps_event_ids() {
    let mut d = empty();
    let own = d.events.len();
    let feed = || vec![feed_event("a", None, "A", 9), feed_event("r", Some("1"), "R", 10), feed_event("r", Some("2"), "R", 11)];
    d.apply_full(2, feed());
    let ids: Vec<u64> = d.events.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), own + 3);
    d.event_mut(ids[0]).unwrap().color = Some([1, 2, 3]);
    d.apply_full(2, feed());
    assert_eq!(d.events.iter().map(|e| e.id).collect::<Vec<_>>(), ids);
    assert_eq!(d.event(ids[0]).unwrap().color, Some([1, 2, 3]), "own colour lost");

    // one changed, one gone, one new
    d.apply_full(2, vec![feed_event("a", None, "A moved", 14), feed_event("r", Some("2"), "R", 11), feed_event("n", None, "N", 12)]);
    let a = d.event(ids[0]).expect("changed event kept its id");
    assert_eq!((a.title.as_str(), a.start.hour()), ("A moved", 14));
    assert!(d.event(ids[1]).is_none(), "event gone from the feed stays");
    assert!(d.event(ids[2]).is_some());
    assert_eq!(d.events.len(), 3);

    // a change on its own, and deletes by key
    d.apply_delta(2, vec![feed_event("n", None, "N2", 13)], &[RemoteKey::Uid("r".into(), Some("2".into()))]);
    assert_eq!(d.events.len(), 2);
    assert!(d.events.iter().any(|e| e.title == "N2"));
    // a changed address keeps only what it still holds
    let at = |uid: &str, rid: &str, href: &str| {
        let mut f = feed_event(uid, Some(rid), "S", 9);
        f.href = Some(href.into());
        f
    };
    let mut s = empty();
    s.apply(3, crate::sync::Delta { upserts: vec![at("s", "1", "/a.ics"), at("s", "2", "/a.ics"), at("t", "1", "/b.ics")], full: true, state: Some("x".into()), ..Default::default() });
    let kept = s.events.iter().find(|e| e.recurrence_id.as_deref() == Some("2")).unwrap().id;
    s.apply(3, crate::sync::Delta { upserts: vec![at("s", "2", "/a.ics")], changed: vec!["/a.ics".into()], state: Some("y".into()), ..Default::default() });
    assert_eq!(s.events.len(), 2, "the occurrence taken out of the series goes");
    assert!(s.event(kept).is_some());
    assert_eq!(s.calendar(3).unwrap().sync_state.as_deref(), Some("y"));
    s.apply(3, crate::sync::Delta { deletes: vec![RemoteKey::Href("/b.ics".into())], ..Default::default() });
    assert_eq!(s.events.len(), 1);
    // another calendar's events are never touched
    d.apply_full(1, vec![]);
    assert_eq!(d.events.len(), 2);
}

#[test]
fn a_task_block_takes_its_list_colour() {
    let mut d = Data::sample(ymd(11, 18));
    d.lists.push(TaskList { id: 9005, color: [1, 2, 3], ..Default::default() });
    d.tasks.push(Task { id: 9007, list: 9005, ..Default::default() });
    let block = Event { task_id: Some(9007), ..Default::default() };
    assert_eq!(d.event_color(&block), [1, 2, 3]);
    let painted = Event { color: Some([9, 9, 9]), ..block.clone() };
    assert_eq!(d.event_color(&painted), [9, 9, 9], "a colour picked for the block wins");
    let plain = Event::default();
    assert_eq!(d.event_color(&plain), d.calendar_color(plain.calendar));
}

#[test]
fn a_planned_block_carries_its_task_note() {
    let mut d = empty();
    let id = add_task(&mut d, "Write", 60, &[]);
    d.task_mut(id).unwrap().notes = "Chapter two".into();
    d.plan_tasks(monday_8());
    let block = d.events.iter().find(|e| e.task_id == Some(id)).unwrap();
    assert_eq!(block.notes, "Chapter two");
}

#[test]
fn an_older_block_note_is_kept_in_its_task() {
    let mut d = empty();
    d.tasks.push(Task { id: 70, title: "Write".into(), notes: "Outline".into(), ..Default::default() });
    d.events.push(Event { id: 71, task_id: Some(70), notes: "Bring the draft".into(), ..Default::default() });
    d.events.push(Event { id: 72, task_id: Some(70), ..Default::default() });
    d.tasks.push(Task { id: 80, title: "Read".into(), ..Default::default() });
    d.events.push(Event { id: 81, task_id: Some(80), notes: "Page 40".into(), ..Default::default() });
    d.events.push(Event { id: 82, task_id: Some(80), notes: "Page 40".into(), ..Default::default() });
    let d = Data::from_json(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(d.task(70).unwrap().notes, "Outline\n\nBring the draft");
    assert_eq!(d.task(80).unwrap().notes, "Page 40", "the same note is not added twice");
    for e in &d.events {
        let t = d.task(e.task_id.unwrap()).unwrap();
        assert_eq!((&e.title, &e.notes), (&t.title, &t.notes), "every block shows its task");
    }
}

#[test]
fn a_task_changed_on_the_server_updates_its_blocks() {
    let mut d = empty();
    d.lists.push(TaskList { id: 60, ..Default::default() });
    d.tasks.push(Task { id: 61, list: 60, title: "Old".into(), uid: Some("t".into()), href: Some("/t.ics".into()), ..Default::default() });
    d.events.push(Event { id: 62, task_id: Some(61), title: "Old".into(), ..Default::default() });
    let task = crate::ics::FeedTask { uid: "t".into(), title: "New".into(), notes: "Server note".into(), due: None, priority: 0, done: false, href: Some("/t.ics".into()), etag: None };
    d.apply_tasks(60, crate::sync::Delta { tasks: vec![task], ..Default::default() });
    let e = d.event(62).unwrap();
    assert_eq!((e.title.as_str(), e.notes.as_str()), ("New", "Server note"));
}

#[test]
fn a_window_save_keeps_what_sync_brought_in() {
    let open = Event { id: 1, title: "Old".into(), etag: Some("1".into()), href: Some("/a.ics".into()), ..Default::default() };
    let mut live = Event { etag: Some("2".into()), color: Some([1, 2, 3]), ..open.clone() };
    live.take_edits(Event { title: "New".into(), notes: "n".into(), ..open });
    assert_eq!((live.title.as_str(), live.notes.as_str(), live.etag.as_deref(), live.color), ("New", "n", Some("2"), Some([1, 2, 3])));

    let open = Task { id: 1, title: "Old".into(), etag: Some("1".into()), ..Default::default() };
    let mut live = Task { etag: Some("2".into()), uid: Some("u".into()), ..open.clone() };
    live.take_edits(Task { title: "New".into(), tags: vec!["x".into()], ..open });
    assert_eq!((live.title.as_str(), live.tags.len(), live.etag.as_deref(), live.uid.as_deref()), ("New", 1, Some("2"), Some("u")));
}

#[test]
fn a_full_frame_today_waits_for_the_next_one_or_goes_now() {
    // today's deep frame is taken, free time follows it
    let busy = |d: &mut Data| {
        d.events.push(Event { id: 99, title: "Call".into(), start: ymd(11, 16).and_hms_opt(9, 0, 0).unwrap(), end: ymd(11, 16).and_hms_opt(11, 0, 0).unwrap(), ..Default::default() });
    };
    let mut d = empty();
    d.frames.push(deep_frame(1, 1, 9, 11));
    busy(&mut d);
    let id = add_task(&mut d, "Write", 60, &["deep"]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Tue 09:00 10:00 Write c1"]);
    let t = d.task(id).cloned();
    assert_eq!(d.next_free_slot(monday_8(), 60, t.as_ref()), Some(ymd(11, 17).and_hms_opt(10, 0, 0).unwrap()));

    let mut d = empty();
    d.prefs.frame_full = FrameFull::Now;
    d.frames.push(deep_frame(1, 1, 9, 11));
    busy(&mut d);
    let id = add_task(&mut d, "Write", 60, &["deep"]);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 08:00 09:00 Write c3"]);
    let t = d.task(id).cloned();
    assert_eq!(d.next_free_slot(monday_8(), 60, t.as_ref()), Some(ymd(11, 16).and_hms_opt(11, 0, 0).unwrap()));
}

#[test]
fn frame_time_goes_first_to_high_priority_or_chosen_tags() {
    // one hour of frame for two tasks of an hour
    let two = |d: &mut Data| {
        d.frames.push(deep_frame(1, 1, 9, 10));
        let low = add_task(d, "Low", 60, &["deep"]);
        d.task_mut(low).unwrap().due = Some(ymd(11, 16));
        let high = add_task(d, "High", 60, &["deep", "focus"]);
        d.task_mut(high).unwrap().priority = 1;
        d.task_mut(high).unwrap().due = Some(ymd(11, 18));
    };
    let mut d = empty();
    two(&mut d);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 09:00 10:00 High c1", "Tue 09:00 10:00 Low c1"]);

    let mut d = empty();
    d.prefs.frame_first = FrameFirst::Due;
    two(&mut d);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 09:00 10:00 Low c1", "Tue 09:00 10:00 High c1"]);

    let mut d = empty();
    d.prefs.frame_first = FrameFirst::Tags;
    d.prefs.frame_first_tags = vec!["Focus".into()];
    two(&mut d);
    d.plan_tasks(monday_8());
    assert_eq!(sessions(&d), ["Mon 09:00 10:00 High c1", "Tue 09:00 10:00 Low c1"]);
}

#[test]
fn planning_one_list_leaves_the_others() {
    let mut d = empty();
    d.lists.push(TaskList { id: 50, ..Default::default() });
    let a = add_task(&mut d, "Work", 60, &[]);
    d.task_mut(a).unwrap().list = 50;
    add_task(&mut d, "Home", 60, &[]);
    assert_eq!(d.plan_tasks_in(monday_8(), Some(50)), (1, 0));
    assert_eq!(sessions(&d), ["Mon 08:00 09:00 Work c3"]);
}

#[test]
fn archiving_puts_away_only_completed_tasks() {
    let mut d = empty();
    d.tasks.push(Task { id: 1, done: true, ..Default::default() });
    d.tasks.push(Task { id: 2, ..Default::default() });
    d.tasks.push(Task { id: 3, done: true, archived: true, ..Default::default() });
    assert_eq!(d.archive_done(), 1);
    assert!(d.task(1).unwrap().archived);
    assert!(!d.task(2).unwrap().archived);
    assert_eq!(d.archive_done(), 0);
}
