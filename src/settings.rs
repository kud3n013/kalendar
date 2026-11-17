//! The settings window: a window of its own with a list of pages on the left.

use crate::strings::{later, tr, trf};
use crate::ids;
use crate::motion;
use crate::theme;
use crate::icons;
use crate::model::*;
use crate::{bind_label, ics, mix, parse_bind, rgb, secrets, App, Cmd, KeyBind, COMMANDS};
use eframe::egui::Key;
use eframe::egui::{self, vec2, Align, Color32, FontId, Layout, Margin, RichText, Sense, Stroke};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    Shortcuts,
    Calendars,
    General,
    Appearance,
    ActiveCalendars,
    Lists,
    Planner,
    Frames,
    QuickAdd,
    Info,
}

const CONTROL_W: f32 = 390.0;
const ROW_H: f32 = 56.0;

/// The order pages are listed in search results.
const SEARCH_ORDER: [(Page, &str); 10] = [
    (Page::General, later("General")),
    (Page::Appearance, later("Appearance")),
    (Page::ActiveCalendars, later("Active calendars")),
    (Page::Lists, later("Task lists")),
    (Page::Planner, later("Planner")),
    (Page::Frames, later("Frames")),
    (Page::QuickAdd, later("Quick add")),
    (Page::Calendars, later("Calendars")),
    (Page::Info, later("Info")),
    (Page::Shortcuts, later("Shortcuts")),
];

/// A settings search in progress: the words looked for, the page heading
/// still to draw before that page's first match, how many matched, and the
/// page to open when a result heading is clicked.
#[derive(Clone, Default)]
struct Find {
    words: Vec<String>,
    head: Option<(Page, &'static str)>,
    hits: usize,
    jump: Option<Page>,
}

fn find_id() -> egui::Id {
    ids::settings_find()
}

fn finding(ui: &egui::Ui) -> bool {
    ui.data(|d| d.get_temp::<Find>(find_id()).is_some())
}

pub fn search_words(q: &str) -> Vec<String> {
    q.to_lowercase().split_whitespace().map(String::from).collect()
}

/// Whether every word appears somewhere in `texts`, ignoring case.
pub fn search_hit(words: &[String], texts: &[&str]) -> bool {
    let hay = texts.join(" ").to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// While searching: whether this item matches, drawing its page's heading
/// before the first match on that page. Outside a search everything shows.
fn shown(ui: &mut egui::Ui, texts: &[&str]) -> bool {
    let Some(mut f) = ui.data(|d| d.get_temp::<Find>(find_id())) else { return true };
    if !search_hit(&f.words, texts) {
        return false;
    }
    let head = f.head.take();
    let first = f.hits == 0;
    f.hits += 1;
    ui.data_mut(|d| d.insert_temp(find_id(), f));
    if let Some((page, name)) = head {
        if !first {
            ui.add_space(22.0);
        }
        let r = ui.add(egui::Label::new(RichText::new(name).font(theme::bold(20.0))).sense(Sense::click()));
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.on_hover_text(tr("Open this page")).clicked() {
            ui.data_mut(|d| d.get_temp_mut_or_default::<Find>(find_id()).jump = Some(page));
        }
        ui.add_space(4.0);
    }
    true
}

/// A search result that stands for a whole page: its title and what is
/// there, opening the page when clicked.
fn page_result(ui: &mut egui::Ui, page: Page, title: &str, desc: &str, extra: &[&str]) {
    let texts: Vec<&str> = [title, desc].into_iter().chain(extra.iter().copied()).collect();
    if !shown(ui, &texts) {
        return;
    }
    let (muted, text) = (ui.visuals().weak_text_color(), ui.visuals().text_color());
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(r, 8.0, mix(ui.visuals().panel_fill, text, 0.05));
    }
    let p = ui.painter();
    p.text(r.left_center() + vec2(0.0, -9.0), egui::Align2::LEFT_CENTER, title, FontId::proportional(theme::size::TITLE), text);
    p.text(r.left_center() + vec2(0.0, 12.0), egui::Align2::LEFT_CENTER, desc, FontId::proportional(theme::size::BODY), muted);
    p.text(r.right_center() - vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, icons::CHEVRON_RIGHT, FontId::proportional(theme::size::LARGE), muted);
    if resp.clicked() {
        ui.data_mut(|d| d.get_temp_mut_or_default::<Find>(find_id()).jump = Some(page));
    }
}

/// A search field: icon, hint, and Esc clears what was typed.
fn search_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) {
    // the dropdowns' fill, so the field shows on the sidebar in both themes
    let fill = ui.visuals().widgets.inactive.bg_fill;
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .background_color(fill)
            .hint_text(format!("{}   {hint}", icons::SEARCH))
            .font(FontId::proportional(theme::size::UI))
            .margin(Margin::symmetric(10, 8))
            .desired_width(width),
    );
    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
        // the first Esc empties the field; only the next one closes settings
        text.clear();
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
    }
}

/// One settings line: the name on the left, its control in a wide box on the right.
fn row(ui: &mut egui::Ui, label: &str, tip: &str, add: impl FnOnce(&mut egui::Ui)) {
    if !shown(ui, &[label, tip]) {
        return;
    }
    let avail = ui.available_width();
    let label_w = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(theme::size::TITLE), Color32::WHITE).size().x + if tip.is_empty() { 0.0 } else { 26.0 };
    let wide = (avail * 0.42).clamp(170.0, CONTROL_W);
    // too narrow for the name beside its control: the control goes below
    let stack = label_w + 24.0 + wide > avail;
    let cw = if stack { avail.min(CONTROL_W) } else { wide };
    ui.data_mut(|d| d.insert_temp(ids::control_w(), cw));
    let name = |ui: &mut egui::Ui| {
        ui.add(egui::Label::new(RichText::new(label).size(theme::size::TITLE)).truncate());
        if !tip.is_empty() {
            ui.label(RichText::new("(i)").size(theme::size::SMALL).weak()).on_hover_text(tip);
        }
    };
    let control = |ui: &mut egui::Ui| {
        ui.allocate_ui_with_layout(vec2(cw, 44.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().interact_size.y = 44.0;
            ui.spacing_mut().button_padding = vec2(14.0, 10.0);
            ui.style_mut().override_font_id = Some(FontId::proportional(theme::size::LARGE));
            add(ui);
        });
    };
    if stack {
        ui.add_space(6.0);
        ui.horizontal(name);
        ui.add_space(2.0);
        control(ui);
        ui.add_space(8.0);
        return;
    }
    ui.allocate_ui_with_layout(vec2(avail, ROW_H), Layout::left_to_right(Align::Center), |ui| {
        name(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

fn combo<T: PartialEq + Copy>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, String)]) {
    let current = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_default();
    let cw = ui.data(|d| d.get_temp::<f32>(ids::control_w())).unwrap_or(CONTROL_W);
    egui::ComboBox::from_id_salt(id).width(cw).selected_text(current).show_ui(ui, |ui| {
        for (v, l) in options {
            ui.selectable_value(value, *v, l);
        }
    });
}

fn divider(ui: &mut egui::Ui, color: Color32) {
    if finding(ui) {
        return;
    }
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, color);
    ui.add_space(8.0);
}

fn heading(ui: &mut egui::Ui, text: &str) {
    if finding(ui) {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(text).font(theme::bold(20.0)));
    ui.add_space(6.0);
}

/// An on/off switch: a pill with a knob that slides across.
fn switch(ui: &mut egui::Ui, on: &mut bool, accent: Color32) -> egui::Response {
    let (r, mut resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, motion::HOVER.secs());
    let off = ui.visuals().widgets.inactive.bg_fill;
    let track = if resp.hovered() { mix(off, ui.visuals().text_color(), 0.06) } else { off };
    ui.painter().rect_filled(r, 11.0, mix(track, accent, t));
    let x = egui::lerp(r.left() + 11.0..=r.right() - 11.0, t);
    ui.painter().circle_filled(egui::pos2(x, r.center().y), 8.0, Color32::WHITE);
    resp
}

/// "Weekdays", "Every day", or the short day names, from a Monday-first bit set.
fn days_label(days: u8) -> String {
    match days & 0x7f {
        0x7f => tr("Every day").into(),
        FRAME_WEEKDAYS => tr("Weekdays").into(),
        0b110_0000 => tr("Weekends").into(),
        d => [tr("Mo"), tr("Tu"), tr("We"), tr("Th"), tr("Fr"), tr("Sa"), tr("Su")].iter().enumerate().filter(|(i, _)| d & (1 << i) != 0).map(|(_, n)| *n).collect::<Vec<_>>().join(" "),
    }
}

fn hour_label(p: &Prefs, h: i64) -> String {
    let t = chrono::NaiveTime::from_hms_opt((h % 24) as u32, 0, 0).unwrap();
    let base = if p.hour12 {
        let (pm, h12) = chrono::Timelike::hour12(&t);
        trf("{}:00 {}", &[&h12, &if pm { "PM" } else { "AM" }])
    } else {
        format!("{:02}:00", h % 24)
    };
    if h >= 24 {
        trf("{} (+1 day)", &[&base])
    } else {
        base
    }
}

impl App {
    pub fn open_settings(&mut self, page: Page) {
        self.settings_page = page;
        self.show_prefs = true;
    }

    /// Shows the settings in a window of their own while they are open.
    pub fn ui_settings_window(&mut self, ctx: &egui::Context) {
        // shortcuts can also be changed from the search results
        if !self.show_prefs || (self.settings_page != Page::Shortcuts && self.settings_query.trim().is_empty()) {
            self.recording = None;
        }
        if !self.show_prefs {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title(tr("Settings"))
            .with_app_id("kalendar")
            .with_inner_size([1060.0, 820.0])
            .with_min_inner_size([760.0, 480.0]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"), builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // this system can't open a second window: egui shows this one
                // inside the app, and Esc closes it
                ui.set_min_size(vec2(900.0, 640.0));
                self.settings_body(ui);
                return;
            }
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| self.settings_body(ui));
            let (close, esc) = ctx.input(|i| (i.viewport().close_requested(), i.key_pressed(egui::Key::Escape)));
            if close || (esc && !ctx.egui_wants_keyboard_input() && self.recording.is_none()) {
                self.show_prefs = false;
            }
        });
    }

    /// The whole content: page list on the left, the chosen page on the right.
    pub fn settings_body(&mut self, ui: &mut egui::Ui) {
        let before = serde_json::to_string(&(&self.data.prefs, &self.data.calendars, &self.data.frames, &self.data.lists)).unwrap_or_default();
        let (side, bg, muted, text, border) = (self.pal.side, self.pal.bg, self.pal.muted, self.pal.text, self.pal.border);
        let full = ui.max_rect();
        let nav_w = (full.width() * 0.3).clamp(190.0, 264.0);
        let (nav_rect, page_rect) = full.split_left_right_at_x(full.left() + nav_w);
        ui.painter().rect_filled(nav_rect, 0.0, side);
        ui.painter().rect_filled(page_rect, 0.0, bg);
        ui.painter().line_segment([nav_rect.right_top(), nav_rect.right_bottom()], Stroke::new(1.0, border));

        // page list
        ui.scope_builder(egui::UiBuilder::new().max_rect(nav_rect.shrink2(vec2(10.0, 0.0))), |ui| {
            egui::ScrollArea::vertical().id_salt("nav").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(22.0);
                search_field(ui, &mut self.settings_query, tr("Search settings"), ui.available_width());
                ui.add_space(20.0);
                let searching = !self.settings_query.trim().is_empty();
                for (group, pages) in [
                    (tr("Explore"), vec![(Page::Shortcuts, icons::KEYBOARD, tr("Shortcuts"))]),
                    (tr("Integrations"), vec![(Page::Calendars, icons::CALENDAR, tr("Calendars"))]),
                    (
                        tr("Preferences"),
                        vec![
                            (Page::General, icons::SETTINGS, tr("General")),
                            (Page::Appearance, icons::PALETTE, tr("Appearance")),
                            (Page::ActiveCalendars, icons::SQUARE_CHECK, tr("Active calendars")),
                            (Page::Lists, icons::LIST_TODO, tr("Task lists")),
                            (Page::Planner, icons::SHUFFLE, tr("Planner")),
                            (Page::Frames, icons::CLOCK, tr("Frames")),
                            (Page::QuickAdd, icons::KEYBOARD, tr("Quick add")),
                        ],
                    ),
                    (tr("About"), vec![(Page::Info, icons::INFO, tr("Info"))]),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new(group).size(theme::size::LARGE).color(muted));
                    });
                    ui.add_space(6.0);
                    for (page, icon, name) in pages {
                        let on = self.settings_page == page && !searching;
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                        let glow = ui.ctx().animate_bool_with_time(ui.scope_id().with(("nav", name)), resp.hovered() || on, motion::HOVER.secs());
                        if glow > 0.0 {
                            ui.painter().rect_filled(r, 9.0, mix(side, text, if on { 0.10 } else { 0.05 } * glow));
                        }
                        ui.painter().text(r.left_center() + vec2(28.0, 0.0), egui::Align2::CENTER_CENTER, icon, FontId::proportional(theme::size::LARGE), muted);
                        ui.painter().text(r.left_center() + vec2(52.0, 0.0), egui::Align2::LEFT_CENTER, name, FontId::proportional(theme::size::TITLE), text);
                        if resp.clicked() {
                            self.settings_page = page;
                            self.settings_query.clear();
                        }
                    }
                    ui.add_space(20.0);
                }
            });
        });

        // the page
        let ctx = ui.ctx().clone();
        ui.scope_builder(egui::UiBuilder::new().max_rect(page_rect), |ui| {
            let words = search_words(&self.settings_query);
            let salt = if words.is_empty() { self.settings_page as u8 } else { u8::MAX };
            // narrow windows get narrower margins
            let (ml, mr) = if page_rect.width() < 640.0 { (24, 28) } else { (44, 56) };
            egui::ScrollArea::vertical().id_salt(("page", salt)).auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: ml, right: mr, top: 30, bottom: 30 }).show(ui, |ui| {
                    let w = ui.available_width().min(980.0);
                    ui.set_max_width(w);
                    if !words.is_empty() {
                        self.search_results(ui, &ctx, words);
                        return;
                    }
                    match self.settings_page {
                        Page::General => self.page_general(ui),
                        Page::Appearance => self.page_appearance(ui),
                        Page::Planner => self.page_planner(ui),
                        Page::Frames => self.page_frames(ui),
                        Page::QuickAdd => self.page_quick_add(ui),
                        Page::ActiveCalendars => self.page_active(ui),
                        Page::Lists => self.page_lists(ui),
                        Page::Calendars => self.page_calendars(ui, &ctx),
                        Page::Shortcuts => self.page_shortcuts(ui),
                        Page::Info => self.page_info(ui),
                    }
                });
            });
        });
        if before != serde_json::to_string(&(&self.data.prefs, &self.data.calendars, &self.data.frames, &self.data.lists)).unwrap_or_default() {
            self.commit();
        }
    }

    /// Every page's settings that match the search, under their page's name,
    /// with their controls working as on the page itself.
    fn search_results(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, words: Vec<String>) {
        ui.data_mut(|d| d.insert_temp(find_id(), Find { words, ..Default::default() }));
        for (page, name) in SEARCH_ORDER {
            let name = tr(name);
            ui.data_mut(|d| d.get_temp_mut_or_default::<Find>(find_id()).head = Some((page, name)));
            ui.push_id(("found", page as u8), |ui| match page {
                Page::General => self.page_general(ui),
                Page::Appearance => self.page_appearance(ui),
                Page::Planner => self.page_planner(ui),
                Page::Frames => self.page_frames(ui),
                Page::QuickAdd => self.page_quick_add(ui),
                Page::ActiveCalendars => self.page_active(ui),
                Page::Lists => self.page_lists(ui),
                Page::Calendars => self.page_calendars(ui, ctx),
                Page::Shortcuts => self.page_shortcuts(ui),
                Page::Info => self.page_info(ui),
            });
        }
        let found = ui.data_mut(|d| d.remove_temp::<Find>(find_id())).unwrap_or_default();
        if found.hits == 0 {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(trf("No settings match \"{}\"", &[&self.settings_query.trim()])).size(theme::size::TITLE).color(self.pal.muted));
            });
        }
        if let Some(page) = found.jump {
            self.settings_page = page;
            self.settings_query.clear();
        }
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        let border = self.pal.border;
        let p = &mut self.data.prefs;
        row(ui, tr("Move task due date"), tr("When a task is scheduled after its due date: move the due date, ask, or leave it."), |ui| {
            combo(ui, "dm", &mut p.due_move, &[(DueMove::Always, tr("Always").into()), (DueMove::Ask, tr("Ask every time").into()), (DueMove::Never, tr("Never").into())]);
        });
        row(ui, tr("Automatically assign due dates"), tr("A task put on the calendar gets that day as its due date."), |ui| {
            combo(ui, "ad", &mut p.auto_due, &[(true, tr("Yes").into()), (false, tr("No").into())]);
        });
        row(ui, tr("Default block length"), tr("How long a task's block is when the task has no time given."), |ui| {
            let opts: Vec<(i64, String)> = [15, 30, 45, 60, 90, 120].iter().map(|m| (*m, fmt_duration(*m))).collect();
            combo(ui, "pd", &mut p.default_task_min, &opts);
        });
        divider(ui, border);
        row(ui, tr("Time format"), "", |ui| {
            combo(ui, "tf", &mut p.hour12, &[(false, "24 hours".into()), (true, "12 hours (AM/PM)".into())]);
        });
        row(ui, tr("First day of the week"), "", |ui| {
            let names = [tr("Sunday"), tr("Monday"), tr("Tuesday"), tr("Wednesday"), tr("Thursday"), tr("Friday"), tr("Saturday")];
            let opts: Vec<(u8, String)> = names.iter().enumerate().map(|(i, n)| (i as u8, n.to_string())).collect();
            combo(ui, "fd", &mut p.first_day, &opts);
        });
        row(ui, tr("Initial calendar view"), "", |ui| {
            let opts: Vec<(View, String)> = View::all().into_iter().map(|v| (v, v.label())).collect();
            combo(ui, "iv", &mut p.initial_view, &opts);
        });
        divider(ui, border);
        let snapshot = p.clone();
        row(ui, tr("Time grid start"), "", |ui| {
            let opts: Vec<(i64, String)> = (0..24).map(|h| (h, hour_label(&snapshot, h))).collect();
            combo(ui, "gs", &mut p.grid_start_h, &opts);
        });
        row(ui, tr("Time grid end"), "", |ui| {
            let opts: Vec<(i64, String)> = (1..=24).map(|h| (h, hour_label(&snapshot, h))).collect();
            combo(ui, "ge", &mut p.grid_end_h, &opts);
        });
        if p.grid_end_h <= p.grid_start_h {
            p.grid_end_h = p.grid_start_h + 1;
        }
        let mins = |m: i64| if m == 60 { "1 hour".to_string() } else { trf("{} minutes", &[&m]) };
        row(ui, tr("Time display resolution"), "", |ui| {
            let before = p.slot_min;
            let opts: Vec<(i64, String)> = SLOT_CHOICES.iter().map(|m| (*m, mins(*m))).collect();
            combo(ui, "sl", &mut p.slot_min, &opts);
            if p.slot_min != before {
                p.hour_height_px = None;
            }
        });
        row(ui, tr("Time dragging resolution"), "", |ui| {
            let opts: Vec<(i64, String)> = SNAP_CHOICES.iter().map(|m| (*m, mins(*m))).collect();
            combo(ui, "sn", &mut p.snap_min, &opts);
        });
        row(ui, tr("Limit events per day"), tr("How many events a day shows in the month view before \"+ more\"."), |ui| {
            let opts: Vec<(usize, String)> = (2..=20).map(|n| (n, n.to_string())).collect();
            combo(ui, "dx", &mut p.day_max_events, &opts);
        });
        divider(ui, border);
        row(ui, tr("Undo grace period"), "", |ui| {
            let opts: Vec<(u32, String)> = UNDO_CHOICES.iter().map(|s| (*s, if *s == 0 { tr("Disabled").into() } else { trf("{} seconds", &[&s]) })).collect();
            combo(ui, "ug", &mut p.undo_grace_s, &opts);
        });
        divider(ui, border);
        let yes_no = [(true, tr("Yes").to_string()), (false, tr("No").to_string())];
        row(ui, tr("Show weekends"), "", |ui| combo(ui, "we", &mut p.weekends_visible, &yes_no));
        row(ui, tr("Dim past events"), "", |ui| combo(ui, "dp", &mut p.dim_past, &yes_no));
        row(ui, tr("Show completed tasks"), tr("On the calendar."), |ui| combo(ui, "ct", &mut p.show_completed_tasks, &yes_no));
        row(ui, tr("Show scheduled tasks in the list"), "", |ui| combo(ui, "st", &mut p.show_scheduled_tasks, &yes_no));
        row(ui, tr("Due dates on the calendar"), "", |ui| combo(ui, "dc", &mut p.due_on_calendar, &yes_no));
    }

    fn page_appearance(&mut self, ui: &mut egui::Ui) {
        heading(ui, tr("Appearance"));
        let p = &mut self.data.prefs;
        row(ui, tr("Theme"), "", |ui| {
            combo(ui, "th", &mut p.theme, &[(Theme::System, tr("System").into()), (Theme::Light, tr("Light").into()), (Theme::Dark, tr("Dark").into())]);
        });
        row(ui, tr("Task panel position"), tr("Which side of the window the task panel sits on."), |ui| {
            combo(ui, "ps", &mut p.sidebar_side, &[(PanelSide::Left, tr("Left").into()), (PanelSide::Right, tr("Right").into())]);
        });
        let names = self.font_names.get_or_insert_with(crate::fonts::families);
        row(ui, tr("Font"), tr("For all text. Emoji always use the system's emoji font."), |ui| {
            let cw = ui.data(|d| d.get_temp::<f32>(ids::control_w())).unwrap_or(CONTROL_W);
            let shown = if p.font.is_empty() { tr("System default").to_string() } else { p.font.clone() };
            egui::ComboBox::from_id_salt("font").width(cw).height(320.0).selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut p.font, String::new(), tr("System default"));
                ui.separator();
                for n in names.iter() {
                    ui.selectable_value(&mut p.font, n.clone(), n);
                }
            });
        });
        row(ui, tr("Size"), tr("How large text and everything else is drawn."), |ui| {
            let sizes: Vec<(u32, String)> = [80u32, 90, 100, 110, 125, 150, 175, 200].iter().map(|s| (*s, trf("{}%", &[&s]))).collect();
            let mut pct = (p.ui_scale * 100.0).round() as u32;
            combo(ui, "sz", &mut pct, &sizes);
            p.ui_scale = pct as f32 / 100.0;
        });
        let accent = self.pal.accent;
        row(ui, tr("Animations"), tr("Sliding pages, gliding blocks, the zoom and fades. Off, everything changes at once."), |ui| {
            switch(ui, &mut p.animations, accent);
        });
    }

    fn page_planner(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        heading(ui, tr("Planner"));
        let searching = finding(ui);
        if !searching {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "\"Plan my tasks\" fits open tasks into your frames: blocks of time set aside for certain kinds of work. \
                         Frames come first (see Frames); otherwise it uses free time on working days, 08:00 to 18:00.",
                    )
                    .size(theme::size::UI)
                    .color(muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
        }
        let p = &mut self.data.prefs;
        let yes_no = [(true, tr("Yes").to_string()), (false, tr("No").to_string())];
        row(ui, tr("Split long tasks"), tr("Tasks of two hours or more may be cut into several sessions."), |ui| combo(ui, "sp", &mut p.split_long_tasks, &yes_no));
        row(ui, tr("Schedule tasks with no due date"), "", |ui| combo(ui, "pn", &mut p.plan_without_due, &yes_no));
    }

    /// Frames: how the planner uses them, then the frames themselves.
    fn page_frames(&mut self, ui: &mut egui::Ui) {
        let (muted, border) = (self.pal.muted, self.pal.border);
        heading(ui, tr("Frames"));
        let searching = finding(ui);
        if !searching {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Frames are blocks of time set aside for certain kinds of work. \
                         \"Plan my tasks\" puts each task into the frames whose filter it matches.",
                    )
                    .size(theme::size::UI)
                    .color(muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
        }
        let p = &mut self.data.prefs;
        let yes_no = [(true, tr("Yes").to_string()), (false, tr("No").to_string())];
        row(ui, tr("Plan into frames"), tr("Put tasks into the frames whose filter they match."), |ui| combo(ui, "pf", &mut p.use_frames, &yes_no));
        row(ui, tr("When no frame fits"), tr("What happens to a task that no frame takes."), |ui| {
            combo(ui, "po", &mut p.plan_outside_frames, &[(true, tr("Plan in working hours").into()), (false, tr("Leave unplanned").into())]);
        });
        row(ui, tr("When today's frames are full"), tr("Where a task goes once its frames have no room left today."), |ui| {
            combo(ui, "ff", &mut p.frame_full, &[(FrameFull::NextFrame, tr("Next matching frame").into()), (FrameFull::Now, tr("Free time now").into())]);
        });
        row(ui, tr("Frame time goes first to"), tr("Which tasks get a frame when there is not room for all of them."), |ui| {
            combo(
                ui,
                "fp",
                &mut p.frame_first,
                &[(FrameFirst::Priority, tr("High priority").into()), (FrameFirst::Tags, tr("Tasks with chosen tags").into()), (FrameFirst::Due, tr("Earliest due date").into())],
            );
        });
        if p.frame_first == FrameFirst::Tags {
            row(ui, tr("Chosen tags"), tr("Separated by spaces; # is optional."), |ui| {
                let id = ids::frame_tags();
                let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| p.frame_first_tags.join(" "));
                let cw = ui.data(|d| d.get_temp::<f32>(ids::control_w())).unwrap_or(CONTROL_W);
                if ui.add(egui::TextEdit::singleline(&mut text).hint_text("deep-work admin").desired_width(cw)).changed() {
                    p.frame_first_tags = text.split_whitespace().map(|t| t.trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect();
                }
                ui.data_mut(|d| d.insert_temp(id, text));
            });
        }
        divider(ui, border);
        if searching {
            let names: Vec<&str> = self.data.frames.iter().map(|f| f.name.as_str()).collect();
            page_result(ui, Page::Frames, tr("Frames"), tr("Blocks of time set aside for certain kinds of work"), &[&["time block", "deep work"][..], &names].concat());
            return;
        }
        self.frame_list(ui);
    }

    /// Every frame on one line: colour, name, when, what it takes; edit or switch off.
    fn frame_list(&mut self, ui: &mut egui::Ui) {
        let (muted, border, side, text, accent) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text, self.pal.accent);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Frames")).font(theme::bold(20.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let b = egui::Button::new(RichText::new("+ New frame").size(theme::size::UI).color(Color32::WHITE)).fill(self.pal.accent).corner_radius(theme::RADIUS);
                if ui.add(b).clicked() {
                    let cal = self.data.first_own_calendar();
                    self.open_new_frame(App::now().date(), 9 * 60, 11 * 60, cal, String::new());
                    self.show_prefs = false;
                }
            });
        });
        ui.add_space(6.0);
        if self.data.frames.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(tr("No frames yet. Make one here, from the + menu, or by right-clicking the calendar.")).size(theme::size::UI).color(muted),
                )
                .wrap(),
            );
            return;
        }
        let p = self.data.prefs.clone();
        let mut edit: Option<u64> = None;
        let lines: Vec<(String, String)> = self
            .data
            .frames
            .iter()
            .map(|f| {
                let t = |m: i64| p.fmt_time(chrono::NaiveTime::from_hms_opt((m / 60 % 24) as u32, (m % 60) as u32, 0).unwrap_or_default());
                let end = if f.end_min >= 24 * 60 { "24:00".to_string() } else { t(f.end_min) };
                let mut when = format!("{}, {} – {}", days_label(f.days), t(f.start_min), end);
                if f.every_weeks > 1 {
                    when += &format!(", every {} weeks", f.every_weeks);
                }
                (when, self.filter_summary(&f.filter, f.exclusive))
            })
            .collect();
        for (i, (when, what)) in lines.into_iter().enumerate() {
            let f = &self.data.frames[i];
            let (id, name, color) = (f.id, f.name.clone(), rgb(self.data.frame_color(f)));
            let f = &mut self.data.frames[i];
            // the buttons first, so the text is cut short rather than run under them
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 56.0), Layout::right_to_left(Align::Center), |ui| {
                switch(ui, &mut f.enabled, accent).on_hover_text(if f.enabled { tr("On: the planner uses it") } else { tr("Off: the planner skips it") });
                let b = egui::Button::new(RichText::new(format!("{} Edit", icons::PEN)).size(theme::size::BODY)).fill(mix(side, text, 0.07)).corner_radius(theme::BUTTON_RADIUS);
                if ui.add(b).clicked() {
                    edit = Some(id);
                }
                ui.add_space(6.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 8.0, if f.enabled { color } else { mix(side, color, 0.35) });
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let c = if f.enabled { text } else { muted };
                        ui.add(egui::Label::new(RichText::new(&name).size(theme::size::LARGE).color(c)).truncate());
                        ui.add(egui::Label::new(RichText::new(trf("{}  ·  {}", &[&when, &what])).size(theme::size::CAPTION).color(muted)).truncate());
                    });
                });
            });
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(id) = edit {
            // the editor lives in the main window
            self.edit_frame(id);
            self.show_prefs = false;
        }
    }

    fn filter_summary(&self, f: &FrameFilter, exclusive: bool) -> String {
        let mut parts: Vec<String> = vec![];
        // lists and tags: one of them is enough unless both are asked for
        let mut what: Vec<String> = vec![];
        if !f.lists.is_empty() {
            let names: Vec<String> = f
                .lists
                .iter()
                .map(|id| if *id == 0 { self.data.prefs.inbox_name() } else { self.data.lists.iter().find(|l| l.id == *id).map(|l| l.name.clone()).unwrap_or_else(|| "?".into()) })
                .collect();
            what.push(names.join(", "));
        }
        if !f.tags.is_empty() {
            let tags: Vec<String> = f.tags.iter().map(|t| format!("#{t}")).collect();
            what.push(tags.join(if f.tags_all { " + " } else { " or " }));
        }
        if !what.is_empty() {
            parts.push(what.join(" and "));
        }
        if let Some(m) = f.max_min {
            parts.push(format!("up to {}", fmt_duration(m)));
        }
        if parts.is_empty() {
            return "any task".into();
        }
        let mut s = parts.join(", ");
        if exclusive {
            s += ", nothing else";
        }
        s
    }

    /// Manage calendars: show or hide, rename, recolour, add and remove.
    fn page_active(&mut self, ui: &mut egui::Ui) {
        if finding(ui) {
            let names: Vec<&str> = self.data.calendars.iter().map(|c| c.name.as_str()).collect();
            let words = [&["show", "hide", "colour", "color", "rename", "add", "remove", "delete", "new calendar"][..], &names].concat();
            page_result(ui, Page::ActiveCalendars, tr("Manage calendars"), tr("Show or hide calendars, rename, recolour, add or remove them"), &words);
            return;
        }
        let ctx = ui.ctx().clone();
        let (muted, border, side, text) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text);
        heading(ui, tr("Active calendars"));
        ui.add(egui::Label::new(RichText::new(tr("Show or hide calendars, rename them, change their colour, add new ones or remove them.")).size(theme::size::UI).color(muted)).wrap());
        ui.add_space(12.0);
        // add a calendar of one's own
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.new_cal_name)
                    .hint_text(tr("New calendar name"))
                    .font(FontId::proportional(theme::size::UI))
                    .margin(Margin::symmetric(10, 8))
                    .desired_width((ui.available_width() - 330.0).clamp(120.0, 300.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let b = egui::Button::new(RichText::new("+ Add calendar").size(theme::size::UI).color(Color32::WHITE)).fill(self.pal.accent).corner_radius(theme::RADIUS);
            if (ui.add(b).clicked() || enter) && !self.new_cal_name.trim().is_empty() {
                self.snapshot();
                let n = self.data.calendars.len();
                let name = std::mem::take(&mut self.new_cal_name);
                let id = self.data.new_id();
                self.data.calendars.push(Calendar {
                    id,
                    name: name.trim().to_string(),
                    color: EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0,
                    visible: true,
                    ..Default::default()
                });
                self.commit();
            }
            if ui.add(egui::Button::new(RichText::new(tr("Subscribe by link…")).size(theme::size::UI)).corner_radius(theme::RADIUS)).clicked() {
                self.settings_page = Page::Calendars;
            }
        });
        ui.add_space(10.0);
        let own_count = self.data.calendars.iter().filter(|c| c.is_local()).count();
        let counts: Vec<usize> = self.data.calendars.iter().map(|c| self.data.events.iter().filter(|e| e.calendar == c.id).count()).collect();
        let mut remove: Option<u64> = None;
        let mut ask: Option<Option<u64>> = None;
        for (i, c) in self.data.calendars.iter_mut().enumerate() {
            let subscribed = !c.is_local();
            let synced = c.account.is_some();
            let cid = c.id;
            let what = if synced && c.read_only { "from an account, read-only".to_string() } else if synced { format!("from an account, {} events", counts[i]) } else if subscribed { "subscribed, read-only".to_string() } else { trf("{} events", &[&counts[i]]) };
            // the buttons first; the name takes what is left
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 52.0), Layout::right_to_left(Align::Center), |ui| {
                {
                    if self.confirm_remove == Some(cid) {
                        if ui.add(egui::Button::new(RichText::new(tr("Cancel")).size(theme::size::BODY)).corner_radius(theme::BUTTON_RADIUS)).clicked() {
                            ask = Some(None);
                        }
                        let label = if subscribed { tr("Unsubscribe").to_string() } else { trf("Remove with its {} events", &[&counts[i]]) };
                        if ui.add(egui::Button::new(RichText::new(label).size(theme::size::BODY).color(Color32::WHITE)).fill(theme::DANGER).corner_radius(theme::BUTTON_RADIUS)).clicked() {
                            remove = Some(cid);
                        }
                    } else {
                        // an account's calendar goes when the account is signed out
                        let last_own = (!subscribed && own_count <= 1) || synced;
                        let b = egui::Button::new(RichText::new(if subscribed { tr("Unsubscribe") } else { tr("Remove") }).size(theme::size::BODY).color(theme::DANGER)).fill(mix(side, text, 0.07)).corner_radius(theme::BUTTON_RADIUS);
                        let resp = ui.add_enabled(!last_own, b);
                        if synced {
                            resp.on_disabled_hover_text(tr("Untick it under Integrations to stop syncing it, or sign out of its account there"));
                        } else if last_own {
                            resp.on_disabled_hover_text(tr("At least one calendar of your own is needed"));
                        } else if resp.clicked() {
                            ask = Some(Some(cid));
                        }
                    }
                }
                // the count only where there is room for it and a name
                if ui.available_width() > 300.0 {
                    ui.label(RichText::new(what).size(theme::size::CAPTION).color(muted));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.checkbox(&mut c.visible, "").on_hover_text(tr("Show on the calendar"));
                    // colour: a dot that opens the palette
                    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 9.0, rgb(c.color));
                    let resp = resp.on_hover_text(tr("Colour"));
                    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
                        ui.set_min_width(200.0);
                        ui.horizontal_wrapped(|ui| {
                            for (col, name) in EVENT_COLORS.iter() {
                                let (cr, cresp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                ui.painter().circle_filled(cr.center(), 8.0, rgb(*col));
                                if cresp.on_hover_text(*name).clicked() {
                                    c.color = *col;
                                }
                            }
                        });
                    });
                    let w = (ui.available_width() - 8.0).clamp(60.0, 280.0);
                    ui.add(egui::TextEdit::singleline(&mut c.name).font(FontId::proportional(theme::size::LARGE)).desired_width(w).id_salt(("calname", cid)));
                });
            });
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(a) = ask {
            self.confirm_remove = a;
        }
        if let Some(id) = remove {
            self.snapshot();
            let name = self.data.calendar(id).map(|c| c.name.clone()).unwrap_or_default();
            self.drop_calendar(id);
            self.confirm_remove = None;
            self.selected = None;
            self.editor = None;
            self.commit();
            self.say(&ctx, trf("Calendar \"{}\" removed", &[&name]), true);
        }
    }

    /// How the quick task field reads text: a preset of marks like another
    /// app's, each mark changeable, plain dates, and the person's own words.
    fn page_quick_add(&mut self, ui: &mut egui::Ui) {
        let (muted, border, accent) = (self.pal.muted, self.pal.border, self.pal.accent);
        let hour12 = self.data.prefs.hour12;
        heading(ui, tr("Quick add"));
        let searching = finding(ui);
        if !searching {
            ui.add(
                egui::Label::new(
                    RichText::new(tr(
                        "How the new task bar (Ctrl+T) reads what you type. A time, as in \"call mum fri at 5pm\", puts the task straight on the calendar; \
                         a date alone makes it the due date. Put words in \"quotes\" to keep them in the title.",
                    ))
                    .size(theme::size::UI)
                    .color(muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
        }
        let q = &mut self.data.prefs.quick;
        let presets: Vec<(QuickPreset, String)> = QuickPreset::ALL.iter().map(|p| (*p, p.label().to_string())).collect();
        row(ui, tr("Preset"), tr("Marks like those of the app you are used to. Changing a mark below makes them your own."), |ui| {
            combo(ui, "qp", &mut q.preset, &presets)
        });
        row(ui, tr("Understand plain dates and times"), tr("\"tomorrow\", \"fri 5pm\", \"every monday\" without a mark before them."), |ui| {
            switch(ui, &mut q.natural, accent);
        });
        row(ui, tr("Numeric dates"), "", |ui| {
            combo(ui, "qd", &mut q.day_first, &[(true, tr("Day/Month (12/4 is 12 April)").into()), (false, tr("Month/Day (12/4 is December 4)").into())])
        });
        divider(ui, border);
        let mut toks = q.tokens();
        let before = toks.clone();
        for k in Kind::ALL {
            let sample = toks.sample(k);
            let tip = trf("Marks for the {}, split by spaces. A mark of two or more letters is a word before the value.", &[&k.label().to_lowercase()]);
            row(ui, k.label(), &tip, |ui| {
                ui.add(egui::TextEdit::singleline(toks.get_mut(k)).desired_width(110.0).hint_text(tr("none")));
                ui.add_space(10.0);
                ui.label(RichText::new(sample).size(theme::size::UI).color(muted));
            });
        }
        if toks != before {
            q.custom = toks;
            q.preset = QuickPreset::Custom;
        }
        divider(ui, border);
        if searching {
            page_result(ui, Page::QuickAdd, tr("Abbreviations"), tr("Your own short words, each standing for others"), &["tmr", "shortcut", "natural language", "todoist", "ticktick", "things"]);
            return;
        }
        ui.label(RichText::new(tr("Abbreviations")).font(theme::bold(20.0)));
        ui.add_space(2.0);
        ui.label(RichText::new(tr("A word of yours that stands for others: \"eod\" for \"at 17:00\", \"w\" for \">Work\".")).size(theme::size::UI).color(muted));
        ui.add_space(6.0);
        let mut remove = None;
        for (i, (short, long)) in q.abbrevs.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(short).desired_width(110.0).hint_text(tr("eod")));
                ui.label(RichText::new(icons::CHEVRON_RIGHT).color(muted));
                ui.add(egui::TextEdit::singleline(long).desired_width((ui.available_width() - 40.0).max(120.0)).hint_text(tr("at 17:00")));
                if ui.add(egui::Button::new(RichText::new(icons::X).color(muted)).frame(false)).on_hover_text(tr("Remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            q.abbrevs.remove(i);
        }
        if ui.add(egui::Button::new(RichText::new(format!("{}  {}", icons::PLUS, tr("Add abbreviation"))).size(theme::size::UI)).frame(false)).clicked() {
            q.abbrevs.push((String::new(), String::new()));
        }
        divider(ui, border);
        ui.label(RichText::new(tr("Try it")).font(theme::bold(20.0)));
        ui.add_space(4.0);
        let id = ids::quick_try();
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut text).hint_text(q.tokens().hint()).desired_width(f32::INFINITY));
        let now = App::now();
        let r = parse_quick(&text, now.date(), q);
        if !text.trim().is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("\u{201c}{}\u{201d}", r.title)).size(theme::size::UI));
            let chips = r.chips(now, hour12);
            if !chips.is_empty() {
                ui.label(RichText::new(chips.join("   ")).size(theme::size::UI).color(accent));
            }
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    }

    /// Task lists: name, colour and icon of each, new ones, and removing.
    fn page_lists(&mut self, ui: &mut egui::Ui) {
        if finding(ui) {
            let names: Vec<&str> = self.data.lists.iter().map(|l| l.name.as_str()).collect();
            let words = [&["task list", "inbox", "icon", "colour", "color", "rename", "add", "remove", "delete", "new list"][..], &names].concat();
            page_result(ui, Page::Lists, tr("Manage task lists"), tr("Rename task lists, change their colour and icon, add or remove them"), &words);
            return;
        }
        let (muted, border, side, text) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text);
        heading(ui, tr("Task lists"));
        ui.add(egui::Label::new(RichText::new(tr("Rename your task lists, change their colour and icon, add new ones or remove them.")).size(theme::size::UI).color(muted)).wrap());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.new_list_name)
                    .hint_text(tr("New list name"))
                    .font(FontId::proportional(theme::size::UI))
                    .margin(Margin::symmetric(10, 8))
                    .desired_width((ui.available_width() - 330.0).clamp(120.0, 300.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let b = egui::Button::new(RichText::new("+ Add list").size(theme::size::UI).color(Color32::WHITE)).fill(self.pal.accent).corner_radius(theme::RADIUS);
            if (ui.add(b).clicked() || enter) && !self.new_list_name.trim().is_empty() {
                self.snapshot();
                let name = std::mem::take(&mut self.new_list_name);
                self.new_list(name.trim());
            }
        });
        ui.add_space(10.0);
        let ids: Vec<u64> = std::iter::once(0).chain(self.data.lists.iter().map(|l| l.id)).collect();
        let mut remove: Option<u64> = None;
        for lid in ids {
            let (mut name, color, icon) = self.data.list_look(lid);
            let synced = self.data.lists.iter().any(|l| l.id == lid && l.account.is_some());
            let count = self.data.tasks.iter().filter(|t| t.list == lid && !t.done).count();
            let what = if synced { format!("from an account, {count} open tasks") } else { trf("{} open tasks", &[&count]) };
            // the button first; the name takes what is left
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 52.0), Layout::right_to_left(Align::Center), |ui| {
                if lid == 0 {
                    ui.label(RichText::new("always there").size(theme::size::CAPTION).color(muted));
                } else {
                    let b = egui::Button::new(RichText::new(tr("Remove")).size(theme::size::BODY).color(theme::DANGER)).fill(mix(side, text, 0.07)).corner_radius(theme::BUTTON_RADIUS);
                    let resp = ui.add_enabled(!synced, b).on_hover_text(tr("Its tasks go to the Inbox"));
                    if resp.on_disabled_hover_text(tr("Untick it under Integrations to stop syncing it, or sign out of its account there")).clicked() {
                        remove = Some(lid);
                    }
                }
                if ui.available_width() > 300.0 {
                    ui.label(RichText::new(what).size(theme::size::CAPTION).color(muted));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // the ring or icon opens colours and icons
                let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                if resp.hovered() {
                    crate::fill_squircle(ui.painter(), r, 8.0, mix(side, text, 0.08));
                }
                if icon.is_empty() {
                    ui.painter().circle_stroke(r.center(), 6.5, Stroke::new(2.0, rgb(color)));
                } else {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, &icon, FontId::proportional(theme::size::HEADING), rgb(color));
                }
                let resp = resp.on_hover_text(tr("Colour and icon"));
                egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(236.0);
                    let mut snapped = true;
                    self.list_fields(ui, lid, false, false, &mut snapped);
                });
                let edit = egui::TextEdit::singleline(&mut name)
                    .hint_text(if lid == 0 { tr("Inbox") } else { tr("List name") })
                    .font(FontId::proportional(theme::size::LARGE))
                    .desired_width((ui.available_width() - 8.0).clamp(60.0, 280.0))
                    .id_salt(("listname", lid));
                if ui.add(edit).changed() {
                    let (_, color, icon) = self.data.list_look(lid);
                    self.data.set_list_look(lid, (name.clone(), color, icon));
                }
                });
            });
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(id) = remove {
            self.snapshot();
            self.data.remove_own_list(id);
        }
    }

    fn page_calendars(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if finding(ui) {
            let names: Vec<&str> = self.data.calendars.iter().filter(|c| c.url.is_some()).map(|c| c.name.as_str()).collect();
            let words = [&["integration", "subscription", "link", "url", "ics", "webcal", "feed", "proton", "import", "refresh", "unsubscribe", "account", "caldav", "nextcloud", "fastmail", "radicale", "sync", "password", "keyring", "sign in", "server", "google", "gmail", "oauth", "client id", "microsoft", "outlook", "office 365", "entra"][..], &names].concat();
            page_result(ui, Page::Calendars, tr("Subscribe to a calendar"), tr("Bring other calendars in by their link, such as Proton Calendar"), &words);
            return;
        }
        let (muted, border, side, text) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text);
        heading(ui, tr("Integrate your calendars"));
        ui.add(egui::Label::new(RichText::new(tr("Bring calendars in from a link or from an account. Pick a kind to set it up.")).size(theme::size::LARGE).color(muted)).wrap());
        ui.add_space(12.0);
        // one button per kind; the chosen one shows its form below
        const KINDS: [(u8, &str); 6] = [(1, later("Calendar feed")), (2, "CalDAV"), (3, "Nextcloud"), (4, "Google"), (5, "Microsoft"), (6, "Proton")];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            let w = ((ui.available_width() - 50.0) / 6.0).floor().clamp(96.0, 230.0);
            for (kind, label) in KINDS {
                let on = self.sub_kind == kind;
                let b = egui::Button::new(RichText::new(tr(label)).size(theme::size::UI))
                    .min_size(vec2(w, 48.0))
                    .corner_radius(theme::RADIUS)
                    .fill(mix(side, text, if on { 0.2 } else { 0.09 }))
                    .stroke(Stroke::new(1.0, if on { self.pal.accent } else { Color32::TRANSPARENT }));
                if ui.add(b).clicked() {
                    self.sub_kind = if on { 0 } else { kind };
                    self.acct_error = None;
                }
            }
        });
        if self.sub_kind != 0 {
            ui.add_space(12.0);
            egui::Frame::new().fill(mix(side, text, 0.04)).stroke(Stroke::new(1.0, border)).corner_radius(theme::CARD_RADIUS).inner_margin(Margin::same(16)).show(ui, |ui| {
                let help = |ui: &mut egui::Ui, t: &str| {
                    ui.add(egui::Label::new(RichText::new(t).size(theme::size::BODY).color(muted)).wrap());
                    ui.add_space(6.0);
                };
                let field = |ui: &mut egui::Ui, value: &mut String, hint: &str, secret: bool| {
                    let r = ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(hint)
                            .password(secret)
                            .font(FontId::proportional(theme::size::UI))
                            .margin(Margin::symmetric(10, 8))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(4.0);
                    r
                };
                let accent = self.pal.accent;
                let go = |ui: &mut egui::Ui, label: &str, enabled: bool| {
                    ui.add_space(2.0);
                    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).size(theme::size::UI).color(Color32::WHITE)).fill(accent).corner_radius(theme::RADIUS)).clicked()
                };
                let waiting = self.nextcloud_wait.is_some();
                let both_ways = tr("Its calendars come in and stay in step both ways: what you change here is sent to the server. Repeating events can't be changed here yet.");
                match self.sub_kind {
                    1 => {
                        // one field takes every kind of link: plain .ics feeds, webcal,
                        // and shared links from Proton, Google, Outlook or iCloud
                        help(ui, tr("Paste the calendar's link: an https://, http:// or webcal:// address, such as a .ics feed or a shared link from Proton, Google, Outlook or iCloud. A calendar brought in by its link is read-only. For Proton Calendar, open Settings > Calendars, pick the calendar, choose \"Share with anyone\" and copy the link."));
                        let r = field(ui, &mut self.sub_url, "https://…/calendar.ics  or  webcal://…", false);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ics::normalize_url(&self.sub_url).ok().as_deref().map(ics::is_proton).unwrap_or(false) {
                            ui.label(RichText::new(tr("Proton Calendar link recognised")).size(theme::size::CAPTION).color(theme::SUCCESS));
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.sub_name).hint_text(tr("Name (optional)")).font(FontId::proportional(theme::size::UI)).margin(Margin::symmetric(10, 8)).desired_width(260.0));
                            let b = egui::Button::new(RichText::new(tr("Subscribe")).size(theme::size::UI).color(Color32::WHITE)).fill(accent).corner_radius(theme::RADIUS);
                            if ui.add(b).clicked() || enter {
                                let count = self.data.calendars.len();
                                self.subscribe(ctx);
                                if self.data.calendars.len() > count {
                                    self.sub_kind = 0;
                                }
                            }
                        });
                    }
                    2 => {
                        help(ui, &trf("Sign in to a calendar server that speaks CalDAV, such as Fastmail, Radicale or Baïkal. {}", &[&both_ways]));
                        field(ui, &mut self.acct_server, tr("Server address, such as https://cloud.example.org"), false);
                        field(ui, &mut self.acct_user, tr("User name"), false);
                        let r = field(ui, &mut self.acct_pass, tr("App password"), true);
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        help(ui, tr("Make an app password in the server's security settings and use it here, not your main password. It is kept in your desktop's keyring, never in kalendar's files."));
                        let connecting = self.data.accounts.iter().any(|a| a.home.is_none() && self.syncing.contains(&a.id));
                        ui.horizontal(|ui| {
                            if (go(ui, tr("Connect"), !connecting) || enter) && !connecting {
                                self.connect_account(ctx);
                            }
                            if connecting {
                                ui.spinner();
                                ui.label(RichText::new(tr("Connecting…")).size(theme::size::UI).color(muted));
                            }
                        });
                    }
                    3 => {
                        help(ui, &trf("Only the server's address is needed. Nextcloud's own sign-in page opens in your browser and hands kalendar an app password of its own, which goes into your desktop's keyring. {}", &[&both_ways]));
                        field(ui, &mut self.acct_server, tr("Server address, such as https://cloud.example.org"), false);
                        if go(ui, tr("Sign in with Nextcloud…"), !waiting) {
                            self.connect_nextcloud(ctx);
                        }
                    }
                    4 => {
                        help(ui, "kalendar has no registration of its own with Google, so you bring one: in the Google Cloud console, make a project, switch on the CalDAV API, add yourself as a test user on the consent screen, and make an OAuth client ID of the type \"Desktop app\". Paste its client ID and client secret here, or the whole JSON file it lets you download. While the project is in Testing, Google asks you to sign in again every 7 days.");
                        field(ui, &mut self.g_email, tr("Your Google address"), false);
                        setup_links(ui, &[(tr("Step-by-step guide"), GUIDE_GOOGLE), (tr("Google Cloud console"), "https://console.cloud.google.com/auth/clients")]);
                        field(ui, &mut self.g_client_id, tr("Client ID, ending in .apps.googleusercontent.com"), false);
                        // the client file Google offers for download fills both fields
                        if self.g_client_id.trim_start().starts_with('{') {
                            if let Some((id, secret)) = crate::sync::google::from_client_json(&self.g_client_id) {
                                self.g_client_id = id;
                                self.g_secret = secret;
                            }
                        }
                        field(ui, &mut self.g_secret, tr("Client secret"), true);
                        if go(ui, tr("Sign in with Google…"), !waiting) {
                            self.connect_google(ctx);
                        }
                    }
                    6 => {
                        help(ui, tr("Proton has no official way in for other calendar programs. kalendar signs in the way Proton's own apps do and decrypts your events on this computer. Your password is used once to sign in and is not kept; the keyring holds the session. Calendars of your own stay in step both ways; calendars shared with you, events with guests and repeating events can only be read. Proton may stop this from working at any time."));
                        field(ui, &mut self.pr_user, tr("Your Proton address or user name"), false);
                        field(ui, &mut self.pr_pass, tr("Password"), true);
                        field(ui, &mut self.pr_code, tr("Two-factor code, if you use one"), false);
                        field(ui, &mut self.pr_second, tr("Second (mailbox) password, if your account has one"), true);
                        if self.pr_busy {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new(tr("Signing in to Proton…")).size(theme::size::BODY).color(muted));
                            });
                        } else if go(ui, tr("Sign in to Proton"), true) {
                            self.connect_proton(ctx);
                        }
                    }
                    _ => {
                        help(ui, tr("For Microsoft 365 and Outlook.com. kalendar has no registration of its own with Microsoft, so you bring one: in the Microsoft Entra admin center, register an app for accounts in any organisation and personal accounts, add the platform \"Mobile and desktop applications\" with the redirect address http://localhost, and allow the delegated Graph permission Calendars.ReadWrite. Paste its Application (client) ID here; there is no secret."));
                        field(ui, &mut self.ms_email, tr("Your Microsoft address"), false);
                        setup_links(ui, &[(tr("Step-by-step guide"), GUIDE_MICROSOFT), (tr("App registrations"), "https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")]);
                        field(ui, &mut self.ms_client_id, tr("Application (client) ID"), false);
                        if go(ui, tr("Sign in with Microsoft…"), !waiting) {
                            self.connect_microsoft(ctx);
                        }
                    }
                }
                // a sign-in that is waiting for the browser
                if let Some((login, _)) = self.nextcloud_wait.clone().filter(|_| self.sub_kind >= 3) {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tr("Waiting for you to sign in on the page that opened in your browser…")).size(theme::size::BODY).color(muted));
                        if ui.add(egui::Button::new(RichText::new(tr("Cancel")).size(theme::size::BODY)).corner_radius(theme::BUTTON_RADIUS)).clicked() {
                            self.cancel_nextcloud();
                        }
                    });
                    if !login.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(tr("If no page opened, open this one:")).size(theme::size::CAPTION).color(muted));
                            ui.hyperlink_to(RichText::new("the sign-in page").size(theme::size::CAPTION), &login);
                        });
                    }
                }
                if let Some(e) = self.acct_error.as_ref().filter(|_| self.sub_kind >= 2) {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(e).size(theme::size::BODY).color(theme::DANGER)).wrap());
                }
            });
        }
        divider(ui, border);
        heading(ui, tr("Manage integrations"));
        let mut refresh: Option<u64> = None;
        let mut remove: Option<u64> = None;
        let subs: Vec<(u64, Calendar)> = self.data.calendars.iter().filter(|c| c.url.is_some()).map(|c| (c.id, c.clone())).collect();
        if subs.is_empty() && !self.data.accounts.iter().any(|a| a.home.is_some()) {
            ui.label(RichText::new(tr("Nothing is connected yet.")).size(theme::size::UI).color(muted));
        }
        for (i, c) in subs {
            let url = c.url.clone().unwrap_or_default();
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 70.0), Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button(RichText::new(icons::ELLIPSIS).size(theme::size::HEADING).color(muted), |ui| {
                        if ui.button(tr("Refresh")).clicked() {
                            refresh = Some(i);
                            ui.close();
                        }
                        if ui.button(RichText::new(tr("Unsubscribe")).color(theme::DANGER)).clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                    ui.add_space(12.0);
                    if self.syncing.contains(&i) {
                        ui.label(RichText::new(tr("Refreshing…")).size(theme::size::LARGE).color(muted));
                        ui.spinner();
                    } else if let Some(e) = &c.error {
                        ui.label(RichText::new(tr("Not connected")).size(theme::size::LARGE).color(theme::DANGER)).on_hover_text(e);
                    } else if c.last_sync.is_some() {
                        ui.label(RichText::new(format!("{} Connected", icons::CHECK)).size(theme::size::LARGE).color(theme::SUCCESS));
                    } else {
                        ui.label(RichText::new(tr("Waiting")).size(theme::size::LARGE).color(muted));
                    }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 7.0, rgb(c.color));
                ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(1.5, muted));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(12.0);
                    ui.add(egui::Label::new(RichText::new(&c.name).size(theme::size::TITLE)).truncate());
                    let kind = if url == SECRET_URL {
                        tr("Private link, kept in the keyring").to_string()
                    } else if ics::is_proton(&url) { tr("Proton Calendar").to_string() } else { trf("Feed from {}", &[&ics::default_name(&url)]) };
                    let when = c.last_sync.as_ref().map(|t| format!("  ·  refreshed {t}")).unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(trf("{}{}", &[&kind, &when])).size(theme::size::BODY).color(muted)).truncate());
                });
                });
            });
            if let Some(e) = &c.error {
                ui.add(egui::Label::new(RichText::new(e).size(theme::size::CAPTION).color(theme::DANGER)).wrap());
            }
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(id) = refresh {
            self.sync_feed(ctx, id);
        }
        if let Some(i) = remove {
            self.snapshot();
            self.drop_calendar(i);
            self.commit();
        }
        self.accounts_section(ui, ctx);
    }

    /// The accounts the person signed in to, each with its calendars.
    fn accounts_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (muted, border) = (self.pal.muted, self.pal.border);
        let mut sync_now: Option<u64> = None;
        let mut remove: Option<u64> = None;
        let mut changed = false;
        // a calendar whose tasks are to be brought in, or no longer
        let mut tasks_toggle: Option<(u64, bool)> = None;
        let with_tasks: std::collections::HashSet<u64> = self.data.calendars.iter().filter(|c| self.calendar_tasks_on(c.id)).map(|c| c.id).collect();
        // accounts still signing in for the first time are not listed yet
        let accounts: Vec<Account> = self.data.accounts.iter().filter(|a| a.home.is_some()).cloned().collect();
        for a in &accounts {
            ui.add_space(6.0);
            // the status and menu first; the name takes what is left
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 64.0), Layout::right_to_left(Align::Center), |ui| {
                {
                    ui.menu_button(RichText::new(icons::ELLIPSIS).size(theme::size::HEADING).color(muted), |ui| {
                        if ui.button(tr("Sync now")).clicked() {
                            sync_now = Some(a.id);
                            ui.close();
                        }
                        if ui.button(RichText::new(tr("Sign out and remove")).color(theme::DANGER)).clicked() {
                            remove = Some(a.id);
                            ui.close();
                        }
                    });
                    ui.add_space(12.0);
                    let busy = self.syncing.contains(&a.id) || self.data.calendars.iter().any(|c| c.account == Some(a.id) && self.syncing.contains(&c.id));
                    if busy {
                        ui.label(RichText::new(tr("Syncing…")).size(theme::size::LARGE).color(muted));
                        ui.spinner();
                    } else if let Some(e) = &a.error {
                        ui.label(RichText::new(tr("Not connected")).size(theme::size::LARGE).color(theme::DANGER)).on_hover_text(e);
                    } else {
                        ui.label(RichText::new(format!("{} Connected", icons::CHECK)).size(theme::size::LARGE).color(theme::SUCCESS));
                    }
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add_space(10.0);
                        ui.add(egui::Label::new(RichText::new(a.label()).size(theme::size::TITLE)).truncate());
                        let when = a.last_sync.as_ref().map(|t| format!("  ·  synced {t}")).unwrap_or_default();
                        let kind = if a.proton { tr("Proton account") } else if a.microsoft { tr("Microsoft account") } else if a.google { tr("Google account") } else if a.nextcloud { tr("Nextcloud account") } else { tr("CalDAV account") };
                        ui.add(egui::Label::new(RichText::new(trf("{}{}", &[&kind, &when])).size(theme::size::BODY).color(muted)).truncate());
                    });
                });
            });
            if let Some(e) = &a.error {
                ui.add(egui::Label::new(RichText::new(e).size(theme::size::CAPTION).color(theme::DANGER)).wrap());
            }
            // its calendars: a tick brings one in and keeps it in step
            for c in self.data.calendars.iter_mut().filter(|c| c.account == Some(a.id)) {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(14.0);
                    if ui.checkbox(&mut c.visible, "").on_hover_text(tr("Show and sync this calendar")).changed() {
                        changed = true;
                        if c.visible {
                            sync_now = Some(a.id);
                        }
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, rgb(c.color));
                    ui.label(RichText::new(&c.name).size(theme::size::UI));
                    if c.read_only {
                        ui.label(RichText::new("read-only").size(theme::size::CAPTION).color(muted));
                    }
                    if let Some(e) = &c.error {
                        ui.label(RichText::new("could not sync").size(theme::size::CAPTION).color(theme::DANGER)).on_hover_text(e);
                    }
                    if c.holds_tasks {
                        let mut on = with_tasks.contains(&c.id);
                        if ui.checkbox(&mut on, RichText::new("tasks too").size(theme::size::CAPTION).color(muted)).on_hover_text(tr("Bring the tasks kept in this calendar in as a task list")).changed() {
                            tasks_toggle = Some((c.id, on));
                        }
                    }
                });
            }
            // lists on the server that hold only tasks
            for l in self.data.lists.iter().filter(|l| l.account == Some(a.id) && !self.data.calendars.iter().any(|c| c.account == l.account && c.remote_id == l.remote_id)) {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(42.0);
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, rgb(l.color));
                    ui.label(RichText::new(&l.name).size(theme::size::UI));
                    ui.label(RichText::new(if l.read_only { "task list, read-only" } else { "task list" }).size(theme::size::CAPTION).color(muted));
                    if let Some(e) = &l.error {
                        ui.label(RichText::new("could not sync").size(theme::size::CAPTION).color(theme::DANGER)).on_hover_text(e);
                    }
                });
            }
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if changed {
            self.commit();
        }
        if let Some((cal, on)) = tasks_toggle {
            self.set_calendar_tasks(ctx, cal, on);
        }
        if let Some(id) = sync_now {
            self.backoff.remove(&id);
            self.sync_account(ctx, id, None);
        }
        if let Some(id) = remove {
            self.remove_account(id);
        }
        ui.add_space(14.0);
        ui.checkbox(&mut self.data.prefs.plaintext_secrets, RichText::new(tr("Keep passwords in a file when no keyring is running")).size(theme::size::BODY))
            .on_hover_text(trf("Off by default. When on, and only if no keyring answers, passwords go to {} as plain text that only you can read.", &[&secrets::file_path().display()]));
    }

    /// Takes the next key combination typed while a shortcut is being
    /// recorded; Escape cancels.
    fn record_key(&mut self, ctx: &egui::Context) {
        let Some(cmd) = self.recording else { return };
        let got = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((modifiers.command, modifiers.shift, *key)),
                // egui reports these combinations as clipboard events
                egui::Event::Copy => Some((true, false, Key::C)),
                egui::Event::Cut => Some((true, false, Key::X)),
                egui::Event::Paste(_) => Some((true, false, Key::V)),
                _ => None,
            })
        });
        let Some((ctrl, shift, key)) = got else { return };
        ctx.input_mut(|i| i.events.clear());
        self.recording = None;
        if key == Key::Escape {
            return;
        }
        let bind = KeyBind { ctrl, shift, key };
        self.data.prefs.keybinds.insert(cmd.id().to_string(), bind_label(&bind));
    }

    fn page_shortcuts(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.record_key(&ctx);
        let (muted, text, side, accent) = (self.pal.muted, self.pal.text, self.pal.side, self.pal.accent);
        let chip = |ui: &mut egui::Ui, k: &str, fill: Color32, color: Color32| {
            egui::Frame::new().fill(fill).corner_radius(theme::RADIUS as u8).inner_margin(Margin::symmetric(9, 5)).show(ui, |ui| {
                ui.label(RichText::new(k).size(theme::size::UI).color(color));
            });
        };
        // drawn right to left: last alternative first, last key first
        let chips = |ui: &mut egui::Ui, keys: &str| {
            let alts: Vec<&str> = keys.split(" / ").collect();
            for (ai, alt) in alts.iter().enumerate().rev() {
                let parts: Vec<&str> = if alt.contains(" then ") { alt.split(" then ").collect() } else { alt.split('+').collect() };
                for k in parts.iter().rev() {
                    chip(ui, k, mix(side, text, 0.14), text);
                }
                if ai > 0 {
                    ui.label(RichText::new("or").size(theme::size::UI).color(muted));
                }
            }
        };
        let fixed_row = |ui: &mut egui::Ui, label: &str, keys: &str| {
            // the keys first; the name is cut short if they leave too little room
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 44.0), Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                chips(ui, keys);
                ui.add_space(10.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(label).size(theme::size::TITLE).color(mix(muted, text, 0.6))).truncate());
                });
            });
        };
        let section_head = |ui: &mut egui::Ui, sec: &str, first: bool| {
            if !first {
                ui.add_space(22.0);
            }
            ui.label(RichText::new(sec).font(theme::bold(20.0)));
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, muted);
            ui.add_space(10.0);
        };
        // in the settings search the matches come without section headings,
        // under the page's own; on the page a field narrows the list
        let searching = finding(ui);
        let words = search_words(&self.shortcut_query);
        if !searching {
            ui.horizontal(|ui| {
                search_field(ui, &mut self.shortcut_query, tr("Search shortcuts"), (ui.available_width() * 0.5).clamp(220.0, 360.0));
                ui.add_space(12.0);
                ui.add(egui::Label::new(RichText::new(tr("Click a shortcut to change it, then press the new keys. Esc cancels.")).size(theme::size::UI).color(muted)).wrap());
            });
            ui.add_space(10.0);
        }
        // a section name only counts on this page, where the sections show;
        // across all settings "calendar" would otherwise list half of them
        let wanted = |ui: &mut egui::Ui, label: &str, keys: &str, sec: &str| {
            if searching {
                shown(ui, &[label, keys])
            } else {
                search_hit(&words, &[label, keys, sec])
            }
        };
        // every command's keys, to warn when two share one
        let all: Vec<(Cmd, &str, Vec<KeyBind>)> = COMMANDS.iter().map(|c| (c.0, tr(c.1), self.binds(c.0))).collect();
        let mut section = "";
        let mut rows = 0;
        for (cmd, label, default, sec) in COMMANDS {
            let (label, sec) = (&tr(label), &tr(sec));
            if *cmd == Cmd::Paste {
                // pasting is done with the mouse
                continue;
            }
            let keys = self.bind_text(*cmd);
            if !wanted(ui, label, &keys, sec) {
                continue;
            }
            rows += 1;
            if *sec != section && !searching {
                section_head(ui, sec, section.is_empty());
                section = sec;
            }
            let custom = self.data.prefs.keybinds.contains_key(cmd.id());
            let mine = parse_bind(&keys);
            let clash: Vec<&str> =
                all.iter().filter(|(c, _, bs)| c != cmd && bs.iter().any(|b| mine.contains(b))).map(|(_, l, _)| *l).collect();
            let recording = self.recording == Some(*cmd);
            let (mut start, mut reset, mut clear) = (false, false, false);
            // the keys first; the name is cut short if they leave too little room
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 44.0), Layout::right_to_left(Align::Center), |ui| {
                {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // reset and clear sit to the right of the keys
                    if custom {
                        let r = ui.add(egui::Button::new(RichText::new(icons::UNDO).size(theme::size::BODY).color(muted)).frame(false));
                        reset = r.on_hover_text(trf("Back to the default ({})", &[&if default.is_empty() { "none" } else { default }])).clicked();
                    }
                    if !keys.is_empty() {
                        let r = ui.add(egui::Button::new(RichText::new(icons::X).size(theme::size::BODY).color(muted)).frame(false));
                        clear = r.on_hover_text(tr("Remove this shortcut")).clicked();
                    }
                    let inner = ui.scope(|ui| {
                        if recording {
                            chip(ui, tr("Press keys…"), mix(side, accent, 0.35), text);
                        } else if keys.is_empty() {
                            chip(ui, tr("Not set"), mix(side, text, 0.06), muted);
                        } else {
                            chips(ui, &keys);
                        }
                    });
                    let r = ui.interact(inner.response.rect, ui.scope_id().with(("bind", cmd.id())), Sense::click());
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    start = r.on_hover_text(tr("Click, then press the new keys")).clicked();
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(*label).size(theme::size::TITLE).color(mix(muted, text, 0.6))).truncate());
                    if !clash.is_empty() {
                        ui.label(RichText::new(icons::INFO).size(theme::size::BODY).color(theme::DANGER)).on_hover_text(trf("Also used by: {}", &[&clash.join(", ")]));
                    }
                });
            });
            if start {
                self.recording = if recording { None } else { Some(*cmd) };
            }
            if reset {
                self.data.prefs.keybinds.remove(cmd.id());
            }
            if clear {
                self.data.prefs.keybinds.insert(cmd.id().to_string(), String::new());
            }
        }
        let mut fixed_head = false;
        for (label, keys) in [
            (tr("Paste here"), tr("Right-click")),
            (tr("Discard the creation of an event"), tr("Esc")),
            (tr("Save changes"), tr("Ctrl+Enter")),
            (tr("Zoom the hours"), tr("Ctrl+Wheel")),
            (tr("Move sideways by days"), tr("Shift+Wheel")),
            (tr("Move back or forward (a day in day views)"), tr("Mouse 4 / Mouse 5")),
        ] {
            if !wanted(ui, label, keys, tr("Fixed")) {
                continue;
            }
            rows += 1;
            if !fixed_head && !searching {
                section_head(ui, tr("Fixed"), section.is_empty());
                fixed_head = true;
            }
            fixed_row(ui, label, keys);
        }
        if rows == 0 && !searching {
            ui.add_space(30.0);
            ui.label(RichText::new(trf("No shortcuts match \"{}\"", &[&self.shortcut_query.trim()])).size(theme::size::LARGE).color(muted));
        }
        if self.recording.is_some() {
            ctx.request_repaint();
        }
    }

    fn page_info(&mut self, ui: &mut egui::Ui) {
        if finding(ui) {
            let words = ["about", "kalendar", "version", env!("CARGO_PKG_VERSION"), "data", "file", "folder", "path", "location", "config", "json", "privacy"];
            page_result(ui, Page::Info, tr("Version and data files"), tr("Where your calendars, tasks and settings are kept"), &words);
            return;
        }
        let muted = self.pal.muted;
        heading(ui, tr("Kalendar"));
        ui.label(RichText::new(trf("Version {}", &[&env!("CARGO_PKG_VERSION")])).size(theme::size::LARGE));
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Your calendars and tasks are kept on this computer, in:")).size(theme::size::UI).color(muted));
        ui.label(RichText::new(data_path().display().to_string()).size(theme::size::UI).monospace());
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Your settings and shortcuts are kept in:")).size(theme::size::UI).color(muted));
        ui.label(RichText::new(config_path().display().to_string()).size(theme::size::UI).monospace());
        ui.add_space(8.0);
        ui.label(
            RichText::new(tr("Account passwords and Proton Calendar links are kept in your desktop's keyring, not in these files. Links of other subscribed calendars are stored in data.json as plain text. Without a running keyring a Proton link stays in data.json too; it contains the keys that give access to that calendar, so keep that file private."))
                .size(theme::size::UI)
                .color(muted),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_every_word_in_any_order() {
        let w = search_words("  GRID  time ");
        assert!(search_hit(&w, &["Time grid start", ""]));
        assert!(!search_hit(&w, &["Time format", ""]));
        assert!(search_hit(&search_words("ctrl+t"), &["Create a new task", "Events and tasks", "Ctrl+T"]));
        assert!(search_hit(&search_words(""), &["anything"]));
    }
}

const GUIDE_GOOGLE: &str = "https://github.com/kud3n013/kalendar/blob/main/docs/connect-google-and-microsoft.md#google-calendar";
const GUIDE_MICROSOFT: &str = "https://github.com/kud3n013/kalendar/blob/main/docs/connect-google-and-microsoft.md#microsoft-365-and-outlookcom";

/// A row of small links to the pages a sign-in has to be set up on.
fn setup_links(ui: &mut egui::Ui, links: &[(&str, &str)]) {
    ui.horizontal_wrapped(|ui| {
        for (i, (name, url)) in links.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("·").size(theme::size::CAPTION).weak());
            }
            ui.hyperlink_to(RichText::new(*name).size(theme::size::CAPTION), *url);
        }
    });
    ui.add_space(2.0);
}
