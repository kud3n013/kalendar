//! What differs between Linux, Windows and macOS: where files are kept and
//! how a link is opened in the browser.

use std::path::PathBuf;

/// The folder the app's data folder goes in, before the portable home is
/// considered: `$XDG_DATA_HOME`, else ~/.local/share on Linux, %APPDATA% on
/// Windows and ~/Library/Application Support on macOS.
pub fn data_base() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| native_base(".local/share"))
}

/// The same for settings: `$XDG_CONFIG_HOME`, else ~/.config on Linux; on
/// Windows and macOS settings sit beside the data.
pub fn config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| native_base(".config"))
}

#[cfg(windows)]
fn native_base(_unix: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn native_base(_unix: &str) -> Option<PathBuf> {
    home().map(|h| h.join("Library/Application Support"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn native_base(unix: &str) -> Option<PathBuf> {
    home().map(|h| h.join(unix))
}

#[cfg(not(windows))]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Opens a web page in the default browser, without waiting for it.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let mut cmd = {
        // not `cmd /c start`, which would read & in the link as its own
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    if let Err(e) = cmd.arg(url).spawn() {
        eprintln!("could not open {url}: {e}");
    }
}
