//! The task panel (left or right) and the bar above the calendar.

use crate::strings::{tr, trf};
use crate::ids;
use crate::motion;
use crate::theme;
use crate::icons;
use crate::model::*;
use crate::views::{context_menu, Menu};
use crate::{fill_squircle, mix, rgb, App, Cmd, Shelf};
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke,
};


/// A flat button that only shows its label.
fn flat(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(label).frame(false))
}

/// The round tick box in front of a task. egui draws checkboxes as rounded
/// squares, so this one is painted by hand.
fn round_check(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let s = ui.spacing().icon_width + 2.0;
    let (r, mut resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let v = ui.style().interact(&resp);
    let c = r.center();
    ui.painter().circle(c, s * 0.5, v.bg_fill, v.bg_stroke);
    if *on {
        let tick = vec![c + vec2(-0.26 * s, 0.0), c + vec2(-0.06 * s, 0.2 * s), c + vec2(0.27 * s, -0.22 * s)];
        ui.painter().add(egui::Shape::line(tick, v.fg_stroke));
    }
    resp
}

/// The rail's shelf a panel section stands for, from the section's key.
fn shelf_of(key: &str) -> Option<Shelf> {
    match key {
        "done" => Some(Shelf::Done),
        _ => match ["overdue", "today", "tomorrow", "soon"].into_iter().find(|k| *k == key) {
            Some(k) => Some(Shelf::Due(k)),
            None => key.strip_prefix("list")?.parse().ok().map(Shelf::List),
        },
    }
}

/// A flat icon button: a Lucide glyph on a squircle that glows on hover.
/// `c` is (side, muted, text) from the palette.
pub fn icon_btn(ui: &mut egui::Ui, glyph: &str, size: f32, tip: &str, c: (Color32, Color32, Color32)) -> egui::Response {
    let (side, muted, text) = c;
    let (r, resp) = ui.allocate_exact_size(vec2(size + 12.0, size + 12.0), Sense::click());
    let glow = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), motion::HOVER.secs());
    if glow > 0.0 {
        fill_squircle(ui.painter(), r, theme::RADIUS, mix(side, text, 0.08 * glow));
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size), mix(muted, text, 0.6 * glow));
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Paints one of the small buttons that show on a hovered row: a list's
/// "+" and three dots, a task's three dots and bin. `glow` is how far the
/// row is lit; the button gets its own squircle while it is hovered or
/// `held` (its menu is open). The squircle lets the row's colour through.
#[allow(clippy::too_many_arguments)]
fn row_btn(ui: &egui::Ui, r: Rect, resp: &egui::Response, glyph: &str, size: f32, glow: f32, held: bool, hot: Color32, (muted, text): (Color32, Color32)) {
    let on = resp.hovered() || held;
    if glow <= 0.0 && !on {
        return;
    }
    if on {
        fill_squircle(ui.painter(), r, theme::RADIUS, text.gamma_multiply(0.09));
    }
    let c = if on { hot } else { muted.gamma_multiply(glow.max(0.0)) };
    ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size), c);
}

/// How wide the folded task panel is, and its list buttons.
/// A group pinned above the task lists: key, name, icon, colour, tasks.
type DueGroup = (&'static str, &'static str, &'static str, Color32, Vec<u64>);

const RAIL_W: f32 = 52.0;
const RAIL_ICON: f32 = 34.0;
/// How wide a list hanging beside the rail is.
const FLYER_W: f32 = 300.0;
/// How long a flyer stays after the pointer leaves it.
const FLYER_LINGER_S: f64 = 0.3;

pub fn due_label(d: NaiveDate, today: NaiveDate) -> String {
    match (d - today).num_days() {
        -1 => tr("Yesterday").into(),
        0 => tr("Today").into(),
        1 => tr("Tomorrow").into(),
        _ => d.format("%b %-d").to_string(),
    }
}

impl App {
    /// The small month for jumping to a date.
    fn ui_quick_nav(&mut self, ctx: &egui::Context, at: egui::Pos2) {
        let a = ctx.animate_bool_with_time(ids::quick_nav(), self.quick_nav, motion::NORMAL.secs());
        if a <= 0.0 {
            return;
        }
        let today = Local::now().date_naive();
        let first = self.anchor.with_day(1).unwrap();
        let start = self.data.prefs.week_start(first);
        let (muted, text, accent) = (self.pal.muted, self.pal.text, self.pal.accent);
        let mut go: Option<NaiveDate> = None;
        let mut step = 0;
        egui::Area::new(ids::quick_nav_area())
            .fixed_pos(at + vec2(0.0, (1.0 - a) * 12.0))
            .constrain(true)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(a);
                egui::Frame::popup(ui.style()).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(7.0 * 30.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(first.format("%B %Y").to_string()).font(theme::bold(theme::size::BODY)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if flat(ui, RichText::new(icons::CHEVRON_RIGHT).size(theme::size::BODY)).clicked() {
                                step = 1;
                            }
                            if flat(ui, RichText::new(icons::CHEVRON_LEFT).size(theme::size::BODY)).clicked() {
                                step = -1;
                            }
                        });
                    });
                    egui::Grid::new("qn").spacing([0.0, 0.0]).min_col_width(30.0).show(ui, |ui| {
                        for i in 0..7 {
                            let d = start + Duration::days(i);
                            ui.vertical_centered(|ui| ui.label(RichText::new(&d.format("%a").to_string()[..2]).size(theme::size::MICRO).color(muted)));
                        }
                        ui.end_row();
                        for i in 0..42 {
                            let d = start + Duration::days(i);
                            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
                            let glow = ctx.animate_bool_with_time(ids::quick_nav_day(i), resp.hovered(), motion::HOVER.secs());
                            if d == today {
                                ui.painter().circle_filled(r.center(), 11.0, theme::DANGER);
                            } else if glow > 0.0 {
                                ui.painter().circle_filled(r.center(), 11.0, mix(self.pal.side, text, 0.14 * glow));
                            }
                            let c = if d == today { Color32::WHITE } else if d.month() != first.month() { muted } else { text };
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(theme::size::SMALL), c);
                            if self.visible_days().contains(&d) && d != today {
                                ui.painter().circle_filled(r.center_bottom() + vec2(0.0, -2.0), 1.5, accent);
                            }
                            if resp.clicked() {
                                go = Some(d);
                            }
                            if i % 7 == 6 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
        if step != 0 {
            let m = chrono::Months::new(1);
            self.anchor = if step > 0 { first.checked_add_months(m) } else { first.checked_sub_months(m) }.unwrap_or(first);
        }
        if let Some(d) = go {
            self.anchor = d;
        }
    }

    /// The menu that shows and hides calendars.
    fn calendars_menu(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        let ctx = &ui.ctx().clone();
        ui.menu_button(RichText::new(icons::CALENDAR).size(theme::size::LARGE).color(muted), |ui| {
            ui.label(RichText::new(tr("Calendars")).color(muted));
            let mut changed = false;
            for cal in self.data.calendars.iter_mut() {
                changed |= ui.checkbox(&mut cal.visible, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color))).changed();
            }
            if changed {
                self.commit();
            }
            ui.separator();
            if ui.button(tr("Subscribe to a calendar…")).clicked() {
                self.open_settings(crate::settings::Page::Calendars);
                ui.close();
            }
            if ui.button(tr("Refresh")).clicked() {
                self.run(ctx, Cmd::Refresh);
                ui.close();
            }
        })
        .response
        .on_hover_text(tr("Calendars"));
    }

    /// The menu that picks the view: day, week, month and so on.
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        let mut v = self.view;
        ui.menu_button(RichText::new(format!("{}  {}", v.label(), icons::CHEVRON_DOWN)).size(theme::size::CAPTION).color(muted), |ui| {
            // just wide enough for the names and their keys
            ui.set_width(130.0);
            for (o, key) in [
                (View::Days(1), "D"),
                (View::Week, "W"),
                (View::Month, "M"),
                (View::Agenda, "A"),
                (View::TwoWeeks, "X"),
            ] {
                ui.horizontal(|ui| {
                    if ui.selectable_label(v == o, o.label()).clicked() {
                        v = o;
                        ui.close();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(key).monospace().size(theme::size::MICRO).color(muted));
                    });
                });
            }
            ui.separator();
            ui.menu_button(tr("Number of days"), |ui| {
                for n in 2..=9u8 {
                    if ui.selectable_label(v == View::Days(n), trf("{} days", &[&n])).clicked() {
                        v = View::Days(n);
                        ui.close();
                    }
                }
            });
        })
        .response
        .on_hover_text(tr("Calendar view"));
        self.view = v;
    }

    // ------------------------------------------------------------ top bar

    pub fn ui_toolbar(&mut self, ui: &mut egui::Ui) {
        let (muted, text) = (self.pal.muted, self.pal.text);
        let mut go_at = egui::Pos2::ZERO;
        egui::Panel::top("bar")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(self.pal.bg).inner_margin(Margin { left: 24, right: 16, top: 12, bottom: 6 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let days = self.visible_days();
                    let (first, last) = match self.view {
                        View::Month => (self.anchor, self.anchor),
                        _ => (days.first().copied().unwrap_or(self.anchor), days.last().copied().unwrap_or(self.anchor)),
                    };
                    let month = if first.month() != last.month() {
                        format!("{} – {}", first.format("%b"), last.format("%b"))
                    } else {
                        first.format("%B").to_string()
                    };
                    let narrow = ui.available_width() < 900.0;
                    let tiny = ui.available_width() < 760.0;
                    ui.spacing_mut().item_spacing.x = 7.0;
                    let r = icon_btn(ui, icons::CALENDAR_DAYS, 17.0, tr("Go to a date"), (self.pal.bg, muted, text));
                    if r.clicked() {
                        self.quick_nav = !self.quick_nav;
                    }
                    go_at = r.rect.left_bottom() + vec2(0.0, 6.0);
                    ui.label(RichText::new(month.to_lowercase()).font(theme::bold(if narrow { 20.0 } else { 26.0 })).color(text));
                    if !tiny {
                        ui.label(RichText::new(last.year().to_string()).size(if narrow { 20.0 } else { 26.0 }).color(muted));
                    }
                    // the view menu sits in the far corner
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.view_menu(ui);
                        self.calendars_menu(ui);
                    });
                });
            });
        let ctx = ui.ctx().clone();
        self.ui_quick_nav(&ctx, go_at);
    }

    // --------------------------------------------------------- task panel

    /// Opens the new-task bar (Ctrl+T), adding to `list` (0 = Inbox).
    pub fn open_new_task(&mut self, list: u64) {
        if list != self.new_task_list {
            self.new_task_text.clear();
        }
        self.new_task_list = list;
        self.new_task_open = true;
        self.new_task_focus = true;
    }

    /// Adds a task typed into a quick field.
    /// A typed time puts it straight onto the calendar.
    pub fn add_task(&mut self, ctx: &egui::Context, text: &str, list: u64) {
        let now = Local::now().naive_local();
        let q = parse_quick(text, now.date(), &self.data.prefs.quick);
        if q.title.trim().is_empty() {
            return;
        }
        self.snapshot();
        let mut list = list;
        if let Some(name) = &q.list_name {
            let found = self.data.lists.iter().find(|l| l.name.to_lowercase() == name.to_lowercase()).map(|l| l.id);
            list = match found {
                Some(id) => id,
                None => self.new_list(name),
            };
        }
        let id = self.data.new_id();
        let start = q.start(now);
        self.data.tasks.insert(
            0,
            Task {
                id,
                title: q.title,
                repeat: q.repeat,
                due: q.due,
                priority: q.priority,
                duration_min: q.duration_min,
                tags: q.tags,
                list,
                ..Default::default()
            },
        );
        if let Some(start) = start {
            self.place_task(ctx, id, start);
        }
        self.commit();
    }

    pub fn new_list(&mut self, name: &str) -> u64 {
        let id = self.data.new_id();
        let color = EVENT_COLORS[self.data.lists.len() % EVENT_COLORS.len()].0;
        self.data.lists.push(TaskList { id, name: name.to_string(), color, ..Default::default() });
        id
    }

    pub fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let today = Local::now().date_naive();
        let side = self.pal.side;
        let c = (self.pal.side, self.pal.muted, self.pal.text);
        let open = self.data.prefs.sidebar_open;
        let right = self.data.prefs.sidebar_side == PanelSide::Right;
        // closed, it folds to a strip of list icons; in between, the panel
        // grows out of the strip or shrinks back into it
        let t = motion::ease_in_out(ctx.animate_bool_with_time(ids::morph(), open, motion::LONG.secs()));
        if t <= 0.0 {
            self.ui_rail(ui, ctx, right, c);
            return;
        }
        self.flyer = None;
        if t < 1.0 {
            self.ui_morph(ui, ctx, right, today, t, c);
            return;
        }
        let panel = if right { egui::Panel::right("tasks") } else { egui::Panel::left("tasks") };
        let mut still_open = open;
        let shown = panel
            .resizable(true)
            .default_size(self.data.prefs.sidebar_w)
            .size_range(280.0..=560.0)
            .show_separator_line(false)
            .drag_to_open(false)
            .frame(egui::Frame::new().fill(side))
            .show_collapsible(ui, &mut still_open, |ui| self.panel_body(ui, ctx, right, today, c));
        match shown {
            Some(s) => {
                self.morph.width = s.response.rect.width();
                self.measure(s.response.rect, right, false);
            }
            None => self.morph.seen.clear(),
        }
        // dragging the edge past the minimum closes the panel, like the button
        if still_open != open {
            self.run(ctx, Cmd::ToggleTasks);
        }
    }

    /// The panel's insides: the bottom row under the lists.
    fn panel_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, today: NaiveDate, c: (Color32, Color32, Color32)) {
        let side = self.pal.side;
        egui::Panel::bottom(ids::panel_bottom())
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 10, right: 10, top: 4, bottom: 8 }))
            .show(ui, |ui| self.panel_bottom(ui, ctx, right, c));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 14, right: 12, top: 10, bottom: 0 }))
            .show(ui, |ui| self.panel_main(ui, ctx, right, today, c));
    }

    /// Keeps where the icons seen this frame sit, from the outer edge and
    /// top of `outer`, as the rail's places or the panel's.
    fn measure(&mut self, outer: Rect, right: bool, rail: bool) {
        let rel = |p: egui::Pos2| vec2(if right { outer.right() - p.x } else { p.x - outer.left() }, p.y - outer.top());
        let seen = std::mem::take(&mut self.morph.seen);
        let map = if rail { &mut self.morph.rail } else { &mut self.morph.panel };
        map.clear();
        for (shelf, p) in seen {
            map.insert(shelf, rel(p));
        }
        if !rail {
            // the task list noted its visible part on the screen
            if let Some(v) = self.morph.view {
                self.morph.view = Some(egui::Rangef::new(v.min - outer.top(), v.max - outer.top()));
            }
        }
    }

    /// The panel part way open, `t` from 0 (the rail) to 1. It widens from
    /// the rail's width, uncovering the full panel as it fades in while the
    /// rail's tools fade out. The menu button stays where it is in both,
    /// and each list's icon glides from its button to its heading.
    fn ui_morph(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, today: NaiveDate, t: f32, c: (Color32, Color32, Color32)) {
        let (side, text) = (self.pal.side, self.pal.text);
        let full_w = if self.morph.width > 0.0 { self.morph.width } else { self.data.prefs.sidebar_w };
        let w = egui::lerp(RAIL_W..=full_w, t);
        let panel = if right { egui::Panel::right(ids::morph_panel()) } else { egui::Panel::left(ids::morph_panel()) };
        panel.resizable(false).exact_size(w).show_separator_line(false).frame(egui::Frame::new().fill(side)).show(ui, |ui| {
            let outer = ui.max_rect();
            let clip = outer.intersect(ui.clip_rect());
            // a strip of `width` along the outer edge, from `top` down
            let strip = |width: f32, top: f32| {
                let x = if right { outer.right() - width } else { outer.left() };
                Rect::from_min_max(pos2(x, outer.top() + top), pos2(x + width, outer.bottom()))
            };
            // the panel at its full width, uncovered as this one widens
            self.morph.moving = true;
            let mut full = ui.new_child(egui::UiBuilder::new().max_rect(strip(full_w, 0.0)).id_salt("morph_full"));
            full.set_clip_rect(clip);
            full.set_opacity(t);
            self.panel_body(&mut full, ctx, right, today, c);
            self.morph.moving = false;
            self.measure(outer, right, false);
            // the rail's tools fading the other way, and an unseen copy of
            // its buttons, to learn where they sit
            let mut col = ui.new_child(egui::UiBuilder::new().max_rect(strip(RAIL_W, 10.0)).id_salt("morph_rail"));
            col.set_clip_rect(clip);
            col.set_opacity(1.0 - t);
            self.rail_tools_panel(&mut col, ctx, c, None);
            let shelves = self.shelves(today);
            col.scope_builder(egui::UiBuilder::new().invisible(), |ui| {
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
                    self.rail_icons(ui, ctx, c, &shelves);
                });
            });
            self.measure(outer, right, true);
            // the menu button, in the same place on the rail and the panel
            let at = strip(RAIL_W, 10.0).center_top() + vec2(0.0, 14.5);
            let mut b = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(at, vec2(29.0, 29.0))).id_salt("morph_menu"));
            if icon_btn(&mut b, icons::MENU, 17.0, &self.tip(tr("Show the task panel"), Cmd::ToggleTasks), c).clicked() {
                self.run(ctx, Cmd::ToggleTasks);
            }
            // the gliding icons
            let p = ui.painter().with_clip_rect(clip);
            let abs = |v: egui::Vec2| pos2(if right { outer.right() - v.x } else { outer.left() + v.x }, outer.top() + v.y);
            let view = self.morph.view;
            for (shelf, glyph, color) in &shelves {
                let from = self.morph.rail.get(shelf).copied();
                let to = self.morph.panel.get(shelf).copied();
                // an icon with no place on one side fades there instead
                let (at, alpha) = match (from, to) {
                    (Some(a), Some(b)) => (a + (b - a) * t, if view.is_none_or(|v| v.contains(b.y)) { 1.0 } else { 1.0 - t }),
                    (Some(a), None) => (a, 1.0 - t),
                    (None, Some(b)) => (b, t),
                    (None, None) => continue,
                };
                let at = abs(at);
                let soft = mix(*color, text, 0.25);
                let tile = Rect::from_center_size(at, vec2(RAIL_ICON, RAIL_ICON));
                fill_squircle(&p, tile, theme::RADIUS, mix(side, soft, 0.04).gamma_multiply(alpha * (1.0 - t)));
                let color = color.gamma_multiply(alpha);
                if glyph.is_empty() {
                    p.circle_stroke(at, 5.5, Stroke::new(2.0, color));
                } else {
                    let size = egui::lerp(theme::size::LARGE..=theme::size::UI, t);
                    p.text(at, Align2::CENTER_CENTER, glyph, FontId::proportional(size), color);
                }
            }
        });
        ctx.request_repaint();
    }

    /// The row of buttons at the bottom of the panel: the new-list button in
    /// the inner corner, preferences in the outer one.
    fn panel_bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, c: (Color32, Color32, Color32)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let tools = |ui: &mut egui::Ui, app: &mut App| {
                if icon_btn(ui, icons::SETTINGS, 16.0, &app.tip(tr("Settings"), Cmd::Prefs), c).clicked() {
                    app.open_settings(crate::settings::Page::General);
                }
                if icon_btn(ui, icons::HELP, 16.0, &app.tip(tr("Shortcuts"), Cmd::Shortcuts), c).clicked() {
                    app.open_settings(crate::settings::Page::Shortcuts);
                }
                if icon_btn(ui, icons::TERMINAL, 15.0, &app.tip(tr("Command bar"), Cmd::CommandBar), c).clicked() {
                    app.run(ctx, Cmd::CommandBar);
                }
                if icon_btn(ui, icons::SEARCH, 15.0, &app.tip(tr("Search"), Cmd::Search), c).clicked() {
                    app.run(ctx, Cmd::Search);
                }
                if icon_btn(ui, icons::SHUFFLE, 15.0, tr("Plan my tasks: fit open tasks into free working time"), c).clicked() {
                    app.run(ctx, Cmd::Plan);
                }
                // undo appears while there is something to take back
                let a = ctx.animate_bool_with_time(ids::undo_button(), !app.undo.is_empty(), motion::NORMAL.secs());
                if a > 0.0 {
                    ui.scope(|ui| {
                        ui.set_opacity(a);
                        if icon_btn(ui, icons::UNDO, 16.0, &app.tip(tr("Undo"), Cmd::Undo), c).clicked() {
                            app.do_undo(ctx);
                        }
                    });
                }
            };
            let new_list = |ui: &mut egui::Ui, app: &mut App| {
                if icon_btn(ui, icons::PLUS, 17.0, tr("New task list"), c).clicked() {
                    app.new_list_text = Some((String::new(), true));
                }
            };
            if right {
                tools(ui, self);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| new_list(ui, self));
            } else {
                new_list(ui, self);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| tools(ui, self));
            }
        });
    }

    /// Everything above the bottom row: navigation, the view controls and
    /// the task lists.
    fn panel_main(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, today: NaiveDate, c: (Color32, Color32, Color32)) {
        let (muted, border) = (self.pal.muted, self.pal.border);
        // collapse toggle in the outer corner, navigation in the inner one:
        // refresh, then back, today and forward
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let nav = |ui: &mut egui::Ui, app: &mut App, flipped: bool| app.nav_row(ui, ctx, c, flipped);
            if right {
                nav(ui, self, false);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_btn(ui, icons::MENU, 17.0, &self.tip(tr("Hide the task panel"), Cmd::ToggleTasks), c).clicked() {
                        self.run(ctx, Cmd::ToggleTasks);
                    }
                });
            } else {
                // centred over the rail's own, so it stays put as the panel
                // folds; drawn in a child, as sticking out past the panel's
                // margin would make the panel widen itself every frame
                let (slot, _) = ui.allocate_exact_size(vec2(29.0, 29.0), Sense::hover());
                let mut b = ui.new_child(egui::UiBuilder::new().max_rect(slot.translate(vec2(-2.5, 0.0))).id_salt("panel_menu"));
                if icon_btn(&mut b, icons::MENU, 17.0, &self.tip(tr("Hide the task panel"), Cmd::ToggleTasks), c).clicked() {
                    self.run(ctx, Cmd::ToggleTasks);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| nav(ui, self, true));
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Tasks")).font(theme::bold(theme::size::HEADING)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.menu_button(RichText::new(icons::SLIDERS).size(theme::size::UI).color(muted), |ui| {
                    ui.set_min_width(220.0);
                    let p = &mut self.data.prefs;
                    ui.label(RichText::new(tr("Pinned lists")).color(muted).size(theme::size::SMALL));
                    let mut ch = ui.checkbox(&mut p.show_due_today, tr("Due today")).changed();
                    ch |= ui.checkbox(&mut p.show_due_tomorrow, tr("Due tomorrow")).changed();
                    ch |= ui.checkbox(&mut p.show_due_soon, tr("Due soon")).changed();
                    ui.separator();
                    ch |= ui.checkbox(&mut p.show_scheduled_tasks, tr("Show scheduled tasks")).changed();
                    ch |= ui.checkbox(&mut p.due_on_calendar, tr("Due dates on the calendar")).changed();
                    ui.separator();
                    ui.label(RichText::new(tr("Sort by")).color(muted).size(theme::size::SMALL));
                    for (i, n) in [tr("My order"), tr("Due date"), tr("Importance")].iter().enumerate() {
                        ui.radio_value(&mut self.sort_by, i, *n);
                    }
                    if ch {
                        self.commit();
                    }
                })
                .response
                .on_hover_text(tr("List options"));
                // fold every list, or open them all once all are folded
                let keys = self.section_keys(today);
                let all_shut = keys.iter().all(|k| self.collapsed.contains(k));
                let (glyph, tip) = if all_shut { (icons::CHEVRONS_UP_DOWN, tr("Expand all lists")) } else { (icons::CHEVRONS_DOWN_UP, tr("Collapse all lists")) };
                if icon_btn(ui, glyph, 16.0, tip, c).clicked() {
                    if all_shut {
                        for k in &keys {
                            self.collapsed.remove(k);
                        }
                    } else {
                        self.collapsed.extend(keys);
                    }
                }
            });
        });
        // tags as a quick filter
        let mut tags: Vec<String> = self.data.tasks.iter().filter(|t| !t.done).flat_map(|t| t.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        if !tags.is_empty() {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for t in &tags {
                    let on = self.filter_tag.as_deref() == Some(t.as_str());
                    if ui.selectable_label(on, RichText::new(format!("#{t}")).size(theme::size::TINY).color(if on { Color32::WHITE } else { muted })).clicked() {
                        self.filter_tag = if on { None } else { Some(t.clone()) };
                    }
                }
            });
        }
        ui.add_space(8.0);

                // which tasks go where
                let p = self.data.prefs.clone();
                let tag = self.filter_tag.clone();
                let groups = self.due_groups(today, tag.as_deref());
                let open_tasks = self.open_tasks(tag.as_deref());
                let ids = |f: &dyn Fn(&Task) -> bool| -> Vec<u64> { open_tasks.iter().filter(|t| f(t)).map(|t| t.id).collect() };
                let visible_in_list = |t: &Task| p.show_scheduled_tasks || !self.data.task_is_scheduled(t.id);
                let mut lists: Vec<(u64, String, String, Color32, Vec<u64>)> = Vec::new();
                for id in std::iter::once(0).chain(self.data.lists.iter().map(|l| l.id)) {
                    let (name, color, icon) = self.data.list_look(id);
                    let name = if id == 0 { self.data.prefs.inbox_name() } else if name.trim().is_empty() { tr("Untitled list").to_string() } else { name };
                    lists.push((id, name, icon, rgb(color), ids(&|t| t.list == id && visible_in_list(t))));
                }
                let done_ids = self.done_ids();

                // the bar gets its own lane beside the lists instead of
                // floating over them, with room for their washes, which
                // reach 6 points past the content
                ui.spacing_mut().scroll = egui::style::ScrollStyle { bar_inner_margin: 9.0, ..egui::style::ScrollStyle::solid() };
                let mut area = egui::ScrollArea::vertical().id_salt("tasklist").auto_shrink([false, false]);
                if self.morph.moving {
                    // the moving copy shows the list as far down as the panel had it
                    area = area.vertical_scroll_offset(self.morph.scroll);
                }
                let out = area.show(ui, |ui| {
                    // room for the first list's border, which reaches above its heading
                    ui.add_space(6.0);
                    for (key, name, icon, color, items) in &groups {
                        self.section(ui, ctx, key, name, Some((icon, *color)), *color, items, None);
                    }
                    if !groups.is_empty() {
                        ui.add_space(4.0);
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                        ui.painter().rect_filled(r, 0.0, border);
                        ui.add_space(8.0);
                    }
                    for (id, name, icon, color, items) in &lists {
                        let glyph = Some((icon.as_str(), *color)).filter(|g| !g.0.is_empty());
                        self.section(ui, ctx, &format!("list{id}"), name, glyph, *color, items, Some(*id));
                    }
                    if !done_ids.is_empty() {
                        self.section(ui, ctx, "done", tr("Completed"), Some((icons::CHECK, muted)), muted, &done_ids, None);
                    }
                    ui.add_space(10.0);
                    if let Some((mut name, fresh)) = self.new_list_text.take() {
                        let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("List name")).desired_width(f32::INFINITY));
                        if fresh {
                            r.request_focus();
                            ui.scroll_to_rect(r.rect, None);
                        }
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if r.lost_focus() && enter && !name.trim().is_empty() {
                            self.snapshot();
                            self.new_list(name.trim());
                            self.commit();
                        } else if fresh || r.has_focus() {
                            // a click anywhere else, or Escape, drops it
                            self.new_list_text = Some((name, false));
                        }
                    }
                    ui.add_space(20.0);
                });
        if !self.morph.moving {
            self.morph.scroll = out.state.clamped_offset().y;
        }
        self.morph.view = Some(out.inner_rect.y_range());
        self.ui_list_edit(ctx);
    }

    /// What a list's menu offers: plan its tasks, change how it looks, or
    /// remove it.
    fn list_menu(&mut self, ui: &mut egui::Ui, lid: u64, at: egui::Pos2) {
        let mut m = Menu::start(ui);
        let open = self.data.tasks.iter().any(|t| t.list == lid && !t.done);
        let b = m.item_if(ui, open, icons::SHUFFLE, tr("Plan this list"), false);
        if b.on_hover_text(tr("Plan the open tasks of this list only")).on_disabled_hover_text(tr("No open tasks in this list")).clicked() {
            let ctx = ui.ctx().clone();
            self.plan(&ctx, Some(lid));
            ui.close();
        }
        m.line(ui);
        if m.item(ui, icons::PEN, tr("Edit"), false).clicked() {
            self.list_edit = Some(crate::ListEdit { id: lid, at, fresh: true, snapped: false });
            ui.close();
        }
        if lid != 0 {
            let synced = self.data.lists.iter().any(|l| l.id == lid && l.account.is_some());
            m.line(ui);
            let b = m.item_if(ui, !synced, icons::TRASH, tr("Remove"), true);
            let b = b.on_hover_text(tr("Its tasks go to the Inbox")).on_disabled_hover_text(tr("This list comes from an account. It goes when it is switched off under Settings, Calendars."));
            if b.clicked() {
                self.snapshot();
                self.data.remove_own_list(lid);
                self.commit();
                ui.close();
            }
        }
        m.end(ui);
    }

    /// The little window where a list's name, colour and icon are changed.
    fn ui_list_edit(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.list_edit.take() else { return };
        if ed.id != 0 && !self.data.lists.iter().any(|l| l.id == ed.id) {
            return;
        }
        let out = egui::Area::new(ids::list_edit()).order(egui::Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(ed.at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(236.0);
                self.list_fields(ui, ed.id, true, ed.fresh, &mut ed.snapped)
            })
            .inner
        });
        if out.inner {
            self.commit();
        }
        let done = ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
        if !done && (ed.fresh || !out.response.clicked_elsewhere()) {
            ed.fresh = false;
            self.list_edit = Some(ed);
        }
    }

    /// The fields that change how a list looks. True when one changed;
    /// the first change is kept for undo unless `snapped` says it was.
    pub fn list_fields(&mut self, ui: &mut egui::Ui, lid: u64, with_name: bool, focus: bool, snapped: &mut bool) -> bool {
        let (side, text, muted) = (self.pal.side, self.pal.text, self.pal.muted);
        let before = self.data.list_look(lid);
        let mut look = before.clone();
        if with_name {
            let r = ui.add(egui::TextEdit::singleline(&mut look.0).hint_text(if lid == 0 { tr("Inbox") } else { tr("List name") }).desired_width(f32::INFINITY));
            if focus {
                r.request_focus();
            }
            ui.add_space(6.0);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (col, cname) in EVENT_COLORS.iter() {
                let (cr, cresp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                ui.painter().circle_filled(cr.center(), 7.5, rgb(*col));
                if look.1 == *col {
                    ui.painter().circle_stroke(cr.center(), 10.0, Stroke::new(1.5, text));
                }
                if cresp.on_hover_text(*cname).clicked() {
                    look.1 = *col;
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for glyph in std::iter::once("").chain(icons::LIST_ICONS) {
                let (gr, gresp) = ui.allocate_exact_size(vec2(27.0, 27.0), Sense::click());
                let on = look.2 == glyph;
                if on || gresp.hovered() {
                    fill_squircle(ui.painter(), gr, theme::RADIUS, mix(side, text, if on { 0.16 } else { 0.08 }));
                }
                let c = if on { rgb(look.1) } else { muted };
                if glyph.is_empty() {
                    ui.painter().circle_stroke(gr.center(), 5.5, Stroke::new(2.0, c));
                } else {
                    ui.painter().text(gr.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(theme::size::UI), c);
                }
                if gresp.clicked() {
                    look.2 = glyph.to_string();
                }
            }
        });
        if look == before {
            return false;
        }
        if !*snapped {
            self.snapshot();
            *snapped = true;
        }
        self.data.set_list_look(lid, look);
        true
    }

    /// One collapsible group of tasks with its heading.
    #[allow(clippy::too_many_arguments)]
    fn section(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        key: &str,
        name: &str,
        icon: Option<(&str, Color32)>,
        color: Color32,
        items: &[u64],
        list: Option<u64>,
    ) {
        let (muted, text, side) = (self.pal.muted, self.pal.text, self.pal.side);
        let collapsed = self.collapsed.contains(key);
        // a task dragged from another list can be dropped anywhere on this
        // one: its heading or its tasks. The mark goes under both.
        let drop_task = self.drag_task.filter(|_| list.is_some_and(|l| l == 0 || self.data.lists.iter().any(|x| x.id == l && !x.read_only)));
        let drop_task = drop_task.filter(|t| self.data.task(*t).is_some_and(|t| Some(t.list) != list && !t.done));
        let drop_mark = ui.painter().add(egui::Shape::Noop);
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        // the menu button sits on the row, so the row itself is not "hovered" there
        let editing = list.is_some() && self.list_edit.as_ref().map(|e| e.id) == list;
        let glow = ctx.animate_bool_with_time(ids::section(key), resp.hovered() || ui.rect_contains_pointer(r) || editing, motion::HOVER.secs());
        // hover is a see-through wash of the list's own colour, so it sits
        // on the list's background instead of covering it with grey
        let tint = if list.is_some() { mix(color, text, 0.25) } else { text };
        if glow > 0.0 {
            fill_squircle(ui.painter(), r.expand2(vec2(6.0, 2.0)), theme::RADIUS, tint.gamma_multiply(0.07 * glow));
        }
        let p = ui.painter();
        let ic = pos2(r.left() + 9.0, r.center().y);
        let shelf = shelf_of(key);
        if let Some(s) = shelf {
            self.morph.seen.push((s, ic));
        }
        // while the panel moves, the icon is drawn gliding instead
        let hide = self.morph.moving && shelf.is_some();
        match icon {
            _ if hide => {}
            Some((g, c)) => {
                p.text(ic, Align2::CENTER_CENTER, g, FontId::proportional(theme::size::UI), c);
            }
            None => {
                p.circle_stroke(ic, 5.5, Stroke::new(2.0, color));
            }
        }
        let g = p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, name, theme::bold(theme::size::UI), text);
        let count = items.len().to_string();
        let pill = Rect::from_center_size(pos2(g.right() + 18.0, r.center().y), vec2(10.0 + 8.0 * count.len() as f32, 18.0));
        fill_squircle(p, pill, theme::RADIUS, mix(side, text, 0.10));
        p.text(pill.center(), Align2::CENTER_CENTER, count, FontId::proportional(theme::size::TINY), muted);
        // chevron turns as the group opens
        let turn = ctx.animate_bool_with_time(ids::section_chevron(key), !collapsed, motion::NORMAL.secs());
        let c = pos2(r.right() - 10.0, r.center().y);
        let ang = std::f32::consts::PI * (1.0 - 0.5 * turn); // left when closed, down when open
        let dir = vec2(ang.cos(), ang.sin());
        let perp = vec2(-dir.y, dir.x);
        let tip = c + dir * 3.0;
        p.line_segment([tip, tip - dir * 5.0 + perp * 5.0], Stroke::new(1.6, muted));
        p.line_segment([tip, tip - dir * 5.0 - perp * 5.0], Stroke::new(1.6, muted));
        let mut add_clicked = false;
        if let Some(lid) = list {
            let pr = Rect::from_center_size(pos2(r.right() - 64.0, r.center().y), vec2(24.0, 24.0));
            let add = ui.interact(pr, ids::list_add(lid), Sense::click()).on_hover_text(tr("New task"));
            row_btn(ui, pr, &add, icons::PLUS, theme::size::LARGE, glow, false, text, (muted, text));
            add_clicked = add.clicked();
        }
        // the Completed list puts its tasks away in one go
        if key == "done" || key == "flydone" {
            let pr = Rect::from_center_size(pos2(r.right() - 38.0, r.center().y), vec2(24.0, 24.0));
            let b = ui.interact(pr, ids::section(&format!("{key}_archive")), Sense::click()).on_hover_text(tr("Archive completed tasks"));
            row_btn(ui, pr, &b, icons::ARCHIVE, theme::size::LARGE, glow, false, text, (muted, text));
            if b.clicked() {
                let n = self.archive_done();
                self.say(ctx, trf("Archived {} tasks", &[&n]), true);
                return;
            }
        }
        if add_clicked {
            if let Some(l) = list {
                self.open_new_task(l);
            }
            self.collapsed.remove(key);
        } else if resp.clicked() {
            if collapsed {
                self.collapsed.remove(key);
            } else {
                self.collapsed.insert(key.to_string());
            }
        }
        // a list is renamed, recoloured or removed from its menu: the
        // three dots at the end of the row, or a right click
        if let Some(lid) = list {
            let at = r.right_bottom() + vec2(0.0, 2.0);
            let dr = Rect::from_center_size(pos2(r.right() - 38.0, r.center().y), vec2(24.0, 24.0));
            let dots = ui.interact(dr, ids::list_dots(lid), Sense::click()).on_hover_text(tr("Edit or remove"));
            let menu = egui::Popup::menu(&dots);
            let open = menu.is_open();
            row_btn(ui, dr, &dots, icons::ELLIPSIS, theme::size::LARGE, glow, open, text, (muted, text));
            menu.show(|ui| self.list_menu(ui, lid, at));
            context_menu(&resp, |ui| self.list_menu(ui, lid, at));
        }
        let a = ctx.animate_bool_with_time(ids::section_open(key), !collapsed, motion::NORMAL.secs());
        if a > 0.0 {
            ui.scope(|ui| {
                ui.set_opacity(a);
                for id in items {
                    self.task_row(ui, ctx, *id, key, tint);
                }
            });
            ui.add_space(6.0 * a);
        }
        // every list sits on a faint wash of its own colour, lit up while
        // a task from another list is held over it
        if let Some(lid) = list {
            let area = Rect::from_min_max(r.min, pos2(r.max.x, ui.cursor().min.y.max(r.max.y))).expand2(vec2(6.0, 2.0));
            let over = drop_task.is_some() && ui.rect_contains_pointer(area);
            let lit = ctx.animate_bool_with_time(ids::section_drop(key), over, motion::HOVER.secs());
            let soft = mix(color, text, 0.25);
            let fill = soft.gamma_multiply(0.05 + 0.09 * lit);
            let line = Stroke::new(1.0 + 0.5 * lit, mix(soft, color, lit).gamma_multiply(0.30 + 0.70 * lit));
            ui.painter().set(drop_mark, egui::Shape::convex_polygon(crate::squircle_points(area, theme::RADIUS), fill, line));
            ui.add_space(8.0);
            let Some(tid) = drop_task.filter(|_| over) else { return };
            if ctx.input(|i| i.pointer.any_released()) {
                self.move_to_list(tid, lid);
                self.drag_task = None;
                self.say(ctx, trf("Moved to {}", &[&name]), true);
            }
        }
    }

    /// Refresh, then back, today and forward; drawn right to left when
    /// `flipped`.
    fn nav_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, c: (Color32, Color32, Color32), flipped: bool) {
        let mut order: [(&str, f32, &str, Cmd); 4] = [
            (icons::REFRESH, 15.0, tr("Refresh"), Cmd::Refresh),
            (icons::CHEVRON_LEFT, 17.0, tr("Previous"), Cmd::Prev),
            (icons::CALENDAR_CHECK, 16.0, tr("Today"), Cmd::Today),
            (icons::CHEVRON_RIGHT, 17.0, tr("Next"), Cmd::Next),
        ];
        if flipped {
            order.reverse();
        }
        for (glyph, size, tip, cmd) in order {
            if icon_btn(ui, glyph, size, &self.tip(tip, cmd), c).clicked() {
                self.run(ctx, cmd);
            }
        }
    }

    /// The completed tasks not yet archived.
    fn done_ids(&self) -> Vec<u64> {
        self.data.tasks.iter().filter(|t| t.done && !t.archived).map(|t| t.id).collect()
    }

    /// The keys of every list the panel shows, to fold or unfold them all.
    fn section_keys(&self, today: NaiveDate) -> Vec<String> {
        let mut keys: Vec<String> = self.due_groups(today, self.filter_tag.as_deref()).iter().map(|g| g.0.to_string()).collect();
        keys.extend(std::iter::once(0).chain(self.data.lists.iter().map(|l| l.id)).map(|id| format!("list{id}")));
        if !self.done_ids().is_empty() {
            keys.push("done".into());
        }
        keys
    }

    /// Everything the rail has a button for, in order, with its icon and
    /// colour: the pinned groups, the lists, then Completed.
    fn shelves(&self, today: NaiveDate) -> Vec<(Shelf, String, Color32)> {
        let mut out: Vec<(Shelf, String, Color32)> =
            self.due_groups(today, None).into_iter().map(|(key, _, icon, color, _)| (Shelf::Due(key), icon.to_string(), color)).collect();
        for lid in std::iter::once(0).chain(self.data.lists.iter().map(|l| l.id)) {
            let (_, color, glyph) = self.data.list_look(lid);
            out.push((Shelf::List(lid), glyph, rgb(color)));
        }
        if !self.done_ids().is_empty() {
            out.push((Shelf::Done, icons::CHECK.to_string(), self.pal.muted));
        }
        out
    }

    /// The open tasks, only those tagged `tag` when given, in the chosen order.
    fn open_tasks(&self, tag: Option<&str>) -> Vec<&Task> {
        let mut open: Vec<&Task> = self.data.tasks.iter().filter(|t| !t.done && tag.is_none_or(|g| t.tags.iter().any(|x| x == g))).collect();
        match self.sort_by {
            1 => open.sort_by_key(|t| t.due.unwrap_or(NaiveDate::MAX)),
            2 => open.sort_by_key(|t| if t.priority == 0 { 10 } else { t.priority }),
            _ => {}
        }
        open
    }

    /// The groups pinned above the lists that have tasks: key, name, icon,
    /// colour and tasks. Overdue always shows; the others as set.
    fn due_groups(&self, today: NaiveDate, tag: Option<&str>) -> Vec<DueGroup> {
        let p = &self.data.prefs;
        let tomorrow = today + Duration::days(1);
        let soon_from = if p.show_due_tomorrow { 2 } else if p.show_due_today { 1 } else { 0 };
        let open = self.open_tasks(tag);
        let groups: [(&'static str, &'static str, &'static str, Color32, bool, &dyn Fn(NaiveDate) -> bool); 4] = [
            ("overdue", tr("Overdue"), icons::ALARM, theme::DANGER, true, &|d| d < today),
            ("today", tr("Due today"), icons::SUN, theme::WARNING, p.show_due_today, &|d| d == today),
            ("tomorrow", tr("Due tomorrow"), icons::ARROW_RIGHT, theme::WARNING, p.show_due_tomorrow, &|d| d == tomorrow),
            ("soon", tr("Due soon"), icons::CHEVRONS_RIGHT, theme::WARNING, p.show_due_soon, &|d| {
                (soon_from..=DUE_SOON_DAYS).contains(&(d - today).num_days())
            }),
        ];
        groups
            .into_iter()
            .filter(|g| g.4)
            .map(|(key, name, icon, color, _, f)| (key, name, icon, color, open.iter().filter(|t| t.due.is_some_and(f)).map(|t| t.id).collect::<Vec<u64>>()))
            .filter(|g| !g.4.is_empty())
            .collect()
    }

    /// The task panel folded to a strip: a button for each list with the
    /// panel's tools stacked under them. Hovering a list's button shows
    /// the list beside the strip; hovering the menu button, the calendar's
    /// navigation.
    fn ui_rail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, c: (Color32, Color32, Color32)) {
        let side = self.pal.side;
        let today = Local::now().date_naive();
        let panel = if right { egui::Panel::right(ids::rail()) } else { egui::Panel::left(ids::rail()) };
        let mut hot: Option<(Shelf, Rect)> = None;
        let groups = self.due_groups(today, None);
        let shelves = self.shelves(today);
        let out = panel
            .resizable(false)
            .exact_size(RAIL_W)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 0, right: 0, top: 10, bottom: 0 }))
            .show(ui, |ui| {
                self.rail_tools_panel(ui, ctx, c, Some(side));
                egui::CentralPanel::default().frame(egui::Frame::new().fill(side)).show(ui, |ui| {
                    hot = self.rail_icons(ui, ctx, c, &shelves);
                });
            });
        self.measure(out.response.rect, right, true);
        self.ui_flyer(ctx, out.response.rect, right, hot, &groups);
    }

    /// The rail's tools along its bottom, on `fill` when given.
    fn rail_tools_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, c: (Color32, Color32, Color32), fill: Option<Color32>) {
        let frame = egui::Frame::new().fill(fill.unwrap_or(Color32::TRANSPARENT)).inner_margin(Margin { left: 0, right: 0, top: 4, bottom: 8 });
        egui::Panel::bottom(ids::rail_bottom()).show_separator_line(false).frame(frame).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                self.rail_tools(ui, ctx, c);
            });
        });
    }

    /// The menu button and a button for each of `shelves`, the pinned
    /// groups set apart from the lists. Returns the one under the pointer.
    fn rail_icons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, c: (Color32, Color32, Color32), shelves: &[(Shelf, String, Color32)]) -> Option<(Shelf, Rect)> {
        let border = self.pal.border;
        let mut hot = None;
        ui.vertical_centered(|ui| {
            let menu = icon_btn(ui, icons::MENU, 17.0, &self.tip(tr("Show the task panel"), Cmd::ToggleTasks), c);
            if menu.clicked() {
                self.run(ctx, Cmd::ToggleTasks);
            }
            if self.drag_task.is_none() && ui.rect_contains_pointer(menu.rect) {
                hot = Some((Shelf::Nav, menu.rect));
            }
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("rail")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        let mut pinned = false;
                        for (shelf, glyph, color) in shelves {
                            let due = matches!(shelf, Shelf::Due(_));
                            if pinned && !due {
                                ui.add_space(2.0);
                                let (r, _) = ui.allocate_exact_size(vec2(20.0, 1.0), Sense::hover());
                                ui.painter().rect_filled(r, 0.0, border);
                                ui.add_space(2.0);
                            }
                            pinned = due;
                            if let Some(r) = self.rail_icon(ui, ctx, *shelf, glyph, *color) {
                                hot = Some((*shelf, r));
                            }
                        }
                        // a new list is named in the full panel
                        if icon_btn(ui, icons::PLUS, 17.0, tr("New task list"), c).clicked() {
                            self.new_list_text = Some((String::new(), true));
                            self.run(ctx, Cmd::ToggleTasks);
                        }
                    });
                });
        });
        hot
    }

    /// The panel's tools, one above the other, settings at the bottom.
    fn rail_tools(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, c: (Color32, Color32, Color32)) {
        // undo appears while there is something to take back
        let a = ctx.animate_bool_with_time(ids::undo_button(), !self.undo.is_empty(), motion::NORMAL.secs());
        if a > 0.0 {
            ui.scope(|ui| {
                ui.set_opacity(a);
                if icon_btn(ui, icons::UNDO, 16.0, &self.tip(tr("Undo"), Cmd::Undo), c).clicked() {
                    self.do_undo(ctx);
                }
            });
        }
        if icon_btn(ui, icons::SHUFFLE, 15.0, tr("Plan my tasks: fit open tasks into free working time"), c).clicked() {
            self.run(ctx, Cmd::Plan);
        }
        if icon_btn(ui, icons::SEARCH, 15.0, &self.tip(tr("Search"), Cmd::Search), c).clicked() {
            self.run(ctx, Cmd::Search);
        }
        if icon_btn(ui, icons::TERMINAL, 15.0, &self.tip(tr("Command bar"), Cmd::CommandBar), c).clicked() {
            self.run(ctx, Cmd::CommandBar);
        }
        if icon_btn(ui, icons::HELP, 16.0, &self.tip(tr("Shortcuts"), Cmd::Shortcuts), c).clicked() {
            self.open_settings(crate::settings::Page::Shortcuts);
        }
        if icon_btn(ui, icons::SETTINGS, 16.0, &self.tip(tr("Settings"), Cmd::Prefs), c).clicked() {
            self.open_settings(crate::settings::Page::General);
        }
    }

    /// One button on the rail, for a list or a pinned group. A task
    /// dropped on a list's button moves to the list. Returns where it is
    /// while the pointer is over it.
    fn rail_icon(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, shelf: Shelf, glyph: &str, color: Color32) -> Option<Rect> {
        let (side, text) = (self.pal.side, self.pal.text);
        let (r, _) = ui.allocate_exact_size(vec2(RAIL_ICON, RAIL_ICON), Sense::hover());
        self.morph.seen.push((shelf, r.center()));
        // the pointer, not egui's hover, which a drag elsewhere turns off
        let over = ui.rect_contains_pointer(r);
        let open = self.flyer.as_ref().is_some_and(|f| f.shelf == shelf);
        let glow = ctx.animate_bool_with_time(ids::rail_icon(shelf), over || open, motion::HOVER.secs());
        let p = ui.painter();
        let soft = mix(color, text, 0.25);
        fill_squircle(p, r, theme::RADIUS, mix(side, soft, 0.04 + 0.12 * glow));
        if glyph.is_empty() {
            p.circle_stroke(r.center(), 5.5, Stroke::new(2.0, color));
        } else {
            p.text(r.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(theme::size::LARGE), color);
        }
        let Shelf::List(lid) = shelf else { return over.then_some(r) };
        let drop = self.drag_task.filter(|t| self.data.task(*t).is_some_and(|t| t.list != lid && !t.done));
        let drop = drop.filter(|_| lid == 0 || self.data.lists.iter().any(|x| x.id == lid && !x.read_only));
        if let Some(tid) = drop.filter(|_| over) {
            crate::stroke_squircle(p, r, theme::RADIUS, Stroke::new(1.5, color));
            if ctx.input(|i| i.pointer.any_released()) {
                let name = if lid == 0 { self.data.prefs.inbox_name() } else { self.data.list_look(lid).0 };
                self.move_to_list(tid, lid);
                self.drag_task = None;
                self.say(ctx, trf("Moved to {}", &[&name]), true);
            }
        }
        over.then_some(r)
    }

    /// The hovered list, hanging beside the rail. It goes once the pointer
    /// has been away from it and its button for a moment, or at once while
    /// a task is carried out of it, so the calendar under it is free.
    fn ui_flyer(&mut self, ctx: &egui::Context, rail: Rect, right: bool, hot: Option<(Shelf, Rect)>, groups: &[DueGroup]) {
        if let Some((shelf, r)) = hot {
            match &mut self.flyer {
                Some(f) if f.shelf == shelf => f.top = r.top(),
                _ => self.flyer = Some(crate::Flyer { shelf, top: r.top(), away: None, rect: Rect::NOTHING }),
            }
        }
        let Some(f) = &self.flyer else { return };
        let (shelf, top) = (f.shelf, f.top);
        let gone = match shelf {
            Shelf::List(lid) => lid != 0 && !self.data.lists.iter().any(|l| l.id == lid),
            Shelf::Due(key) => !groups.iter().any(|g| g.0 == key),
            Shelf::Done => self.done_ids().is_empty(),
            Shelf::Nav => false,
        };
        if gone {
            self.flyer = None;
            return;
        }
        let (side, muted) = (self.pal.side, self.pal.muted);
        let c = (side, muted, self.pal.text);
        let nav = shelf == Shelf::Nav;
        let screen = ctx.content_rect();
        let x = if right { rail.left() - 4.0 } else { rail.right() + 4.0 };
        // the navigation row lines up with the menu button
        let y = if nav { top - 5.0 } else { (top - 10.0).max(screen.top() + 8.0) };
        let max_h = (screen.bottom() - y - 12.0).max(160.0);
        let out = egui::Area::new(ids::flyer())
            .order(egui::Order::Foreground)
            .pivot(if right { Align2::RIGHT_TOP } else { Align2::LEFT_TOP })
            .fixed_pos(pos2(x, y))
            .constrain(true)
            .show(ctx, |ui| {
                let margin = if nav { Margin::same(4) } else { Margin { left: 14, right: 12, top: 6, bottom: 4 } };
                egui::Frame::popup(ui.style()).fill(side).inner_margin(margin).show(ui, |ui| {
                    if nav {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.nav_row(ui, ctx, c, false);
                        });
                        return;
                    }
                    ui.set_width(FLYER_W);
                    ui.spacing_mut().scroll = egui::style::ScrollStyle { bar_inner_margin: 9.0, ..egui::style::ScrollStyle::solid() };
                    egui::ScrollArea::vertical().id_salt("flyer").max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                        ui.add_space(6.0);
                        match shelf {
                            Shelf::List(lid) => self.flyer_list(ui, ctx, lid),
                            Shelf::Due(key) => {
                                if let Some((key, name, icon, color, items)) = groups.iter().find(|g| g.0 == key) {
                                    self.section(ui, ctx, &format!("fly{key}"), name, Some((icon, *color)), *color, items, None);
                                    self.collapsed.remove(&format!("fly{key}"));
                                }
                            }
                            Shelf::Done => {
                                let items = self.done_ids();
                                self.section(ui, ctx, "flydone", tr("Completed"), Some((icons::CHECK, muted)), muted, &items, None);
                                self.collapsed.remove("flydone");
                            }
                            Shelf::Nav => {}
                        }
                    });
                });
            });
        let rect = out.response.rect;
        let now = ctx.input(|i| i.time);
        let inside = hot.is_some() || ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.expand(6.0).contains(p));
        // its menus and the list editor hang outside it
        let held = egui::Popup::is_any_open(ctx) || self.list_edit.is_some();
        let carried = self.drag_task.is_some();
        let Some(f) = &mut self.flyer else { return };
        f.rect = rect;
        if inside || held {
            f.away = None;
        } else if carried {
            self.flyer = None;
        } else {
            let since = *f.away.get_or_insert(now);
            if now - since > FLYER_LINGER_S {
                self.flyer = None;
            } else {
                ctx.request_repaint();
            }
        }
    }

    /// A list as the flyer shows it: the same heading and rows as the
    /// panel, always open.
    fn flyer_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, lid: u64) {
        let (name, color, glyph) = self.data.list_look(lid);
        let name = if lid == 0 { self.data.prefs.inbox_name() } else if name.trim().is_empty() { tr("Untitled list").to_string() } else { name };
        let color = rgb(color);
        let show_scheduled = self.data.prefs.show_scheduled_tasks;
        let mut tasks = self.open_tasks(None);
        tasks.retain(|t| t.list == lid && (show_scheduled || !self.data.task_is_scheduled(t.id)));
        let items: Vec<u64> = tasks.iter().map(|t| t.id).collect();
        let key = format!("fly{lid}");
        let icon = Some((glyph.as_str(), color)).filter(|g| !g.0.is_empty());
        self.section(ui, ctx, &key, &name, icon, color, &items, Some(lid));
        // its heading folds the panel's lists, not this one
        self.collapsed.remove(&key);
        if items.is_empty() {
            ui.label(RichText::new(tr("No open tasks")).size(theme::size::SMALL).color(self.pal.muted));
            ui.add_space(8.0);
        }
    }

    /// Whether `pos` is on the flyer, where the calendar under it takes no drop.
    pub fn over_flyer(&self, pos: egui::Pos2) -> bool {
        self.flyer.as_ref().is_some_and(|f| f.rect.contains(pos))
    }

    /// Archives every completed task, as one step that can be undone.
    pub fn archive_done(&mut self) -> usize {
        self.snapshot();
        let n = self.data.archive_done();
        self.commit();
        n
    }

    /// Puts a task in another list, as one step that can be undone.
    pub fn move_to_list(&mut self, id: u64, list: u64) {
        self.snapshot();
        if let Some(x) = self.data.task_mut(id) {
            x.list = list;
        }
        self.commit();
    }

    /// One task in a list. `tint` is the colour its hover is washed in.
    fn task_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: u64, salt: &str, tint: Color32) {
        let Some(t) = self.data.task(id).cloned() else { return };
        let today = Local::now().date_naive();
        let (muted, text, border) = (self.pal.muted, self.pal.text, self.pal.border);
        let scheduled = self.data.task_is_scheduled(id);
        ui.push_id((salt, id), |ui| {
            // the hover goes under the row, painted once its size is known
            let under = ui.painter().add(egui::Shape::Noop);
            let inner = egui::Frame::new().inner_margin(Margin { left: 2, right: 2, top: 4, bottom: 4 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let btns = ui.horizontal(|ui| {
                    let mut d = t.done;
                    if round_check(ui, &mut d).changed() {
                        self.toggle_task(id);
                        self.say(ctx, if d { tr("Task completed") } else { tr("Task reopened") }, true);
                    }
                    // the bin and the three dots keep their place at the end,
                    // so a long title stops short of them
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (br, bin) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
                        let (dr, dots) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
                        let bin = bin.on_hover_text(tr("Delete task"));
                        let dots = dots.on_hover_text(tr("Edit"));
                        ui.add_space(4.0);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            // tasks left without a name by an older version still show one
                            let mut title = RichText::new(if t.title.trim().is_empty() { tr("New task") } else { &t.title }).size(theme::size::BODY);
                            if t.done {
                                title = title.color(muted).strikethrough();
                            }
                            let r = ui
                                .add(egui::Label::new(title).truncate().selectable(false).sense(Sense::click_and_drag()))
                                .on_hover_cursor(egui::CursorIcon::Grab);
                            if r.drag_started() {
                                self.drag_task = Some(id);
                            }
                            if r.clicked() {
                                self.edit_task(id);
                            }
                            context_menu(&r, |ui| {
                                let mut m = Menu::start(ui);
                                if m.item(ui, icons::PEN, tr("Edit"), false).clicked() {
                                    self.edit_task(id);
                                    ui.close();
                                }
                                m.sub(ui, icons::ARROW_RIGHT, tr("Move to list"), |ui| {
                                    for (lid, name, col) in self.list_items(true) {
                                        if crate::dialogs::pick_row(ui, t.list == lid, &name, col).clicked() {
                                            self.move_to_list(id, lid);
                                            ui.close();
                                        }
                                    }
                                });
                                m.line(ui);
                                if m.item(ui, icons::TRASH, tr("Delete task"), true).clicked() {
                                    self.delete_task(ctx, id);
                                    ui.close();
                                }
                                m.end(ui);
                            });
                        });
                        [(dr, dots, icons::ELLIPSIS, theme::size::LARGE, text), (br, bin, icons::TRASH, theme::size::SMALL, theme::DANGER)]
                    })
                    .inner
                })
                .inner;
                // small labels under the title
                let mut chips: Vec<(String, Color32)> = vec![];
                if scheduled {
                    chips.push(("scheduled".into(), muted));
                }
                if let Some(d) = t.due {
                    let c = if d < today && !t.done { theme::DANGER } else { muted };
                    let rep = if t.repeat != Repeat::None { format!("  {}", icons::REPEAT) } else { String::new() };
                    chips.push((format!("{} {}{rep}", icons::FLAG, due_label(d, today)), c));
                }
                if let Some(m) = t.duration_min {
                    chips.push((fmt_duration(m), muted));
                }
                if t.priority != 0 {
                    chips.push((format!("! {}", priority_name(t.priority)), if t.priority <= 3 { theme::DANGER } else { muted }));
                }
                for g in &t.tags {
                    chips.push((format!("#{g}"), muted));
                }
                if !chips.is_empty() {
                    // each pill is measured first, so one that does not fit
                    // starts the next line, under the title
                    ui.horizontal_top(|ui| {
                        ui.add_space(27.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(5.0, 3.0);
                            let font = FontId::proportional(theme::size::TINY);
                            for (label, c) in chips {
                                let g = ui.painter().layout_no_wrap(label, font.clone(), c);
                                let size = g.size() + vec2(10.0, 2.0);
                                let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                                ui.painter().rect_stroke(r, theme::CHIP_RADIUS, Stroke::new(1.0, border), egui::StrokeKind::Inside);
                                ui.painter().galley(r.center() - g.size() / 2.0, g, c);
                            }
                        });
                    });
                }
                btns
            });
            let hovered = ui.rect_contains_pointer(inner.response.rect);
            let glow = ctx.animate_bool_with_time(ids::task_row((salt, id)), hovered, motion::HOVER.secs());
            if glow > 0.0 {
                let r = inner.response.rect.expand2(vec2(4.0, 0.0));
                ui.painter().set(under, egui::Shape::convex_polygon(crate::squircle_points(r, theme::RADIUS), tint.gamma_multiply(0.07 * glow), Stroke::NONE));
            }
            let [(dr, dots, dg, ds, dh), (br, bin, bg, bs, bh)] = inner.inner;
            row_btn(ui, dr, &dots, dg, ds, glow, false, dh, (muted, text));
            row_btn(ui, br, &bin, bg, bs, glow, false, bh, (muted, text));
            if dots.clicked() {
                self.edit_task(id);
            }
            if bin.clicked() {
                self.delete_task(ctx, id);
            }
        });
    }
}
