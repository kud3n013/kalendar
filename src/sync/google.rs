//! Google Calendar, reached over Google's CalDAV address with a sign-in
//! through the browser. kalendar ships no registration with Google: the
//! person brings the client ID and secret of their own Google Cloud
//! project.

use super::oauth::{url_encode, OAuthApp};
use serde_json::{json, Value};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// The registration to sign in with. `email` fills in the account on
/// Google's page.
pub fn app(client_id: &str, client_secret: &str, email: &str) -> OAuthApp {
    OAuthApp {
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: TOKEN_URL.into(),
        client_id: client_id.trim().to_string(),
        client_secret: Some(client_secret.trim().to_string()).filter(|s| !s.is_empty()),
        // Google's CalDAV address takes nothing narrower
        scope: "https://www.googleapis.com/auth/calendar".into(),
        // "offline" and "consent" make Google hand out a refresh token
        extra: vec![("access_type".into(), "offline".into()), ("prompt".into(), "consent".into()), ("login_hint".into(), email.trim().to_string())],
        localhost_redirect: false,
    }
}

/// The client ID and secret out of the JSON file the Google Cloud console
/// offers for download, when that is pasted whole.
pub fn from_client_json(text: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let c = v.get("installed").or_else(|| v.get("web")).unwrap_or(&v);
    let id = c["client_id"].as_str()?.trim().to_string();
    Some((id, c["client_secret"].as_str().unwrap_or("").trim().to_string()))
}

/// Where a Google account's calendars are asked for.
pub fn server(email: &str) -> String {
    format!("https://apidata.googleusercontent.com/caldav/v2/{}/user", url_encode(email.trim()))
}

/// What the keyring holds for a Google account, as one text.
pub fn pack(refresh_token: &str, client_secret: &str) -> String {
    json!({ "refresh_token": refresh_token, "client_secret": client_secret }).to_string()
}

/// The refresh token and client secret out of the keyring's text.
pub fn unpack(secret: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(secret).ok()?;
    Some((v["refresh_token"].as_str()?.to_string(), v["client_secret"].as_str().unwrap_or("").to_string()))
}

/// Tells Google to forget the sign-in, on signing out.
pub fn revoke(refresh_token: &str) {
    let _ = ureq::post("https://oauth2.googleapis.com/revoke")
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .send_form([("token", refresh_token)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_downloaded_client_file_fills_both_fields() {
        let file = r#"{"installed":{"client_id":"1-abc.apps.googleusercontent.com","project_id":"k","client_secret":"not-secret","redirect_uris":["http://localhost"]}}"#;
        assert_eq!(from_client_json(file), Some(("1-abc.apps.googleusercontent.com".into(), "not-secret".into())));
        assert_eq!(from_client_json("1-abc.apps.googleusercontent.com"), None);
    }

    #[test]
    fn addresses_and_keyring_text() {
        assert_eq!(server(" sam@example.com "), "https://apidata.googleusercontent.com/caldav/v2/sam%40example.com/user");
        let a = app(" id.apps.googleusercontent.com ", "", "sam@example.com");
        assert_eq!((a.client_id.as_str(), a.client_secret), ("id.apps.googleusercontent.com", None));
        assert!(a.extra.contains(&("access_type".to_string(), "offline".to_string())));
        assert_eq!(unpack(&pack("r\"t", "s")), Some(("r\"t".to_string(), "s".to_string())));
        assert_eq!(unpack("a plain app password"), None);
    }
}
