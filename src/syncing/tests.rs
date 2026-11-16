use super::*;
use chrono::Duration;
use std::path::PathBuf;

/// An app with one account whose first calendar takes changes and
/// holds one synced event (id 500).
fn synced_app() -> App {
    let mut app = App::new(Some(PathBuf::from("unused.png")));
    app.data.events.clear();
    app.data.accounts.push(Account { id: 900, home: Some("https://x.example/dav/".into()), ..Default::default() });
    app.data.calendars.push(Calendar { id: 901, name: "Synced".into(), visible: true, account: Some(900), remote_id: Some("https://x.example/dav/c/".into()), ..Default::default() });
    app.data.calendars.push(Calendar { id: 902, name: "Locked".into(), visible: true, account: Some(900), read_only: true, ..Default::default() });
    let start = App::now();
    app.data.events.push(Event {
        id: 500,
        title: "Synced".into(),
        start,
        end: start + Duration::hours(1),
        calendar: 901,
        remote: true,
        uid: Some("u1".into()),
        href: Some("https://x.example/dav/c/u1.ics".into()),
        etag: Some("\"1\"".into()),
        ..Default::default()
    });
    app.reshadow();
    app
}

#[test]
fn edits_in_a_synced_calendar_are_marked_to_send() {
    let mut app = synced_app();
    app.commit();
    assert!(!app.data.event(500).unwrap().dirty, "nothing changed yet");
    assert!(app.outbox_changed.is_none());

    // a change
    app.data.event_mut(500).unwrap().title = "Renamed".into();
    app.commit();
    assert!(app.data.event(500).unwrap().dirty);
    assert!(app.outbox_changed.is_some());

    // a new event, and one in a local calendar
    let own = app.data.first_own_calendar();
    app.data.events.push(Event { id: 501, title: "New".into(), calendar: 901, ..Default::default() });
    app.data.events.push(Event { id: 502, title: "Own".into(), calendar: own, ..Default::default() });
    app.commit();
    assert!(app.data.event(501).unwrap().dirty && !app.data.event(502).unwrap().dirty);

    // a copy of a synced event is a new event, not a second name for the old one
    let mut copy = app.data.event(500).unwrap().clone();
    copy.id = 503;
    app.data.events.push(copy);
    app.commit();
    let c = app.data.event(503).unwrap();
    assert!(c.dirty && c.href.is_none() && c.uid.is_none() && !c.remote);

    // what a server sends is not an edit
    app.data.event_mut(500).unwrap().dirty = false;
    app.data.event_mut(500).unwrap().title = "From the server".into();
    app.reshadow();
    app.commit();
    assert!(!app.data.event(500).unwrap().dirty);

    // a deletion is noted with the version it was read at
    app.data.events.retain(|e| e.id != 500);
    app.commit();
    assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "https://x.example/dav/c/u1.ics".into(), etag: Some("\"1\"".into()) }]);
    app.commit();
    assert_eq!(app.data.outbox.len(), 1, "noted once");
}

#[test]
fn moving_an_event_out_of_a_synced_calendar_deletes_it_there() {
    let mut app = synced_app();
    let own = app.data.first_own_calendar();
    app.data.event_mut(500).unwrap().calendar = own;
    app.commit();
    let e = app.data.event(500).unwrap();
    assert!(!e.dirty && !e.remote && e.href.is_none());
    assert_eq!(app.data.outbox.len(), 1);
    // and back in: it is a new event for the server
    app.data.event_mut(500).unwrap().calendar = 901;
    app.commit();
    let e = app.data.event(500).unwrap();
    assert!(e.dirty && e.href.is_none());
}

#[test]
fn task_edits_in_a_synced_list_are_marked_to_send() {
    let ctx = egui::Context::default();
    let mut app = synced_app();
    app.data.tasks.clear();
    app.data.lists.push(TaskList { id: 950, name: "Synced".into(), account: Some(900), remote_id: Some("https://x.example/dav/t/".into()), ..Default::default() });
    // what the server sends comes in without being marked
    let ft = |uid: &str, title: &str, done: bool| ics::FeedTask { uid: uid.into(), title: title.into(), done, href: Some(format!("https://x.example/dav/t/{uid}.ics")), etag: Some("\"1\"".into()), ..Default::default() };
    app.data.apply_tasks(950, sync::Delta { tasks: vec![ft("a", "A", false), ft("b", "B", false)], full: true, state: Some("s".into()), ..Default::default() });
    app.reshadow();
    app.commit();
    assert_eq!(app.data.tasks.len(), 2);
    assert!(app.data.tasks.iter().all(|t| !t.dirty && t.list == 950));
    let (a, b) = (app.data.tasks[0].id, app.data.tasks[1].id);

    // ticking one off marks it; a new one is marked too; a task elsewhere is not
    app.data.task_mut(a).unwrap().done = true;
    app.data.tasks.push(Task { id: 960, title: "New".into(), list: 950, ..Default::default() });
    app.data.tasks.push(Task { id: 961, title: "Own".into(), list: 1, ..Default::default() });
    app.commit();
    assert!(app.data.task(a).unwrap().dirty && app.data.task(960).unwrap().dirty && !app.data.task(961).unwrap().dirty && !app.data.task(b).unwrap().dirty);
    assert!(app.outbox_changed.is_some());

    // a pull does not write over the edit, and brings the rest up to date
    app.data.apply_tasks(950, sync::Delta { tasks: vec![ft("a", "A from the server", false), ft("b", "B renamed", false)], changed: vec!["https://x.example/dav/t/a.ics".into(), "https://x.example/dav/t/b.ics".into()], state: Some("s2".into()), ..Default::default() });
    assert!(app.data.task(a).unwrap().done && app.data.task(a).unwrap().title == "A");
    assert_eq!(app.data.task(b).unwrap().title, "B renamed");
    assert!(app.data.task(960).is_some(), "a task not sent yet stays through a pull");

    // saved: the flag goes; a conflict keeps the edit in the Inbox
    let sent = app.data.task(a).unwrap().clone();
    app.take_push_results(&ctx, 950, vec![sync::PushResult::TaskSaved { sent, uid: "a".into(), href: "https://x.example/dav/t/a.ics".into(), etag: Some("\"2\"".into()) }]);
    assert!(!app.data.task(a).unwrap().dirty && app.data.task(a).unwrap().etag.as_deref() == Some("\"2\""));
    app.reshadow();
    app.data.task_mut(b).unwrap().title = "My B".into();
    app.commit();
    app.take_push_results(&ctx, 950, vec![sync::PushResult::TaskConflict { task: b }]);
    let copy = app.data.tasks.last().unwrap();
    assert_eq!((copy.title.as_str(), copy.list, copy.href.is_none()), ("My B", 0, true));
    assert!(!app.data.task(b).unwrap().dirty);
    app.reshadow();

    // deleting one is noted; moving one out deletes it on the server and makes it one's own
    app.data.tasks.retain(|t| t.id != a);
    app.data.task_mut(b).unwrap().list = 1;
    app.commit();
    let hrefs: Vec<&str> = app.data.outbox.iter().map(|d| d.href.as_str()).collect();
    assert!(hrefs.contains(&"https://x.example/dav/t/a.ics") && hrefs.contains(&"https://x.example/dav/t/b.ics"), "{hrefs:?}");
    assert!(app.data.task(b).unwrap().href.is_none() && !app.data.task(b).unwrap().dirty);

    // a full pull drops what the server no longer has, but not what was never sent
    app.data.outbox.clear();
    app.data.apply_tasks(950, sync::Delta { tasks: vec![], full: true, ..Default::default() });
    let left: Vec<u64> = app.data.tasks.iter().filter(|t| t.list == 950).map(|t| t.id).collect();
    assert_eq!(left, vec![960]);

    // taking the list away here says nothing to the server
    app.reshadow();
    app.data.tasks.push(Task { id: 970, title: "Kept on the server".into(), list: 950, href: Some("h".into()), uid: Some("k".into()), ..Default::default() });
    app.reshadow();
    app.data.remove_synced_list(950);
    app.commit();
    assert!(app.data.outbox.is_empty() && app.data.tasks.iter().all(|t| t.list != 950));
}

#[test]
fn locked_events() {
    let mut app = synced_app();
    assert!(app.data.locked(app.data.event(500).unwrap()).is_none());
    app.data.event_mut(500).unwrap().recurrence_id = Some("20261116T080000Z".into());
    assert!(app.data.locked(app.data.event(500).unwrap()).is_some(), "an occurrence of a repeating event");
    app.data.event_mut(500).unwrap().recurrence_id = None;
    app.data.event_mut(500).unwrap().calendar = 902;
    assert!(app.data.locked(app.data.event(500).unwrap()).is_some(), "a calendar the server does not let one write");
    assert_eq!(app.data.writable_or_own(902), app.data.first_own_calendar());
    assert_eq!(app.data.writable_or_own(901), 901);
}

#[test]
fn push_results_are_taken_in() {
    let ctx = egui::Context::default();
    let mut app = synced_app();
    // saved: the server's name and version are kept, and the flag goes
    app.data.events.push(Event { id: 501, title: "New".into(), calendar: 901, ..Default::default() });
    app.commit();
    let sent = app.data.event(501).unwrap().clone();
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: sent.clone(), uid: "n".into(), href: "h".into(), etag: Some("e".into()) }]);
    let e = app.data.event(501).unwrap();
    assert!(!e.dirty && e.remote && e.href.as_deref() == Some("h") && e.etag.as_deref() == Some("e"));
    // changed again while on its way: it stays marked
    app.data.event_mut(501).unwrap().title = "Newer".into();
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent, uid: "n".into(), href: "h".into(), etag: Some("e2".into()) }]);
    assert!(app.data.event(501).unwrap().dirty);

    // a conflict: the edit is kept as a copy in one's own calendar
    app.data.event_mut(500).unwrap().title = "My edit".into();
    app.commit();
    let before = app.data.events.len();
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Conflict { event: 500 }]);
    assert_eq!(app.data.events.len(), before + 1);
    let copy = app.data.events.last().unwrap();
    assert_eq!((copy.title.as_str(), copy.calendar, copy.href.is_none()), ("My edit", app.data.first_own_calendar(), true));
    assert!(!app.data.event(500).unwrap().dirty, "the server's copy may now come in");
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("Conflict")));
    // the server's copy then replaces the original
    let mut f = ics::FeedEvent { title: "Server's".into(), start: App::now(), end: App::now(), all_day: false, notes: String::new(), uid: "u1".into(), recurrence_id: None, href: Some("https://x.example/dav/c/u1.ics".into()), etag: Some("\"2\"".into()) };
    app.data.apply_delta(901, vec![f.clone()], &[]);
    assert_eq!(app.data.event(500).unwrap().title, "Server's");
    // but an edit not yet sent is not written over by a pull
    app.data.event_mut(500).unwrap().title = "Mine again".into();
    app.data.event_mut(500).unwrap().dirty = true;
    f.title = "Server's, later".into();
    app.data.apply_delta(901, vec![f], &[]);
    assert_eq!(app.data.event(500).unwrap().title, "Mine again");

    // an event that was removed here while it was being written: the
    // server's fresh copy is deleted too, at its new version
    let mut gone = app.data.event(501).unwrap().clone();
    gone.id = 777;
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: gone.clone(), uid: "g".into(), href: "gone".into(), etag: Some("1".into()) }]);
    assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "gone".into(), etag: Some("1".into()) }]);
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Saved { sent: gone, uid: "g".into(), href: "gone".into(), etag: Some("2".into()) }]);
    assert_eq!(app.data.outbox, vec![PendingDelete { calendar: 901, href: "gone".into(), etag: Some("2".into()) }]);
    assert!(app.outbox_changed.is_some());

    // deletions leave the outbox either way
    app.data.outbox = vec![PendingDelete { calendar: 901, href: "a".into(), etag: None }, PendingDelete { calendar: 901, href: "b".into(), etag: None }];
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Deleted { href: "a".into() }, sync::PushResult::DeleteConflict { href: "b".into() }]);
    assert!(app.data.outbox.is_empty());
    // a failure keeps the event marked and shows on the calendar
    app.take_push_results(&ctx, 901, vec![sync::PushResult::Failed { error: sync::SyncError::RateLimited }]);
    assert!(app.data.calendar(901).unwrap().error.is_some() && app.data.event(500).unwrap().dirty);
}
