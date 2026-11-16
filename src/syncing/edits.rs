//! Noting what changed here, so it can be sent to the server.

use super::*;

impl App {
    /// Remembers synced events as they are now; what differs from this
    /// later was changed here.
    pub fn reshadow(&mut self) {
        let writable: std::collections::HashSet<u64> = self.data.calendars.iter().filter(|c| c.is_synced_writable()).map(|c| c.id).collect();
        self.shadow = self.data.events.iter().filter(|e| writable.contains(&e.calendar)).map(|e| (e.id, e.clone())).collect();
        let lists: std::collections::HashSet<u64> = self.data.lists.iter().filter(|l| l.is_synced_writable()).map(|l| l.id).collect();
        self.task_shadow = self.data.tasks.iter().filter(|t| lists.contains(&t.list)).map(|t| (t.id, t.clone())).collect();
    }

    /// The same as `note_edits`, for tasks in lists that sync both ways.
    pub fn note_task_edits(&mut self) -> bool {
        let lists: std::collections::HashSet<u64> = self.data.lists.iter().filter(|l| l.is_synced_writable()).map(|l| l.id).collect();
        if lists.is_empty() && self.task_shadow.is_empty() {
            return false;
        }
        let mut touched = false;
        let mut here = std::collections::HashSet::new();
        for t in &mut self.data.tasks {
            here.insert(t.id);
            let old = self.task_shadow.get(&t.id);
            if let Some(o) = old.filter(|o| o.list != t.list) {
                // moved to another list: gone from the old one, new in this.
                // A list that was removed here keeps its tasks on the server.
                if let Some(href) = o.href.as_ref().filter(|_| lists.contains(&o.list)) {
                    self.data.outbox.push(PendingDelete { calendar: o.list, href: href.clone(), etag: o.etag.clone() });
                    touched = true;
                }
                t.detach();
            } else if old.is_none() && t.href.is_some() && lists.contains(&t.list) {
                // a copy of a synced task, or one brought back by undo
                t.detach();
            }
            if !lists.contains(&t.list) {
                continue;
            }
            let same = old.is_some_and(|o| o.list == t.list && o.same_content(t));
            if !same {
                t.dirty = true;
                touched = true;
            }
        }
        for (id, o) in &self.task_shadow {
            if here.contains(id) || !lists.contains(&o.list) {
                continue;
            }
            if let Some(href) = o.href.clone().filter(|h| !self.data.outbox.iter().any(|d| &d.href == h)) {
                self.data.outbox.push(PendingDelete { calendar: o.list, href, etag: o.etag.clone() });
                touched = true;
            }
        }
        touched
    }

    /// Finds what was changed here in calendars that sync both ways, and
    /// marks it to be sent: changed and new events are flagged, deleted
    /// ones are noted in the outbox. Runs on every save.
    pub fn note_edits(&mut self) {
        let writable: std::collections::HashSet<u64> = self.data.calendars.iter().filter(|c| c.is_synced_writable()).map(|c| c.id).collect();
        let mut touched = self.note_task_edits();
        if writable.is_empty() && self.shadow.is_empty() {
            self.reshadow();
            if touched {
                self.outbox_changed = Some(f64::NAN);
            }
            return;
        }
        let mut here = std::collections::HashSet::new();
        for e in &mut self.data.events {
            here.insert(e.id);
            let old = self.shadow.get(&e.id);
            if let Some(o) = old.filter(|o| o.calendar != e.calendar) {
                // moved to another calendar: gone from the old one, new in this
                if let Some(href) = &o.href {
                    self.data.outbox.push(PendingDelete { calendar: o.calendar, href: href.clone(), etag: o.etag.clone() });
                    touched = true;
                }
                e.detach();
            } else if old.is_none() && (e.remote || e.href.is_some()) && writable.contains(&e.calendar) {
                // a copy of a synced event, or one brought back by undo
                e.detach();
            }
            if !writable.contains(&e.calendar) {
                continue;
            }
            let same = old.is_some_and(|o| o.calendar == e.calendar && o.same_content(e));
            if !same && !e.dirty {
                e.dirty = true;
            }
            touched |= !same;
        }
        for (id, o) in &self.shadow {
            if here.contains(id) {
                continue;
            }
            if let Some(href) = o.href.clone().filter(|h| !self.data.outbox.iter().any(|d| &d.href == h)) {
                // a calendar that went away takes its events without a word to the server
                if writable.contains(&o.calendar) {
                    self.data.outbox.push(PendingDelete { calendar: o.calendar, href, etag: o.etag.clone() });
                    touched = true;
                }
            }
        }
        self.reshadow();
        if touched {
            self.outbox_changed = Some(f64::NAN);
        }
    }

    /// Takes in how the changes sent to a server fared.
    /// Something was written to the server while it was removed or
    /// replaced here: the server's copy has to go too, or it would come
    /// back as a second one.
    pub fn forget_on_server(&mut self, cal: u64, href: String, etag: Option<String>) {
        match self.data.outbox.iter_mut().find(|d| d.calendar == cal && d.href == href) {
            // already to be deleted, but the write gave it a new version
            Some(d) => d.etag = etag,
            None => self.data.outbox.push(PendingDelete { calendar: cal, href, etag }),
        }
        self.outbox_changed = Some(f64::NAN);
    }
}
