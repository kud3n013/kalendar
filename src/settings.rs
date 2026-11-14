//! The settings window: a window of its own with a list of pages on the left.

use crate::icons;
use crate::model::*;
use crate::{bind_label, ics, mix, parse_bind, rgb, App, Cmd, KeyBind, COMMANDS};
use eframe::egui::Key;
use eframe::egui::{self, vec2, Align, Color32, FontId, Layout, Margin, RichText, Sense, Stroke};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    Shortcuts,
    Calendars,
    General,
    Appearance,
    ActiveCalendars,
    Planner,
    Info,
}

const GREEN: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x7a);
const RED: Color32 = Color32::from_rgb(0xeb, 0x62, 0x62);
const CONTROL_W: f32 = 390.0;
const ROW_H: f32 = 56.0;

fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// One settings line: the name on the left, its control in a wide box on the right.
fn row(ui: &mut egui::Ui, label: &str, tip: &str, add: impl FnOnce(&mut egui::Ui)) {
    let cw = (ui.available_width() * 0.42).clamp(170.0, CONTROL_W);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("control_w"), cw));
    ui.allocate_ui_with_layout(vec2(ui.available_width(), ROW_H), Layout::left_to_right(Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(label).size(17.0)).truncate());
        if !tip.is_empty() {
            ui.label(RichText::new("(i)").size(12.0).weak()).on_hover_text(tip);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.allocate_ui_with_layout(vec2(cw, 44.0), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().interact_size.y = 44.0;
                ui.spacing_mut().button_padding = vec2(14.0, 10.0);
                ui.style_mut().override_font_id = Some(FontId::proportional(16.0));
                add(ui);
            });
        });
    });
}

fn combo<T: PartialEq + Copy>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, String)]) {
    let current = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_default();
    let cw = ui.data(|d| d.get_temp::<f32>(egui::Id::new("control_w"))).unwrap_or(CONTROL_W);
    egui::ComboBox::from_id_salt(id).width(cw).selected_text(current).show_ui(ui, |ui| {
        for (v, l) in options {
            ui.selectable_value(value, *v, l);
        }
    });
}

fn divider(ui: &mut egui::Ui, color: Color32) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, color);
    ui.add_space(8.0);
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).font(bold(20.0)));
    ui.add_space(6.0);
}

fn hour_label(p: &Prefs, h: i64) -> String {
    let t = chrono::NaiveTime::from_hms_opt((h % 24) as u32, 0, 0).unwrap();
    let base = if p.hour12 {
        let (pm, h12) = chrono::Timelike::hour12(&t);
        format!("{h12}:00 {}", if pm { "PM" } else { "AM" })
    } else {
        format!("{:02}:00", h % 24)
    };
    if h >= 24 {
        format!("{base} (+1 day)")
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
        if !self.show_prefs || self.settings_page != Page::Shortcuts {
            self.recording = None;
        }
        if !self.show_prefs {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_app_id("kalendar")
            .with_inner_size([1060.0, 820.0])
            .with_min_inner_size([760.0, 480.0]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"), builder, |ctx, class| {
            if class == egui::ViewportClass::Embedded {
                // this system can't open a second window: fall back to one inside the app
                let mut open = true;
                egui::Window::new("Settings").open(&mut open).default_size([900.0, 640.0]).show(ctx, |ui| self.settings_body(ui));
                if !open {
                    self.show_prefs = false;
                }
                return;
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ctx, |ui| self.settings_body(ui));
            let (close, esc) = ctx.input(|i| (i.viewport().close_requested(), i.key_pressed(egui::Key::Escape)));
            if close || (esc && !ctx.wants_keyboard_input() && self.recording.is_none()) {
                self.show_prefs = false;
            }
        });
    }

    /// The whole content: page list on the left, the chosen page on the right.
    pub fn settings_body(&mut self, ui: &mut egui::Ui) {
        let before = serde_json::to_string(&(&self.data.prefs, &self.data.calendars)).unwrap_or_default();
        let (side, bg, muted, text, border) = (self.pal.side, self.pal.bg, self.pal.muted, self.pal.text, self.pal.border);
        let full = ui.max_rect();
        let nav_w = 264.0f32.min(full.width() * 0.4);
        let (nav_rect, page_rect) = full.split_left_right_at_x(full.left() + nav_w);
        ui.painter().rect_filled(nav_rect, 0.0, side);
        ui.painter().rect_filled(page_rect, 0.0, bg);
        ui.painter().line_segment([nav_rect.right_top(), nav_rect.right_bottom()], Stroke::new(1.0, border));

        // page list
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(nav_rect.shrink2(vec2(10.0, 0.0))), |ui| {
            egui::ScrollArea::vertical().id_salt("nav").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(26.0);
                for (group, pages) in [
                    ("Explore", vec![(Page::Shortcuts, icons::KEYBOARD, "Shortcuts")]),
                    ("Integrations", vec![(Page::Calendars, icons::CALENDAR, "Calendars")]),
                    (
                        "Preferences",
                        vec![
                            (Page::General, icons::SETTINGS, "General"),
                            (Page::Appearance, icons::PALETTE, "Appearance"),
                            (Page::ActiveCalendars, icons::SQUARE_CHECK, "Active calendars"),
                            (Page::Planner, icons::SHUFFLE, "Planner"),
                        ],
                    ),
                    ("About", vec![(Page::Info, icons::INFO, "Info")]),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new(group).size(16.0).color(muted));
                    });
                    ui.add_space(6.0);
                    for (page, icon, name) in pages {
                        let on = self.settings_page == page;
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                        let glow = ui.ctx().animate_bool_with_time(ui.id().with(("nav", name)), resp.hovered() || on, ANIM_FAST_S);
                        if glow > 0.0 {
                            ui.painter().rect_filled(r, 9.0, mix(side, text, if on { 0.10 } else { 0.05 } * glow));
                        }
                        ui.painter().text(r.left_center() + vec2(28.0, 0.0), egui::Align2::CENTER_CENTER, icon, FontId::proportional(16.0), muted);
                        ui.painter().text(r.left_center() + vec2(52.0, 0.0), egui::Align2::LEFT_CENTER, name, FontId::proportional(17.0), text);
                        if resp.clicked() {
                            self.settings_page = page;
                        }
                    }
                    ui.add_space(20.0);
                }
            });
        });

        // the page
        let ctx = ui.ctx().clone();
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(page_rect), |ui| {
            // close button in the corner
            let x = egui::Rect::from_min_size(page_rect.right_top() + vec2(-44.0, 10.0), vec2(30.0, 30.0));
            let resp = ui.interact(x, ui.id().with("close"), Sense::click());
            let c = if resp.hovered() { text } else { muted };
            ui.painter().line_segment([x.center() + vec2(-6.0, -6.0), x.center() + vec2(6.0, 6.0)], Stroke::new(1.6, c));
            ui.painter().line_segment([x.center() + vec2(-6.0, 6.0), x.center() + vec2(6.0, -6.0)], Stroke::new(1.6, c));
            if resp.clicked() {
                self.show_prefs = false;
            }
            egui::ScrollArea::vertical().id_salt(("page", self.settings_page as u8)).auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: 44, right: 56, top: 30, bottom: 30 }).show(ui, |ui| {
                    let w = ui.available_width().min(980.0);
                    ui.set_max_width(w);
                    match self.settings_page {
                        Page::General => self.page_general(ui),
                        Page::Appearance => self.page_appearance(ui),
                        Page::Planner => self.page_planner(ui),
                        Page::ActiveCalendars => self.page_active(ui),
                        Page::Calendars => self.page_calendars(ui, &ctx),
                        Page::Shortcuts => self.page_shortcuts(ui),
                        Page::Info => self.page_info(ui),
                    }
                });
            });
        });
        if before != serde_json::to_string(&(&self.data.prefs, &self.data.calendars)).unwrap_or_default() {
            self.commit();
        }
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        let border = self.pal.border;
        let p = &mut self.data.prefs;
        row(ui, "Move task due date", "When a task is scheduled after its due date: move the due date, ask, or leave it.", |ui| {
            combo(ui, "dm", &mut p.due_move, &[(DueMove::Always, "Always".into()), (DueMove::Ask, "Ask every time".into()), (DueMove::Never, "Never".into())]);
        });
        row(ui, "Automatically assign due dates", "A task put on the calendar gets that day as its due date.", |ui| {
            combo(ui, "ad", &mut p.auto_due, &[(true, "Yes".into()), (false, "No".into())]);
        });
        divider(ui, border);
        row(ui, "Time format", "", |ui| {
            combo(ui, "tf", &mut p.hour12, &[(false, "24 hours".into()), (true, "12 hours (AM/PM)".into())]);
        });
        row(ui, "First day of the week", "", |ui| {
            let names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
            let opts: Vec<(u8, String)> = names.iter().enumerate().map(|(i, n)| (i as u8, n.to_string())).collect();
            combo(ui, "fd", &mut p.first_day, &opts);
        });
        row(ui, "Initial calendar view", "", |ui| {
            let opts: Vec<(View, String)> = View::all().into_iter().map(|v| (v, v.label())).collect();
            combo(ui, "iv", &mut p.initial_view, &opts);
        });
        divider(ui, border);
        let snapshot = p.clone();
        row(ui, "Time grid start", "", |ui| {
            let opts: Vec<(i64, String)> = (0..24).map(|h| (h, hour_label(&snapshot, h))).collect();
            combo(ui, "gs", &mut p.grid_start_h, &opts);
        });
        row(ui, "Time grid end", "", |ui| {
            let opts: Vec<(i64, String)> = (1..=24).map(|h| (h, hour_label(&snapshot, h))).collect();
            combo(ui, "ge", &mut p.grid_end_h, &opts);
        });
        if p.grid_end_h <= p.grid_start_h {
            p.grid_end_h = p.grid_start_h + 1;
        }
        let mins = |m: i64| if m == 60 { "1 hour".to_string() } else { format!("{m} minutes") };
        row(ui, "Time display resolution", "", |ui| {
            let before = p.slot_min;
            let opts: Vec<(i64, String)> = SLOT_CHOICES.iter().map(|m| (*m, mins(*m))).collect();
            combo(ui, "sl", &mut p.slot_min, &opts);
            if p.slot_min != before {
                p.hour_height_px = None;
            }
        });
        row(ui, "Time dragging resolution", "", |ui| {
            let opts: Vec<(i64, String)> = SNAP_CHOICES.iter().map(|m| (*m, mins(*m))).collect();
            combo(ui, "sn", &mut p.snap_min, &opts);
        });
        row(ui, "Limit events per day", "How many events a day shows in the month view before \"+ more\".", |ui| {
            let opts: Vec<(usize, String)> = (2..=20).map(|n| (n, n.to_string())).collect();
            combo(ui, "dx", &mut p.day_max_events, &opts);
        });
        divider(ui, border);
        row(ui, "Undo grace period", "", |ui| {
            let opts: Vec<(u32, String)> = UNDO_CHOICES.iter().map(|s| (*s, if *s == 0 { "Disabled".into() } else { format!("{s} seconds") })).collect();
            combo(ui, "ug", &mut p.undo_grace_s, &opts);
        });
        divider(ui, border);
        let yes_no = [(true, "Yes".to_string()), (false, "No".to_string())];
        row(ui, "Show weekends", "", |ui| combo(ui, "we", &mut p.weekends_visible, &yes_no));
        row(ui, "Dim past events", "", |ui| combo(ui, "dp", &mut p.dim_past, &yes_no));
        row(ui, "Show completed tasks", "On the calendar.", |ui| combo(ui, "ct", &mut p.show_completed_tasks, &yes_no));
        row(ui, "Show scheduled tasks in the list", "", |ui| combo(ui, "st", &mut p.show_scheduled_tasks, &yes_no));
        row(ui, "Due dates on the calendar", "", |ui| combo(ui, "dc", &mut p.due_on_calendar, &yes_no));
    }

    fn page_appearance(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Appearance");
        let p = &mut self.data.prefs;
        row(ui, "Theme", "", |ui| {
            combo(ui, "th", &mut p.theme, &[(Theme::System, "System".into()), (Theme::Light, "Light".into()), (Theme::Dark, "Dark".into())]);
        });
        row(ui, "Task panel position", "Which side of the window the task panel sits on.", |ui| {
            combo(ui, "ps", &mut p.sidebar_side, &[(PanelSide::Left, "Left".into()), (PanelSide::Right, "Right".into())]);
        });
    }

    fn page_planner(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        heading(ui, "Planner");
        ui.label(RichText::new("\"Plan my tasks\" fits open tasks into free time on working days, 08:00 to 18:00.").size(15.0).color(muted));
        ui.add_space(10.0);
        let p = &mut self.data.prefs;
        row(ui, "Default duration", "Used for tasks without a time estimate.", |ui| {
            let opts: Vec<(i64, String)> = [15, 30, 45, 60, 90, 120].iter().map(|m| (*m, fmt_duration(*m))).collect();
            combo(ui, "pd", &mut p.default_task_min, &opts);
        });
        let yes_no = [(true, "Yes".to_string()), (false, "No".to_string())];
        row(ui, "Split long tasks", "Tasks of two hours or more may be cut into several sessions.", |ui| combo(ui, "ps", &mut p.split_long_tasks, &yes_no));
        row(ui, "Schedule tasks with no due date", "", |ui| combo(ui, "pn", &mut p.plan_without_due, &yes_no));
    }

    /// Manage calendars: show or hide, rename, recolour, add and remove.
    fn page_active(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (muted, border, side, text) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text);
        heading(ui, "Active calendars");
        ui.add(egui::Label::new(RichText::new("Show or hide calendars, rename them, change their colour, add new ones or remove them.").size(15.0).color(muted)).wrap());
        ui.add_space(12.0);
        // add a calendar of one's own
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.new_cal_name)
                    .hint_text("New calendar name")
                    .font(FontId::proportional(15.0))
                    .margin(Margin::symmetric(10, 8))
                    .desired_width((ui.available_width() - 330.0).clamp(120.0, 300.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let b = egui::Button::new(RichText::new("+ Add calendar").size(15.0).color(Color32::WHITE)).fill(self.pal.accent).corner_radius(8);
            if (ui.add(b).clicked() || enter) && !self.new_cal_name.trim().is_empty() {
                self.snapshot();
                let n = self.data.calendars.len();
                let name = std::mem::take(&mut self.new_cal_name);
                self.data.calendars.push(Calendar {
                    name: name.trim().to_string(),
                    color: EVENT_COLORS[(n * 3 + 1) % EVENT_COLORS.len()].0,
                    visible: true,
                    ..Default::default()
                });
                self.commit();
            }
            if ui.add(egui::Button::new(RichText::new("Subscribe by link…").size(15.0)).corner_radius(8)).clicked() {
                self.settings_page = Page::Calendars;
            }
        });
        ui.add_space(10.0);
        let own_count = self.data.calendars.iter().filter(|c| c.url.is_none()).count();
        let counts: Vec<usize> = (0..self.data.calendars.len()).map(|i| self.data.events.iter().filter(|e| e.calendar == i).count()).collect();
        let mut remove: Option<usize> = None;
        let mut ask: Option<Option<usize>> = None;
        for (i, c) in self.data.calendars.iter_mut().enumerate() {
            let subscribed = c.url.is_some();
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 52.0), Layout::left_to_right(Align::Center), |ui| {
                ui.checkbox(&mut c.visible, "").on_hover_text("Show on the calendar");
                // colour: a dot that opens the palette
                let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                ui.painter().circle_filled(r.center(), 9.0, rgb(c.color));
                let popup = ui.make_persistent_id(("calcolor", i));
                if resp.on_hover_text("Colour").clicked() {
                    ui.memory_mut(|m| m.toggle_popup(popup));
                }
                egui::popup_below_widget(ui, popup, &ui.interact(r, ui.id().with(("cc", i)), Sense::hover()), egui::PopupCloseBehavior::CloseOnClick, |ui| {
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
                ui.add(egui::TextEdit::singleline(&mut c.name).font(FontId::proportional(16.0)).desired_width((ui.available_width() - 330.0).clamp(110.0, 280.0)).id_salt(("calname", i)));
                let what = if subscribed { "subscribed, read-only".to_string() } else { format!("{} events", counts[i]) };
                ui.label(RichText::new(what).size(13.0).color(muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.confirm_remove == Some(i) {
                        if ui.add(egui::Button::new(RichText::new("Cancel").size(14.0)).corner_radius(7)).clicked() {
                            ask = Some(None);
                        }
                        let label = if subscribed { "Unsubscribe".to_string() } else { format!("Remove with its {} events", counts[i]) };
                        if ui.add(egui::Button::new(RichText::new(label).size(14.0).color(Color32::WHITE)).fill(RED).corner_radius(7)).clicked() {
                            remove = Some(i);
                        }
                    } else {
                        let last_own = !subscribed && own_count <= 1;
                        let b = egui::Button::new(RichText::new(if subscribed { "Unsubscribe" } else { "Remove" }).size(14.0).color(RED)).fill(mix(side, text, 0.07)).corner_radius(7);
                        let resp = ui.add_enabled(!last_own, b);
                        if last_own {
                            resp.on_disabled_hover_text("At least one calendar of your own is needed");
                        } else if resp.clicked() {
                            ask = Some(Some(i));
                        }
                    }
                });
            });
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(a) = ask {
            self.confirm_remove = a;
        }
        if let Some(i) = remove {
            self.snapshot();
            let name = self.data.calendars[i].name.clone();
            self.data.remove_calendar(i);
            self.confirm_remove = None;
            self.selected = None;
            self.editor = None;
            self.commit();
            self.say(&ctx, format!("Calendar \"{name}\" removed"), true);
        }
    }

    fn page_calendars(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (muted, border, side, text) = (self.pal.muted, self.pal.border, self.pal.side, self.pal.text);
        heading(ui, "Integrate your calendars");
        ui.label(RichText::new("Bring other calendars in by subscribing to their link. Subscribed calendars are read-only.").size(16.0).color(muted));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (kind, label) in [(1u8, "Calendar feed"), (2u8, "Proton Calendar")] {
                let on = self.sub_kind == kind;
                let b = egui::Button::new(RichText::new(format!("   {label}")).size(17.0))
                    .min_size(vec2(((ui.available_width() - 12.0) / 2.0).clamp(150.0, 260.0), 52.0))
                    .corner_radius(8)
                    .fill(mix(side, text, if on { 0.16 } else { 0.09 }));
                if ui.add(b).clicked() {
                    self.sub_kind = if on { 0 } else { kind };
                }
            }
        });
        if self.sub_kind != 0 {
            ui.add_space(12.0);
            egui::Frame::new().fill(mix(side, text, 0.04)).stroke(Stroke::new(1.0, border)).corner_radius(10).inner_margin(Margin::same(16)).show(ui, |ui| {
                let help = if self.sub_kind == 2 {
                    "In Proton Calendar open Settings → Calendars, pick the calendar, choose \"Share with anyone\" and create a link. Paste that link here."
                } else {
                    "Paste the calendar's link. It starts with https:// or webcal:// and usually ends in .ics."
                };
                ui.label(RichText::new(help).size(14.0).color(muted));
                ui.add_space(6.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.sub_url)
                        .hint_text(if self.sub_kind == 2 { "https://calendar.proton.me/api/calendar/v1/url/…" } else { "https://…/calendar.ics" })
                        .font(FontId::proportional(15.0))
                        .margin(Margin::symmetric(10, 8))
                        .desired_width(f32::INFINITY),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let normal = ics::normalize_url(&self.sub_url).ok();
                if normal.as_deref().map(ics::is_proton).unwrap_or(false) {
                    ui.label(RichText::new("Proton Calendar link recognised").size(13.0).color(GREEN));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.sub_name).hint_text("Name (optional)").font(FontId::proportional(15.0)).margin(Margin::symmetric(10, 8)).desired_width(260.0));
                    let b = egui::Button::new(RichText::new("Subscribe").size(15.0).color(Color32::WHITE)).fill(self.pal.accent).corner_radius(8);
                    if ui.add(b).clicked() || enter {
                        let count = self.data.calendars.len();
                        self.subscribe(ctx);
                        if self.data.calendars.len() > count {
                            self.sub_kind = 0;
                        }
                    }
                });
            });
        }
        divider(ui, border);
        heading(ui, "Manage integrations");
        let mut refresh: Option<String> = None;
        let mut remove: Option<usize> = None;
        let subs: Vec<(usize, Calendar)> = self.data.calendars.iter().cloned().enumerate().filter(|(_, c)| c.url.is_some()).collect();
        if subs.is_empty() {
            ui.label(RichText::new("No subscribed calendars yet.").size(15.0).color(muted));
        }
        for (i, c) in subs {
            let url = c.url.clone().unwrap_or_default();
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 70.0), Layout::left_to_right(Align::Center), |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 7.0, rgb(c.color));
                ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(1.5, muted));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(12.0);
                    ui.label(RichText::new(&c.name).size(17.0));
                    let kind = if ics::is_proton(&url) { "Proton Calendar".to_string() } else { format!("Feed from {}", ics::default_name(&url)) };
                    let when = c.last_sync.as_ref().map(|t| format!("  ·  refreshed {t}")).unwrap_or_default();
                    ui.label(RichText::new(format!("{kind}{when}")).size(14.0).color(muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button(RichText::new(icons::ELLIPSIS).size(18.0).color(muted), |ui| {
                        if ui.button("Refresh").clicked() {
                            refresh = Some(url.clone());
                            ui.close_menu();
                        }
                        if ui.button(RichText::new("Unsubscribe").color(RED)).clicked() {
                            remove = Some(i);
                            ui.close_menu();
                        }
                    });
                    ui.add_space(60.0);
                    if self.syncing.contains(&url) {
                        ui.label(RichText::new("Refreshing…").size(16.0).color(muted));
                        ui.spinner();
                    } else if let Some(e) = &c.error {
                        ui.label(RichText::new("Not connected").size(16.0).color(RED)).on_hover_text(e);
                    } else if c.last_sync.is_some() {
                        ui.label(RichText::new(format!("{} Connected", icons::CHECK)).size(16.0).color(GREEN));
                    } else {
                        ui.label(RichText::new("Waiting").size(16.0).color(muted));
                    }
                });
            });
            if let Some(e) = &c.error {
                ui.label(RichText::new(e).size(13.0).color(RED));
            }
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, border);
        }
        if let Some(u) = refresh {
            self.sync_one(ctx, &u);
        }
        if let Some(i) = remove {
            self.snapshot();
            self.data.remove_calendar(i);
            self.commit();
        }
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
            egui::Frame::new().fill(fill).corner_radius(RADIUS as u8).inner_margin(Margin::symmetric(9, 5)).show(ui, |ui| {
                ui.label(RichText::new(k).size(15.0).color(color));
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
                    ui.label(RichText::new("or").size(15.0).color(muted));
                }
            }
        };
        let fixed_row = |ui: &mut egui::Ui, label: &str, keys: &str| {
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 44.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(label).size(17.0).color(mix(muted, text, 0.6)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    chips(ui, keys);
                });
            });
        };
        let section_head = |ui: &mut egui::Ui, sec: &str, first: bool| {
            if !first {
                ui.add_space(22.0);
            }
            ui.label(RichText::new(sec).font(bold(20.0)));
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, muted);
            ui.add_space(10.0);
        };
        ui.label(RichText::new("Click a shortcut to change it, then press the new keys. Esc cancels.").size(15.0).color(muted));
        ui.add_space(10.0);
        // every command's keys, to warn when two share one
        let all: Vec<(Cmd, &str, Vec<KeyBind>)> = COMMANDS.iter().map(|c| (c.0, c.1, self.binds(c.0))).collect();
        let mut section = "";
        for (cmd, label, default, sec) in COMMANDS {
            if *cmd == Cmd::Paste {
                // pasting is done with the mouse
                continue;
            }
            if *sec != section {
                section_head(ui, sec, section.is_empty());
                section = sec;
            }
            let keys = self.bind_text(*cmd);
            let custom = self.data.prefs.keybinds.contains_key(cmd.id());
            let mine = parse_bind(&keys);
            let clash: Vec<&str> =
                all.iter().filter(|(c, _, bs)| c != cmd && bs.iter().any(|b| mine.contains(b))).map(|(_, l, _)| *l).collect();
            let recording = self.recording == Some(*cmd);
            let (mut start, mut reset, mut clear) = (false, false, false);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 44.0), Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(*label).size(17.0).color(mix(muted, text, 0.6)));
                if !clash.is_empty() {
                    ui.label(RichText::new(icons::INFO).size(14.0).color(RED)).on_hover_text(format!("Also used by: {}", clash.join(", ")));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // reset and clear sit to the right of the keys
                    if custom {
                        let r = ui.add(egui::Button::new(RichText::new(icons::UNDO).size(14.0).color(muted)).frame(false));
                        reset = r.on_hover_text(format!("Back to the default ({})", if default.is_empty() { "none" } else { default })).clicked();
                    }
                    if !keys.is_empty() {
                        let r = ui.add(egui::Button::new(RichText::new(icons::X).size(14.0).color(muted)).frame(false));
                        clear = r.on_hover_text("Remove this shortcut").clicked();
                    }
                    let inner = ui.scope(|ui| {
                        if recording {
                            chip(ui, "Press keys…", mix(side, accent, 0.35), text);
                        } else if keys.is_empty() {
                            chip(ui, "Not set", mix(side, text, 0.06), muted);
                        } else {
                            chips(ui, &keys);
                        }
                    });
                    let r = ui.interact(inner.response.rect, ui.id().with(("bind", cmd.id())), Sense::click());
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    start = r.on_hover_text("Click, then press the new keys").clicked();
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
        section_head(ui, "Fixed", false);
        fixed_row(ui, "Paste here", "Right-click");
        fixed_row(ui, "Discard the creation of an event", "Esc");
        fixed_row(ui, "Save changes", "Ctrl+Enter");
        fixed_row(ui, "Zoom the hours", "Ctrl+Wheel");
        fixed_row(ui, "Move sideways by days", "Shift+Wheel");
        if self.recording.is_some() {
            ctx.request_repaint();
        }
    }

    fn page_info(&mut self, ui: &mut egui::Ui) {
        let muted = self.pal.muted;
        heading(ui, "Kalendar");
        ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(16.0));
        ui.add_space(8.0);
        ui.label(RichText::new("Your calendars and tasks are kept on this computer, in:").size(15.0).color(muted));
        ui.label(RichText::new(data_path().display().to_string()).size(15.0).monospace());
        ui.add_space(8.0);
        ui.label(RichText::new("Your settings and shortcuts are kept in:").size(15.0).color(muted));
        ui.label(RichText::new(config_path().display().to_string()).size(15.0).monospace());
        ui.add_space(8.0);
        ui.label(
            RichText::new("Links of subscribed calendars are stored in data.json as plain text. A Proton Calendar link contains the keys that give access to that calendar, so keep that file private.")
                .size(15.0)
                .color(muted),
        );
    }
}
