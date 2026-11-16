//! The task panel (left or right) and the bar above the calendar.

use crate::strings::{tr, trf};
use crate::ids;
use crate::motion;
use crate::theme;
use crate::icons;
use crate::model::*;
use crate::{fill_squircle, mix, rgb, App, Cmd};
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
        let closed = !self.data.prefs.sidebar_open;
        let closed_left = closed && self.data.prefs.sidebar_side == PanelSide::Left;
        let closed_right = closed && !closed_left;
        let mut go_at = egui::Pos2::ZERO;
        egui::Panel::top("bar")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(self.pal.bg).inner_margin(Margin { left: 24, right: 16, top: 12, bottom: 6 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // room for the floating panel handle while the panel is closed
                    if closed_left {
                        ui.add_space(36.0);
                    }
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
                        if closed_right {
                            ui.add_space(36.0);
                        }
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
    pub fn add_task(&mut self, text: &str, list: u64) {
        let today = Local::now().date_naive();
        let q = parse_quick(text, today);
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
        self.data.tasks.insert(
            0,
            Task {
                id,
                title: q.title,
                due: q.due,
                priority: q.priority,
                duration_min: q.duration_min,
                tags: q.tags,
                list,
                ..Default::default()
            },
        );
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
        let panel = if right { egui::Panel::right("tasks") } else { egui::Panel::left("tasks") };
        let mut still_open = open;
        panel
            .resizable(true)
            .default_size(self.data.prefs.sidebar_w)
            .size_range(280.0..=560.0)
            .show_separator_line(false)
            .drag_to_open(false)
            .frame(egui::Frame::new().fill(side))
            .show_collapsible(ui, &mut still_open, |ui| {
                egui::Panel::bottom(ids::panel_bottom())
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 10, right: 10, top: 4, bottom: 8 }))
                    .show(ui, |ui| self.panel_bottom(ui, ctx, right, c));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 14, right: 12, top: 10, bottom: 0 }))
                    .show(ui, |ui| self.panel_main(ui, ctx, right, today, c));
            });
        // dragging the edge past the minimum closes the panel, like the button
        if still_open != open {
            self.run(ctx, Cmd::ToggleTasks);
        }
        // while the panel is closed, a lone handle brings it back
        if !open {
            let (anchor, dx) = if right { (Align2::RIGHT_TOP, -10.0) } else { (Align2::LEFT_TOP, 10.0) };
            egui::Area::new(ids::panel_handle()).anchor(anchor, [dx, 10.0]).order(egui::Order::Foreground).show(ctx, |ui| {
                if icon_btn(ui, icons::MENU, 17.0, &self.tip(tr("Show the task panel"), Cmd::ToggleTasks), c).clicked() {
                    self.run(ctx, Cmd::ToggleTasks);
                }
            });
        }
    }

    /// The row of buttons at the bottom of the panel: the new-list button in
    /// the inner corner, preferences in the outer one.
    fn panel_bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, c: (Color32, Color32, Color32)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut tools = |ui: &mut egui::Ui, app: &mut App| {
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
            let mut new_list = |ui: &mut egui::Ui, app: &mut App| {
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
            let mut nav = |ui: &mut egui::Ui, app: &mut App, flipped: bool| {
                // drawn right to left when flipped
                let order: [(&str, f32, &str, Cmd); 4] = if flipped {
                    [
                        (icons::CHEVRON_RIGHT, 17.0, tr("Next"), Cmd::Next),
                        (icons::CALENDAR_CHECK, 16.0, tr("Today"), Cmd::Today),
                        (icons::CHEVRON_LEFT, 17.0, tr("Previous"), Cmd::Prev),
                        (icons::REFRESH, 15.0, tr("Refresh"), Cmd::Refresh),
                    ]
                } else {
                    [
                        (icons::REFRESH, 15.0, tr("Refresh"), Cmd::Refresh),
                        (icons::CHEVRON_LEFT, 17.0, tr("Previous"), Cmd::Prev),
                        (icons::CALENDAR_CHECK, 16.0, tr("Today"), Cmd::Today),
                        (icons::CHEVRON_RIGHT, 17.0, tr("Next"), Cmd::Next),
                    ]
                };
                for (glyph, size, tip, cmd) in order {
                    if icon_btn(ui, glyph, size, &app.tip(tip, cmd), c).clicked() {
                        app.run(ctx, cmd);
                    }
                }
            };
            if right {
                nav(ui, self, false);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_btn(ui, icons::MENU, 17.0, &self.tip(tr("Hide the task panel"), Cmd::ToggleTasks), c).clicked() {
                        self.run(ctx, Cmd::ToggleTasks);
                    }
                });
            } else {
                if icon_btn(ui, icons::MENU, 17.0, &self.tip(tr("Hide the task panel"), Cmd::ToggleTasks), c).clicked() {
                    self.run(ctx, Cmd::ToggleTasks);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| nav(ui, self, true));
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Tasks")).font(theme::bold(theme::size::HEADING)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
                let tomorrow = today + Duration::days(1);
                let p = self.data.prefs.clone();
                let tag = self.filter_tag.clone();
                let mut open_tasks: Vec<Task> = self
                    .data
                    .tasks
                    .iter()
                    .filter(|t| !t.done)
                    .filter(|t| tag.as_ref().map(|g| t.tags.contains(g)).unwrap_or(true))
                    .cloned()
                    .collect();
                match self.sort_by {
                    1 => open_tasks.sort_by_key(|t| t.due.unwrap_or(NaiveDate::MAX)),
                    2 => open_tasks.sort_by_key(|t| if t.priority == 0 { 10 } else { t.priority }),
                    _ => {}
                }
                let ids = |f: &dyn Fn(&Task) -> bool| -> Vec<u64> { open_tasks.iter().filter(|t| f(t)).map(|t| t.id).collect() };
                let overdue = ids(&|t| t.due.map(|d| d < today).unwrap_or(false));
                let due_today = ids(&|t| t.due == Some(today));
                let due_tomorrow = ids(&|t| t.due == Some(tomorrow));
                let soon_from = if p.show_due_tomorrow { 2 } else if p.show_due_today { 1 } else { 0 };
                let due_soon = ids(&|t| t.due.map(|d| (d - today).num_days() >= soon_from && (d - today).num_days() <= DUE_SOON_DAYS).unwrap_or(false));
                let visible_in_list = |t: &Task| p.show_scheduled_tasks || !self.data.task_is_scheduled(t.id);
                let mut lists: Vec<(u64, String, String, Color32, Vec<u64>)> = Vec::new();
                for id in std::iter::once(0).chain(self.data.lists.iter().map(|l| l.id)) {
                    let (name, color, icon) = self.data.list_look(id);
                    let name = if id == 0 { self.data.prefs.inbox_name() } else if name.trim().is_empty() { tr("Untitled list").to_string() } else { name };
                    lists.push((id, name, icon, rgb(color), ids(&|t| t.list == id && visible_in_list(t))));
                }
                let done_ids: Vec<u64> = self.data.tasks.iter().filter(|t| t.done).map(|t| t.id).collect();

                egui::ScrollArea::vertical().id_salt("tasklist").auto_shrink([false, false]).show(ui, |ui| {
                    let mut any_pinned = false;
                    for (key, name, icon, color, items, show) in [
                        ("overdue", tr("Overdue"), icons::ALARM, theme::DANGER, &overdue, true),
                        ("today", tr("Due today"), icons::SUN, theme::WARNING, &due_today, p.show_due_today),
                        ("tomorrow", tr("Due tomorrow"), icons::ARROW_RIGHT, theme::WARNING, &due_tomorrow, p.show_due_tomorrow),
                        ("soon", tr("Due soon"), icons::CHEVRONS_RIGHT, theme::WARNING, &due_soon, p.show_due_soon),
                    ] {
                        if show && !items.is_empty() {
                            any_pinned = true;
                            self.section(ui, ctx, key, name, Some((icon, color)), color, items, None);
                        }
                    }
                    if any_pinned {
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
        self.ui_list_edit(ctx);
    }

    /// What a list's menu offers: change how it looks, or remove it.
    fn list_menu(&mut self, ui: &mut egui::Ui, lid: u64, at: egui::Pos2) {
        ui.set_min_width(130.0);
        if ui.button(format!("{}  Edit", icons::PEN)).clicked() {
            self.list_edit = Some(crate::ListEdit { id: lid, at, fresh: true, snapped: false });
            ui.close();
        }
        if lid == 0 {
            return;
        }
        let synced = self.data.lists.iter().any(|l| l.id == lid && l.account.is_some());
        let b = ui.add_enabled(!synced, egui::Button::new(RichText::new(format!("{}  Remove", icons::TRASH)).color(theme::DANGER)));
        let b = b.on_hover_text(tr("Its tasks go to the Inbox")).on_disabled_hover_text(tr("This list comes from an account. It goes when it is switched off under Settings, Calendars."));
        if b.clicked() {
            self.snapshot();
            self.data.remove_own_list(lid);
            self.commit();
            ui.close();
        }
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
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        // the menu button sits on the row, so the row itself is not "hovered" there
        let editing = list.is_some() && self.list_edit.as_ref().map(|e| e.id) == list;
        let glow = ctx.animate_bool_with_time(ids::section(key), resp.hovered() || ui.rect_contains_pointer(r) || editing, motion::HOVER.secs());
        if glow > 0.0 {
            fill_squircle(ui.painter(), r.expand2(vec2(6.0, 0.0)), theme::RADIUS, mix(side, text, 0.05 * glow));
        }
        let p = ui.painter();
        let ic = pos2(r.left() + 9.0, r.center().y);
        match icon {
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
        if let Some(_l) = list {
            if glow > 0.0 {
                let pr = Rect::from_center_size(pos2(r.right() - 66.0, r.center().y), vec2(22.0, 22.0));
                let over = ui.rect_contains_pointer(pr);
                ui.painter().text(pr.center(), Align2::CENTER_CENTER, "+", FontId::proportional(theme::size::HEADING), if over { text } else { muted.gamma_multiply(glow) });
                if over && resp.clicked() {
                    add_clicked = true;
                }
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
            if glow > 0.0 || open {
                if dots.hovered() || open {
                    fill_squircle(ui.painter(), dr, theme::RADIUS, mix(side, text, 0.10));
                }
                let dc = if dots.hovered() || open { text } else { muted.gamma_multiply(glow) };
                ui.painter().text(dr.center(), Align2::CENTER_CENTER, icons::ELLIPSIS, FontId::proportional(theme::size::LARGE), dc);
            }
            menu.show(|ui| self.list_menu(ui, lid, at));
            resp.context_menu(|ui| self.list_menu(ui, lid, at));
        }
        let a = ctx.animate_bool_with_time(ids::section_open(key), !collapsed, motion::NORMAL.secs());
        if a > 0.0 {
            ui.scope(|ui| {
                ui.set_opacity(a);
                for id in items {
                    self.task_row(ui, ctx, *id, key);
                }
            });
            ui.add_space(6.0 * a);
        }
    }

    fn task_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: u64, salt: &str) {
        let Some(t) = self.data.task(id).cloned() else { return };
        let today = Local::now().date_naive();
        let (muted, text, side, border) = (self.pal.muted, self.pal.text, self.pal.side, self.pal.border);
        let scheduled = self.data.task_is_scheduled(id);
        ui.push_id((salt, id), |ui| {
            let inner = egui::Frame::new().inner_margin(Margin { left: 2, right: 2, top: 4, bottom: 4 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let mut d = t.done;
                    if round_check(ui, &mut d).changed() {
                        self.toggle_task(id);
                        self.say(ctx, if d { tr("Task completed") } else { tr("Task reopened") }, true);
                    }
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
                    r.context_menu(|ui| {
                        if ui.button(tr("Edit")).clicked() {
                            self.edit_task(id);
                            ui.close();
                        }
                        ui.menu_button(tr("Move to list"), |ui| {
                            for (lid, name, col) in self.list_items(true) {
                                if crate::dialogs::pick_row(ui, t.list == lid, &name, col).clicked() {
                                    self.snapshot();
                                    if let Some(x) = self.data.task_mut(id) {
                                        x.list = lid;
                                    }
                                    self.commit();
                                    ui.close();
                                }
                            }
                        });
                        if ui.button(RichText::new(tr("Delete task")).color(theme::DANGER)).clicked() {
                            self.delete_task(ctx, id);
                            ui.close();
                        }
                    });
                });
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
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(5.0, 3.0);
                        ui.add_space(27.0);
                        for (label, c) in chips {
                            egui::Frame::new().stroke(Stroke::new(1.0, border)).corner_radius(theme::CHIP_RADIUS).inner_margin(Margin::symmetric(5, 1)).show(ui, |ui| {
                                ui.label(RichText::new(label).size(theme::size::TINY).color(c));
                            });
                        }
                    });
                }
            });
            let hovered = ui.rect_contains_pointer(inner.response.rect);
            let glow = ctx.animate_bool_with_time(ids::task_row((salt, id)), hovered, motion::HOVER.secs());
            if glow > 0.0 {
                fill_squircle(ui.painter(), inner.response.rect.expand2(vec2(6.0, 0.0)), theme::RADIUS, mix(side, text, 0.06).gamma_multiply(glow * 0.6));
            }
        });
    }
}
