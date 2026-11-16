//! Kalendar: a calendar and task planner.

mod ics;
mod icons;
mod ids;
mod model;
mod testaid;
mod theme;
pub use theme::{mix, rgb, Palette};
mod motion;
mod panel;
mod secrets;
mod settings;
mod sync;
mod tz;
mod dialogs;
mod fonts;
mod views;

mod app;
mod commands;
mod frame;
mod syncing;
pub use app::*;
pub use commands::*;
pub use syncing::*;
pub use theme::{fill_squircle, squircle_points, stroke_squircle};

use eframe::egui;
use std::path::PathBuf;
use sync::caldav::CalDav;
use sync::Provider;

fn main() {
    let mut shot = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => match args.next() {
                Some(p) => shot = Some(PathBuf::from(p)),
                None => {
                    eprintln!("--screenshot needs a file name");
                    std::process::exit(2);
                }
            },
            // test aid: fetch a calendar link and say what was read
            "--check-feed" => {
                let Some(link) = args.next() else {
                    eprintln!("--check-feed needs a link");
                    std::process::exit(2);
                };
                let res = ics::normalize_url(&link).and_then(|u| ics::fetch(&u, App::now()));
                match res {
                    Ok(f) => {
                        println!("{}: {} events", f.name.unwrap_or_else(|| "calendar".into()), f.events.len());
                        for e in f.events.iter().filter(|e| e.start >= App::now()).take(5) {
                            println!("  {}  {}{}", e.start.format("%Y-%m-%d %H:%M"), e.title, if e.all_day { "  (all day)" } else { "" });
                        }
                        return;
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
            }
            // test aid: sign in to a CalDAV server and say what it holds.
            // The password is read from KALENDAR_PASS, so it stays out of
            // the command line; nothing is kept.
            "--check-caldav" => {
                let (Some(server), Some(user)) = (args.next(), args.next()) else {
                    eprintln!("--check-caldav needs a server address and a user name; the password is read from KALENDAR_PASS");
                    std::process::exit(2);
                };
                let pass = std::env::var("KALENDAR_PASS").unwrap_or_default();
                let mut dav = CalDav::new(&server, &user, &pass, None);
                let cals = dav.list_calendars().unwrap_or_else(|e| {
                    eprintln!("{}", e.text());
                    std::process::exit(1);
                });
                println!("{} calendars at {}", cals.len(), dav.home.clone().unwrap_or_default());
                for c in &cals {
                    match dav.pull(c, None, App::now()) {
                        Ok(d) => {
                            let tokens = d.state.as_deref().is_some_and(|s| !s.ends_with('|'));
                            println!("  {}: {} events{}{}", c.name, d.upserts.len(), if c.read_only { ", read-only" } else { "" }, if tokens { "" } else { ", no sync tokens" });
                        }
                        Err(e) => println!("  {}: {}", c.name, e.text()),
                    }
                }
                return;
            }
            "--help" | "-h" => {
                println!("kalendar [--screenshot <file.png>]");
                return;
            }
            other => {
                eprintln!("unknown option: {other}");
                std::process::exit(1);
            }
        }
    }
    let mut vb = egui::ViewportBuilder::default()
        .with_title("Kalendar")
        .with_app_id("kalendar")
        .with_inner_size([1400.0, 860.0])
        .with_min_inner_size([360.0, 480.0]);
    if shot.is_some() {
        vb = vb.with_active(false);
        if let Some(size) = testaid::get().window {
            vb = vb.with_inner_size(size);
        }
        #[cfg(target_os = "windows")]
        {
            vb = vb.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let options = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let res = eframe::run_native(
        "Kalendar",
        options,
        Box::new(move |cc| {
            let _ = &cc;
            Ok(Box::new(App::new(shot)))
        }),
    );
    if let Err(e) = res {
        eprintln!("could not open the window: {e}");
        std::process::exit(127);
    }
}

