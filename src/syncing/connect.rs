//! Adding and removing calendars and accounts: links, sign-ins and sign-outs.

use crate::strings::tr;
use super::*;

impl App {
    /// Starts subscribing to the link typed in preferences.
    pub fn subscribe(&mut self, ctx: &egui::Context) {
        let url = match ics::normalize_url(&self.sub_url) {
            Ok(u) => u,
            Err(e) => return self.say(ctx, e, false),
        };
        if self.data.calendars.iter().any(|c| c.url.as_deref() == Some(url.as_str())) {
            return self.say(ctx, tr("You are already subscribed to this calendar"), false);
        }
        let name = self.sub_name.trim();
        let n = self.data.calendars.iter().filter(|c| c.url.is_some()).count();
        let id = self.data.new_id();
        self.data.calendars.push(Calendar {
            id,
            read_only: true,
            name: if name.is_empty() { ics::default_name(&url) } else { name.to_string() },
            color: EVENT_COLORS[(6 + n) % EVENT_COLORS.len()].0,
            visible: true,
            url: Some(url.clone()),
            ..Default::default()
        });
        self.sub_url.clear();
        self.sub_name.clear();
        self.commit();
        self.keep_private_links(ctx);
        self.sync_feed(ctx, id);
    }

    /// A Proton link holds the keys to its calendar: it is moved into the
    /// keyring, and links already there are read back.
    pub fn keep_private_links(&mut self, ctx: &egui::Context) {
        if self.shot.is_some() {
            return;
        }
        let allow_file = self.data.prefs.plaintext_secrets;
        for c in &self.data.calendars {
            let (id, Some(url)) = (c.id, c.url.clone()) else { continue };
            if c.url_secret && url == SECRET_URL {
                self.in_background(ctx, move |send| send(SyncDone::Link { cal: id, res: secrets::get(&secrets::feed_key(id), allow_file) }));
            } else if !c.url_secret && ics::is_proton(&url) {
                self.in_background(ctx, move |send| send(SyncDone::LinkKept { cal: id, res: secrets::set(&secrets::feed_key(id), &url, allow_file) }));
            }
        }
    }

    /// Removes a calendar and forgets its private link.
    pub fn drop_calendar(&mut self, id: u64) {
        if self.data.calendar(id).is_some_and(|c| c.url_secret) && self.shot.is_none() {
            crate::worker::spawn("forget-link", move || secrets::delete(&secrets::feed_key(id)));
        }
        self.data.remove_calendar(id);
    }

    /// Signs in to the server typed in preferences.
    pub fn connect_account(&mut self, ctx: &egui::Context) {
        let (server, user) = (self.acct_server.trim().to_string(), self.acct_user.trim().to_string());
        if server.is_empty() || user.is_empty() || self.acct_pass.is_empty() {
            self.acct_error = Some(tr("Fill in the server address, the user name and the app password.").into());
            return;
        }
        if self.data.accounts.iter().any(|a| a.server == server && a.username == user) {
            self.acct_error = Some(tr("This account is already connected.").into());
            return;
        }
        self.acct_error = None;
        let id = self.data.new_id();
        self.data.accounts.push(Account { id, server, username: user, ..Default::default() });
        // the password leaves the form at once; it is kept only once it worked
        let pass = std::mem::take(&mut self.acct_pass);
        self.sync_account(ctx, id, Some(pass));
    }

    /// Signs out: the calendars go, and the password leaves the keyring.
    pub fn remove_account(&mut self, id: u64) {
        self.snapshot();
        let gone = self.data.account(id).cloned();
        self.data.remove_account(id);
        let allow_file = self.data.prefs.plaintext_secrets;
        if self.shot.is_none() {
            crate::worker::spawn("sign-out", move || {
                let key = secrets::account_key(id);
                // a Nextcloud app password is taken back at the server too
                // a Google sign-in likewise
                // a Proton session is ended there as well
                if let Some((uid, refresh, _)) = gone.as_ref().filter(|a| a.proton).and_then(|_| secrets::get(&key, allow_file).ok()).and_then(|s| sync::proton::unpack(&s)) {
                    if let Ok((access, _)) = sync::proton::refresh(sync::proton::API, &uid, &refresh) {
                        sync::proton::sign_out(sync::proton::API, &uid, &access);
                    }
                }
                if let Some(a) = gone.filter(|a| a.nextcloud || a.google) {
                    if let Ok(pass) = secrets::get(&key, allow_file) {
                        match sync::google::unpack(&pass).filter(|_| a.google) {
                            Some((refresh, _)) => sync::google::revoke(&refresh),
                            None => sync::nextcloud::revoke(&a.server, &a.username, &pass),
                        }
                    }
                }
                secrets::delete(&key);
            });
        }
        self.commit();
    }

    /// Signs in to a Nextcloud server through its own page in the
    /// browser. The page is opened, and the server is asked every two
    /// seconds whether the person is through, for up to five minutes.
    pub fn connect_nextcloud(&mut self, ctx: &egui::Context) {
        let server = self.acct_server.trim().to_string();
        if server.is_empty() {
            self.acct_error = Some(tr("Fill in the server address first.").into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            use std::sync::atomic::Ordering;
            let res = sync::nextcloud::start(&server).and_then(|pending| {
                let _ = std::process::Command::new("xdg-open").arg(&pending.login).spawn();
                send(SyncDone::NextcloudOpened { login: pending.login.clone() });
                let began = std::time::Instant::now();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if stop.load(Ordering::Relaxed) {
                        return Err(sync::SyncError::Other(tr("Sign-in was called off.").into()));
                    }
                    if began.elapsed().as_secs() > NEXTCLOUD_WAIT_S {
                        return Err(sync::SyncError::Other(tr("Nobody signed in within five minutes. Try again.").into()));
                    }
                    if let Some(g) = sync::nextcloud::poll(&pending)? {
                        return Ok(g);
                    }
                }
            });
            send(SyncDone::NextcloudDone { res });
        });
    }

    /// Signs in to Google through its page in the browser, with the
    /// client ID and secret the person brought.
    pub fn connect_google(&mut self, ctx: &egui::Context) {
        let (email, client_id, secret) = (self.g_email.trim().to_string(), self.g_client_id.trim().to_string(), self.g_secret.trim().to_string());
        if !email.contains('@') || client_id.is_empty() {
            self.acct_error = Some(tr("Fill in your Google address and the client ID of your Google Cloud project.").into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            let app = sync::google::app(&client_id, &secret, &email);
            let opened = |url: &str| {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                send(SyncDone::NextcloudOpened { login: url.to_string() });
            };
            let res = sync::oauth::authorize(&app, &opened, &stop, NEXTCLOUD_WAIT_S).and_then(|t| match t.refresh {
                Some(r) => Ok(GoogleGrant { microsoft: false, email, client_id, secret: sync::google::pack(&r, &secret), access: t.access, expires_in: t.expires_in }),
                None => Err(sync::SyncError::Other(tr("Google signed you in but gave nothing to stay signed in with. Remove kalendar under your Google account's third-party access, then try again.").into())),
            });
            send(SyncDone::GoogleDone { res });
        });
    }

    /// Signs in to Microsoft through its page in the browser, with the
    /// client ID of the app registration the person brought.
    pub fn connect_microsoft(&mut self, ctx: &egui::Context) {
        let (email, client_id) = (self.ms_email.trim().to_string(), self.ms_client_id.trim().to_string());
        if !email.contains('@') || client_id.is_empty() {
            self.acct_error = Some(tr("Fill in your Microsoft address and the client ID of your app registration.").into());
            return;
        }
        if self.shot.is_some() || self.nextcloud_wait.is_some() {
            return;
        }
        self.acct_error = None;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.nextcloud_wait = Some((String::new(), stop.clone()));
        self.in_background(ctx, move |send| {
            let app = sync::graph::app(&client_id, &email);
            let opened = |url: &str| {
                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                send(SyncDone::NextcloudOpened { login: url.to_string() });
            };
            let res = sync::oauth::authorize(&app, &opened, &stop, NEXTCLOUD_WAIT_S).and_then(|t| match t.refresh {
                Some(r) => Ok(GoogleGrant { microsoft: true, email, client_id, secret: sync::google::pack(&r, ""), access: t.access, expires_in: t.expires_in }),
                None => Err(sync::SyncError::Other(tr("Microsoft signed you in but gave nothing to stay signed in with. Check that the app registration allows the offline_access permission.").into())),
            });
            send(SyncDone::GoogleDone { res });
        });
    }

    /// Signs in to Proton with what the form holds. The password is used
    /// for this one sign-in and not kept.
    pub fn connect_proton(&mut self, ctx: &egui::Context) {
        let username = self.pr_user.trim().to_string();
        if username.is_empty() || self.pr_pass.is_empty() {
            self.acct_error = Some(tr("Fill in your Proton address and password.").into());
            return;
        }
        if self.shot.is_some() || self.pr_busy {
            return;
        }
        self.acct_error = None;
        self.pr_busy = true;
        let (pass, code, second, human) = (self.pr_pass.clone(), self.pr_code.clone(), self.pr_second.clone(), self.pr_human.clone());
        self.in_background(ctx, move |send| {
            let res = sync::proton::sign_in(sync::proton::API, &username, &pass, &code, &second, human.as_deref());
            send(SyncDone::ProtonDone { username, res });
        });
    }

    pub fn cancel_nextcloud(&mut self) {
        if let Some((_, stop)) = self.nextcloud_wait.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
