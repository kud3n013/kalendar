//! Test aids: environment variables that set the app up for a picture
//! (`--screenshot`) or a manual check. They are read once, at the first
//! call to [`get`], so the list below is every one there is.
//!
//! Not here, because they are not test aids: `KATLENDAR_PASS` (the password
//! for `--check-caldav`), `KATLENDAR_PROTON_APPVERSION` (a user override) and
//! the `KATLENDAR_TEST_*` server addresses of the integration tests.
//!
//! Marked *(picture)* below: only honoured when a screenshot is being taken.

use std::sync::OnceLock;

use chrono::NaiveDateTime;
use eframe::egui;

/// What a new block window is opened as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Create {
    Block,
    /// as after choosing Frame in that window
    Frame,
}

#[derive(Debug, Default)]
pub struct Aids {
    /// `KATLENDAR_NOW=2026-11-11T10:30`: the current time
    pub now: Option<NaiveDateTime>,
    /// `KATLENDAR_WINDOW=760x480`: the window size *(picture)*
    pub window: Option<egui::Vec2>,
    /// `KATLENDAR_THEME=light|dark` *(picture)*
    pub theme: Option<String>,
    /// `KATLENDAR_FONT=<family>`: a font other than the system's
    pub font: Option<String>,
    /// `KATLENDAR_SLOT=<minutes>`: the grid slot length *(picture)*
    pub slot: Option<i64>,
    /// `KATLENDAR_PANEL_RIGHT`: the task panel on the right
    pub panel_right: bool,
    /// `KATLENDAR_NARROW`: as if the window were narrow *(picture)*
    pub narrow: bool,
    /// `KATLENDAR_DAYS=3`: show that many days *(picture)*
    pub days: Option<u8>,
    /// `KATLENDAR_NAV=<n>`: turn that many pages just before the picture *(picture)*
    pub nav: Option<i64>,
    /// `KATLENDAR_NAV_DAYS=<n>`: move that many single days, as the day keys do *(picture)*
    pub nav_days: Option<i64>,
    /// `KATLENDAR_DEMO_ACCOUNT`: a signed-in account with calendars and lists *(picture)*
    pub demo_account: bool,
    /// `KATLENDAR_GREY`: scheduled tasks are done and every frame skips tomorrow
    pub grey: bool,
    /// `KATLENDAR_SHOW_MENU=<name>`: a right-click menu, shown in a window
    pub show_menu: Option<String>,
    /// `KATLENDAR_SHOW_SETTINGS=<page>`: the settings fill the window
    pub show_settings: Option<String>,
    /// `KATLENDAR_SETTINGS_QUERY=<text>`: typed into the settings search
    pub settings_query: Option<String>,
    /// `KATLENDAR_SHORTCUT_QUERY=<text>`: typed into the shortcut search
    pub shortcut_query: Option<String>,
    /// `KATLENDAR_SUB_KIND=<n>`: which kind of integration is being set up
    pub sub_kind: Option<u8>,
    /// `KATLENDAR_SUB_URL=<url>`: a subscription address being typed
    pub sub_url: Option<String>,
    /// `KATLENDAR_SHOW_CREATE[=frame]`: the new block window is open
    pub show_create: Option<Create>,
    /// `KATLENDAR_CREATE_AT=15`: its block starts at that hour (default ten)
    pub create_at: u32,
    /// `KATLENDAR_SHOW_FRAME`: the first frame's editor is open *(picture)*
    pub show_frame: bool,
    /// `KATLENDAR_SHOW_EDIT=<n>`: the editor of the nth block is open *(picture)*
    pub show_edit: Option<usize>,
    /// `KATLENDAR_SHOW_NEWTASK=<list>`: a new task opened as that list's + does
    pub show_newtask: Option<String>,
    /// `KATLENDAR_LIST_EDIT=<list>`: that list has an icon and its editor is open
    pub list_edit: Option<String>,
    /// `KATLENDAR_NEW_LIST`: the new-list window open, as the + button opens it *(picture)*
    pub new_list: bool,
    /// `KATLENDAR_DRAG="title@x,y"`: that task held as if dragged, pointer at x,y *(picture)*
    pub drag: Option<(String, egui::Pos2)>,
    /// `KATLENDAR_MORPH=<0..1>`: the task panel held that far between the rail and open *(picture)*
    pub morph: Option<f32>,
    /// `KATLENDAR_UPDATE=<version>`: the notice that that version is out *(picture)*
    pub update: Option<String>,
}

/// The aids, read once.
pub fn get() -> &'static Aids {
    static AIDS: OnceLock<Aids> = OnceLock::new();
    AIDS.get_or_init(|| read(|k| std::env::var(k).ok()))
}

fn read(var: impl Fn(&str) -> Option<String>) -> Aids {
    let set = |k: &str| var(&format!("KATLENDAR_{k}"));
    let on = |k: &str| set(k).is_some();
    let num = |k: &str| set(k).and_then(|v| v.trim().parse::<i64>().ok());
    Aids {
        now: set("NOW").and_then(|v| NaiveDateTime::parse_from_str(&v, "%Y-%m-%dT%H:%M").ok()),
        window: set("WINDOW").and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))
        }),
        theme: set("THEME"),
        font: set("FONT"),
        slot: num("SLOT"),
        panel_right: on("PANEL_RIGHT"),
        narrow: on("NARROW"),
        days: set("DAYS").and_then(|v| v.parse().ok()),
        nav: num("NAV"),
        nav_days: num("NAV_DAYS"),
        demo_account: on("DEMO_ACCOUNT"),
        grey: on("GREY"),
        show_menu: set("SHOW_MENU"),
        show_settings: set("SHOW_SETTINGS"),
        settings_query: set("SETTINGS_QUERY"),
        shortcut_query: set("SHORTCUT_QUERY"),
        sub_kind: set("SUB_KIND").and_then(|v| v.parse().ok()),
        sub_url: set("SUB_URL"),
        show_create: set("SHOW_CREATE").map(|v| if v == "frame" { Create::Frame } else { Create::Block }),
        create_at: set("CREATE_AT").and_then(|h| h.parse::<u32>().ok()).filter(|h| *h < 23).unwrap_or(10),
        show_frame: on("SHOW_FRAME"),
        show_edit: set("SHOW_EDIT").and_then(|v| v.parse().ok()),
        show_newtask: set("SHOW_NEWTASK"),
        list_edit: set("LIST_EDIT"),
        new_list: on("NEW_LIST"),
        update: set("UPDATE"),
        morph: set("MORPH").and_then(|v| v.trim().parse::<f32>().ok()).map(|t| t.clamp(0.0, 1.0)),
        drag: set("DRAG").and_then(|v| {
            let (title, at) = v.rsplit_once('@')?;
            let (x, y) = at.split_once(',')?;
            Some((title.to_string(), egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?)))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(&str, &str)]) -> Aids {
        read(|k| pairs.iter().find(|p| p.0 == k).map(|p| p.1.to_string()))
    }

    #[test]
    fn nothing_set_means_nothing_changes() {
        let a = with(&[]);
        assert!(a.now.is_none() && a.show_create.is_none() && !a.grey);
        assert_eq!(a.create_at, 10);
    }

    #[test]
    fn values_are_parsed() {
        let a = with(&[
            ("KATLENDAR_WINDOW", "760x480"),
            ("KATLENDAR_DRAG", "Write a@12, 34"),
            ("KATLENDAR_SHOW_CREATE", "frame"),
            ("KATLENDAR_CREATE_AT", "23"),
            ("KATLENDAR_NOW", "2026-11-11T10:30"),
        ]);
        assert_eq!(a.window, Some(egui::vec2(760.0, 480.0)));
        assert_eq!(a.drag, Some(("Write a".into(), egui::pos2(12.0, 34.0))));
        assert_eq!(a.show_create, Some(Create::Frame));
        assert_eq!(a.create_at, 10, "an hour too late for a block falls back to ten");
        assert!(a.now.is_some());
    }
}
