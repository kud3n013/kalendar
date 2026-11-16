//! Starting a sync: of a feed, of an account, of all that are due.

use super::*;

impl App {
    /// Fetches one subscribed calendar in the background.
    pub fn sync_feed(&mut self, ctx: &egui::Context, cal: u64) {
        let Some(url) = self.data.calendar(cal).and_then(|c| c.url.clone()).filter(|u| u != SECRET_URL) else { return };
        if self.shot.is_some() || !self.syncing.insert(cal) {
            return;
        }
        let now = Self::now();
        self.in_background(ctx, move |send| {
            let res = sync::IcsFeed { url }.pull(&sync::RemoteCalendar::default(), None, now);
            send(SyncDone::Pulled { cal, res });
        });
    }

    /// Asks an account's server which calendars it has; their events are
    /// pulled once the answer is in. `password` is given on first sign-in,
    /// and kept in the keyring if the server takes it.
    pub fn sync_account(&mut self, ctx: &egui::Context, account: u64, password: Option<String>) {
        let Some(a) = self.data.account(account).cloned() else { return };
        if self.shot.is_some() || !self.syncing.insert(account) {
            return;
        }
        let allow_file = self.data.prefs.plaintext_secrets;
        let first = password.is_some();
        let tokens = self.tokens.clone();
        self.in_background(ctx, move |send| {
            let key = secrets::account_key(account);
            let pass = match password {
                Some(p) => Ok(p),
                None => secrets::get(&key, allow_file),
            };
            let (home, res) = match pass {
                Err(e) => (None, Err(sync::SyncError::Other(e))),
                Ok(p) => match open_account(&a, &p, &tokens, allow_file) {
                  Err(e) => (None, Err(e)),
                  Ok(mut dav) => {
                    let mut res = dav.list_calendars();
                    // the calendars may have moved since they were found
                    if a.home.is_some() && matches!(res, Err(sync::SyncError::Other(_))) {
                        dav.forget_home();
                        res = dav.list_calendars();
                    }
                    if first && res.is_ok() {
                        if let Err(e) = secrets::set(&key, &p, allow_file) {
                            res = Err(sync::SyncError::Other(e));
                        }
                    }
                    (dav.home(), res)
                  }
                },
            };
            send(SyncDone::Listed { account, first, home, res });
        });
    }

    /// Pulls the shown calendars of an account, one after the other.
    pub fn pull_account(&mut self, ctx: &egui::Context, account: u64) {
        let Some(a) = self.data.account(account).cloned() else { return };
        if self.shot.is_some() {
            return;
        }
        // the last part says whether it is a task list
        let mut jobs: Vec<(u64, sync::RemoteCalendar, Option<String>, Vec<sync::PushJob>, bool)> = vec![];
        for c in self.data.calendars.iter().filter(|c| c.account == Some(account) && c.visible) {
            let Some(id) = c.remote_id.clone() else { continue };
            if !self.syncing.insert(c.id) {
                continue;
            }
            // what was changed here goes out first
            let mut out: Vec<sync::PushJob> = vec![];
            if c.is_synced_writable() {
                out.extend(self.data.events.iter().filter(|e| e.calendar == c.id && e.dirty).cloned().map(sync::PushJob::Save));
                out.extend(self.data.outbox.iter().filter(|d| d.calendar == c.id).map(|d| sync::PushJob::Delete { href: d.href.clone(), etag: d.etag.clone() }));
            }
            jobs.push((c.id, sync::RemoteCalendar { id, ctag: c.ctag.clone(), ..Default::default() }, c.sync_state.clone(), out, false));
        }
        // and its task lists
        for l in self.data.lists.iter().filter(|l| l.account == Some(account)) {
            let Some(id) = l.remote_id.clone() else { continue };
            if !self.syncing.insert(l.id) {
                continue;
            }
            let mut out: Vec<sync::PushJob> = vec![];
            if l.is_synced_writable() {
                out.extend(self.data.tasks.iter().filter(|t| t.list == l.id && t.dirty).cloned().map(sync::PushJob::SaveTask));
                out.extend(self.data.outbox.iter().filter(|d| d.calendar == l.id).map(|d| sync::PushJob::Delete { href: d.href.clone(), etag: d.etag.clone() }));
            }
            jobs.push((l.id, sync::RemoteCalendar { id, ctag: l.ctag.clone(), ..Default::default() }, l.sync_state.clone(), out, true));
        }
        if jobs.is_empty() {
            return;
        }
        let (allow_file, now) = (self.data.prefs.plaintext_secrets, Self::now());
        let tokens = self.tokens.clone();
        self.in_background(ctx, move |send| {
            // the keyring's answer and the sign-in both have to work out
            let pass = secrets::get(&secrets::account_key(account), allow_file).map_err(sync::SyncError::Other);
            let opened = pass.and_then(|p| open_account(&a, &p, &tokens, allow_file));
            let (mut dav, pass) = match opened {
                Ok(d) => (Some(d), Ok(())),
                Err(e) => (None, Err(e)),
            };
            for (cal, remote, mut state, out, tasks) in jobs {
                if let (Some(dav), false) = (&mut dav, out.is_empty()) {
                    let results = sync::push(dav.as_mut(), &remote, &out, chrono::Utc::now().naive_utc());
                    // after a conflict the server's copy is read afresh
                    if results.iter().any(|r| matches!(r, sync::PushResult::Conflict { .. } | sync::PushResult::TaskConflict { .. } | sync::PushResult::DeleteConflict { .. })) {
                        state = None;
                    }
                    send(SyncDone::Pushed { cal, results });
                }
                let res = match (&mut dav, &pass) {
                    (Some(dav), _) if tasks => dav.pull_tasks(&remote, state.as_deref(), now),
                    (Some(dav), _) => dav.pull(&remote, state.as_deref(), now),
                    (None, Err(e)) => Err(e.clone()),
                    (None, Ok(_)) => unreachable!(),
                };
                send(if tasks { SyncDone::PulledTasks { list: cal, res } } else { SyncDone::Pulled { cal, res } });
            }
        });
    }

    /// Refreshes everything now, whatever failed before.
    pub fn sync_all(&mut self, ctx: &egui::Context) -> usize {
        self.backoff.clear();
        self.sync_due(ctx, f64::MAX)
    }

    /// Refreshes what is not waiting after a failure.
    pub fn sync_due(&mut self, ctx: &egui::Context, time: f64) -> usize {
        let ready = |b: &std::collections::HashMap<u64, (u32, f64)>, id: u64| b.get(&id).is_none_or(|(_, next)| time >= *next);
        let feeds: Vec<u64> = self.data.calendars.iter().filter(|c| c.url.is_some() && ready(&self.backoff, c.id)).map(|c| c.id).collect();
        let accounts: Vec<u64> = self.data.accounts.iter().filter(|a| ready(&self.backoff, a.id)).map(|a| a.id).collect();
        for id in &feeds {
            self.sync_feed(ctx, *id);
        }
        for id in &accounts {
            self.sync_account(ctx, *id, None);
        }
        feeds.len() + accounts.len()
    }

    /// After a failure the next automatic try waits twice as long each
    /// time, up to four hours.
    pub fn back_off(&mut self, id: u64, time: f64) {
        let fails = self.backoff.get(&id).map(|b| b.0).unwrap_or(0) + 1;
        let wait = SYNC_EVERY_S * 2f64.powi(fails.min(4) as i32);
        self.backoff.insert(id, (fails, time + wait));
    }

    /// Whether the tasks kept in a synced calendar are brought in as a list.
    pub fn calendar_tasks_on(&self, cal: u64) -> bool {
        let Some(c) = self.data.calendar(cal) else { return false };
        self.data.lists.iter().any(|l| l.account == c.account && l.account.is_some() && l.remote_id == c.remote_id)
    }

    /// Brings the tasks kept in a synced calendar in as a task list, or
    /// takes that list away again. The server keeps its tasks either way.
    pub fn set_calendar_tasks(&mut self, ctx: &egui::Context, cal: u64, on: bool) {
        let Some(c) = self.data.calendar(cal).cloned() else { return };
        let (Some(account), Some(remote_id)) = (c.account, c.remote_id.clone()) else { return };
        if on && !self.calendar_tasks_on(cal) {
            self.add_synced_list(account, remote_id, &c.name, Some(c.color), c.read_only);
            self.reshadow();
            self.commit();
            self.pull_account(ctx, account);
        } else if !on {
            let lists: Vec<u64> = self.data.lists.iter().filter(|l| l.account == Some(account) && l.remote_id.as_ref() == Some(&remote_id)).map(|l| l.id).collect();
            for l in lists {
                self.data.remove_synced_list(l);
            }
            self.reshadow();
            self.commit();
        }
    }
}
