//! Asks GitHub whether a newer release of Katlendar is out. Only the list of
//! releases is fetched; nothing about the person or their data is sent.

use serde::Deserialize;

/// The repository releases are published from.
pub const REPO: &str = "kud3n013/katlendar";
/// The page every release is listed on.
pub const RELEASES_PAGE: &str = "https://github.com/kud3n013/katlendar/releases";

/// The version this program is.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A release newer than this program.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// as written in the tag, without its leading "v"
    pub version: String,
    /// the release's page, where its downloads are
    pub url: String,
}

/// Where a check for a newer version stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Check {
    #[default]
    NotYet,
    Running,
    UpToDate,
    Found(Release),
    Failed(String),
}

/// A release as GitHub's API lists it; only the fields needed.
#[derive(Deserialize)]
struct Listed {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest release that is newer than `current`, if there is one.
/// Blocks on the network, so it runs on a background thread.
pub fn newest(current: &str) -> Result<Option<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(20))).build().into();
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=30");
    let mut resp = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", &format!("katlendar/{}", current))
        .call()
        .map_err(|e| e.to_string())?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let list: Vec<Listed> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(pick(current, &list))
}

/// Reads a tag such as "v0.2.0-beta.1".
fn version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

/// The highest release above `current`. Someone running a beta is offered
/// betas too; someone on a stable version only stable ones.
fn pick(current: &str, list: &[Listed]) -> Option<Release> {
    let cur = version(current)?;
    let betas = !cur.pre.is_empty();
    list.iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Some((version(&r.tag_name)?, r)))
        .filter(|(v, r)| betas || (v.pre.is_empty() && !r.prerelease))
        .filter(|(v, _)| *v > cur)
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(v, r)| Release { version: v.to_string(), url: r.html_url.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, prerelease: bool) -> Listed {
        Listed { tag_name: tag.into(), html_url: format!("https://example.org/{tag}"), draft: false, prerelease }
    }

    #[test]
    fn newest_higher_release_is_offered() {
        let list = [rel("v0.1.0", false), rel("v0.3.0", false), rel("v0.2.0", false)];
        assert_eq!(pick("0.1.0", &list).map(|r| r.version), Some("0.3.0".into()));
        assert_eq!(pick("0.3.0", &list), None, "nothing newer");
    }

    #[test]
    fn betas_are_offered_only_to_beta_users() {
        let list = [rel("v0.1.0-beta.1", true), rel("v0.1.0-beta.2", true), rel("v0.2.0-beta.1", true)];
        assert_eq!(pick("0.1.0-beta.1", &list).map(|r| r.version), Some("0.2.0-beta.1".into()));
        assert_eq!(pick("0.1.0", &list), None, "a stable user is not moved to a beta");
        // the final release is newer than any of its betas
        let list = [rel("v0.1.0-beta.2", true), rel("v0.1.0", false)];
        assert_eq!(pick("0.1.0-beta.1", &list).map(|r| r.url), Some("https://example.org/v0.1.0".into()));
    }

    #[test]
    fn drafts_and_odd_tags_are_skipped() {
        let mut draft = rel("v9.0.0", false);
        draft.draft = true;
        let list = [draft, rel("nightly", true), rel("0.2.0", false)];
        assert_eq!(pick("0.1.0", &list).map(|r| r.version), Some("0.2.0".into()));
    }

    #[test]
    fn this_version_reads() {
        assert!(version(current()).is_some());
    }
}

// ---- in the app ----

use crate::strings::{tr, trf};
use crate::{icons, ids, mix, theme, App, SyncDone};
use eframe::egui::{self, Align2, Margin, RichText, Stroke};

/// How often a long-running app asks again.
const RECHECK_S: f64 = 12.0 * 3600.0;

impl App {
    /// Asks GitHub for a newer release in the background.
    pub fn check_for_update(&mut self, ctx: &egui::Context) {
        if self.update == Check::Running {
            return;
        }
        self.update = Check::Running;
        self.update_at = Some(ctx.input(|i| i.time));
        self.in_background(ctx, |send| send(SyncDone::Update { res: newest(current()) }));
    }

    /// On start and twice a day after, unless turned off or a picture is
    /// being taken.
    pub fn auto_update_check(&mut self, ctx: &egui::Context) {
        if self.shot.is_some() || !self.data.prefs.check_updates {
            return;
        }
        let now = ctx.input(|i| i.time);
        if self.update_at.is_none_or(|at| now - at > RECHECK_S) {
            self.check_for_update(ctx);
        }
    }

    pub fn take_update(&mut self, res: Result<Option<Release>, String>) {
        self.update = match res {
            Ok(Some(r)) => {
                // a version not seen before is shown again even if one was closed
                if !matches!(&self.update, Check::Found(old) if *old == r) {
                    self.update_hidden = false;
                }
                Check::Found(r)
            }
            Ok(None) => Check::UpToDate,
            Err(e) => Check::Failed(e),
        };
    }

    /// A card in the bottom corner when a newer version is out, with a link
    /// to its downloads.
    pub fn update_notice(&mut self, ctx: &egui::Context) {
        let Check::Found(r) = &self.update else { return };
        if self.update_hidden {
            return;
        }
        let r = r.clone();
        let fill = mix(self.pal.side, self.pal.text, 0.06);
        let (border, accent, muted) = (self.pal.border, self.pal.accent, self.pal.muted);
        let mut close = false;
        egui::Area::new(ids::update_notice())
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(theme::CARD_RADIUS)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icons::DOWNLOAD).size(theme::size::LARGE).color(accent));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(trf("Katlendar {} is available", &[&r.version])).font(theme::bold(theme::size::BODY)));
                                ui.label(RichText::new(trf("You have {}.", &[&current()])).size(theme::size::CAPTION).color(muted));
                            });
                            ui.add_space(6.0);
                            if ui.button(tr("Download")).on_hover_text(&r.url).clicked() {
                                crate::platform::open_url(&r.url);
                            }
                            if ui.button(RichText::new(icons::X).size(theme::size::BODY)).on_hover_text(tr("Not now")).clicked() {
                                close = true;
                            }
                        });
                    });
            });
        if close {
            self.update_hidden = true;
        }
    }

    /// The updates part of Settings > Info: the switch, the last answer and
    /// a button to ask now.
    pub fn update_status(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        let (text, link) = match &self.update {
            Check::NotYet => (tr("Not checked yet.").to_string(), None),
            Check::Running => (tr("Checking…").to_string(), None),
            Check::UpToDate => (tr("You have the newest version.").to_string(), None),
            Check::Found(r) => (trf("Version {} is available.", &[&r.version]), Some(r.url.clone())),
            Check::Failed(e) => (trf("Could not check for updates ({}).", &[e]), None),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(text).size(theme::size::UI).color(muted));
            if let Some(url) = link {
                if ui.button(tr("Download")).clicked() {
                    crate::platform::open_url(&url);
                }
            }
            let running = self.update == Check::Running;
            if ui.add_enabled(!running, egui::Button::new(tr("Check now"))).clicked() {
                self.check_for_update(ui.ctx());
            }
            if ui.button(tr("All releases")).clicked() {
                crate::platform::open_url(RELEASES_PAGE);
            }
        });
    }
}
