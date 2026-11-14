//! The task panel (left or right) and the bar above the calendar.

use crate::icons;
use crate::model::*;
use crate::{fill_squircle, mix, rgb, App, Cmd};
use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Id, Layout, Margin, Rect, RichText, Sense, Stroke,
};

const RED: Color32 = Color32::from_rgb(0xeb, 0x62, 0x62);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x8f, 0x1e);

fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// A flat button that only shows its label.
fn flat(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(label).frame(false))
}

/// A flat icon button: a Lucide glyph on a squircle that glows on hover.
/// `c` is (side, muted, text) from the palette.
pub fn icon_btn(ui: &mut egui::Ui, glyph: &str, size: f32, tip: &str, c: (Color32, Color32, Color32)) -> egui::Response {
    let (side, muted, text) = c;
    let (r, resp) = ui.allocate_exact_size(vec2(size + 12.0, size + 12.0), Sense::click());
    let glow = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), ANIM_FAST_S);
    if glow > 0.0 {
        fill_squircle(ui.painter(), r, RADIUS, mix(side, text, 0.08 * glow));
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
        -1 => "Yesterday".into(),
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        _ => d.format("%b %-d").to_string(),
    }
}

impl App {
    /// The small month for jumping to a date.
    fn ui_quick_nav(&mut self, ctx: &egui::Context) {
        let a = ctx.animate_bool_with_time(Id::new("quicknav"), self.quick_nav, ANIM_S);
        if a <= 0.0 {
            return;
        }
        let today = Local::now().date_naive();
        let first = self.anchor.with_day(1).unwrap();
        let start = self.data.prefs.week_start(first);
        let (muted, text, accent) = (self.pal.muted, self.pal.text, self.pal.accent);
        let mut go: Option<NaiveDate> = None;
        let mut step = 0;
        let right = self.data.prefs.sidebar_side == PanelSide::Right;
        let (anchor, dx) = if right { (Align2::RIGHT_TOP, -14.0) } else { (Align2::LEFT_TOP, 14.0) };
        egui::Area::new(Id::new("quicknav_area"))
            .anchor(anchor, [dx, 96.0 + (1.0 - a) * 12.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(a);
                egui::Frame::popup(ui.style()).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(7.0 * 30.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(first.format("%B %Y").to_string()).font(bold(14.0)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if flat(ui, RichText::new(icons::CHEVRON_RIGHT).size(14.0)).clicked() {
                                step = 1;
                            }
                            if flat(ui, RichText::new(icons::CHEVRON_LEFT).size(14.0)).clicked() {
                                step = -1;
                            }
                        });
                    });
                    egui::Grid::new("qn").spacing([0.0, 0.0]).min_col_width(30.0).show(ui, |ui| {
                        for i in 0..7 {
                            let d = start + Duration::days(i);
                            ui.vertical_centered(|ui| ui.label(RichText::new(&d.format("%a").to_string()[..2]).size(10.5).color(muted)));
                        }
                        ui.end_row();
                        for i in 0..42 {
                            let d = start + Duration::days(i);
                            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
                            let glow = ctx.animate_bool_with_time(Id::new(("qnd", i)), resp.hovered(), ANIM_FAST_S);
                            if d == today {
                                ui.painter().circle_filled(r.center(), 11.0, RED);
                            } else if glow > 0.0 {
                                ui.painter().circle_filled(r.center(), 11.0, mix(self.pal.side, text, 0.14 * glow));
                            }
                            let c = if d == today { Color32::WHITE } else if d.month() != first.month() { muted } else { text };
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(12.0), c);
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

    // ------------------------------------------------------------ top bar

    pub fn ui_toolbar(&mut self, ctx: &egui::Context) {
        let (muted, text) = (self.pal.muted, self.pal.text);
        let closed_left = !self.data.prefs.sidebar_open && self.data.prefs.sidebar_side == PanelSide::Left;
        egui::TopBottomPanel::top("bar")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(self.pal.bg).inner_margin(Margin { left: 24, right: 16, top: 12, bottom: 6 }))
            .show(ctx, |ui| {
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
                    ui.label(RichText::new(month.to_lowercase()).font(bold(if narrow { 20.0 } else { 26.0 })).color(text));
                    if !tiny {
                        ui.label(RichText::new(last.year().to_string()).size(if narrow { 20.0 } else { 26.0 }).color(muted));
                    }
                });
            });
    }

    // --------------------------------------------------------- task panel

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

    fn new_list(&mut self, name: &str) -> u64 {
        let id = self.data.new_id();
        let color = EVENT_COLORS[self.data.lists.len() % EVENT_COLORS.len()].0;
        self.data.lists.push(TaskList { id, name: name.to_string(), color });
        id
    }

    pub fn ui_sidebar(&mut self, ctx: &egui::Context) {
        let today = Local::now().date_naive();
        let side = self.pal.side;
        let c = (self.pal.side, self.pal.muted, self.pal.text);
        let open = self.data.prefs.sidebar_open;
        let right = self.data.prefs.sidebar_side == PanelSide::Right;
        let panel = if right { egui::SidePanel::right("tasks") } else { egui::SidePanel::left("tasks") };
        panel
            .resizable(true)
            .default_width(self.data.prefs.sidebar_w)
            .width_range(280.0..=560.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(side))
            .show_animated(ctx, open, |ui| {
                egui::TopBottomPanel::bottom(Id::new("panel_bottom"))
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 10, right: 10, top: 4, bottom: 8 }))
                    .show_inside(ui, |ui| self.panel_bottom(ui, ctx, right, c));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(side).inner_margin(Margin { left: 14, right: 12, top: 10, bottom: 0 }))
                    .show_inside(ui, |ui| self.panel_main(ui, ctx, right, today, c));
            });
        // while the panel is closed, a lone handle brings it back
        if !open {
            let (anchor, dx) = if right { (Align2::RIGHT_TOP, -10.0) } else { (Align2::LEFT_TOP, 10.0) };
            egui::Area::new(Id::new("panel_handle")).anchor(anchor, [dx, 10.0]).order(egui::Order::Foreground).show(ctx, |ui| {
                if icon_btn(ui, icons::MENU, 17.0, &self.tip("Show the task panel", Cmd::ToggleTasks), c).clicked() {
                    self.run(ctx, Cmd::ToggleTasks);
                }
            });
        }
        self.ui_quick_nav(ctx);
    }

    /// The row of buttons at the bottom of the panel: the new-list button in
    /// the inner corner, preferences in the outer one.
    fn panel_bottom(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, right: bool, c: (Color32, Color32, Color32)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut tools = |ui: &mut egui::Ui, app: &mut App| {
                if icon_btn(ui, icons::SETTINGS, 16.0, &app.tip("Settings", Cmd::Prefs), c).clicked() {
                    app.open_settings(crate::settings::Page::General);
                }
                if icon_btn(ui, icons::HELP, 16.0, &app.tip("Shortcuts", Cmd::Shortcuts), c).clicked() {
                    app.open_settings(crate::settings::Page::Shortcuts);
                }
                if icon_btn(ui, icons::TERMINAL, 15.0, &app.tip("Command bar", Cmd::CommandBar), c).clicked() {
                    app.run(ctx, Cmd::CommandBar);
                }
                if icon_btn(ui, icons::SEARCH, 15.0, &app.tip("Search", Cmd::Search), c).clicked() {
                    app.run(ctx, Cmd::Search);
                }
                if icon_btn(ui, icons::SHUFFLE, 15.0, "Plan my tasks: fit open tasks into free working time", c).clicked() {
                    app.run(ctx, Cmd::Plan);
                }
            };
            let mut new_list = |ui: &mut egui::Ui, app: &mut App| {
                if icon_btn(ui, icons::PLUS, 17.0, "New task list", c).clicked() {
                    app.new_list_text = Some(String::new());
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
        // collapse toggle in the outer corner, navigation in the inner one,
        // the refresh button beside the navigation
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut nav = |ui: &mut egui::Ui, app: &mut App, flipped: bool| {
                // drawn right to left when flipped
                let order: [(&str, f32, &str, Cmd); 3] = if flipped {
                    [
                        (icons::CHEVRON_RIGHT, 17.0, "Next", Cmd::Next),
                        (icons::CHEVRON_LEFT, 17.0, "Previous", Cmd::Prev),
                        (icons::REFRESH, 15.0, "Refresh", Cmd::Refresh),
                    ]
                } else {
                    [
                        (icons::REFRESH, 15.0, "Refresh", Cmd::Refresh),
                        (icons::CHEVRON_LEFT, 17.0, "Previous", Cmd::Prev),
                        (icons::CHEVRON_RIGHT, 17.0, "Next", Cmd::Next),
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
                    if icon_btn(ui, icons::MENU, 17.0, &self.tip("Hide the task panel", Cmd::ToggleTasks), c).clicked() {
                        self.run(ctx, Cmd::ToggleTasks);
                    }
                });
            } else {
                if icon_btn(ui, icons::MENU, 17.0, &self.tip("Hide the task panel", Cmd::ToggleTasks), c).clicked() {
                    self.run(ctx, Cmd::ToggleTasks);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| nav(ui, self, true));
            }
        });
        // today, go to a date, calendars, the view — and undo while it applies
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_btn(ui, icons::CALENDAR_CHECK, 16.0, &self.tip("Today", Cmd::Today), c).clicked() {
                self.run(ctx, Cmd::Today);
            }
            if icon_btn(ui, icons::CALENDAR_DAYS, 16.0, "Go to a date", c).clicked() {
                self.quick_nav = !self.quick_nav;
            }
            ui.menu_button(RichText::new(icons::CALENDAR).size(16.0).color(muted), |ui| {
                ui.label(RichText::new("Calendars").color(muted));
                let mut changed = false;
                for cal in self.data.calendars.iter_mut() {
                    changed |= ui.checkbox(&mut cal.visible, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color))).changed();
                }
                if changed {
                    self.commit();
                }
                ui.separator();
                if ui.button("Subscribe to a calendar…").clicked() {
                    self.open_settings(crate::settings::Page::Calendars);
                    ui.close_menu();
                }
                if ui.button("Refresh").clicked() {
                    self.run(ctx, Cmd::Refresh);
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text("Calendars");
            let mut v = self.view;
            ui.menu_button(RichText::new(format!("{}  {}", v.label(), icons::CHEVRON_DOWN)).size(13.5).color(muted), |ui| {
                ui.set_min_width(170.0);
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
                            ui.close_menu();
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(key).monospace().size(11.0).color(muted));
                        });
                    });
                }
                ui.separator();
                ui.menu_button("Number of days", |ui| {
                    for n in 2..=9u8 {
                        if ui.selectable_label(v == View::Days(n), format!("{n} days")).clicked() {
                            v = View::Days(n);
                            ui.close_menu();
                        }
                    }
                });
            })
            .response
            .on_hover_text("Calendar view");
            self.view = v;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // undo appears while there is something to take back
                let a = ctx.animate_bool_with_time(Id::new("undo_btn"), !self.undo.is_empty(), ANIM_S);
                if a > 0.0 {
                    ui.scope(|ui| {
                        ui.set_opacity(a);
                        if icon_btn(ui, icons::UNDO, 16.0, &self.tip("Undo", Cmd::Undo), c).clicked() {
                            self.do_undo(ctx);
                        }
                    });
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tasks").font(bold(18.0)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button(RichText::new(icons::SLIDERS).size(15.0).color(muted), |ui| {
                    ui.set_min_width(220.0);
                    let p = &mut self.data.prefs;
                    ui.label(RichText::new("Pinned lists").color(muted).size(12.0));
                    let mut ch = ui.checkbox(&mut p.show_due_today, "Due today").changed();
                    ch |= ui.checkbox(&mut p.show_due_tomorrow, "Due tomorrow").changed();
                    ch |= ui.checkbox(&mut p.show_due_soon, "Due soon").changed();
                    ui.separator();
                    ch |= ui.checkbox(&mut p.show_scheduled_tasks, "Show scheduled tasks").changed();
                    ch |= ui.checkbox(&mut p.due_on_calendar, "Due dates on the calendar").changed();
                    ui.separator();
                    ui.label(RichText::new("Sort by").color(muted).size(12.0));
                    for (i, n) in ["My order", "Due date", "Importance"].iter().enumerate() {
                        ui.radio_value(&mut self.sort_by, i, *n);
                    }
                    if ch {
                        self.commit();
                    }
                })
                .response
                .on_hover_text("List options");
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
                    if ui.selectable_label(on, RichText::new(format!("#{t}")).size(11.5).color(if on { Color32::WHITE } else { muted })).clicked() {
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
                let mut lists: Vec<(u64, String, Color32, Vec<u64>)> = vec![(0, "Inbox".into(), Color32::from_rgb(0x35, 0x85, 0xe1), ids(&|t| t.list == 0 && visible_in_list(t)))];
                for l in &self.data.lists {
                    lists.push((l.id, l.name.clone(), rgb(l.color), ids(&|t| t.list == l.id && visible_in_list(t))));
                }
                let done_ids: Vec<u64> = self.data.tasks.iter().filter(|t| t.done).map(|t| t.id).collect();

                egui::ScrollArea::vertical().id_salt("tasklist").auto_shrink([false, false]).show(ui, |ui| {
                    let mut any_pinned = false;
                    for (key, name, icon, color, items, show) in [
                        ("overdue", "Overdue", icons::ALARM, RED, &overdue, true),
                        ("today", "Due today", icons::SUN, ORANGE, &due_today, p.show_due_today),
                        ("tomorrow", "Due tomorrow", icons::ARROW_RIGHT, ORANGE, &due_tomorrow, p.show_due_tomorrow),
                        ("soon", "Due soon", icons::CHEVRONS_RIGHT, ORANGE, &due_soon, p.show_due_soon),
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
                    for (id, name, color, items) in &lists {
                        self.section(ui, ctx, &format!("list{id}"), name, None, *color, items, Some(*id));
                    }
                    if !done_ids.is_empty() {
                        self.section(ui, ctx, "done", "Completed", Some((icons::CHECK, muted)), muted, &done_ids, None);
                    }
                    ui.add_space(10.0);
                    if let Some(mut name) = self.new_list_text.take() {
                        let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("List name").desired_width(f32::INFINITY));
                        r.request_focus();
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if enter && !name.trim().is_empty() {
                            self.snapshot();
                            self.new_list(name.trim());
                            self.commit();
                        } else if !esc && !enter {
                            self.new_list_text = Some(name);
                        }
                    }
                    ui.add_space(20.0);
                });
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
        let glow = ctx.animate_bool_with_time(Id::new(("sec", key)), resp.hovered(), ANIM_FAST_S);
        if glow > 0.0 {
            fill_squircle(ui.painter(), r.expand2(vec2(6.0, 0.0)), RADIUS, mix(side, text, 0.05 * glow));
        }
        let p = ui.painter();
        let ic = pos2(r.left() + 9.0, r.center().y);
        match icon {
            Some((g, c)) => {
                p.text(ic, Align2::CENTER_CENTER, g, FontId::proportional(15.0), c);
            }
            None => {
                p.circle_stroke(ic, 5.5, Stroke::new(2.0, color));
            }
        }
        let g = p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, name, bold(15.0), text);
        let count = items.len().to_string();
        let pill = Rect::from_center_size(pos2(g.right() + 18.0, r.center().y), vec2(10.0 + 8.0 * count.len() as f32, 18.0));
        fill_squircle(p, pill, RADIUS, mix(side, text, 0.10));
        p.text(pill.center(), Align2::CENTER_CENTER, count, FontId::proportional(11.5), muted);
        // chevron turns as the group opens
        let turn = ctx.animate_bool_with_time(Id::new(("chev", key)), !collapsed, ANIM_S);
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
                let pr = Rect::from_center_size(pos2(r.right() - 38.0, r.center().y), vec2(22.0, 22.0));
                let over = ui.rect_contains_pointer(pr);
                ui.painter().text(pr.center(), Align2::CENTER_CENTER, "+", FontId::proportional(18.0), if over { text } else { muted.gamma_multiply(glow) });
                if over && resp.clicked() {
                    add_clicked = true;
                }
            }
        }
        if add_clicked {
            self.adding_to = list.map(|l| (l, String::new()));
            self.collapsed.remove(key);
        } else if resp.clicked() {
            if collapsed {
                self.collapsed.remove(key);
            } else {
                self.collapsed.insert(key.to_string());
            }
        }
        // a list can be renamed, recoloured or removed from its menu
        if let Some(lid) = list.filter(|l| *l != 0) {
            resp.context_menu(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(200.0);
                    for (col, cname) in EVENT_COLORS.iter() {
                        let (cr, cresp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                        ui.painter().circle_filled(cr.center(), 7.0, rgb(*col));
                        if cresp.on_hover_text(*cname).clicked() {
                            if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == lid) {
                                l.color = *col;
                            }
                            self.commit();
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                if ui.button(RichText::new("Delete list (tasks go to Inbox)").color(RED)).clicked() {
                    self.snapshot();
                    self.data.lists.retain(|l| l.id != lid);
                    for t in self.data.tasks.iter_mut().filter(|t| t.list == lid) {
                        t.list = 0;
                    }
                    self.commit();
                    ui.close_menu();
                }
            });
        }
        let a = ctx.animate_bool_with_time(Id::new(("open", key)), !collapsed, ANIM_S);
        if a > 0.0 {
            ui.scope(|ui| {
                ui.set_opacity(a);
                if let Some((l, mut textv)) = self.adding_to.take().filter(|(l, _)| Some(*l) == list) {
                    let r = ui.add(egui::TextEdit::singleline(&mut textv).hint_text("New task").desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5)));
                    r.request_focus();
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if enter {
                        self.add_task(&textv, l);
                        self.adding_to = Some((l, String::new()));
                    } else if !esc {
                        self.adding_to = Some((l, textv));
                    }
                } else if self.adding_to.as_ref().map(|(l, _)| Some(*l) != list).unwrap_or(false) {
                    // another list is being added to; leave it alone
                }
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
                    if ui.checkbox(&mut d, "").changed() {
                        self.toggle_task(id);
                        self.say(ctx, if d { "Task completed" } else { "Task reopened" }, true);
                    }
                    let mut title = RichText::new(&t.title).size(14.5);
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
                        if ui.button("Edit").clicked() {
                            self.edit_task(id);
                            ui.close_menu();
                        }
                        ui.menu_button("Move to list", |ui| {
                            let mut options = vec![(0u64, "Inbox".to_string())];
                            options.extend(self.data.lists.iter().map(|l| (l.id, l.name.clone())));
                            for (lid, name) in options {
                                if ui.selectable_label(t.list == lid, name).clicked() {
                                    self.snapshot();
                                    if let Some(x) = self.data.task_mut(id) {
                                        x.list = lid;
                                    }
                                    self.commit();
                                    ui.close_menu();
                                }
                            }
                        });
                        if ui.button(RichText::new("Delete task").color(RED)).clicked() {
                            self.delete_task(ctx, id);
                            ui.close_menu();
                        }
                    });
                });
                // small labels under the title
                let mut chips: Vec<(String, Color32)> = vec![];
                if scheduled {
                    chips.push(("scheduled".into(), muted));
                }
                if let Some(d) = t.due {
                    let c = if d < today && !t.done { RED } else { muted };
                    let rep = if t.repeat != Repeat::None { format!("  {}", icons::REPEAT) } else { String::new() };
                    chips.push((format!("{} {}{rep}", icons::FLAG, due_label(d, today)), c));
                }
                if let Some(m) = t.duration_min {
                    chips.push((fmt_duration(m), muted));
                }
                if t.priority != 0 {
                    chips.push((format!("! {}", priority_name(t.priority)), if t.priority <= 3 { RED } else { muted }));
                }
                for g in &t.tags {
                    chips.push((format!("#{g}"), muted));
                }
                if !chips.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(5.0, 3.0);
                        ui.add_space(27.0);
                        for (label, c) in chips {
                            egui::Frame::new().stroke(Stroke::new(1.0, border)).corner_radius(5).inner_margin(Margin::symmetric(5, 1)).show(ui, |ui| {
                                ui.label(RichText::new(label).size(11.5).color(c));
                            });
                        }
                    });
                }
            });
            let hovered = ui.rect_contains_pointer(inner.response.rect);
            let glow = ctx.animate_bool_with_time(Id::new(("row", salt, id)), hovered, ANIM_FAST_S);
            if glow > 0.0 {
                fill_squircle(ui.painter(), inner.response.rect.expand2(vec2(6.0, 0.0)), RADIUS, mix(side, text, 0.06).gamma_multiply(glow * 0.6));
            }
        });
    }
}
