//! The window's side of syncing: opening accounts, sending the work to
//! background threads and taking in what they hand back.

use chrono::Local;
use eframe::egui;
use crate::model::*;
use crate::*;
use crate::sync::{self, caldav::CalDav, Provider};


mod connect;
mod edits;
mod run;
mod take;
#[cfg(test)]
mod tests;

/// What a background thread hands back to the window.
pub enum SyncDone {
    /// a calendar's events were read
    Pulled { cal: u64, res: Result<sync::Delta, sync::SyncError> },
    /// an account's calendars were listed; `first` on signing in
    Listed { account: u64, first: bool, home: Option<String>, res: Result<Vec<sync::RemoteCalendar>, sync::SyncError> },
    /// a private link was read from the keyring
    Link { cal: u64, res: Result<String, String> },
    /// a private link was put into the keyring
    LinkKept { cal: u64, res: Result<(), String> },
    /// a task list's tasks were read
    PulledTasks { list: u64, res: Result<sync::Delta, sync::SyncError> },
    /// changes made here were sent to the server
    Pushed { cal: u64, results: Vec<sync::PushResult> },
    /// Nextcloud's sign-in page is open in the browser
    NextcloudOpened { login: String },
    /// the person signed in on Nextcloud's page, or it failed
    NextcloudDone { res: Result<sync::nextcloud::Granted, sync::SyncError> },
    /// the person signed in on Google's page, or it failed
    GoogleDone { res: Result<GoogleGrant, sync::SyncError> },
    /// a sign-in to Proton went through, wants a check first, or failed
    ProtonDone { username: String, res: Result<sync::proton::SignIn, sync::SyncError> },
    /// GitHub was asked for a newer release
    Update { res: Result<Option<crate::update::Release>, String> },
}

pub struct GoogleGrant {
    microsoft: bool,
    email: String,
    client_id: String,
    /// what goes into the keyring: refresh token and client secret
    secret: String,
    access: String,
    expires_in: u64,
}

/// Access tokens of signed-in accounts: in memory only, shared with the
/// background threads. Each is kept with the time it runs out.
pub type TokenCache = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u64, (String, std::time::Instant)>>>;

/// A connection to an account's server. `secret` is what the keyring
/// holds for it: an app password, or for Google the refresh token, from
/// which an access token is had when the last one has run out.
pub fn open_account(a: &Account, secret: &str, tokens: &TokenCache, allow_file: bool) -> Result<Box<dyn Provider>, sync::SyncError> {
    if a.proton {
        let (uid, refresh, key_pass) = sync::proton::unpack(secret).ok_or(sync::SyncError::Auth)?;
        let access = tokens.lock().ok().and_then(|t| t.get(&a.id).map(|(t, _)| t.clone())).unwrap_or_default();
        // Proton hands out a new refresh token with every access token;
        // the newest is the one to keep
        let (id, cache, kept_uid, kept_pass) = (a.id, tokens.clone(), uid.clone(), key_pass.clone());
        let renewed = move |access: &str, refresh: &str| {
            if let Ok(mut c) = cache.lock() {
                c.insert(id, (access.to_string(), std::time::Instant::now()));
            }
            let _ = secrets::set(&secrets::account_key(id), &sync::proton::pack(&kept_uid, refresh, &kept_pass), allow_file);
        };
        return Ok(Box::new(sync::proton::Proton::new(&uid, &access, &refresh, &key_pass, Some(Box::new(renewed)))));
    }
    if !a.google && !a.microsoft {
        return Ok(Box::new(CalDav::new(&a.server, &a.username, secret, a.home.clone())));
    }
    let now = std::time::Instant::now();
    let cached = tokens.lock().ok().and_then(|t| t.get(&a.id).filter(|(_, until)| *until > now).map(|(t, _)| t.clone()));
    let access = match cached {
        Some(t) => t,
        None => {
            let (refresh, client_secret) = sync::google::unpack(secret).ok_or(sync::SyncError::Auth)?;
            let client_id = a.client_id.as_deref().unwrap_or("");
            let app = if a.microsoft { sync::graph::app(client_id, &a.username) } else { sync::google::app(client_id, &client_secret, &a.username) };
            let t = sync::oauth::refresh(&app, &refresh)?;
            if let Ok(mut cache) = tokens.lock() {
                cache.insert(a.id, (t.access.clone(), now + std::time::Duration::from_secs(t.expires_in.saturating_sub(60))));
            }
            // Microsoft hands out a new refresh token each time; the
            // newest is the one to keep
            if let Some(newer) = t.refresh.filter(|r| *r != refresh) {
                let _ = secrets::set(&secrets::account_key(a.id), &sync::google::pack(&newer, &client_secret), allow_file);
            }
            t.access
        }
    };
    if a.microsoft {
        return Ok(Box::new(sync::graph::Graph::new(&access)));
    }
    Ok(Box::new(CalDav::with_bearer(&a.server, &a.username, &access, a.home.clone())))
}

/// Why an account's sync failed, in words that say what to do.
pub fn account_error(a: Option<&Account>, e: &sync::SyncError) -> String {
    match (a, e) {
        (Some(a), sync::SyncError::Auth) if a.proton => "Proton wants you to sign in again (the session ended or the password changed): sign out of this account here, then sign in once more.".into(),
        (Some(a), sync::SyncError::Auth) if a.microsoft => "Microsoft wants you to sign in again: sign out of this account here, then sign in once more.".into(),
        (Some(a), sync::SyncError::Auth) if a.google => "Google wants you to sign in again: sign out of this account here, then sign in once more. While a Google Cloud project is in Testing, a sign-in lasts 7 days.".into(),
        _ => e.text(),
    }
}

impl App {
    /// Runs `work` on a background thread and hands its result to the
    /// window, which is woken to take it in.
    pub fn in_background(&self, ctx: &egui::Context, work: impl FnOnce(&dyn Fn(SyncDone)) + Send + 'static) {
        let (tx, ctx) = (self.sync_tx.clone(), ctx.clone());
        crate::worker::spawn("sync", move || {
            work(&|done| {
                let _ = tx.send(done);
                ctx.request_repaint();
            });
        });
    }
}
