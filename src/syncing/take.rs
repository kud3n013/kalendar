//! Taking in what the background threads hand back.

use crate::strings::{tr, trf};
use super::*;

impl App {
    /// Takes in whatever the background work has finished.
    pub fn poll_sync(&mut self, ctx: &egui::Context) {
        let time = ctx.input(|i| i.time);
        let stamp = || Some(Local::now().format("%b %-d, %H:%M").to_string());
        while let Ok(done) = self.sync_rx.try_recv() {
            match done {
                SyncDone::Update { res } => self.take_update(res),
                SyncDone::Pulled { cal, res } => {
                    self.syncing.remove(&cal);
                    let Some(c) = self.data.calendar(cal) else { continue };
                    let (feed_url, account, name) = (c.url.clone(), c.account, c.name.clone());
                    match res {
                        Ok(delta) => {
                            let count = delta.upserts.len();
                            // a feed that names itself replaces the stand-in name
                            let own = delta.name.clone().filter(|_| feed_url.as_deref().map(ics::default_name).as_deref() == Some(name.as_str()));
                            self.data.apply(cal, delta);
                            let Some(c) = self.data.calendar_mut(cal) else { continue };
                            c.last_sync = stamp();
                            c.error = None;
                            if let Some(n) = own {
                                c.name = n;
                            }
                            let name = c.name.clone();
                            self.backoff.remove(&cal);
                            if feed_url.is_some() {
                                self.say(ctx, trf("{}: {} events", &[&name, &count]), false);
                            }
                        }
                        Err(e) => {
                            let text = e.text();
                            if feed_url.is_some() {
                                self.say(ctx, trf("{}: {}", &[&name, &text]), false);
                            }
                            if let Some(c) = self.data.calendar_mut(cal) {
                                c.error = Some(text.clone());
                            }
                            let why = account_error(account.and_then(|a| self.data.account(a)), &e);
                            if let Some(a) = account.and_then(|a| self.data.account_mut(a)) {
                                a.error = Some(why);
                            }
                            self.back_off(account.unwrap_or(cal), time);
                        }
                    }
                }
                SyncDone::Listed { account, first, home, res } => {
                    self.syncing.remove(&account);
                    if self.data.account(account).is_none() {
                        continue;
                    }
                    match res {
                        Ok(list) => {
                            self.take_calendar_list(account, home, &list);
                            self.backoff.remove(&account);
                            if first {
                                self.sub_kind = 0;
                                self.acct_server.clear();
                                self.acct_user.clear();
                                let n = list.len();
                                self.say(ctx, trf("Connected: {} calendar{}", &[&n, &if n == 1 { "" } else { "s" }]), false);
                            }
                            self.pull_account(ctx, account);
                        }
                        Err(e) if first => {
                            // nothing was kept: the form shows what went wrong
                            self.acct_server = self.data.account(account).map(|a| a.server.clone()).unwrap_or_default();
                            self.acct_user = self.data.account(account).map(|a| a.username.clone()).unwrap_or_default();
                            self.data.remove_account(account);
                            self.acct_error = Some(e.text());
                        }
                        Err(e) => {
                            let why = account_error(self.data.account(account), &e);
                            if let Some(a) = self.data.account_mut(account) {
                                a.error = Some(why);
                            }
                            self.back_off(account, time);
                        }
                    }
                }
                SyncDone::PulledTasks { list, res } => {
                    self.syncing.remove(&list);
                    let Some(account) = self.data.lists.iter().find(|l| l.id == list).and_then(|l| l.account) else { continue };
                    match res {
                        Ok(delta) => self.data.apply_tasks(list, delta),
                        Err(e) => {
                            let why = account_error(self.data.account(account), &e);
                            if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == list) {
                                l.error = Some(e.text());
                            }
                            if let Some(a) = self.data.account_mut(account) {
                                a.error = Some(why);
                            }
                            self.back_off(account, time);
                        }
                    }
                }
                SyncDone::Pushed { cal, results } => self.take_push_results(ctx, cal, results),
                SyncDone::NextcloudOpened { login } => {
                    if let Some(w) = &mut self.nextcloud_wait {
                        w.0 = login;
                    }
                }
                SyncDone::NextcloudDone { res } => {
                    if self.nextcloud_wait.take().is_none() {
                        continue; // called off meanwhile
                    }
                    match res {
                        Ok(g) => {
                            let (server, user) = (g.caldav(), g.username.clone());
                            if self.data.accounts.iter().any(|a| a.server == server && a.username == user) {
                                self.acct_error = Some(tr("This account is already connected.").into());
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username: user, nextcloud: true, ..Default::default() });
                                self.sync_account(ctx, id, Some(g.app_password));
                            }
                        }
                        Err(e) => self.acct_error = Some(e.text()),
                    }
                }
                SyncDone::GoogleDone { res } => {
                    if self.nextcloud_wait.take().is_none() {
                        continue; // called off meanwhile
                    }
                    match res {
                        Ok(g) => {
                            let server = if g.microsoft { format!("microsoft:{}", g.email.to_lowercase()) } else { sync::google::server(&g.email) };
                            if self.data.accounts.iter().any(|a| a.server == server) {
                                self.acct_error = Some(tr("This account is already connected.").into());
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username: g.email, google: !g.microsoft, microsoft: g.microsoft, client_id: Some(g.client_id), ..Default::default() });
                                if let Ok(mut cache) = self.tokens.lock() {
                                    cache.insert(id, (g.access, std::time::Instant::now() + std::time::Duration::from_secs(g.expires_in.saturating_sub(60))));
                                }
                                self.g_secret.clear();
                                self.sub_kind = 0;
                                self.sync_account(ctx, id, Some(g.secret));
                            }
                        }
                        Err(e) => self.acct_error = Some(e.text()),
                    }
                }
                SyncDone::ProtonDone { username, res } => {
                    self.pr_busy = false;
                    match res {
                        Ok(sync::proton::SignIn::Done(g)) => {
                            let server = format!("proton:{}", username.to_lowercase());
                            if self.data.accounts.iter().any(|a| a.server == server) {
                                self.acct_error = Some(tr("This account is already connected.").into());
                                crate::worker::spawn("sign-out", move || sync::proton::sign_out(sync::proton::API, &g.uid, &g.access));
                            } else {
                                let id = self.data.new_id();
                                self.data.accounts.push(Account { id, server, username, proton: true, ..Default::default() });
                                if let Ok(mut cache) = self.tokens.lock() {
                                    cache.insert(id, (g.access, std::time::Instant::now()));
                                }
                                self.pr_pass.clear();
                                self.pr_code.clear();
                                self.pr_second.clear();
                                self.pr_human = None;
                                self.sub_kind = 0;
                                self.sync_account(ctx, id, Some(sync::proton::pack(&g.uid, &g.refresh, &g.key_pass)));
                            }
                        }
                        Ok(sync::proton::SignIn::Human { token, url }) => {
                            self.pr_human = Some(token);
                            crate::platform::open_url(&url);
                            self.acct_error = Some(tr("Proton asks for a check that you are human. Finish it on the page that opened in your browser, then press the sign-in button again.").into());
                        }
                        Err(e) => {
                            self.pr_human = None;
                            self.acct_error = Some(match e {
                                sync::SyncError::Auth => tr("Proton refused the user name or password.").into(),
                                e => e.text(),
                            });
                        }
                    }
                }
                SyncDone::Link { cal, res } => {
                    let Some(c) = self.data.calendar_mut(cal) else { continue };
                    match res {
                        Ok(url) => {
                            c.url = Some(url);
                            self.sync_feed(ctx, cal);
                        }
                        Err(e) => c.error = Some(trf("Its link is in the keyring and could not be read. {}", &[&e])),
                    }
                }
                SyncDone::LinkKept { cal, res } => {
                    // without a keyring the link stays in data.json, as before
                    if let (Ok(()), Some(c)) = (res, self.data.calendar_mut(cal)) {
                        c.url_secret = true;
                    }
                }
            }
            // what a server sent is not a change made here
            self.reshadow();
            self.commit();
        }
        // a change made here goes out a moment later, so a burst of edits
        // travels together
        if let Some(at) = self.outbox_changed {
            if at.is_nan() {
                self.outbox_changed = Some(time + PUSH_AFTER_S);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(PUSH_AFTER_S + 0.1));
            } else if time >= at {
                self.outbox_changed = None;
                let accounts: Vec<u64> = self.data.accounts.iter().map(|a| a.id).collect();
                for a in accounts {
                    self.pull_account(ctx, a);
                }
            }
        }
        // on start, every 15 minutes, and when the window is come back to;
        // never while it is minimised
        let (focused, minimized) = ctx.input(|i| (i.focused, i.viewport().minimized.unwrap_or(false)));
        let came_back = focused && !self.was_focused && time - self.last_auto_sync > SYNC_ON_FOCUS_S;
        self.was_focused = focused;
        if !self.synced_on_start {
            self.reshadow();
            self.keep_private_links(ctx);
        }
        if (!self.synced_on_start || time - self.last_auto_sync > SYNC_EVERY_S || came_back) && !minimized {
            self.synced_on_start = true;
            self.last_auto_sync = time;
            self.sync_due(ctx, time);
        }
    }

    pub fn take_push_results(&mut self, ctx: &egui::Context, cal: u64, results: Vec<sync::PushResult>) {
        use sync::PushResult::*;
        let (mut conflicts, mut task_conflicts) = (0, 0);
        let mut failed: Option<String> = None;
        for r in results {
            match r {
                Saved { sent, uid, href, etag } => {
                    let Some(e) = self.data.event_mut(sent.id).filter(|e| e.calendar == cal) else {
                        self.forget_on_server(cal, href, etag);
                        continue;
                    };
                    // changed again while it was on its way: it goes out once more
                    e.dirty = !e.same_content(&sent);
                    e.remote = true;
                    (e.uid, e.href, e.etag) = (Some(uid), Some(href), etag);
                }
                Conflict { event } => {
                    // the server's copy wins and comes in with the pull that
                    // follows; the edit made here is kept beside it
                    let own = self.data.first_own_calendar();
                    let Some(e) = self.data.event_mut(event) else { continue };
                    e.dirty = false;
                    let mut copy = e.clone();
                    copy.detach();
                    copy.calendar = own;
                    copy.id = self.data.new_id();
                    self.data.events.push(copy);
                    conflicts += 1;
                }
                TaskSaved { sent, uid, href, etag } => {
                    let Some(t) = self.data.task_mut(sent.id).filter(|t| t.list == cal) else {
                        self.forget_on_server(cal, href, etag);
                        continue;
                    };
                    t.dirty = !t.same_content(&sent);
                    (t.uid, t.href, t.etag) = (Some(uid), Some(href), etag);
                }
                TaskConflict { task } => {
                    // the server's copy wins; the edit made here is kept in the Inbox
                    let Some(t) = self.data.task_mut(task) else { continue };
                    t.dirty = false;
                    let mut copy = t.clone();
                    copy.detach();
                    copy.list = 0;
                    copy.id = self.data.new_id();
                    self.data.tasks.push(copy);
                    task_conflicts += 1;
                }
                Deleted { href } => self.data.outbox.retain(|d| d.href != href),
                DeleteConflict { href } => {
                    // changed on the server since: it stays, and comes back
                    self.data.outbox.retain(|d| d.href != href);
                    conflicts += 1;
                }
                Failed { error } => failed = Some(error.text()),
            }
        }
        let own = self.data.calendar(self.data.first_own_calendar()).map(|c| c.name.clone()).unwrap_or_default();
        if conflicts > 0 {
            self.say(ctx, trf("Conflict: the event was changed on the server, whose copy was kept. Your edit was saved as a copy in {}.", &[&own]), false);
        } else if task_conflicts > 0 {
            self.say(ctx, tr("Conflict: the task was changed on the server, whose copy was kept. Your edit was saved as a copy in the Inbox."), false);
        } else if let Some(e) = &failed {
            self.say(ctx, trf("A change could not be sent and will be tried again. {}", &[&e]), false);
        }
        if let Some(c) = self.data.calendar_mut(cal) {
            c.error = failed;
        } else if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == cal) {
            l.error = failed;
        }
    }

    pub fn add_synced_list(&mut self, account: u64, remote_id: String, name: &str, color: Option<[u8; 3]>, read_only: bool) -> u64 {
        let n = self.data.lists.len();
        let id = self.data.new_id();
        self.data.lists.push(TaskList {
            id,
            name: name.to_string(),
            color: color.unwrap_or(EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0),
            account: Some(account),
            remote_id: Some(remote_id),
            read_only,
            ..Default::default()
        });
        id
    }

    /// Brings the account's calendars in line with the server's list: new
    /// ones are added, ones that are gone leave with their events.
    pub fn take_calendar_list(&mut self, account: u64, home: Option<String>, list: &[sync::RemoteCalendar]) {
        if let Some(a) = self.data.account_mut(account) {
            a.home = home;
            a.error = None;
            a.last_sync = Some(Local::now().format("%b %-d, %H:%M").to_string());
        }
        let gone: Vec<u64> = self
            .data
            .calendars
            .iter()
            .filter(|c| c.account == Some(account) && !list.iter().any(|r| Some(&r.id) == c.remote_id.as_ref()))
            .map(|c| c.id)
            .collect();
        for id in gone {
            self.data.remove_calendar(id);
        }
        // task lists: one for each collection that holds only tasks; one
        // for a calendar that holds both only where the person asked for it
        let gone: Vec<u64> = self.data.lists.iter().filter(|l| l.account == Some(account) && !list.iter().any(|r| r.tasks && Some(&r.id) == l.remote_id.as_ref())).map(|l| l.id).collect();
        for id in gone {
            self.data.remove_synced_list(id);
        }
        for r in list.iter().filter(|r| r.tasks) {
            if let Some(l) = self.data.lists.iter_mut().find(|l| l.account == Some(account) && l.remote_id.as_ref() == Some(&r.id)) {
                l.read_only = r.read_only;
            } else if !r.events {
                self.add_synced_list(account, r.id.clone(), &r.name, r.color, r.read_only);
            }
            // whether it can hold tasks is kept with the calendar, for settings
            if let Some(c) = self.data.calendars.iter_mut().find(|c| c.account == Some(account) && c.remote_id.as_ref() == Some(&r.id)) {
                c.holds_tasks = true;
            }
        }
        for r in list.iter().filter(|r| r.events) {
            if let Some(c) = self.data.calendars.iter_mut().find(|c| c.account == Some(account) && c.remote_id.as_ref() == Some(&r.id)) {
                c.read_only = r.read_only;
                continue;
            }
            let n = self.data.calendars.len();
            let id = self.data.new_id();
            self.data.calendars.push(Calendar {
                id,
                name: r.name.clone(),
                color: r.color.unwrap_or(EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0),
                visible: true,
                account: Some(account),
                remote_id: Some(r.id.clone()),
                read_only: r.read_only,
                ..Default::default()
            });
        }
    }
}
