//! Signing in to Nextcloud through the browser ("Login Flow v2"): the
//! person signs in on the server's own page, and the server hands this
//! program an app password of its own. The main password is never seen.

use super::SyncError;
use serde_json::Value;

/// A sign-in that has been started and waits for the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    /// the page to open in the browser
    pub login: String,
    endpoint: String,
    token: String,
}

/// What the server hands over once the person has signed in.
#[derive(Clone, Debug, PartialEq)]
pub struct Granted {
    pub server: String,
    pub username: String,
    pub app_password: String,
}

impl Granted {
    /// Where the server's calendars are reached.
    pub fn caldav(&self) -> String {
        format!("{}/remote.php/dav", self.server.trim_end_matches('/'))
    }
}

fn base(server: &str) -> Result<String, SyncError> {
    let s = server.trim().trim_end_matches('/');
    let s = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    // the app password comes back over this connection
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| s.starts_with(p));
    if !s.starts_with("https://") && !local {
        return Err(SyncError::Other("The server address must start with https://, so the password stays private.".into()));
    }
    Ok(s.trim_end_matches("/remote.php/dav").to_string())
}

fn post(url: &str, form: &[(&str, &str)]) -> Result<(u16, String), SyncError> {
    let mut resp = ureq::post(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .header("User-Agent", "kalendar")
        .header("OCS-APIRequest", "true")
        .send_form(form.iter().copied())
        .map_err(|e| SyncError::Network(e.to_string()))?;
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    Ok((resp.status().as_u16(), body))
}

fn parse_start(body: &str) -> Option<Pending> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(Pending {
        login: v["login"].as_str()?.to_string(),
        endpoint: v["poll"]["endpoint"].as_str()?.to_string(),
        token: v["poll"]["token"].as_str()?.to_string(),
    })
}

fn parse_granted(body: &str) -> Option<Granted> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(Granted {
        server: v["server"].as_str()?.to_string(),
        username: v["loginName"].as_str()?.to_string(),
        app_password: v["appPassword"].as_str()?.to_string(),
    })
}

/// Asks the server to start a sign-in.
pub fn start(server: &str) -> Result<Pending, SyncError> {
    let base = base(server)?;
    let (status, body) = post(&format!("{base}/index.php/login/v2"), &[])?;
    let pending = parse_start(&body).filter(|_| status == 200).ok_or_else(|| SyncError::Other("This does not look like a Nextcloud server.".into()))?;
    // the page to open and the place to ask must be on the server typed
    let host = |u: &str| u.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_lowercase();
    if host(&pending.endpoint) != host(&base) || !(pending.login.starts_with("https://") || pending.login.starts_with("http://")) {
        return Err(SyncError::Other("The server sent a sign-in address that is not its own.".into()));
    }
    Ok(pending)
}

/// Asks once whether the person has signed in yet. `Ok(None)`: not yet.
pub fn poll(p: &Pending) -> Result<Option<Granted>, SyncError> {
    let (status, body) = post(&p.endpoint, &[("token", &p.token)])?;
    match status {
        200 => parse_granted(&body).map(Some).ok_or_else(|| SyncError::Other("The server's answer could not be read.".into())),
        404 => Ok(None),
        code => Err(SyncError::Other(format!("The server answered with error {code}."))),
    }
}

/// Tells the server to forget the app password, on signing out.
pub fn revoke(server: &str, username: &str, app_password: &str) {
    let Ok(base) = base(server) else { return };
    let dav = super::caldav::CalDav::new(&base, username, app_password, None);
    dav.revoke_app_password(&format!("{base}/ocs/v2.php/core/apppassword"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a server that speaks the sign-in flow. Set
    /// KALENDAR_TEST_NEXTCLOUD to its address; without it nothing runs.
    /// The server must grant the sign-in on the second ask.
    #[test]
    fn against_a_server() {
        let Ok(server) = std::env::var("KALENDAR_TEST_NEXTCLOUD") else { return };
        let p = start(&server).expect("start");
        assert!(p.login.contains("/login/v2/flow/"));
        assert_eq!(poll(&p).expect("first ask"), None, "not signed in yet");
        let g = poll(&p).expect("second ask").expect("granted");
        assert_eq!((g.username.as_str(), g.app_password.as_str()), ("sam", "app-pass"));
        assert!(start(&format!("{server}/nothing-here")).is_err());
    }

    #[test]
    fn reads_the_servers_answers() {
        // the shapes given in Nextcloud's developer manual for Login Flow v2
        let p = parse_start(r#"{"poll":{"token":"abc","endpoint":"https://cloud.example.com/login/v2/poll"},"login":"https://cloud.example.com/login/v2/flow/xyz"}"#).unwrap();
        assert_eq!((p.login.as_str(), p.endpoint.as_str(), p.token.as_str()), ("https://cloud.example.com/login/v2/flow/xyz", "https://cloud.example.com/login/v2/poll", "abc"));
        let g = parse_granted(r#"{"server":"https://cloud.example.com","loginName":"sam","appPassword":"x-y-z"}"#).unwrap();
        assert_eq!(g.caldav(), "https://cloud.example.com/remote.php/dav");
        assert_eq!(g.username, "sam");
        assert!(parse_start("<html>").is_none() && parse_granted("{}").is_none());
        assert_eq!(base("cloud.example.com/remote.php/dav/").unwrap(), "https://cloud.example.com");
        assert!(base("http://cloud.example.com").is_err());
        assert!(base("http://localhost:8080").is_ok());
    }
}
