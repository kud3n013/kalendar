//! One frame of the window: styling, input, the panels and views in order,
//! and the screenshot mode.

use chrono::Duration;
use eframe::egui::{self, Color32};
use crate::model::*;
use crate::*;

impl App {
    pub fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        // for screenshots: step this many periods just before the picture,
        // to catch the slide half way
        if self.frames == 104 {
            if let Some(n) = testaid::get().nav {
                self.step(n);
            }
            if let Some(n) = testaid::get().nav_days {
                self.anchor += Duration::days(n);
            }
        }
        if self.shot.is_some() && self.frames == 20 {
            if let Some(n) = testaid::get().days {
                self.view = View::Days(n as _);
            }
        }
        // the picture arrives later, from the renderer, so it is passed back here
        static PICTURE: std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>> = std::sync::Mutex::new(None);
        if self.frames == 120 {
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                *PICTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(img);
                c.request_repaint();
            });
        }
        let img = PICTURE.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(img) = img {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(dir);
                }
            }
            let ok = image::save_buffer(
                &path,
                img.as_raw(),
                img.size[0] as u32,
                img.size[1] as u32,
                image::ColorType::Rgba8,
            );
            match ok {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("could not save the picture: {e}");
                    std::process::exit(1);
                }
            }
        }
        if self.frames > 1200 {
            eprintln!("no picture arrived");
            std::process::exit(1);
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // keeps the view, the scroll and the panel for next time
        self.save();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some((_, pos)) = self.shot.as_ref().and(testaid::get().drag.as_ref()) {
            raw_input.events.push(egui::Event::PointerMoved(*pos));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        motion::ANIMATE.store(self.data.prefs.animations, std::sync::atomic::Ordering::Relaxed);
        ctx.set_theme(match self.data.prefs.theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
        // the font and the size of everything follow the settings
        let want = (self.data.prefs.font.clone(), self.data.prefs.ui_scale.clamp(0.8, 2.0));
        if self.font_applied.as_ref() != Some(&want) {
            if self.font_applied.as_ref().map(|a| &a.0) != Some(&want.0) {
                fonts::apply(ctx, &want.0);
            }
            ctx.set_zoom_factor(want.1);
            self.font_applied = Some(want);
        }
        self.pal = Palette::new(ctx.global_style().visuals.dark_mode);
        self.pal.accent = crate::rgb(self.data.main_color());
        let (bg, side, text) = (self.pal.bg, self.pal.side, self.pal.text);
        ctx.global_style_mut(|s| {
            s.visuals.panel_fill = bg;
            s.visuals.window_fill = side;
            s.visuals.override_text_color = Some(text);
            s.visuals.selection.bg_fill = theme::ACCENT;
            s.visuals.selection.stroke.color = Color32::WHITE;
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.animation_time = motion::NORMAL.secs();
            // the one corner radius, everywhere
            let cr = egui::CornerRadius::same(theme::RADIUS as u8);
            s.visuals.window_corner_radius = cr;
            s.visuals.menu_corner_radius = cr;
            for w in [
                &mut s.visuals.widgets.noninteractive,
                &mut s.visuals.widgets.inactive,
                &mut s.visuals.widgets.hovered,
                &mut s.visuals.widgets.active,
                &mut s.visuals.widgets.open,
            ] {
                w.corner_radius = cr;
            }
        });

        self.poll_sync(ctx);
        self.auto_update_check(ctx);
        if let Some(page) = &testaid::get().show_settings {
            // the settings fill the main window, so a picture can be taken
            use settings::Page;
            let aid = testaid::get();
            self.settings_page = match page.as_str() {
                "calendars" => Page::Calendars,
                "active" => Page::ActiveCalendars,
                "shortcuts" => Page::Shortcuts,
                "planner" => Page::Planner,
                "frames" => Page::Frames,
                "lists" => Page::Lists,
                "tags" => Page::Tags,
                "appearance" => Page::Appearance,
                "info" => Page::Info,
                _ => Page::General,
            };
            if let Some(q) = &aid.settings_query {
                self.settings_query = q.clone();
            }
            if let Some(q) = &aid.shortcut_query {
                self.shortcut_query = q.clone();
            }
            if let Some(k) = aid.sub_kind {
                self.sub_kind = k;
            }
            if let Some(u) = &aid.sub_url {
                self.sub_kind = 1;
                self.sub_url = u.clone();
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| self.settings_body(ui));
            self.dlg_toast_public(ctx);
            return self.screenshot(ctx);
        }
        self.keys(ctx);
        self.mouse_nav(ctx);
        // a narrow window shows one day and no task panel; widening it
        // brings back what was there
        let narrow = ctx.content_rect().width() < COMPACT_W || (self.shot.is_some() && testaid::get().narrow);
        match (narrow, self.compact) {
            (true, None) => {
                self.compact = Some((self.data.prefs.sidebar_open, self.view));
                self.data.prefs.sidebar_open = false;
                if !matches!(self.view, View::Agenda) {
                    self.view = View::Days(1);
                }
            }
            (false, Some((open, view))) => {
                self.compact = None;
                self.data.prefs.sidebar_open = open;
                self.view = view;
            }
            _ => {}
        }
        self.ui_sidebar(ui);
        self.ui_toolbar(ui);
        // moving in time slides the old page out and the new one in, the way
        // the days run; another view fades in
        let time = ctx.input(|i| i.time);
        let shown = (self.view, self.anchor);
        // a step in time, as the share of the columns it moved (a step
        // shorter than the days on screen moves just that many columns)
        let mut stepped: Option<f32> = None;
        if let Some(last) = self.last_shown {
            if last != shown {
                let moved = (shown.1 - last.1).num_days();
                if last.0 != shown.0 || moved == 0 {
                    self.slide.fade(time);
                } else {
                    let part = match shown.0 {
                        View::Days(n) => (moved.abs() as f32 / n.max(1) as f32).min(1.0),
                        _ => 1.0,
                    };
                    stepped = Some(moved.signum() as f32 * part);
                }
            }
        }
        self.last_shown = Some(shown);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg)).show(ui, |ui| {
            let full = ui.max_rect();
            ui.set_clip_rect(full);
            // the hour gutter of the time grid stays put; the day columns
            // slide past it, exactly as far as the days moved
            let gutter = if matches!(self.view, View::Month | View::Agenda) { 0.0 } else { GUTTER_W };
            let cols = full.with_min_x(full.left() + gutter);
            if let Some(share) = stepped {
                self.slide.step(time, share * cols.width(), std::mem::take(&mut self.page_shot), cols.width());
            }
            self.slide.settle(time);
            if self.slide.moving(time) {
                ui.ctx().request_repaint();
            }
            let sliding = self.slide.sliding(time);
            let dx = self.slide.offset(time).round();
            let layer = ui.layer_id();
            let first = ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(full.translate(egui::vec2(dx, 0.0))), |ui| {
                ui.set_opacity(self.slide.opacity(time));
                match self.view {
                    View::Month => self.ui_month(ui),
                    View::Agenda => self.ui_agenda(ui),
                    _ => self.ui_time_grid(ui),
                }
            });
            // keep what was painted, in its resting place, for the next slide
            let spread = dx.abs() + 1.0;
            self.page_shot = ctx.graphics(|g| {
                g.get(layer).map_or(vec![], |l| {
                    l.all_entries()
                        .skip(first)
                        .map(|cs| {
                            let mut cs = cs.clone();
                            cs.shape.translate(egui::vec2(-dx, 0.0));
                            // what the edge of the view cut off while sliding
                            // is there again once the page is at rest
                            if sliding && cs.clip_rect.left() <= full.left() + 0.5 {
                                cs.clip_rect.min.x -= spread;
                            }
                            if sliding && cs.clip_rect.right() >= full.right() - 0.5 {
                                cs.clip_rect.max.x += spread;
                            }
                            cs.clip_rect = cs.clip_rect.translate(egui::vec2(-dx, 0.0));
                            cs
                        })
                        .collect()
                })
            });
            if sliding && dx != 0.0 {
                let p = ui.painter();
                // the pages slid aside, as they were last painted, over the
                // stretch the page on screen does not cover
                let open = if dx > 0.0 { cols.with_max_x(cols.left() + dx) } else { cols.with_min_x(cols.right() + dx) };
                p.rect_filled(open, 0.0, self.pal.bg);
                for (shapes, rest) in &self.slide.pages {
                    let off = egui::vec2(dx + rest, 0.0);
                    for cs in shapes {
                        let mut shape = cs.shape.clone();
                        shape.translate(off);
                        p.with_clip_rect(cs.clip_rect.translate(off).intersect(open)).add(shape);
                    }
                }
                // and the gutter of the page on screen, where it rests
                if gutter > 0.0 {
                    let strip = full.with_max_x(cols.left());
                    p.rect_filled(strip, 0.0, self.pal.bg);
                    for cs in &self.page_shot {
                        p.with_clip_rect(cs.clip_rect.intersect(strip)).add(cs.shape.clone());
                    }
                }
            }
        });
        self.ui_dialogs(ctx);
        if let Some(kind) = self.demo_menu.clone() {
            let want_task = kind == "task";
            if let Some(id) = self.data.events.iter().find(|e| e.task_id.is_some() == want_task && !e.all_day).map(|e| e.id) {
                egui::Window::new("menu").id(ids::demo_menu()).title_bar(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    self.block_menu(ui, id);
                });
            }
        }
        self.ui_drag_ghost(ctx);
        let drag_aid_on = self.shot.is_some() && testaid::get().drag.is_some();
        if !ctx.input(|i| i.pointer.any_down()) && !drag_aid_on {
            self.drag_task = None;
        }
        // keep the clock line moving
        ctx.request_repaint_after(std::time::Duration::from_secs(CLOCK_REFRESH_S));
        self.screenshot(ctx);
    }
}
