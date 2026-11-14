//! Signing in through the browser, as RFC 8252 describes for programs on
//! a person's own computer: the sign-in page is opened in the browser, and
//! its answer comes back to a port on this computer that only listens for
//! that one answer. A fresh secret per sign-in (PKCE) ties the answer to
//! this program.

use super::SyncError;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A service to sign in to, and this program's registration with it.
#[derive(Clone, Debug)]
pub struct OAuthApp {
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    /// some services give "installed" programs a secret that is not
    /// secret, and still want it sent
    pub client_secret: Option<String>,
    pub scope: String,
    /// more parameters for the sign-in page
    pub extra: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    /// opens the service for a short while; kept in memory only
    pub access: String,
    /// gets new access tokens; goes into the keyring
    pub refresh: Option<String>,
    pub expires_in: u64,
}

fn base64url(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

/// Bytes nobody can guess, from the system.
fn random(n: usize) -> Result<Vec<u8>, SyncError> {
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).map_err(|e| SyncError::Other(format!("No random numbers could be had ({e}).")))?;
    Ok(buf)
}

/// The challenge sent with the sign-in for a verifier kept here.
pub fn challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

pub fn url_encode(v: &str) -> String {
    let mut out = String::new();
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(v: &str) -> String {
    let bytes = v.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < bytes.len() {
        match bytes[i] {
            b'%' if bytes.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()).is_some() => {
                out.push(u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("0"), 16).unwrap_or(b'?'));
                i += 3;
                continue;
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The parameters in the first line of a request, "GET /?a=b&c=d HTTP/1.1".
fn query_of(request: &str) -> Vec<(String, String)> {
    let path = request.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("");
    let query = path.split_once('?').map(|x| x.1).unwrap_or("");
    query.split('&').filter_map(|p| p.split_once('=')).map(|(k, v)| (url_decode(k), url_decode(v))).collect()
}

/// Tokens only travel encrypted, or to this computer itself.
fn safe(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")
}

fn token_request(app: &OAuthApp, form: &[(&str, &str)]) -> Result<Tokens, SyncError> {
    if !safe(&app.token_url) {
        return Err(SyncError::Other("The sign-in service's address must start with https://.".into()));
    }
    let mut form: Vec<(&str, &str)> = form.to_vec();
    form.push(("client_id", &app.client_id));
    if let Some(s) = &app.client_secret {
        form.push(("client_secret", s));
    }
    let mut resp = ureq::post(&app.token_url)
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .header("User-Agent", "kalendar")
        .header("Accept", "application/json")
        .send_form(form)
        .map_err(|e| SyncError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if status != 200 {
        // the service's own words say most
        let why = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no reason given");
        return Err(match v["error"].as_str() {
            // the sign-in has run out or was taken back
            Some("invalid_grant") => SyncError::Auth,
            _ => SyncError::Other(format!("The sign-in service refused ({why}).")),
        });
    }
    let access = v["access_token"].as_str().ok_or_else(|| SyncError::Other("The sign-in service's answer could not be read.".into()))?;
    Ok(Tokens { access: access.to_string(), refresh: v["refresh_token"].as_str().map(str::to_string), expires_in: v["expires_in"].as_u64().unwrap_or(3600) })
}

/// A new access token from the refresh token kept in the keyring.
/// `SyncError::Auth` means the person has to sign in again.
pub fn refresh(app: &OAuthApp, refresh_token: &str) -> Result<Tokens, SyncError> {
    token_request(app, &[("grant_type", "refresh_token"), ("refresh_token", refresh_token)])
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>kalendar</title><body style=\"font-family:sans-serif;margin:3em\"><h2>You are signed in.</h2><p>You can close this page and go back to kalendar.</p>";
const FAILED_PAGE: &str = "<!doctype html><meta charset=utf-8><title>kalendar</title><body style=\"font-family:sans-serif;margin:3em\"><h2>Sign-in did not go through.</h2><p>Go back to kalendar to see why.</p>";

fn answer(stream: &mut std::net::TcpStream, status: &str, page: &str) {
    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
}

/// Runs a whole sign-in. `opened` is handed the page to show the person
/// (it opens the browser); the call then waits for the browser's answer,
/// for at most `wait_s` seconds or until `stop` is set.
pub fn authorize(app: &OAuthApp, opened: &dyn Fn(&str), stop: &AtomicBool, wait_s: u64) -> Result<Tokens, SyncError> {
    // only this computer can reach the port, and it is open only for now
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| SyncError::Other(format!("No port could be opened for the sign-in ({e}).")))?;
    let port = listener.local_addr().map_err(|e| SyncError::Other(e.to_string()))?.port();
    listener.set_nonblocking(true).map_err(|e| SyncError::Other(e.to_string()))?;
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = base64url(&random(48)?);
    let state = base64url(&random(24)?);
    let mut params: Vec<(String, String)> = vec![
        ("client_id".into(), app.client_id.clone()),
        ("redirect_uri".into(), redirect.clone()),
        ("response_type".into(), "code".into()),
        ("scope".into(), app.scope.clone()),
        ("code_challenge".into(), challenge(&verifier)),
        ("code_challenge_method".into(), "S256".into()),
        ("state".into(), state.clone()),
    ];
    params.extend(app.extra.iter().cloned());
    let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", url_encode(v))).collect();
    opened(&format!("{}?{}", app.auth_url, query.join("&")));

    let began = Instant::now();
    let code = loop {
        if stop.load(Ordering::Relaxed) {
            return Err(SyncError::Other("Sign-in was called off.".into()));
        }
        if began.elapsed().as_secs() > wait_s {
            return Err(SyncError::Other("Nobody signed in in time. Try again.".into()));
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let q = query_of(&String::from_utf8_lossy(&buf[..n]));
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        // an answer that does not carry this sign-in's own mark is not ours
        if get("state").as_deref() != Some(state.as_str()) {
            answer(&mut stream, "404 Not Found", "");
            continue;
        }
        if let Some(e) = get("error") {
            answer(&mut stream, "200 OK", FAILED_PAGE);
            return Err(SyncError::Other(if e == "access_denied" { "Sign-in was declined in the browser.".into() } else { format!("The sign-in page reported: {e}.") }));
        }
        match get("code") {
            Some(c) => {
                answer(&mut stream, "200 OK", DONE_PAGE);
                break c;
            }
            None => answer(&mut stream, "404 Not Found", ""),
        }
    };
    drop(listener);
    token_request(app, &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect), ("code_verifier", &verifier)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn pieces() {
        // the example of RFC 7636, appendix B
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(url_encode("a b/c@d.e~f"), "a%20b%2Fc%40d.e~f");
        assert_eq!(url_decode("a%20b%2Fc+d%zz"), "a b/c d%zz");
        assert_eq!(query_of("GET /?code=4%2Fabc&state=x HTTP/1.1\r\nHost: x\r\n"), vec![("code".to_string(), "4/abc".to_string()), ("state".to_string(), "x".to_string())]);
        assert!(safe("https://x/") && safe("http://127.0.0.1:9/") && !safe("http://example.org/"));
        assert_ne!(random(16).unwrap(), random(16).unwrap());
    }

    fn http_get(url: &str) -> String {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
        let mut s = std::net::TcpStream::connect(host).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    /// A stand-in for the sign-in service's token address: it answers one
    /// request and hands back what it was sent.
    fn token_server(reply: &'static str, status: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/token", listener.local_addr().unwrap().port());
        let got = Arc::new(Mutex::new(String::new()));
        let seen = got.clone();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut all = vec![];
            let mut buf = [0u8; 4096];
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                all.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&all).to_string();
                let Some((head, body)) = text.split_once("\r\n\r\n") else { continue };
                let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                if body.len() >= len || n == 0 {
                    *seen.lock().unwrap() = body.to_string();
                    break;
                }
            }
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
        });
        (url, got)
    }

    fn app(token_url: String) -> OAuthApp {
        OAuthApp { auth_url: "https://signin.example/auth".into(), token_url, client_id: "client-1".into(), client_secret: Some("not-secret".into()), scope: "cal read".into(), extra: vec![("access_type".into(), "offline".into())] }
    }

    #[test]
    fn a_whole_sign_in() {
        let (token_url, got) = token_server(r#"{"access_token":"at","refresh_token":"rt","expires_in":3599}"#, "200 OK");
        let seen_challenge = Arc::new(Mutex::new(String::new()));
        let sc = seen_challenge.clone();
        // the "browser": reads the page's address, then calls back as the service would
        let opened = move |url: &str| {
            assert!(url.starts_with("https://signin.example/auth?client_id=client-1&"));
            let q = query_of(&format!("GET /{} HTTP/1.1", &url[url.find('?').unwrap()..]));
            let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
            assert_eq!((get("scope").as_str(), get("code_challenge_method").as_str(), get("access_type").as_str()), ("cal read", "S256", "offline"));
            *sc.lock().unwrap() = get("code_challenge");
            let (redirect, state) = (get("redirect_uri"), get("state"));
            assert!(redirect.starts_with("http://127.0.0.1:"));
            std::thread::spawn(move || {
                // a stray request and one with a wrong mark are passed over
                assert!(http_get(&format!("{redirect}/favicon.ico")).starts_with("HTTP/1.1 404"));
                assert!(http_get(&format!("{redirect}/?code=stolen&state=wrong")).starts_with("HTTP/1.1 404"));
                assert!(http_get(&format!("{redirect}/?code=the%2Fcode&state={state}")).contains("You are signed in"));
            });
        };
        let t = authorize(&app(token_url), &opened, &AtomicBool::new(false), 20).expect("sign-in");
        assert_eq!(t, Tokens { access: "at".into(), refresh: Some("rt".into()), expires_in: 3599 });
        // the exchange carried the code and the verifier behind the challenge
        let form = got.lock().unwrap().clone();
        let q = query_of(&format!("GET /?{form} HTTP/1.1"));
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!((get("grant_type").as_str(), get("code").as_str(), get("client_id").as_str(), get("client_secret").as_str()), ("authorization_code", "the/code", "client-1", "not-secret"));
        assert_eq!(challenge(&get("code_verifier")), *seen_challenge.lock().unwrap());
    }

    #[test]
    fn declined_called_off_and_run_out() {
        let opened = |url: &str| {
            let q = query_of(&format!("GET /{} HTTP/1.1", &url[url.find('?').unwrap()..]));
            let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
            let (redirect, state) = (get("redirect_uri"), get("state"));
            std::thread::spawn(move || http_get(&format!("{redirect}/?error=access_denied&state={state}")));
        };
        let e = authorize(&app("https://unused.example/token".into()), &opened, &AtomicBool::new(false), 20).unwrap_err();
        assert_eq!(e, SyncError::Other("Sign-in was declined in the browser.".into()));
        let e = authorize(&app("https://unused.example/token".into()), &|_| {}, &AtomicBool::new(true), 20).unwrap_err();
        assert_eq!(e, SyncError::Other("Sign-in was called off.".into()));

        // a refresh token that has run out asks for a new sign-in
        let (token_url, got) = token_server(r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#, "400 Bad Request");
        assert_eq!(refresh(&app(token_url), "old").unwrap_err(), SyncError::Auth);
        assert!(got.lock().unwrap().contains("grant_type=refresh_token") && got.lock().unwrap().contains("refresh_token=old"));
        let (token_url, _) = token_server(r#"{"access_token":"new","expires_in":10}"#, "200 OK");
        assert_eq!(refresh(&app(token_url), "rt").unwrap(), Tokens { access: "new".into(), refresh: None, expires_in: 10 });
        assert!(refresh(&app("http://example.org/token".into()), "rt").is_err(), "tokens never go out unencrypted");
    }
}
