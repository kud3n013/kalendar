//! Proton Calendar, read-only for now. Proton publishes no calendar API
//! for other programs; this follows what its own open-source clients do
//! (WebClients, go-proton-api, go-srp). Events are end-to-end encrypted:
//! the password opens the account's keys, those open each calendar's key,
//! and that opens the events. The password itself is never kept.

use super::proton_keys::{any, b64, signed_modulus, to_b64, Key};
use super::proton_srp;
use super::{Delta, Provider, RemoteCalendar, SyncError};
use crate::ics;
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const API: &str = "https://mail.proton.me/api";
/// Everything is read again after this many days, so that repeating
/// events are written out further ahead as time passes.
const FULL_EVERY_DAYS: i64 = 30;
const PAGE: usize = 100;

/// How kalendar names itself to Proton. Proton's servers decide which
/// names they take; `KALENDAR_PROTON_APPVERSION` sets another.
fn app_version() -> String {
    std::env::var("KALENDAR_PROTON_APPVERSION").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "Other".into())
}

/// An answer Proton gave that is not a plain yes.
struct Refused {
    status: u16,
    code: i64,
    why: String,
    details: Value,
}

impl Refused {
    fn error(self) -> SyncError {
        match (self.status, self.code) {
            (0, _) => SyncError::Network(self.why),
            (401, _) => SyncError::Auth,
            (429 | 503, _) => SyncError::RateLimited,
            // 8002: wrong password
            (_, 8002) => SyncError::Auth,
            (_, 5002 | 5003) => SyncError::Other(format!("Proton does not take requests from this program as it names itself ({}).", self.why)),
            (_, 9001) => SyncError::Other("Proton asks for a check that you are human. Sign out of this account here and sign in again.".into()),
            (s, c) => SyncError::Other(format!("Proton answered with error {s}/{c}: {}", self.why)),
        }
    }
}

/// Who is asking: the session and its access token.
#[derive(Clone, Copy)]
struct As<'a> {
    uid: &'a str,
    access: &'a str,
}

fn call(base: &str, method: &str, path: &str, who: Option<As>, body: Option<&Value>, human: Option<&str>) -> Result<Value, Refused> {
    let refused = |why: String| Refused { status: 0, code: 0, why, details: Value::Null };
    // passwords' proofs and tokens only travel encrypted
    if !(base.starts_with("https://") || base.starts_with("http://127.0.0.1:")) {
        return Err(refused("the address is not secure".into()));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(30))).http_status_as_error(false).max_redirects(0).build().into();
    let mut req = ureq::http::Request::builder()
        .method(method)
        .uri(format!("{base}{path}"))
        .header("User-Agent", "kalendar")
        .header("Accept", "application/vnd.protonmail.v1+json")
        .header("x-pm-appversion", app_version());
    if let Some(w) = who {
        req = req.header("x-pm-uid", w.uid).header("Authorization", format!("Bearer {}", w.access));
    }
    if let Some(token) = human {
        req = req.header("x-pm-human-verification-token-type", "captcha").header("x-pm-human-verification-token", token);
    }
    let text = match body {
        Some(b) => {
            req = req.header("Content-Type", "application/json");
            b.to_string()
        }
        None => String::new(),
    };
    let req = req.body(text).map_err(|e| refused(e.to_string()))?;
    let mut resp = agent.run(req).map_err(|e| refused(e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().unwrap_or_default();
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(body);
    }
    Err(Refused { status, code: body["Code"].as_i64().unwrap_or(0), why: body["Error"].as_str().unwrap_or("no reason given").to_string(), details: body["Details"].clone() })
}

/// What a sign-in leaves to stay signed in with.
pub struct Granted {
    pub uid: String,
    pub access: String,
    pub refresh: String,
    /// opens the account's keys; derived from the password, not the
    /// password itself
    pub key_pass: String,
}

pub enum SignIn {
    Done(Granted),
    /// Proton wants a check in the browser first; then the sign-in is
    /// tried again with `token`.
    Human { token: String, url: String },
}

/// What the keyring holds for a Proton account.
pub fn pack(uid: &str, refresh: &str, key_pass: &str) -> String {
    json!({ "uid": uid, "refresh": refresh, "key_pass": key_pass }).to_string()
}

pub fn unpack(secret: &str) -> Option<(String, String, String)> {
    let v: Value = serde_json::from_str(secret).ok()?;
    Some((v["uid"].as_str()?.to_string(), v["refresh"].as_str()?.to_string(), v["key_pass"].as_str()?.to_string()))
}

/// Ends a session at Proton.
pub fn sign_out(base: &str, uid: &str, access: &str) {
    let _ = call(base, "DELETE", "/auth/v4", Some(As { uid, access }), None, None);
}

/// A fresh access token for a session, and the refresh token to keep
/// from now on (Proton hands out a new one each time).
pub fn refresh(base: &str, uid: &str, refresh: &str) -> Result<(String, String), SyncError> {
    let body = json!({ "UID": uid, "RefreshToken": refresh, "ResponseType": "token", "GrantType": "refresh_token", "RedirectURI": "https://protonmail.ch", "State": super::new_uid() });
    let v = call(base, "POST", "/auth/v4/refresh", None, Some(&body), None).map_err(|r| match r.status {
        // the session is over
        400 | 401 | 422 => SyncError::Auth,
        _ => r.error(),
    })?;
    match (v["AccessToken"].as_str(), v["RefreshToken"].as_str()) {
        (Some(a), Some(r)) => Ok((a.to_string(), r.to_string())),
        _ => Err(SyncError::Auth),
    }
}

/// Signs in with the password. `code` is the two-factor code and `second`
/// the second password, for accounts that have them. `human` is the token
/// of a check Proton asked for before.
pub fn sign_in(base: &str, username: &str, password: &str, code: &str, second: &str, human: Option<&str>) -> Result<SignIn, SyncError> {
    let other = |e: String| SyncError::Other(e);
    let info = call(base, "POST", "/auth/v4/info", None, Some(&json!({ "Username": username })), None).map_err(Refused::error)?;
    let text = |v: &Value, k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| SyncError::Other("Proton's answer to the sign-in was not understood.".into()));
    let modulus = signed_modulus(&text(&info, "Modulus")?).map_err(other)?;
    let salt = b64(&text(&info, "Salt")?).map_err(other)?;
    let server_ephemeral = b64(&text(&info, "ServerEphemeral")?).map_err(other)?;
    let hashed = proton_srp::hash_password(info["Version"].as_i64().unwrap_or(0), password.as_bytes(), &salt, &modulus).map_err(other)?;
    let proofs = proton_srp::proofs(&hashed, &modulus, &server_ephemeral, None).map_err(other)?;
    let body = json!({
        "Username": username,
        "ClientEphemeral": to_b64(&proofs.client_ephemeral),
        "ClientProof": to_b64(&proofs.client_proof),
        "SRPSession": text(&info, "SRPSession")?,
    });
    let auth = match call(base, "POST", "/auth/v4", None, Some(&body), human) {
        Ok(v) => v,
        Err(r) if r.code == 9001 => {
            let token = r.details["HumanVerificationToken"].as_str().unwrap_or("").to_string();
            if token.is_empty() {
                return Err(r.error());
            }
            let url = r.details["WebUrl"].as_str().filter(|u| u.starts_with("https://")).map(str::to_string).unwrap_or_else(|| format!("https://verify.proton.me/?methods=captcha&token={token}"));
            return Ok(SignIn::Human { token, url });
        }
        Err(r) => return Err(r.error()),
    };
    // the server must know the password too
    if b64(&text(&auth, "ServerProof")?).map_err(other)? != proofs.expected_server_proof {
        return Err(SyncError::Other("The server could not show that it is Proton. Nothing was signed in.".into()));
    }
    let (uid, access, refresh) = (text(&auth, "UID")?, text(&auth, "AccessToken")?, text(&auth, "RefreshToken")?);
    let who = As { uid: &uid, access: &access };
    // from here on a failure ends the half-open session again
    let fail = |e: SyncError| {
        sign_out(base, &uid, &access);
        e
    };
    let two_factor = auth["2FA"]["Enabled"].as_i64().unwrap_or(0);
    if two_factor & 1 != 0 {
        if code.trim().is_empty() {
            return Err(fail(SyncError::Other("This account asks for a two-factor code. Fill it in and sign in again.".into())));
        }
        call(base, "POST", "/auth/v4/2fa", Some(who), Some(&json!({ "TwoFactorCode": code.trim() })), None).map_err(|r| {
            fail(match r.status {
                401 | 422 => SyncError::Other("Proton did not take the two-factor code.".into()),
                _ => r.error(),
            })
        })?;
    } else if two_factor != 0 {
        return Err(fail(SyncError::Other("This account signs in with a security key only, which kalendar cannot do yet.".into())));
    }
    let mailbox = if auth["PasswordMode"].as_i64() == Some(2) {
        if second.is_empty() {
            return Err(fail(SyncError::Other("This account has a second password for its mailbox. Fill it in and sign in again.".into())));
        }
        second
    } else {
        password
    };
    let key_pass = (|| {
        let user = call(base, "GET", "/core/v4/users", Some(who), None, None).map_err(Refused::error)?;
        let keys = user["User"]["Keys"].as_array().cloned().unwrap_or_default();
        let primary = keys.iter().find(|k| k["Primary"].as_i64() == Some(1)).or(keys.first()).ok_or_else(|| SyncError::Other("This Proton account has no keys yet. Open Proton Calendar on the web once.".into()))?;
        let salts = call(base, "GET", "/core/v4/keys/salts", Some(who), None, None).map_err(Refused::error)?;
        let salt = salts["KeySalts"].as_array().and_then(|s| s.iter().find(|s| s["ID"] == primary["ID"])).and_then(|s| s["KeySalt"].as_str());
        let key_pass = match salt {
            Some(s) => proton_srp::key_passphrase(mailbox.as_bytes(), &b64(s).map_err(other)?).map_err(other)?,
            // very old keys are locked with the password as it is
            None => mailbox.to_string(),
        };
        Key::open(primary["PrivateKey"].as_str().unwrap_or(""), key_pass.as_bytes()).map_err(|_| SyncError::Other("The password signed in, but it does not open the account's keys.".into()))?;
        Ok(key_pass)
    })()
    .map_err(fail)?;
    Ok(SignIn::Done(Granted { uid, access, refresh, key_pass }))
}

/// One of the account's addresses with its opened keys.
struct Address {
    id: String,
    email: String,
    keys: Vec<Key>,
}

pub struct Proton {
    /// where Proton is reached; another address only in tests
    pub base: String,
    uid: String,
    access: String,
    refresh: String,
    key_pass: String,
    addresses: Option<Vec<Address>>,
    /// told the new access and refresh tokens whenever Proton hands out
    /// fresh ones, to keep them
    renewed: Option<Box<dyn Fn(&str, &str) + Send>>,
}

/// "#RRGGBB" as three numbers.
fn color_of(text: &str) -> Option<[u8; 3]> {
    let h = text.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let n = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([n(0)?, n(2)?, n(4)?])
}

/// The lines of the event inside one decrypted part, long lines joined,
/// without what is nested in it (reminders).
fn event_lines(part: &str) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for l in part.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&l[1..]);
        } else if !l.is_empty() {
            lines.push(l.to_string());
        }
    }
    let mut out = vec![];
    let (mut inside, mut depth) = (false, 0);
    for l in lines {
        let upper = l.to_uppercase();
        if !inside {
            inside = upper == "BEGIN:VEVENT";
        } else if upper == "END:VEVENT" && depth == 0 {
            break;
        } else if upper.starts_with("BEGIN:") {
            depth += 1;
        } else if upper.starts_with("END:") {
            depth -= 1;
        } else if depth == 0 {
            out.push(l);
        }
    }
    out
}

fn name_of(line: &str) -> String {
    line.split([';', ':']).next().unwrap_or("").to_uppercase()
}

/// One event out of its parts: a later part's fields replace the same
/// fields of an earlier one.
fn merge(parts: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for part in parts {
        let lines = event_lines(part);
        let names: HashSet<String> = lines.iter().map(|l| name_of(l)).collect();
        out.retain(|l| !names.contains(&name_of(l)));
        out.extend(lines);
    }
    out
}

impl Proton {
    /// `access` may be empty or old: a fresh one is then fetched with
    /// `refresh`, and `renewed` hears of both new tokens.
    pub fn new(uid: &str, access: &str, refresh: &str, key_pass: &str, renewed: Option<Box<dyn Fn(&str, &str) + Send>>) -> Proton {
        Proton { base: API.into(), uid: uid.to_string(), access: access.to_string(), refresh: refresh.to_string(), key_pass: key_pass.to_string(), addresses: None, renewed }
    }

    fn renew(&mut self) -> Result<(), SyncError> {
        let (access, newer) = refresh(&self.base, &self.uid, &self.refresh)?;
        if let Some(tell) = &self.renewed {
            tell(&access, &newer);
        }
        (self.access, self.refresh) = (access, newer);
        Ok(())
    }

    fn ask(&mut self, path: &str) -> Result<Value, SyncError> {
        if self.access.is_empty() {
            self.renew()?;
        }
        match call(&self.base, "GET", path, Some(As { uid: &self.uid, access: &self.access }), None, None) {
            // the access token ran out: one fresh one, one more try
            Err(r) if r.status == 401 => {
                self.renew()?;
                call(&self.base, "GET", path, Some(As { uid: &self.uid, access: &self.access }), None, None).map_err(Refused::error)
            }
            other => other.map_err(Refused::error),
        }
    }

    /// Opens the account's keys: the user keys with the kept passphrase,
    /// and each address key with a token only a user key can read.
    fn addresses(&mut self) -> Result<&[Address], SyncError> {
        if self.addresses.is_none() {
            let user = self.ask("/core/v4/users")?;
            let key_pass = self.key_pass.clone();
            let user_keys: Vec<Key> = user["User"]["Keys"].as_array().into_iter().flatten().filter_map(|k| Key::open(k["PrivateKey"].as_str()?, key_pass.as_bytes()).ok()).collect();
            if user_keys.is_empty() {
                // the password was changed elsewhere: the kept passphrase is old
                return Err(SyncError::Auth);
            }
            let list = self.ask("/core/v4/addresses")?;
            let mut out = vec![];
            for a in list["Addresses"].as_array().into_iter().flatten() {
                let mut keys = vec![];
                for k in a["Keys"].as_array().into_iter().flatten() {
                    let Some(private) = k["PrivateKey"].as_str() else { continue };
                    let opened = match (k["Token"].as_str(), k["Signature"].as_str()) {
                        (Some(token), Some(sig)) => any(&user_keys, |u| u.decrypt(token)).ok().filter(|t| user_keys.iter().any(|u| u.signed(sig, t))).and_then(|t| Key::open(private, &t).ok()),
                        _ => Key::open(private, self.key_pass.as_bytes()).ok(),
                    };
                    keys.extend(opened);
                }
                out.push(Address { id: a["ID"].as_str().unwrap_or("").to_string(), email: a["Email"].as_str().unwrap_or("").to_lowercase(), keys });
            }
            self.addresses = Some(out);
        }
        Ok(self.addresses.as_deref().unwrap_or(&[]))
    }

    /// The keys of one calendar, opened through the member this account is.
    fn calendar_keys(&mut self, id: &str) -> Result<Vec<Key>, SyncError> {
        let boot = self.ask(&format!("/calendar/v2/{id}/bootstrap"))?;
        let addresses = self.addresses()?;
        let mut passphrases: HashMap<String, Vec<u8>> = HashMap::new();
        for m in boot["Members"].as_array().into_iter().flatten() {
            let email = m["Email"].as_str().unwrap_or("").to_lowercase();
            let Some(addr) = addresses.iter().find(|a| Some(a.id.as_str()) == m["AddressID"].as_str() || (!email.is_empty() && a.email == email)) else { continue };
            for p in boot["Passphrase"]["MemberPassphrases"].as_array().into_iter().flatten() {
                if p["MemberID"] != m["ID"] {
                    continue;
                }
                // read-only use: who signed the passphrase is not checked
                // yet; it must be before anything is written with it
                if let Ok(plain) = any(&addr.keys, |k| k.decrypt(p["Passphrase"].as_str().unwrap_or(""))) {
                    passphrases.insert(boot["Passphrase"]["ID"].as_str().unwrap_or("").to_string(), plain);
                }
            }
        }
        let mut keys = vec![];
        for k in boot["Keys"].as_array().into_iter().flatten() {
            let pass = passphrases.get(k["PassphraseID"].as_str().unwrap_or("")).or_else(|| passphrases.values().next());
            if let (Some(private), Some(pass)) = (k["PrivateKey"].as_str(), pass) {
                keys.extend(Key::open(private, pass).ok());
            }
        }
        if keys.is_empty() {
            return Err(SyncError::Other("This calendar's key could not be opened with the account's keys.".into()));
        }
        Ok(keys)
    }

    /// Every event of a calendar as Proton holds it, page by page.
    fn all_events(&mut self, id: &str) -> Result<Vec<Value>, SyncError> {
        let mut out: Vec<Value> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        let mut from: Option<String> = None;
        for _ in 0..500 {
            let begin = from.as_ref().map(|f| format!("&BeginID={}", super::oauth::url_encode(f))).unwrap_or_default();
            let v = self.ask(&format!("/calendar/v1/{id}/events?PageSize={PAGE}{begin}"))?;
            let page = v["Events"].as_array().cloned().unwrap_or_default();
            let n = page.len();
            let mut fresh = 0;
            for e in page {
                let Some(eid) = e["ID"].as_str().map(str::to_string) else { continue };
                from = Some(eid.clone());
                if seen.insert(eid) {
                    fresh += 1;
                    out.push(e);
                }
            }
            if n < PAGE || fresh == 0 {
                break;
            }
        }
        Ok(out)
    }

    /// The readable text of one event's parts.
    fn parts_of(e: &Value, keys: &[Key], addresses: &[Address]) -> Vec<String> {
        let packet = |name: &str| e[name].as_str().and_then(|p| b64(p).ok());
        let shared = packet("SharedKeyPacket");
        let own = packet("CalendarKeyPacket");
        // an invitation's key may be wrapped for an address instead
        let invited = packet("AddressKeyPacket");
        let mut out = vec![];
        for (list, kp) in [("SharedEvents", &shared), ("CalendarEvents", &own)] {
            for part in e[list].as_array().into_iter().flatten() {
                let Some(data) = part["Data"].as_str() else { continue };
                if part["Type"].as_i64().unwrap_or(0) & 1 == 0 {
                    out.push(data.to_string());
                    continue;
                }
                let Ok(data) = b64(data) else { continue };
                let plain = match (kp, &invited) {
                    (Some(kp), _) => any(keys, |k| k.decrypt_split(kp, &data)),
                    (None, Some(kp)) if list == "SharedEvents" => any(addresses.iter().flat_map(|a| &a.keys), |k| k.decrypt_split(kp, &data)),
                    _ => continue,
                };
                if let Ok(text) = plain.map(|p| String::from_utf8_lossy(&p).to_string()) {
                    out.push(text);
                }
            }
        }
        out
    }
}

impl Provider for Proton {
    fn home(&self) -> Option<String> {
        Some(self.base.clone())
    }

    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError> {
        let v = self.ask("/calendar/v1")?;
        let mut out = vec![];
        for c in v["Calendars"].as_array().into_iter().flatten() {
            let Some(id) = c["ID"].as_str() else { continue };
            // name and colour belong to the member the account is
            let own = &c["Members"][0];
            let field = |k: &str| c[k].as_str().or(own[k].as_str());
            out.push(RemoteCalendar {
                id: id.to_string(),
                name: field("Name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Proton").to_string(),
                color: field("Color").and_then(color_of),
                ctag: None,
                // nothing is written to Proton yet
                read_only: true,
                events: true,
                tasks: false,
            });
        }
        Ok(out)
    }

    fn pull(&mut self, cal: &RemoteCalendar, state: Option<&str>, now: NaiveDateTime) -> Result<Delta, SyncError> {
        // a small question first: did anything change at all?
        let latest = self.ask(&format!("/calendar/v1/{}/modelevents/latest", cal.id)).ok().and_then(|v| v["CalendarModelEventID"].as_str().map(str::to_string));
        if let (Some((day, was)), Some(latest)) = (state.and_then(|s| s.split_once('|')), &latest) {
            let fresh = NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok_and(|d| (now.date() - d).num_days() < FULL_EVERY_DAYS);
            if fresh && was == latest {
                return Ok(Delta { state: state.map(str::to_string), ..Default::default() });
            }
        }
        let keys = self.calendar_keys(&cal.id)?;
        let events = self.all_events(&cal.id)?;
        let addresses = self.addresses()?;
        // all events go through the reader together, so that a changed day
        // of a repeating event replaces the day it stands for
        let mut text = String::from("BEGIN:VCALENDAR\r\n");
        let mut ids: HashMap<String, (String, String)> = HashMap::new();
        let mut unread = 0;
        for e in &events {
            let lines = merge(&Self::parts_of(e, &keys, addresses));
            // without its times an event cannot be shown
            if !lines.iter().any(|l| name_of(l) == "DTSTART") {
                unread += 1;
                continue;
            }
            let uid = lines.iter().find(|l| name_of(l) == "UID").and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
            let main = !lines.iter().any(|l| name_of(l) == "RECURRENCE-ID");
            if main || !ids.contains_key(&uid) {
                ids.insert(uid, (e["ID"].as_str().unwrap_or("").to_string(), e["ModifyTime"].as_i64().unwrap_or(0).to_string()));
            }
            text.push_str("BEGIN:VEVENT\r\n");
            for l in lines {
                text.push_str(&l);
                text.push_str("\r\n");
            }
            text.push_str("END:VEVENT\r\n");
        }
        text.push_str("END:VCALENDAR\r\n");
        if unread > 0 && unread == events.len() {
            return Err(SyncError::Other("None of this calendar's events could be decrypted.".into()));
        }
        let mut upserts = ics::parse(&text, now).map_err(SyncError::Other)?.events;
        for e in &mut upserts {
            if let Some((id, version)) = ids.get(&e.uid) {
                e.href = Some(id.clone());
                e.etag = Some(version.clone());
            }
        }
        let state = latest.map(|l| format!("{}|{l}", now.date().format("%Y-%m-%d")));
        Ok(Delta { upserts, full: true, state, ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::super::proton_keys::tests::{make_key, seal, seal_armored, sign};
    use super::*;
    use num_bigint::BigUint;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    const SIGNED_MODULUS: &str = "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\nW2z5HBi8RvsfYzZTS7qBaUxxPhsfHJFZpu3Kd6s1JafNrCCH9rfvPLrfuqocxWPgWDH2R8neK7PkNvjxto9TStuY5z7jAzWRvFWN9cQhAKkdWgy0JY6ywVn22+HFpF4cYesHrqFIKUPDMSSIlWjBVmEJZ/MusD44ZT29xcPrOqeZvwtCffKtGAIjLYPZIEbZKnDM1Dm3q2K/xS5h+xdhjnndhsrkwm9U9oyA2wxzSXFL+pdfj2fOdRwuR5nW0J2NFrq3kJjkRmpO/Genq1UW+TEknIWAb6VzJJJA244K/H8cnSx2+nSNZO3bbo6Ys228ruV9A8m6DhxmS+bihN3ttQ==\n-----BEGIN PGP SIGNATURE-----\nVersion: ProtonMail\nComment: https://protonmail.com\n\nwl4EARYIABAFAlwB1j0JEDUFhcTpUY8mAAD8CgEAnsFnF4cF0uSHKkXa1GIa\nGO86yMV4zDZEZcDSJo0fgr8A/AlupGN9EdHlsrZLmTA1vhIx+rOgxdEff28N\nkvNM7qIK\n=q6vu\n-----END PGP SIGNATURE-----";

    type Handler = Box<dyn Fn(&str, &str, &str, &Value) -> (u16, Value) + Send>;

    /// A stand-in for Proton: `handler` gets the method, the path, the
    /// headers and the body of each request. Every "METHOD path" is kept.
    fn stand_in(handler: Handler) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let got = Arc::new(Mutex::new(vec![]));
        let seen = got.clone();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { break };
                let mut all = vec![];
                let mut buf = [0u8; 8192];
                let (head, body) = loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    all.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&all).to_string();
                    let Some((head, body)) = text.split_once("\r\n\r\n") else { continue };
                    let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                    if body.len() >= len || n == 0 {
                        break (head.to_string(), body.to_string());
                    }
                };
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
                seen.lock().unwrap().push(format!("{method} {path}"));
                let (status, reply) = handler(method, path, &head.to_lowercase(), &serde_json::from_str(&body).unwrap_or(Value::Null));
                let reply = reply.to_string();
                let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            }
        });
        (base, got)
    }

    fn le(n: &BigUint) -> Vec<u8> {
        let mut b = n.to_bytes_le();
        b.resize(256, 0);
        b
    }

    /// The server's half of the password proof, for the password "hunter2".
    struct Srp {
        n: BigUint,
        v: BigUint,
        b: BigUint,
        big_b: Vec<u8>,
    }

    impl Srp {
        fn new(password: &str, salt: &[u8]) -> Srp {
            let modulus = signed_modulus(SIGNED_MODULUS).unwrap();
            let n = BigUint::from_bytes_le(&modulus);
            let two = BigUint::from(2u8);
            let x = BigUint::from_bytes_le(&proton_srp::hash_password(4, password.as_bytes(), salt, &modulus).unwrap());
            let v = two.modpow(&x, &n);
            let b = BigUint::from_bytes_le(&[0x3c; 180]);
            let mut gn = le(&two);
            gn.extend_from_slice(&le(&n));
            let k = BigUint::from_bytes_le(&proton_srp::expand_hash(&gn)) % &n;
            let big_b = le(&((&k * &v + two.modpow(&b, &n)) % &n));
            Srp { n, v, b, big_b }
        }

        /// The server proof when the client's proof holds, else None.
        fn check(&self, a: &[u8], m1: &[u8]) -> Option<Vec<u8>> {
            let mut ab = a.to_vec();
            ab.extend_from_slice(&self.big_b);
            let u = BigUint::from_bytes_le(&proton_srp::expand_hash(&ab));
            let shared = le(&((BigUint::from_bytes_le(a) * self.v.modpow(&u, &self.n)) % &self.n).modpow(&self.b, &self.n));
            let mut want = ab.clone();
            want.extend_from_slice(&shared);
            if proton_srp::expand_hash(&want) != m1 {
                return None;
            }
            let mut m2 = a.to_vec();
            m2.extend_from_slice(m1);
            m2.extend_from_slice(&shared);
            Some(proton_srp::expand_hash(&m2))
        }
    }

    /// A whole pretend account: its keys, one calendar and three events.
    fn account(two_factor: bool, human_first: bool) -> Handler {
        let srp_salt = [9u8; 10];
        let key_salt = [4u8; 16];
        let key_pass = proton_srp::key_passphrase(b"hunter2", &key_salt).unwrap();
        let user_key = make_key(&key_pass);
        let addr_key = make_key("address-token");
        let cal_key = make_key("calendar-passphrase");
        let srp = Srp::new("hunter2", &srp_salt);

        let part = |kind: i64, text: &str, key: Option<&(Vec<u8>, String)>| -> Value {
            let _ = key;
            json!({ "Type": kind, "Data": text, "Signature": null, "Author": "me@proton.me" })
        };
        // one event: times in the open, title sealed
        let event = |id: &str, open: &str, sealed: &str| -> Value {
            let (kp, data) = seal(&cal_key, format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\n{sealed}END:VEVENT\r\nEND:VCALENDAR").as_bytes());
            json!({
                "ID": id, "ModifyTime": 1700000000,
                "SharedKeyPacket": to_b64(&kp), "CalendarKeyPacket": null,
                "SharedEvents": [
                    part(2, &format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\n{open}END:VEVENT\r\nEND:VCALENDAR"), None),
                    part(3, &to_b64(&data), None),
                ],
                "CalendarEvents": [],
            })
        };
        let events = vec![
            event("ev1", "UID:one@proton.me\r\nDTSTAMP:20261101T000000Z\r\nDTSTART:20261116T150000Z\r\nDTEND:20261116T160000Z\r\n", "UID:one@proton.me\r\nDTSTAMP:20261101T000000Z\r\nSUMMARY:Dentist\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nEND:VALARM\r\nDESCRIPTION:Bring the\r\n  card\r\n"),
            event("ev2", "UID:two@proton.me\r\nDTSTAMP:20261101T000000Z\r\nDTSTART:20261109T090000Z\r\nDTEND:20261109T093000Z\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\n", "UID:two@proton.me\r\nDTSTAMP:20261101T000000Z\r\nSUMMARY:Standup\r\n"),
            // the second standup moved to the afternoon
            event("ev3", "UID:two@proton.me\r\nDTSTAMP:20261101T000000Z\r\nRECURRENCE-ID:20261116T090000Z\r\nDTSTART:20261116T140000Z\r\nDTEND:20261116T143000Z\r\n", "UID:two@proton.me\r\nDTSTAMP:20261101T000000Z\r\nSUMMARY:Standup, late\r\n"),
        ];

        let token = "address-token";
        let token_sealed = seal_armored(&user_key, token.as_bytes());
        let token_sig = sign(&user_key, &key_pass, token.as_bytes());
        let pass_sealed = seal_armored(&addr_key, b"calendar-passphrase");
        let asked_human = Mutex::new(false);

        Box::new(move |method, path, head, body| {
            let signed_in = head.contains("x-pm-uid: uid1") && head.contains("authorization: bearer access1");
            if !head.contains("x-pm-appversion: other") {
                return (400, json!({ "Code": 5003, "Error": "bad app version" }));
            }
            let path_only = path.split('?').next().unwrap_or("");
            match (method, path_only) {
                ("POST", "/auth/v4/info") => (200, json!({ "Code": 1000, "Version": 4, "Modulus": SIGNED_MODULUS, "ServerEphemeral": to_b64(&srp.big_b), "Salt": to_b64(&srp_salt), "SRPSession": "sess" })),
                ("POST", "/auth/v4") => {
                    if human_first && !head.contains("x-pm-human-verification-token: hv-token") {
                        *asked_human.lock().unwrap() = true;
                        return (422, json!({ "Code": 9001, "Error": "Human verification required", "Details": { "HumanVerificationToken": "hv-token", "HumanVerificationMethods": ["captcha"], "WebUrl": "https://verify.proton.me/?token=hv-token" } }));
                    }
                    let (a, m1) = (b64(body["ClientEphemeral"].as_str().unwrap()).unwrap(), b64(body["ClientProof"].as_str().unwrap()).unwrap());
                    match srp.check(&a, &m1) {
                        Some(m2) if body["SRPSession"] == "sess" => (200, json!({ "Code": 1000, "UID": "uid1", "AccessToken": "access1", "RefreshToken": "refresh1", "ServerProof": to_b64(&m2), "PasswordMode": 1, "2FA": { "Enabled": if two_factor { 1 } else { 0 } } })),
                        _ => (422, json!({ "Code": 8002, "Error": "Incorrect login credentials" })),
                    }
                }
                ("POST", "/auth/v4/2fa") if signed_in => match body["TwoFactorCode"].as_str() {
                    Some("123456") => (200, json!({ "Code": 1000 })),
                    _ => (422, json!({ "Code": 8002, "Error": "Incorrect code" })),
                },
                ("POST", "/auth/v4/refresh") => match (body["UID"].as_str(), body["RefreshToken"].as_str()) {
                    (Some("uid1"), Some("refresh1")) => (200, json!({ "Code": 1000, "AccessToken": "access1", "RefreshToken": "refresh2" })),
                    _ => (422, json!({ "Code": 10013, "Error": "Invalid refresh token" })),
                },
                ("DELETE", "/auth/v4") => (200, json!({ "Code": 1000 })),
                _ if !signed_in => (401, json!({ "Code": 401, "Error": "Invalid access token" })),
                ("GET", "/core/v4/users") => (200, json!({ "User": { "Keys": [{ "ID": "uk1", "PrivateKey": user_key, "Primary": 1, "Active": 1 }] } })),
                ("GET", "/core/v4/keys/salts") => (200, json!({ "KeySalts": [{ "ID": "uk1", "KeySalt": to_b64(&key_salt) }] })),
                ("GET", "/core/v4/addresses") => (200, json!({ "Addresses": [{ "ID": "addr1", "Email": "Me@proton.me", "Keys": [{ "ID": "ak1", "PrivateKey": addr_key, "Token": token_sealed, "Signature": token_sig, "Primary": 1, "Active": 1 }] }] })),
                ("GET", "/calendar/v1") => (200, json!({ "Calendars": [{ "ID": "cal1", "Type": 0, "Members": [{ "ID": "mem1", "Email": "me@proton.me", "Name": " Private ", "Color": "#8080FF" }] }] })),
                ("GET", "/calendar/v1/cal1/modelevents/latest") => (200, json!({ "CalendarModelEventID": "model7" })),
                ("GET", "/calendar/v2/cal1/bootstrap") => (
                    200,
                    json!({
                        "Keys": [{ "ID": "ck1", "PrivateKey": cal_key, "PassphraseID": "pp1", "Flags": 3 }],
                        "Passphrase": { "ID": "pp1", "MemberPassphrases": [{ "MemberID": "other", "Passphrase": "x", "Signature": "x" }, { "MemberID": "mem1", "Passphrase": pass_sealed, "Signature": "unchecked" }] },
                        "Members": [{ "ID": "mem1", "Email": "me@proton.me", "AddressID": "addr1" }],
                    }),
                ),
                ("GET", "/calendar/v1/cal1/events") => (200, json!({ "Events": if path.contains("BeginID=ev3") { vec![] } else { events.clone() } })),
                _ => (404, json!({ "Code": 2501, "Error": "not found" })),
            }
        })
    }

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 13).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    #[test]
    fn signs_in_and_reads_a_calendar() {
        let (base, got) = stand_in(account(false, false));
        // the wrong password never gets a session
        assert_eq!(sign_in(&base, "me", "wrong", "", "", None).err(), Some(SyncError::Auth));
        let SignIn::Done(g) = sign_in(&base, "me", "hunter2", "", "", None).unwrap() else { panic!("no check was asked for") };
        assert_eq!((g.uid.as_str(), g.access.as_str(), g.refresh.as_str()), ("uid1", "access1", "refresh1"));
        assert_eq!(g.key_pass.len(), 31);
        assert_ne!(g.key_pass, "hunter2");

        let (uid, kept, key_pass) = unpack(&pack(&g.uid, &g.refresh, &g.key_pass)).unwrap();
        let (access, newer) = refresh(&base, &uid, &kept).unwrap();
        assert_eq!(newer, "refresh2");
        assert_eq!(refresh(&base, &uid, "stale").err(), Some(SyncError::Auth));

        let mut p = Proton::new(&uid, &access, &newer, &key_pass, None);
        p.base = base.clone();
        let cals = p.list_calendars().unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!((cals[0].id.as_str(), cals[0].name.as_str(), cals[0].color, cals[0].read_only), ("cal1", "Private", Some([0x80, 0x80, 0xFF]), true));

        let d = p.pull(&cals[0], None, now()).unwrap();
        assert!(d.full);
        let mut titles: Vec<(String, u32)> = d.upserts.iter().map(|e| (e.title.clone(), chrono::Timelike::hour(&e.start.and_utc().with_timezone(&chrono::Local).naive_utc()))).collect();
        titles.sort();
        // the dentist, and three standups of which the second moved
        assert_eq!(titles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Dentist", "Standup", "Standup", "Standup, late"]);
        let dentist = d.upserts.iter().find(|e| e.title == "Dentist").unwrap();
        assert_eq!((dentist.href.as_deref(), dentist.etag.as_deref(), dentist.notes.as_str()), (Some("ev1"), Some("1700000000"), "Bring the card"));
        assert!(d.upserts.iter().filter(|e| e.title.starts_with("Standup")).all(|e| e.href.as_deref() == Some("ev2")));

        // nothing changed: the next pull asks one small question only
        let before = got.lock().unwrap().len();
        let again = p.pull(&cals[0], d.state.as_deref(), now()).unwrap();
        assert!(!again.full && again.upserts.is_empty() && again.state == d.state);
        assert_eq!(got.lock().unwrap().len(), before + 1);
        // a month on, everything is read again
        assert!(p.pull(&cals[0], d.state.as_deref(), now() + chrono::Duration::days(40)).unwrap().full);

        // an access token that ran out is renewed, and the new tokens are told
        let told = Arc::new(Mutex::new(vec![]));
        let t = told.clone();
        let mut stale = Proton::new(&uid, "old", "refresh1", &key_pass, Some(Box::new(move |a, r| t.lock().unwrap().push(format!("{a} {r}")))));
        stale.base = base.clone();
        assert_eq!(stale.list_calendars().unwrap().len(), 1);
        assert_eq!(*told.lock().unwrap(), ["access1 refresh2"]);
        // a session that is over, and keys the passphrase no longer opens
        let mut gone = Proton::new(&uid, "", "stale", &key_pass, None);
        gone.base = base.clone();
        assert_eq!(gone.list_calendars().err(), Some(SyncError::Auth));
        let mut changed = Proton::new(&uid, &access, &newer, "not-the-passphrase", None);
        changed.base = base;
        assert_eq!(changed.pull(&cals[0], None, now()).err(), Some(SyncError::Auth));
    }

    #[test]
    fn two_factor_and_the_human_check() {
        let (base, got) = stand_in(account(true, true));
        let SignIn::Human { token, url } = sign_in(&base, "me", "hunter2", "", "", None).unwrap() else { panic!("a check was due") };
        assert_eq!((token.as_str(), url.as_str()), ("hv-token", "https://verify.proton.me/?token=hv-token"));
        // without the code the half-open session is ended again
        assert!(matches!(sign_in(&base, "me", "hunter2", "", "", Some(&token)), Err(SyncError::Other(_))));
        assert_eq!(got.lock().unwrap().last().map(String::as_str), Some("DELETE /auth/v4"));
        assert!(matches!(sign_in(&base, "me", "hunter2", "000000", "", Some(&token)), Err(SyncError::Other(_))));
        assert!(matches!(sign_in(&base, "me", "hunter2", " 123456 ", "", Some(&token)), Ok(SignIn::Done(_))));
    }

    #[test]
    fn parts_merge_into_one_event() {
        let a = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nDTSTART:20261116T150000Z\nSUMMARY:old\nEND:VEVENT\nEND:VCALENDAR".to_string();
        let b = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nSUMMARY:a long\r\n  title\r\nBEGIN:VALARM\r\nTRIGGER:-PT5M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR".to_string();
        assert_eq!(merge(&[a, b]), ["DTSTART:20261116T150000Z", "UID:x", "SUMMARY:a long title"]);
        assert_eq!(color_of("#ff0080"), Some([255, 0, 128]));
        assert_eq!(color_of("red"), None);
    }
}
