//! Passwords and private links, kept in the desktop's keyring and never in
//! data.json or config.json.

use crate::strings::{later, tr, trf};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const SERVICE: &str = "katlendar";
/// The name secrets were kept under before the app was renamed.
const OLD_SERVICE: &str = "kalendar";

/// The name a synced account's password is kept under.
pub fn account_key(id: u64) -> String {
    format!("account:{id}")
}

/// The name a subscribed calendar's private link is kept under.
pub fn feed_key(id: u64) -> String {
    format!("feed:{id}")
}

const NO_KEYRING: &str = later("No keyring is running, so this cannot be kept safely. Start one (KWallet or GNOME Keyring), or allow a password file under Settings > Integrations.");

/// Connects to the Secret Service once.
fn keyring() -> Result<(), String> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    READY
        .get_or_init(|| {
            let store = zbus_secret_service_keyring_store::Store::new().map_err(|e| e.to_string())?;
            keyring_core::set_default_store(store);
            Ok(())
        })
        .clone()
}

fn entry(key: &str) -> Result<keyring_core::Entry, String> {
    keyring()?;
    keyring_core::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

/// The file used when the person allows passwords outside a keyring.
pub fn file_path() -> PathBuf {
    crate::model::data_path().with_file_name("secrets.json")
}

fn read_file(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Writes the file so that only its owner can read it.
fn write_file(path: &Path, all: &BTreeMap<String, String>) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if all.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
    // a file that was already there keeps its old mode unless told
    f.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn file_set(path: &Path, key: &str, secret: Option<&str>) -> Result<(), String> {
    let mut all = read_file(path);
    match secret {
        Some(s) => all.insert(key.to_string(), s.to_string()),
        None => all.remove(key),
    };
    write_file(path, &all)
}

/// Keeps a secret: in the keyring, or, when there is none and the person
/// allowed it, in the password file. May block while the keyring asks to
/// be unlocked, so it runs on a background thread.
pub fn set(key: &str, secret: &str, allow_file: bool) -> Result<(), String> {
    match entry(key).and_then(|e| e.set_password(secret).map_err(|e| e.to_string())) {
        Ok(()) => {
            // one place only
            let _ = file_set(&file_path(), key, None);
            Ok(())
        }
        Err(_) if allow_file => file_set(&file_path(), key, Some(secret)),
        Err(_) => Err(tr(NO_KEYRING).into()),
    }
}

pub fn get(key: &str, allow_file: bool) -> Result<String, String> {
    match entry(key).and_then(|e| e.get_password().map_err(|e| e.to_string())) {
        Ok(s) => Ok(s),
        Err(e) => {
            if let Some(s) = adopt_old(key) {
                return Ok(s);
            }
            if allow_file {
                if let Some(s) = read_file(&file_path()).remove(key) {
                    return Ok(s);
                }
            }
            Err(if keyring().is_err() { tr(NO_KEYRING).into() } else { trf("The keyring does not have it ({}).", &[&e]) })
        }
    }
}

/// A secret kept while the app was still called kalendar, moved under the
/// new name so the next lookup finds it there.
fn adopt_old(key: &str) -> Option<String> {
    let old = keyring_core::Entry::new(OLD_SERVICE, key).ok()?;
    let s = old.get_password().ok()?;
    if entry(key).and_then(|e| e.set_password(&s).map_err(|e| e.to_string())).is_ok() {
        let _ = old.delete_credential();
    }
    Some(s)
}

/// Forgets a secret everywhere.
pub fn delete(key: &str) {
    if let Ok(e) = entry(key) {
        let _ = e.delete_credential();
    }
    if let Ok(e) = keyring_core::Entry::new(OLD_SERVICE, key) {
        let _ = e.delete_credential();
    }
    let path = file_path();
    if path.exists() {
        let _ = file_set(&path, key, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes to the real keyring, so it only runs when asked for:
    /// `cargo test keyring_round_trip -- --ignored`.
    #[test]
    #[ignore]
    fn keyring_round_trip() {
        let key = format!("test:{}", std::process::id());
        set(&key, "kept", false).expect("set");
        assert_eq!(get(&key, false).as_deref(), Ok("kept"));
        delete(&key);
        assert!(get(&key, false).is_err());
    }

    #[test]
    fn password_file_is_private_and_goes_when_empty() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("katlendar-secrets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("secrets.json");
        file_set(&path, &account_key(7), Some("first")).unwrap();
        file_set(&path, &feed_key(9), Some("second")).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let all = read_file(&path);
        assert_eq!((all.get("account:7").map(String::as_str), all.get("feed:9").map(String::as_str)), (Some("first"), Some("second")));
        file_set(&path, &account_key(7), None).unwrap();
        assert_eq!(read_file(&path).len(), 1);
        file_set(&path, &feed_key(9), None).unwrap();
        assert!(!path.exists(), "an empty password file is removed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
