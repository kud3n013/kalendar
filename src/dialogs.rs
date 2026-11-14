//! Pop-up windows: editors, preferences, shortcuts, command bar, messages.

use crate::icons;
use crate::model::*;
use crate::views::midnight;
use crate::{mix, rgb, App, Cmd, CreateKind, COMMANDS};
use chrono::{Duration, Local, NaiveDate, Timelike};
use eframe::egui::{self, pos2, Align, Align2, FontId, Id, Layout, Margin, RichText, Stroke};

/// Where a window of about `size` goes beside the block `r`: to its right,
/// else its left, else below, else above, kept on screen. Once the window
/// has been drawn its measured size is used instead of the guess.
pub fn place_beside(ctx: &egui::Context, id: Id, r: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    let size = ctx.memory(|m| m.area_rect(id)).map(|a| a.size()).unwrap_or(size);
    let (w, h, gap) = (size.x, size.y, 10.0);
    let screen = ctx.content_rect().shrink(8.0);
    let pos = if r.right() + gap + w <= screen.right() {
        pos2(r.right() + gap, r.top())
    } else if r.left() - gap - w >= screen.left() {
        pos2(r.left() - gap - w, r.top())
    } else if r.bottom() + gap + h <= screen.bottom() {
        pos2(r.center().x - w / 2.0, r.bottom() + gap)
    } else {
        pos2(r.center().x - w / 2.0, r.top() - gap - h)
    };
    pos2(
        pos.x.clamp(screen.left(), (screen.right() - w).max(screen.left())),
        pos.y.clamp(screen.top(), (screen.bottom() - h).max(screen.top())),
    )
}

impl App {
    pub fn ui_dialogs(&mut self, ctx: &egui::Context) {
        self.dlg_create(ctx);
        self.dlg_event(ctx);
        self.dlg_task(ctx);
        self.dlg_frame(ctx);
        self.ui_settings_window(ctx);
        self.dlg_command(ctx);
        self.dlg_new_task(ctx);
        self.dlg_toast(ctx);
    }

    /// The small window beside a block drawn on the calendar: it saves the
    /// block as a task session (the default) or as a plain event, into one
    /// chosen calendar, or turns it into a frame in the frame editor.
    fn dlg_create(&mut self, ctx: &egui::Context) {
        let Some(mut c) = self.create.take() else { return };
        let (mut save, mut cancel, mut to_frame) = (false, false, false);
        let muted = self.pal.muted;
        let p = self.data.prefs.clone();
        let w = 280.0;
        let pos = place_beside(ctx, Id::unique("create_popup"), c.anchor_rect, egui::vec2(w, 240.0));
        egui::Window::new("create")
            .id(Id::unique("create_popup"))
            .title_bar(false)
            .resizable(false)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_width(w);
                // what the block becomes: a task session, a plain event or a frame
                ui.horizontal(|ui| {
                    for (kind, label) in [(CreateKind::Task, "Task"), (CreateKind::Event, "Event"), (CreateKind::Frame, "Frame")] {
                        let r = ui.selectable_label(c.kind == kind, RichText::new(label).size(13.5));
                        let r = if kind == CreateKind::Frame { r.on_hover_text("Reserve this time every week for a kind of work") } else { r };
                        if r.clicked() {
                            c.kind = kind;
                            to_frame = kind == CreateKind::Frame;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} – {}", p.fmt_time(c.ev.start.time()), p.fmt_time(c.ev.end.time()))).size(12.0).color(muted));
                    });
                });
                let hint = if c.kind == CreateKind::Event { "New Event" } else { "New Task" };
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut c.ev.title)
                        .hint_text(hint)
                        .font(FontId::proportional(16.0))
                        .desired_width(f32::INFINITY),
                );
                if c.focus {
                    resp.request_focus();
                    c.focus = false;
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(2.0);
                ui.label(RichText::new(c.ev.start.format("%a, %b %-d").to_string()).size(12.5).color(muted));
                ui.add_space(2.0);
                // one calendar, picked with a box per calendar
                ui.label(RichText::new("Calendar").size(12.0).color(muted));
                for (i, cal) in self.data.calendars.iter().enumerate().filter(|(_, cal)| cal.url.is_none()) {
                    let mut on = c.ev.calendar == i;
                    if ui.checkbox(&mut on, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color))).changed() && on {
                        c.ev.calendar = i;
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).on_hover_text("Enter").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        cancel = true;
                    }
                });
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
            });
        if to_frame {
            let day = c.ev.start.date();
            let s = (c.ev.start - midnight(day)).num_minutes();
            let e = if c.ev.end.date() == day { (c.ev.end - midnight(day)).num_minutes() } else { 24 * 60 };
            self.open_new_frame(day, s, e, c.ev.calendar, c.ev.title.clone());
        } else if save {
            self.snapshot();
            let mut ev = c.ev.clone();
            if ev.end <= ev.start {
                ev.end = ev.start + Duration::minutes(self.data.prefs.snap_min);
            }
            let id = ev.id;
            let day = ev.start.date();
            if c.kind == CreateKind::Task {
                // the block becomes a session of a new task
                let task_id = self.data.new_id();
                self.data.tasks.insert(
                    0,
                    Task {
                        id: task_id,
                        title: ev.title.clone(),
                        duration_min: Some(ev.minutes()),
                        ..Default::default()
                    },
                );
                ev.task_id = Some(task_id);
                self.data.events.push(ev);
                self.after_task_scheduled(ctx, task_id, day);
            } else {
                self.data.events.push(ev);
            }
            self.selected = Some(id);
            self.commit();
        } else if !cancel {
            self.create = Some(c);
        }
    }

    /// The frame editor: name, calendar, colour, hours, weekdays, repeat,
    /// end date, which tasks it takes, and whether it keeps others out.
    fn dlg_frame(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.frame_editor.take() else { return };
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let muted = self.pal.muted;
        let cal_color = self.data.calendar_color(ed.frame.calendar);
        // every tag in use, plus the ones the frame already asks for
        let mut all_tags: Vec<String> = self.data.tasks.iter().flat_map(|t| t.tags.iter().cloned()).collect();
        all_tags.extend(ed.frame.filter.tags.iter().cloned());
        all_tags.sort_by_key(|t| t.to_lowercase());
        all_tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let mut lists: Vec<(u64, String, [u8; 3])> = vec![(0, "Inbox".into(), [128, 128, 128])];
        lists.extend(self.data.lists.iter().map(|l| (l.id, l.name.clone(), l.color)));
        let hour12 = self.data.prefs.hour12;
        egui::Window::new(if ed.is_new { "New frame" } else { "Frame" })
            .id(Id::unique("frame_editor"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, -30.0])
            .show(ctx, |ui| {
                ui.set_width(380.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut ed.frame.name)
                        .hint_text("Frame name, e.g. Deep work")
                        .font(FontId::proportional(18.0))
                        .desired_width(f32::INFINITY),
                );
                if ed.focus {
                    r.request_focus();
                    ed.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(4.0);
                let label = |ui: &mut egui::Ui, t: &str| {
                    ui.label(RichText::new(t).color(muted));
                };
                egui::Grid::new("framegrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    label(ui, "Calendar");
                    let cname = |i: usize| self.data.calendars.get(i).map(|c| c.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("fcal").selected_text(cname(ed.frame.calendar)).show_ui(ui, |ui| {
                        for (i, cal) in self.data.calendars.iter().enumerate().filter(|(_, c)| c.url.is_none()) {
                            ui.selectable_value(&mut ed.frame.calendar, i, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color)));
                        }
                    });
                    ui.end_row();

                    label(ui, "Colour");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let swatch = |ui: &mut egui::Ui, col: [u8; 3], on: bool, tip: &str| {
                            let (cr, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                            ui.painter().circle_filled(cr.center(), 7.0, rgb(col));
                            if on {
                                ui.painter().circle_stroke(cr.center(), 8.5, Stroke::new(1.5_f32, ui.visuals().text_color()));
                            }
                            resp.on_hover_text(tip).clicked()
                        };
                        if swatch(ui, cal_color, ed.frame.color.is_none(), "Calendar colour") {
                            ed.frame.color = None;
                        }
                        ui.add_space(6.0);
                        for (col, cname) in EVENT_COLORS.iter() {
                            if swatch(ui, *col, ed.frame.color == Some(*col), cname) {
                                ed.frame.color = Some(*col);
                            }
                        }
                    });
                    ui.end_row();

                    // hours, in 5-minute steps; a frame ends on its own day
                    let time = |ui: &mut egui::Ui, m: &mut i64, lo: i64, hi: i64| {
                        let (mut h, mut mm) = (*m / 60, *m % 60);
                        ui.add(egui::DragValue::new(&mut h).range(0..=24).speed(0.1).custom_formatter(|v, _| {
                            let v = v as i64;
                            if hour12 {
                                format!("{}", if v % 12 == 0 { 12 } else { v % 12 })
                            } else {
                                format!("{v:02}")
                            }
                        }));
                        ui.label(":");
                        ui.add(egui::DragValue::new(&mut mm).range(0..=55).speed(0.2).custom_formatter(|v, _| format!("{:02}", v as i64 / 5 * 5)));
                        if hour12 {
                            ui.label(RichText::new(if h % 24 < 12 { "am" } else { "pm" }).color(muted));
                        }
                        *m = (h * 60 + mm / 5 * 5).clamp(lo, hi);
                    };
                    label(ui, "Time");
                    ui.horizontal(|ui| {
                        time(ui, &mut ed.frame.start_min, 0, 24 * 60 - 15);
                        ui.label(RichText::new("to").color(muted));
                        let lo = ed.frame.start_min + 15;
                        time(ui, &mut ed.frame.end_min, lo, 24 * 60);
                    });
                    ui.end_row();

                    label(ui, "Days");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (i, d) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate() {
                            let bit = 1u8 << i;
                            if ui.selectable_label(ed.frame.days & bit != 0, *d).clicked() {
                                ed.frame.days ^= bit;
                            }
                        }
                    });
                    ui.end_row();

                    label(ui, "Repeat");
                    ui.horizontal(|ui| {
                        ui.label("every");
                        ui.add(egui::DragValue::new(&mut ed.frame.every_weeks).range(1..=8).speed(0.05));
                        ui.label(if ed.frame.every_weeks == 1 { "week" } else { "weeks" });
                        ui.label(RichText::new(format!("from {}", ed.frame.from.format("%b %-d"))).color(muted));
                    });
                    ui.end_row();

                    label(ui, "Until");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut ed.until_text).hint_text("YYYY-MM-DD").desired_width(100.0));
                        if !ed.until_text.is_empty() && ui.small_button("Forever").clicked() {
                            ed.until_text.clear();
                        }
                    });
                    ui.end_row();

                    if !ed.frame.skip.is_empty() {
                        label(ui, "Skipped");
                        ui.horizontal(|ui| {
                            let n = ed.frame.skip.len();
                            ui.label(format!("{n} day{}", if n == 1 { "" } else { "s" }));
                            if ui.small_button("Bring back").clicked() {
                                ed.frame.skip.clear();
                            }
                        });
                        ui.end_row();
                    }
                });

                ui.add_space(6.0);
                ui.label(RichText::new("Tasks it takes").strong());
                ui.label(RichText::new("Nothing picked means any task.").size(11.5).color(muted));
                ui.add_space(2.0);
                egui::Grid::new("framefilter").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    label(ui, "Lists");
                    ui.horizontal_wrapped(|ui| {
                        for (id, name, col) in &lists {
                            let on = ed.frame.filter.lists.contains(id);
                            let text = RichText::new(format!("{} {name}", icons::CIRCLE)).color(if on { rgb(*col) } else { muted });
                            if ui.selectable_label(on, text).clicked() {
                                if on {
                                    ed.frame.filter.lists.retain(|l| l != id);
                                } else {
                                    ed.frame.filter.lists.push(*id);
                                }
                            }
                        }
                    });
                    ui.end_row();

                    label(ui, "Tags");
                    ui.horizontal_wrapped(|ui| {
                        if all_tags.is_empty() {
                            ui.label(RichText::new("No tags yet").color(muted));
                        }
                        for tag in &all_tags {
                            let on = ed.frame.filter.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
                            if ui.selectable_label(on, format!("#{tag}")).clicked() {
                                if on {
                                    ed.frame.filter.tags.retain(|t| !t.eq_ignore_ascii_case(tag));
                                } else {
                                    ed.frame.filter.tags.push(tag.clone());
                                }
                            }
                        }
                    });
                    ui.end_row();

                    if ed.frame.filter.tags.len() > 1 {
                        label(ui, "Match");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut ed.frame.filter.tags_all, false, "Any tag");
                            ui.selectable_value(&mut ed.frame.filter.tags_all, true, "All tags");
                        });
                        ui.end_row();
                    }

                    label(ui, "Up to");
                    let dur = |d: Option<i64>| d.map(fmt_duration).unwrap_or_else(|| "Any length".into());
                    egui::ComboBox::from_id_salt("fmax").selected_text(dur(ed.frame.filter.max_min)).show_ui(ui, |ui| {
                        for v in [None, Some(15), Some(30), Some(45), Some(60), Some(90), Some(120), Some(180), Some(240)] {
                            ui.selectable_value(&mut ed.frame.filter.max_min, v, dur(v));
                        }
                    });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.checkbox(&mut ed.frame.exclusive, "Keep other tasks out")
                    .on_hover_text("The planner puts no other task here");
                ui.checkbox(&mut ed.frame.enabled, "Enabled");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).on_hover_text("Ctrl+Enter").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        cancel = true;
                    }
                    if !ed.is_new {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(RichText::new("Delete").color(egui::Color32::from_rgb(0xeb, 0x62, 0x62))).clicked() {
                                delete = true;
                            }
                        });
                    }
                });
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
            });
        if save {
            let text = ed.until_text.trim();
            let until = if text.is_empty() {
                None
            } else if let Ok(d) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                Some(d)
            } else {
                self.say(ctx, "The end date should look like 2026-12-31", false);
                self.frame_editor = Some(ed);
                return;
            };
            if ed.frame.days == 0 {
                self.say(ctx, "Pick at least one day", false);
                self.frame_editor = Some(ed);
                return;
            }
            let mut f = ed.frame;
            f.until = until;
            if f.name.trim().is_empty() {
                f.name = "Frame".into();
            }
            // never a subscribed calendar
            if self.data.calendars.get(f.calendar).is_none_or(|c| c.url.is_some()) {
                f.calendar = self.data.first_own_calendar();
            }
            self.snapshot();
            match self.data.frame_mut(f.id) {
                Some(old) => *old = f,
                None => self.data.frames.push(f),
            }
            self.commit();
        } else if delete {
            self.delete_frame(ctx, ed.frame.id);
        } else if !cancel {
            self.frame_editor = Some(ed);
        }
    }

    /// Ctrl+T: a bar in the middle, like the command bar, for a new task.
    fn dlg_new_task(&mut self, ctx: &egui::Context) {
        if !self.new_task_open {
            return;
        }
        let muted = self.pal.muted;
        let focus = std::mem::take(&mut self.new_task_focus);
        let mut close = false;
        egui::Window::new("newtask")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icons::SQUARE_CHECK).size(16.0).color(muted));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.new_task_text)
                            .hint_text("New task   #tag  !high  >list  30m  .tomorrow")
                            .font(FontId::proportional(16.0))
                            .desired_width(f32::INFINITY),
                    );
                    if focus {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let t = std::mem::take(&mut self.new_task_text);
                            if !t.trim().is_empty() {
                                self.add_task(&t, 0);
                                close = true;
                            }
                        } else if ui.input(|i| i.pointer.any_click()) {
                            close = true;
                        }
                    }
                });
                ui.label(RichText::new("Enter adds the task to your Inbox  ·  Esc closes").size(11.5).color(muted));
            });
        if close {
            self.new_task_open = false;
        }
    }

    fn dlg_event(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.editor.take() else { return };
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let is_task = ed.ev.task_id.is_some();
        let hour12 = self.data.prefs.hour12;
        let muted = self.pal.muted;
        let title = if ed.is_new {
            "New event"
        } else if is_task {
            "Task session"
        } else {
            "Event"
        };
        let id = Id::unique("event_editor");
        let mut win = egui::Window::new(title).id(id).collapsible(false).resizable(false);
        win = match ed.anchor_rect {
            // the width includes the window's frame around the 330 of content
            Some(r) => win.fixed_pos(place_beside(ctx, id, r, egui::vec2(346.0, 400.0))),
            None => win.anchor(Align2::RIGHT_TOP, [-16.0, 64.0]),
        };
        win.show(ctx, |ui| {
                ui.set_width(330.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut ed.ev.title)
                        .hint_text(if is_task { "New Task" } else { "New Event" })
                        .font(FontId::proportional(18.0))
                        .desired_width(f32::INFINITY),
                );
                if ed.focus {
                    r.request_focus();
                    ed.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(4.0);
                let was_all_day = ed.ev.all_day;
                ui.checkbox(&mut ed.ev.all_day, "All day");
                if ed.ev.all_day && !was_all_day {
                    ed.ev.start = midnight(ed.ev.start.date());
                    ed.ev.end = ed.ev.start + Duration::days(1);
                } else if !ed.ev.all_day && was_all_day {
                    ed.ev.start = midnight(ed.ev.start.date()) + Duration::hours(NEW_EVENT_NOON_H);
                    ed.ev.end = ed.ev.start + Duration::minutes(NEW_EVENT_MIN);
                }
                ui.horizontal(|ui| {
                    if ui.small_button(icons::CHEVRON_LEFT).clicked() {
                        ed.ev.start -= Duration::days(1);
                        ed.ev.end -= Duration::days(1);
                    }
                    ui.label(ed.ev.start.format("%a, %b %-d %Y").to_string());
                    if ui.small_button(icons::CHEVRON_RIGHT).clicked() {
                        ed.ev.start += Duration::days(1);
                        ed.ev.end += Duration::days(1);
                    }
                });
                if ed.ev.all_day {
                    let mut n = (ed.ev.end.date() - ed.ev.start.date()).num_days().max(1);
                    ui.horizontal(|ui| {
                        ui.label("Days");
                        ui.add(egui::DragValue::new(&mut n).range(1..=60));
                    });
                    ed.ev.end = midnight(ed.ev.start.date()) + Duration::days(n);
                } else {
                    let len = ed.ev.end - ed.ev.start;
                    let (mut sh, mut sm) = (ed.ev.start.hour(), ed.ev.start.minute());
                    let (mut eh, mut em) = (ed.ev.end.hour(), ed.ev.end.minute());
                    let time_row = |ui: &mut egui::Ui, label: &str, h: &mut u32, m: &mut u32| -> bool {
                        let mut ch = false;
                        ui.horizontal(|ui| {
                            ui.add_sized([44.0, 18.0], egui::Label::new(RichText::new(label).color(muted)).halign(Align::LEFT));
                            ch |= ui.add(egui::DragValue::new(h).range(0..=23).speed(0.1)).changed();
                            ui.label(":");
                            ch |= ui
                                .add(
                                    egui::DragValue::new(m)
                                        .range(0..=59)
                                        .speed(0.25)
                                        .custom_formatter(|v, _| format!("{:02}", v as u32)),
                                )
                                .changed();
                            if hour12 {
                                let h12 = if *h % 12 == 0 { 12 } else { *h % 12 };
                                let ap = if *h < 12 { "AM" } else { "PM" };
                                ui.label(RichText::new(format!("{h12}:{:02} {ap}", *m)).color(muted));
                            }
                        });
                        ch
                    };
                    if time_row(ui, "Start", &mut sh, &mut sm) {
                        ed.ev.start = ed.ev.start.date().and_hms_opt(sh, sm, 0).unwrap();
                        ed.ev.end = ed.ev.start + len;
                    }
                    if time_row(ui, "End", &mut eh, &mut em) {
                        let mut e = ed.ev.start.date().and_hms_opt(eh, em, 0).unwrap();
                        if e <= ed.ev.start {
                            e += Duration::days(1);
                        }
                        ed.ev.end = e;
                    }
                    let mins = ed.ev.minutes();
                    let text = if mins % 60 == 0 {
                        format!("{} h", mins / 60)
                    } else if mins > 60 {
                        format!("{} h {} min", mins / 60, mins % 60)
                    } else {
                        format!("{mins} min")
                    };
                    ui.label(RichText::new(text).color(muted).size(11.5));
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Calendar").color(muted));
                    let name = self.data.calendars.get(ed.ev.calendar).map(|c| c.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("cal").selected_text(name).show_ui(ui, |ui| {
                        for (i, c) in self.data.calendars.iter().enumerate().filter(|(_, c)| c.url.is_none()) {
                            ui.selectable_value(&mut ed.ev.calendar, i, RichText::new(format!("{} {}", icons::CIRCLE, c.name)).color(rgb(c.color)));
                        }
                    });
                });
                ui.add(egui::TextEdit::multiline(&mut ed.ev.notes).hint_text("Notes").desired_rows(3).desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).on_hover_text("Ctrl+Enter").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        cancel = true;
                    }
                    if !ed.is_new {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Delete").clicked() {
                                delete = true;
                            }
                        });
                    }
                });
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
            });
        if save && ed.ev.remote {
            self.say(ctx, "This event comes from a subscribed calendar and can't be changed here", false);
        } else if save {
            let mut ev = ed.ev.clone();
            if ev.all_day {
                ev.start = midnight(ev.start.date());
                ev.end = midnight(ev.end.date()).max(ev.start + Duration::days(1));
            } else if ev.end <= ev.start {
                ev.end = ev.start + Duration::minutes(self.data.prefs.snap_min);
            }
            self.snapshot();
            let id = ev.id;
            let sched = ev.task_id.map(|t| (t, ev.start.date()));
            if let Some(slot) = self.data.event_mut(id) {
                *slot = ev;
            } else {
                self.data.events.push(ev);
            }
            if let Some((t, day)) = sched {
                self.after_task_scheduled(ctx, t, day);
            }
            self.selected = Some(id);
            self.commit();
        } else if delete {
            self.delete_event(ctx, ed.ev.id);
        } else if !cancel {
            self.editor = Some(ed);
        }
    }

    fn dlg_task(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.task_editor.take() else { return };
        let (mut save, mut cancel, mut delete) = (false, false, false);
        let today = Local::now().date_naive();
        let muted = self.pal.muted;
        egui::Window::new("Task")
            .id(Id::unique("task_editor"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, -40.0])
            .show(ctx, |ui| {
                ui.set_width(360.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut ed.task.title)
                        .hint_text("New Task")
                        .font(FontId::proportional(18.0))
                        .desired_width(f32::INFINITY),
                );
                if ed.focus {
                    r.request_focus();
                    ed.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.checkbox(&mut ed.task.done, "Completed");
                egui::Grid::new("taskgrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new("Due").color(muted));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut ed.due_text).hint_text("YYYY-MM-DD").desired_width(100.0));
                        if ui.small_button("Today").clicked() {
                            ed.due_text = today.to_string();
                        }
                        if ui.small_button("Tomorrow").clicked() {
                            ed.due_text = (today + Duration::days(1)).to_string();
                        }
                        if ui.small_button("None").clicked() {
                            ed.due_text.clear();
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Priority").color(muted));
                    egui::ComboBox::from_id_salt("prio").selected_text(priority_name(ed.task.priority)).show_ui(ui, |ui| {
                        for v in [0u8, 1, 5, 9] {
                            ui.selectable_value(&mut ed.task.priority, v, priority_name(v));
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Duration").color(muted));
                    let dur = |d: Option<i64>| d.map(|m| format!("{m} min")).unwrap_or_else(|| "No estimate".into());
                    egui::ComboBox::from_id_salt("dur").selected_text(dur(ed.task.duration_min)).show_ui(ui, |ui| {
                        for v in [None, Some(15), Some(30), Some(45), Some(60), Some(90), Some(120), Some(180), Some(240)] {
                            ui.selectable_value(&mut ed.task.duration_min, v, dur(v));
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Repeat due").color(muted));
                    egui::ComboBox::from_id_salt("rep").selected_text(ed.task.repeat.label()).show_ui(ui, |ui| {
                        for v in Repeat::ALL {
                            ui.selectable_value(&mut ed.task.repeat, v, v.label());
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Earliest start").color(muted));
                    ui.add(egui::TextEdit::singleline(&mut ed.earliest_text).hint_text("YYYY-MM-DD").desired_width(100.0));
                    ui.end_row();
                    ui.label(RichText::new("List").color(muted));
                    let lname = |id: u64| self.data.lists.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_else(|| "Inbox".into());
                    egui::ComboBox::from_id_salt("tlist").selected_text(lname(ed.task.list)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut ed.task.list, 0, "Inbox");
                        for l in &self.data.lists {
                            ui.selectable_value(&mut ed.task.list, l.id, &l.name);
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Tags").color(muted));
                    ui.add(egui::TextEdit::singleline(&mut ed.tags_text).hint_text("admin deep-work").desired_width(200.0));
                    ui.end_row();
                    ui.label(RichText::new("Flexible").color(muted));
                    ui.checkbox(&mut ed.task.flexible, "");
                    ui.end_row();
                });
                // time already planned for this task
                let planned: i64 = self.data.events.iter().filter(|e| e.task_id == Some(ed.task.id)).map(|e| e.minutes()).sum();
                if planned > 0 {
                    let need = ed.task.duration_min.map(|d| (d - planned).max(0));
                    let mut line = format!("Scheduled {}", fmt_duration(planned));
                    if let Some(n) = need {
                        line += &format!("  ·  still needed {}", fmt_duration(n));
                    }
                    ui.label(RichText::new(line).size(11.5).color(muted));
                }
                ui.add(egui::TextEdit::multiline(&mut ed.task.notes).hint_text("Notes").desired_rows(3).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Delete").clicked() {
                            delete = true;
                        }
                    });
                });
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
            });
        if save {
            let text = ed.due_text.trim().to_string();
            if text.is_empty() {
                ed.task.due = None;
            } else if let Ok(d) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
                ed.task.due = Some(d);
            } else {
                self.say(ctx, "The due date should look like 2026-11-30", false);
                self.task_editor = Some(ed);
                return;
            }
            ed.task.tags = ed.tags_text.split_whitespace().map(|t| t.trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect();
            ed.task.earliest = NaiveDate::parse_from_str(ed.earliest_text.trim(), "%Y-%m-%d").ok();
            self.snapshot();
            let (id, title) = (ed.task.id, ed.task.title.clone());
            if let Some(t) = self.data.task_mut(id) {
                *t = ed.task;
            }
            for e in self.data.events.iter_mut().filter(|e| e.task_id == Some(id)) {
                e.title = title.clone();
            }
            self.commit();
        } else if delete {
            self.snapshot();
            let id = ed.task.id;
            self.data.tasks.retain(|t| t.id != id);
            self.data.events.retain(|e| e.task_id != Some(id));
            self.commit();
            self.say(ctx, "Task deleted", true);
        } else if !cancel {
            self.task_editor = Some(ed);
        }
    }

    fn dlg_command(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.command_bar.take() else { return };
        let mut run: Option<Cmd> = None;
        let mut goto: Option<(u64, NaiveDate)> = None;
        let mut open_task: Option<u64> = None;
        let mut close = false;
        let muted = self.pal.muted;
        let focus = std::mem::take(&mut self.command_focus);
        egui::Window::new("command")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut q)
                        .hint_text("Type a command or search events and tasks")
                        .font(FontId::proportional(16.0))
                        .desired_width(f32::INFINITY),
                );
                if focus {
                    r.request_focus();
                }
                let ql = q.to_lowercase();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let mut first = true;
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    if !ql.is_empty() {
                        for e in self.data.events.iter().filter(|e| e.title.to_lowercase().contains(&ql)).take(6) {
                            let label = format!("{}  {}   —   {}", icons::CALENDAR, e.title, e.start.format("%a, %b %-d"));
                            if ui.selectable_label(false, label).clicked() || (enter && first) {
                                goto = Some((e.id, e.start.date()));
                            }
                            first = false;
                        }
                        for t in self.data.tasks.iter().filter(|t| t.title.to_lowercase().contains(&ql)).take(6) {
                            if ui.selectable_label(false, format!("{}  {}", icons::SQUARE_CHECK, t.title)).clicked() || (enter && first) {
                                open_task = Some(t.id);
                            }
                            first = false;
                        }
                    }
                    for (cmd, label, _, _) in COMMANDS.iter().filter(|c| c.1.to_lowercase().contains(&ql)) {
                        if matches!(cmd, Cmd::CommandBar | Cmd::Search) {
                            continue;
                        }
                        let keys = &self.bind_text(*cmd);
                        ui.horizontal(|ui| {
                            let clicked = ui.selectable_label(false, *label).clicked();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(keys).color(muted).monospace().size(11.0));
                            });
                            if clicked || (enter && first) {
                                run = Some(*cmd);
                            }
                        });
                        first = false;
                    }
                });
                let nothing = run.is_none() && goto.is_none() && open_task.is_none();
                if r.lost_focus() && !enter && nothing && ui.input(|i| i.pointer.any_click()) {
                    close = true;
                }
            });
        if let Some(c) = run {
            self.run(ctx, c);
        } else if let Some((id, d)) = goto {
            self.anchor = d;
            self.selected = Some(id);
        } else if let Some(t) = open_task {
            self.edit_task(t);
        } else if !close {
            self.command_bar = Some(q);
        }
    }

    pub fn dlg_toast_public(&mut self, ctx: &egui::Context) {
        self.dlg_toast(ctx);
    }

    fn dlg_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(t) = &self.toast else { return };
        if now > t.until {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        let (text, undo, move_due) = (t.text.clone(), t.undo, t.move_due);
        let (born, until) = (if t.born < 0.0 { now } else { t.born }, t.until);
        if let Some(t) = &mut self.toast {
            t.born = born;
        }
        // fades and rises in, fades out
        let a_in = (((now - born) as f32) / ANIM_S).clamp(0.0, 1.0);
        let a_out = (((until - now) as f32) / ANIM_S).clamp(0.0, 1.0);
        let alpha = a_in.min(a_out);
        if alpha < 1.0 {
            ctx.request_repaint();
        }
        let (mut do_undo, mut do_move, mut dismiss) = (false, false, false);
        let fill = mix(self.pal.side, self.pal.text, 0.12);
        let border = self.pal.border;
        egui::Area::new(Id::unique("toast"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -24.0 + (1.0 - a_in) * 16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(text);
                            if undo && ui.button("Undo").clicked() {
                                do_undo = true;
                            }
                            if move_due.is_some() {
                                if ui.button("Move due date").clicked() {
                                    do_move = true;
                                }
                                if ui.button("Keep").clicked() {
                                    dismiss = true;
                                }
                            }
                        });
                    });
            });
        if do_undo {
            self.do_undo(ctx);
        } else if do_move {
            if let Some((tid, day)) = move_due {
                if let Some(t) = self.data.task_mut(tid) {
                    t.due = Some(day);
                }
                self.commit();
            }
            self.toast = None;
        } else if dismiss {
            self.toast = None;
        }
    }
}
