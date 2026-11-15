//! What is drawn in the main window: toolbar, task list and calendar views.

use crate::icons;
use crate::model::*;
use crate::{fill_squircle, mix, rgb, stroke_squircle, App, Cmd, CreateEdit, CreateKind, Drag, Palette};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Id, Layout, Margin, PointerButton, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind,
};

struct Item {
    id: u64,
    rect: Rect,
    title: String,
    sub: String,
    color: Color32,
    task: Option<(u64, bool)>,
    draft: bool,
    past: bool,
}

/// Where one day's stretch of a frame was painted.
struct FrameBox {
    id: u64,
    day: NaiveDate,
    rect: Rect,
    /// the name label, double-clicked to edit the frame
    label: Rect,
    color: Color32,
}

/// Draws one calendar block. Plain events are filled with their colour and
/// carry dark text; task sessions are dark with a coloured bar and a box to
/// tick. `hover` is 0..1 and brightens the block.
fn draw_item(p: &egui::Painter, pal: &Palette, it: &Item, selected: bool, hover: f32) {
    let r = it.rect.shrink2(vec2(1.0, 0.0));
    let done = matches!(it.task, Some((_, true)));
    let is_task = it.task.is_some();
    let soft = mix(it.color, Color32::from_gray(if pal.dark { 228 } else { 245 }), 0.5);
    let (fill, text, sub) = if is_task {
        let base = mix(pal.bg, it.color, if it.draft || it.past { 0.08 } else { 0.14 });
        (mix(base, it.color, 0.10 * hover), if done || it.past { pal.muted } else { pal.text }, pal.muted)
    } else {
        let base = if selected { it.color } else { soft };
        let base = if it.draft || it.past { mix(pal.bg, base, 0.45) } else { base };
        let ink = Color32::from_rgb(0x10, 0x20, 0x26);
        (mix(base, it.color, 0.5 * hover), ink, mix(ink, base, 0.25))
    };
    fill_squircle(p, r, RADIUS, fill);
    let edge = if selected { pal.text } else if is_task { mix(pal.bg, it.color, 0.75) } else { fill };
    stroke_squircle(p, r.shrink(0.5), RADIUS, Stroke::new(if selected { 1.5 } else { 1.0 }, edge));
    let mut x = r.left() + 6.0;
    let tall = r.height() >= 36.0;
    if is_task {
        if r.width() > 14.0 {
            let bar = Rect::from_min_max(pos2(r.left() + 4.0, r.top() + 4.0), pos2(r.left() + 7.0, r.bottom() - 4.0));
            p.rect_filled(bar, 1.5, it.color);
        }
        x = r.left() + 12.0;
    }
    let clip = p.with_clip_rect(r.shrink(2.0).intersect(p.clip_rect()));
    let title = if it.title.is_empty() { "New Event" } else { &it.title };
    let title_font = FontId::new(13.0, egui::FontFamily::Name("bold".into()));
    let mut ty = r.top() + 2.5;
    if tall {
        clip.text(pos2(x, r.top() + 3.0), Align2::LEFT_TOP, &it.sub, FontId::proportional(11.5), sub);
        ty = r.top() + 18.0;
    }
    let mut tx = x;
    if is_task {
        let c = Rect::from_min_size(pos2(x, ty + 1.5), vec2(12.0, 12.0));
        if done {
            clip.rect_filled(c, 3.5, it.color);
            let mid = c.center_bottom() + vec2(-1.0, -3.0);
            clip.line_segment([c.left_center() + vec2(2.5, 0.5), mid], Stroke::new(1.5, Color32::WHITE));
            clip.line_segment([mid, c.right_top() + vec2(-2.5, 3.0)], Stroke::new(1.5, Color32::WHITE));
        } else {
            clip.rect_stroke(c, 3.5, Stroke::new(1.3, mix(pal.muted, pal.text, 0.5)), StrokeKind::Inside);
        }
        tx = c.right() + 7.0;
    }
    let font = if !tall && r.height() < 20.0 { FontId::new(11.0, egui::FontFamily::Name("bold".into())) } else { title_font };
    clip.text(pos2(tx, ty), Align2::LEFT_TOP, title, font, text);
}

/// Where the tick box of a task session sits inside its block.
fn check_rect(r: Rect) -> Rect {
    let ty = if r.height() >= 36.0 { r.top() + 18.0 } else { r.top() + 2.5 };
    Rect::from_min_size(pos2(r.left() + 12.0, ty + 1.5), vec2(12.0, 12.0))
}

pub fn midnight(d: NaiveDate) -> NaiveDateTime {
    d.and_hms_opt(0, 0, 0).unwrap()
}

/// mode: -1 floor, 0 nearest, 1 ceil
fn snap_dt(t: NaiveDateTime, snap: i64, mode: i8) -> NaiveDateTime {
    let m = t.hour() as i64 * 60 + t.minute() as i64;
    let s = match mode {
        -1 => m / snap * snap,
        1 => (m + snap - 1) / snap * snap,
        _ => (m + snap / 2) / snap * snap,
    };
    midnight(t.date()) + Duration::minutes(s)
}

/// Places the events of one day side by side ("overlap" layout). `boxes` are (top, bottom) in pixels, already in
/// event order; `inset` is the overlap inset as a share of the day's width.
/// Returns (left, width, layer) with left and width as shares of the width.
fn overlap_layout(boxes: &[(f32, f32)], inset: f32) -> Vec<(f32, f32, usize)> {
    let n = boxes.len();
    let mut out = vec![(0.0f32, 1.0f32, 1usize); n];
    // the part of an event that must stay readable
    let head = |i: usize| boxes[i].1.min(boxes[i].0 + OVERLAP_CONTENT_H);
    let hits = |a: (f32, f32), b: (f32, f32)| a.0 < b.1 && b.0 < a.1;
    let mut by_top: Vec<usize> = (0..n).collect();
    by_top.sort_by(|&x, &y| boxes[x].0.total_cmp(&boxes[y].0).then(x.cmp(&y)));
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut cur: Vec<usize> = vec![];
    let mut reach = f32::NEG_INFINITY;
    for i in by_top {
        if !cur.is_empty() && boxes[i].0 >= reach {
            groups.push(std::mem::take(&mut cur));
            reach = f32::NEG_INFINITY;
        }
        cur.push(i);
        reach = reach.max(boxes[i].1);
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    for g in groups {
        let last_top = boxes[*g.last().unwrap()].0;
        let mut ranked = g.clone();
        ranked.sort();
        // every event's heading is still open when the last one starts:
        // a fan of equal steps, each two steps wide
        if g.iter().all(|&i| last_top < head(i)) {
            let m = ranked.len();
            let step = 1.0 / (m + usize::from(m > 1)) as f32;
            for (k, &i) in ranked.iter().enumerate() {
                out[i] = (k as f32 * step, if m > 1 { 2.0 * step } else { step }, k + 1);
            }
            continue;
        }
        let mut cols: Vec<Vec<usize>> = vec![];
        for &i in &ranked {
            let c = cols.iter().position(|col| col.iter().all(|&j| !hits(boxes[i], boxes[j]))).unwrap_or(cols.len());
            if c == cols.len() {
                cols.push(vec![]);
            }
            cols[c].push(i);
        }
        let s = cols.len();
        let step = 1.0 / (s + 1) as f32;
        let small = inset.clamp(0.0, step);
        if small == step || g.iter().all(|&i| head(i) >= boxes[i].1) {
            for (c, col) in cols.iter().enumerate() {
                for &i in col {
                    let top_part = (boxes[i].0, head(i));
                    let mut j = c + 1;
                    while j < s && cols[j].iter().all(|&k| !hits(top_part, boxes[k])) {
                        j += 1;
                    }
                    let left = c as f32 * step;
                    let right = if j < s { (j + 1) as f32 * step } else { 1.0 };
                    out[i] = (left, right - left, c + 1);
                }
            }
            continue;
        }
        // tall events: later columns step in by a full step where a heading
        // would be covered, and by the small inset otherwise
        let mut left = vec![0.0f32; n];
        let mut right = vec![1.0f32; n];
        let covers = |x: usize, y: usize| boxes[y].0 < head(x) && boxes[x].0 < boxes[y].1;
        for a in 0..s {
            for b in a + 1..s {
                for &x in &cols[a] {
                    for &y in &cols[b] {
                        if hits(boxes[x], boxes[y]) {
                            left[y] = left[y].max(left[x] + if covers(x, y) { step } else { small });
                        }
                    }
                }
            }
        }
        for a in 0..s {
            for b in a + 1..s {
                for &x in &cols[a] {
                    for &y in &cols[b] {
                        if hits(boxes[x], boxes[y]) && covers(x, y) {
                            right[x] = right[x].min(left[y] + step);
                        }
                    }
                }
            }
        }
        for (c, col) in cols.iter().enumerate() {
            for &i in col {
                out[i] = (left[i], (right[i] - left[i]).max(step * 0.5), c + 1);
            }
        }
    }
    out
}

impl App {
    pub fn ui_drag_ghost(&mut self, ctx: &egui::Context) {
        let Some(id) = self.drag_task else { return };
        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
        let Some(t) = self.data.task(id) else { return };
        let misfit = ctx.data_mut(|d| d.remove_temp::<bool>(Id::unique("drag_misfit"))).unwrap_or(false);
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("ghost")));
        let h = if misfit { 50.0 } else { 34.0 };
        let r = Rect::from_min_size(pos + vec2(10.0, 8.0), vec2(230.0, h));
        fill_squircle(&p, r, RADIUS, mix(self.pal.side, self.pal.text, 0.12));
        stroke_squircle(&p, r.shrink(0.5), RADIUS, Stroke::new(1.0, self.pal.accent));
        let clip = p.with_clip_rect(r.shrink(4.0));
        let title_y = if misfit { r.top() + 17.0 } else { r.center().y };
        clip.text(pos2(r.left() + 10.0, title_y), Align2::LEFT_CENTER, &t.title, FontId::proportional(13.0), self.pal.text);
        if misfit {
            clip.text(
                pos2(r.left() + 10.0, r.bottom() - 14.0),
                Align2::LEFT_CENTER,
                "Not in this frame's filter",
                FontId::proportional(11.0),
                self.pal.muted,
            );
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    // ------------------------------------------------------------ time grid

    pub fn ui_time_grid(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let days = self.visible_days();
        let n = days.len().max(1);
        let now = App::now();
        let today = now.date();
        let full_w = ui.available_width();
        let col_w = (full_w - GUTTER_W) / n as f32;

        // all-day rows: one row per event across the days it covers
        let range = (days[0], *days.last().unwrap());
        let mut allday: Vec<Event> =
            self.data.events.iter().filter(|e| e.all_day && self.data.event_visible(e)).cloned().collect();
        let mut draft_id = None;
        if let Some(ed) = &self.editor {
            if ed.is_new && ed.ev.all_day {
                draft_id = Some(ed.ev.id);
                allday.push(ed.ev.clone());
            }
        }
        let end_of = |e: &Event| e.end.date().max(e.start.date() + Duration::days(1));
        allday.retain(|e| e.start.date() <= range.1 && end_of(e) > range.0);
        allday.sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
        let mut row_end: Vec<NaiveDate> = vec![];
        let mut placed: Vec<(Event, usize)> = vec![];
        for e in allday {
            let end = end_of(&e);
            let row = row_end.iter().position(|re| *re <= e.start.date()).unwrap_or(row_end.len());
            if row == row_end.len() {
                row_end.push(end);
            } else {
                row_end[row] = end;
            }
            placed.push((e, row));
        }
        // due dates of open tasks sit under the all-day events
        let due_by_day: Vec<Vec<(u64, String)>> = days
            .iter()
            .map(|d| {
                if !self.data.prefs.due_on_calendar {
                    return vec![];
                }
                self.data.tasks.iter().filter(|t| !t.done && t.due == Some(*d)).map(|t| (t.id, t.title.clone())).collect()
            })
            .collect();
        let due_rows = due_by_day.iter().map(|v| v.len()).max().unwrap_or(0);
        let ev_rows = row_end.len();
        let rows = (ev_rows + due_rows).max(1) as f32;
        let head_h = 44.0 + rows * 22.0 + 6.0;
        let (hrect, hresp) = ui.allocate_exact_size(vec2(full_w, head_h), Sense::click());
        let hp = ui.painter_at(hrect);
        let x0 = hrect.left() + GUTTER_W;
        hp.text(pos2(x0 - 8.0, hrect.top() + 50.0), Align2::RIGHT_TOP, "all-day", FontId::proportional(10.0), self.pal.muted);
        hp.text(
            pos2(x0 - 8.0, hrect.top() + 22.0),
            Align2::RIGHT_CENTER,
            format!("W{}", days[0].iso_week().week()),
            FontId::proportional(10.0),
            self.pal.muted,
        );
        for (i, d) in days.iter().enumerate() {
            let cx = x0 + i as f32 * col_w;
            // "Wed 11" centred over its day; today's number sits in a red disc
            let name = if col_w < 70.0 { d.format("%a").to_string()[..1].to_string() } else { d.format("%a").to_string() };
            let num = d.day().to_string();
            let is_today = *d == today;
            let gn = hp.layout_no_wrap(name, FontId::proportional(15.0), if is_today { self.pal.text } else { self.pal.muted });
            let gd = hp.layout_no_wrap(num, FontId::proportional(15.0), if is_today { Color32::WHITE } else { self.pal.muted });
            let disc = if is_today { 24.0 } else { gd.size().x };
            let total = gn.size().x + 6.0 + disc;
            let left = cx + (col_w - total) / 2.0;
            let cy = hrect.top() + 22.0;
            hp.galley(pos2(left, cy - gn.size().y / 2.0), gn.clone(), self.pal.muted);
            let dc = pos2(left + gn.size().x + 6.0 + disc / 2.0, cy);
            if is_today {
                hp.circle_filled(dc, 12.0, Color32::from_rgb(0xeb, 0x52, 0x52));
            }
            hp.galley(dc - gd.size() / 2.0, gd, self.pal.muted);
            hp.line_segment([pos2(cx, hrect.top() + 44.0), pos2(cx, hrect.bottom())], Stroke::new(1.0, self.pal.border_minor));
        }
        hp.line_segment(
            [pos2(hrect.left(), hrect.top() + 44.0), pos2(hrect.right(), hrect.top() + 44.0)],
            Stroke::new(1.0, self.pal.border_minor),
        );
        hp.line_segment([hrect.left_bottom(), hrect.right_bottom()], Stroke::new(1.0, self.pal.border));
        let mut chips: Vec<(u64, Rect)> = vec![];
        for (e, row) in &placed {
            let end = end_of(e);
            let first = days.iter().position(|d| *d >= e.start.date() && *d < end);
            let last = days.iter().rposition(|d| *d >= e.start.date() && *d < end);
            let (Some(a), Some(b)) = (first, last) else { continue };
            let y = hrect.top() + 47.0 + *row as f32 * 22.0;
            let r = Rect::from_min_max(pos2(x0 + a as f32 * col_w + 2.0, y), pos2(x0 + (b + 1) as f32 * col_w - 4.0, y + 20.0));
            let color = rgb(self.data.event_color(e));
            let is_draft = draft_id == Some(e.id);
            hp.rect_filled(r, 4.0, mix(self.pal.bg, color, if is_draft { 0.25 } else { 0.55 }));
            if self.selected == Some(e.id) {
                hp.rect_stroke(r, 4.0, Stroke::new(1.5, self.pal.text), StrokeKind::Inside);
            }
            let title = if e.title.is_empty() { "New Event" } else { &e.title };
            hp.with_clip_rect(r.shrink(2.0)).text(
                r.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(12.0),
                self.pal.text,
            );
            if !is_draft {
                chips.push((e.id, r));
            }
        }
        let mut due_chips: Vec<(u64, Rect)> = vec![];
        for (i, list) in due_by_day.iter().enumerate() {
            for (k, (tid, title)) in list.iter().enumerate() {
                let y = hrect.top() + 47.0 + (ev_rows + k) as f32 * 22.0;
                let r = Rect::from_min_max(pos2(x0 + i as f32 * col_w + 2.0, y), pos2(x0 + (i + 1) as f32 * col_w - 4.0, y + 20.0));
                let over = hresp.hover_pos().map(|p| r.contains(p)).unwrap_or(false);
                let glow = ui.ctx().animate_bool_with_time(Id::unique(("due", *tid)), over, ANIM_FAST_S);
                hp.rect_filled(r, 4.0, mix(self.pal.bg, self.pal.text, 0.03 + 0.07 * glow));
                hp.rect_stroke(r, 4.0, Stroke::new(1.0, mix(self.pal.border, self.pal.text, 0.25)), StrokeKind::Inside);
                let c = hp.with_clip_rect(r.shrink(2.0));
                c.text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, icons::FLAG, FontId::proportional(12.0), self.pal.text);
                c.text(r.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, title, FontId::new(12.5, egui::FontFamily::Name("bold".into())), self.pal.text);
                due_chips.push((*tid, r));
            }
        }
        if hresp.clicked() {
            if let Some(pos) = hresp.interact_pointer_pos() {
                if let Some((tid, _)) = due_chips.iter().find(|(_, r)| r.contains(pos)) {
                    self.edit_task(*tid);
                } else if let Some((id, _)) = chips.iter().find(|(_, r)| r.contains(pos)) {
                    self.selected = Some(*id);
                } else if pos.x >= x0 && pos.y > hrect.top() + 44.0 {
                    let di = (((pos.x - x0) / col_w) as usize).min(n - 1);
                    self.open_new_event(midnight(days[di]), 1440, true);
                }
            }
        }
        if hresp.double_clicked() {
            if let Some(pos) = hresp.interact_pointer_pos() {
                if let Some((id, r)) = chips.iter().find(|(_, r)| r.contains(pos)) {
                    self.edit_event_at(*id, Some(*r));
                }
            }
        }

        let mut sa = egui::ScrollArea::vertical().id_salt("grid").auto_shrink([false, false]).scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
        if let Some(o) = self.scroll_to.take() {
            sa = sa.vertical_scroll_offset(o);
        }
        // Ctrl+wheel (or a pinch) zooms the hours; Shift+wheel moves by days
        let ctx = ui.ctx().clone();
        if ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            let (zoom, side) = ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta.x));
            if (zoom - 1.0).abs() > 0.001 {
                // the zoom heads for a goal and glides there; the hour under
                // the mouse stays under the mouse
                let from = self.zoom_goal.map_or(self.data.prefs.hour_height(), |g| g.0);
                let my = ctx.pointer_latest_pos().map_or(0.0, |p| (p.y - self.grid_scroll.1).max(0.0));
                self.zoom_goal = Some(((from * zoom).clamp(HOUR_H_MIN, HOUR_H_MAX), my));
            }
            self.side_scroll += side;
            if self.side_scroll.abs() > 60.0 {
                self.anchor -= Duration::days(self.side_scroll.signum() as i64);
                self.side_scroll = 0.0;
            }
        }
        if let Some((goal, my)) = self.zoom_goal {
            let was = self.data.prefs.hour_height();
            let dt = ctx.input(|i| i.stable_dt).min(0.05);
            let mut h = was + (goal - was) * (1.0 - (-dt * ZOOM_RATE).exp());
            if (goal - h).abs() < 0.25 {
                h = goal;
                self.zoom_goal = None;
            } else {
                ctx.request_repaint();
            }
            self.data.prefs.hour_height_px = Some(h);
            sa = sa.vertical_scroll_offset(((self.grid_scroll.0 + my) * h / was - my).max(0.0));
        }
        let out = sa.show(ui, |ui| self.grid_body(ui, &days, col_w, now));
        self.grid_scroll = (out.state.clamped_offset().y, out.inner_rect.top());
    }

    fn grid_body(&mut self, ui: &mut egui::Ui, days: &[NaiveDate], col_w: f32, now: NaiveDateTime) {
        let ctx = ui.ctx().clone();
        let p = self.data.prefs.clone();
        let hh = p.hour_height();
        let n = days.len();
        let gs = p.grid_start_h * 60;
        let ge = (p.grid_end_h * 60).max(gs + 60);
        let total_h = (ge - gs) as f32 / 60.0 * hh;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), total_h), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let x0 = rect.left() + GUTTER_W;
        let top = rect.top();
        let y_of = |m: i64| top + (m - gs) as f32 / 60.0 * hh;
        let today = now.date();

        for (i, d) in days.iter().enumerate() {
            let c = Rect::from_min_max(pos2(x0 + i as f32 * col_w, top), pos2(x0 + (i + 1) as f32 * col_w, rect.bottom()));
            if *d == today {
                painter.rect_filled(c, 0.0, self.pal.today);
            } else if d.weekday().num_days_from_monday() >= 5 {
                painter.rect_filled(c, 0.0, self.pal.weekend);
            }
        }
        let slot_px = hh * p.slot_min as f32 / 60.0;
        let mut m = gs;
        while m <= ge {
            let y = y_of(m);
            if m % 60 == 0 {
                painter.line_segment([pos2(x0, y), pos2(rect.right(), y)], Stroke::new(1.0, self.pal.border));
                if m > gs && m < ge {
                    let t = chrono::NaiveTime::from_hms_opt((m / 60) as u32 % 24, 0, 0).unwrap();
                    painter.text(pos2(x0 - 8.0, y), Align2::RIGHT_CENTER, p.fmt_hour(t), FontId::proportional(11.0), self.pal.muted);
                }
            } else if slot_px >= 12.0 {
                painter.line_segment([pos2(x0, y), pos2(rect.right(), y)], Stroke::new(1.0, self.pal.border_minor));
            }
            m += p.slot_min;
        }
        for i in 0..=n {
            let x = x0 + i as f32 * col_w;
            painter.line_segment([pos2(x, top), pos2(x, rect.bottom())], Stroke::new(1.0, self.pal.border));
        }

        // frames: tinted bands under the events; painting only, so clicks
        // and drags still reach the grid
        let mut frame_boxes: Vec<FrameBox> = vec![];
        for (di, day) in days.iter().enumerate() {
            // the frame in the editor is shown as it is being set up
            let edited = self.frame_editor.as_ref().map(|ed| &ed.frame);
            let mut shown: Vec<&Frame> = self.data.frames_on(*day).iter().filter_map(|o| self.data.frame(o.frame)).filter(|f| edited.is_none_or(|e| e.id != f.id)).collect();
            if let Some(f) = edited {
                if f.end_min > f.start_min && Data::frame_occurs_on(f, *day) {
                    shown.push(f);
                }
            }
            for f in shown {
                let s = (f.start_min.max(gs)).min(ge);
                let e = (f.end_min.min(ge)).max(gs);
                if e <= s {
                    continue;
                }
                let col_l = x0 + di as f32 * col_w;
                let r = Rect::from_min_max(pos2(col_l + 2.0, y_of(s)), pos2(col_l + col_w - 2.0, y_of(e)));
                let color = rgb(self.data.frame_color(f));
                // see-through, so the grid lines stay visible: lightened on a
                // dark theme, a plain tint on a light one
                let wash = if self.pal.dark { mix(color, Color32::WHITE, 0.25).gamma_multiply(0.14) } else { color.gamma_multiply(0.09) };
                fill_squircle(&painter, r, RADIUS, wash);
                // a line of its colour along the top and the bottom
                if r.height() > 8.0 {
                    for y in [r.top() + 1.0, r.bottom() - 3.0] {
                        painter.rect_filled(Rect::from_min_max(pos2(r.left() + 4.0, y), pos2(r.right() - 4.0, y + 2.0)), 1.0, color);
                    }
                }
                let label = painter.with_clip_rect(r.intersect(painter.clip_rect())).text(
                    r.left_top() + vec2(6.0, 5.0),
                    Align2::LEFT_TOP,
                    &f.name,
                    FontId::proportional(11.0),
                    self.pal.muted,
                );
                frame_boxes.push(FrameBox { id: f.id, day: *day, rect: r, label, color });
            }
        }

        // has the grid itself changed size since the last frame?
        let grid_id = Id::unique("grid_shape");
        let shape = (hh, col_w);
        let regrid = ctx.data(|d| d.get_temp::<(f32, f32)>(grid_id)) != Some(shape);
        ctx.data_mut(|d| d.insert_temp(grid_id, shape));
        let mut items: Vec<Item> = vec![];
        for (di, day) in days.iter().enumerate() {
            let d0 = midnight(*day);
            let d1 = d0 + Duration::days(1);
            let mut evs: Vec<(Event, bool)> = self.data.timed_on(*day).into_iter().map(|e| (e.clone(), false)).collect();
            if let Some(ed) = &self.editor {
                if ed.is_new && !ed.ev.all_day && ed.ev.start < d1 && ed.ev.end > d0 {
                    evs.push((ed.ev.clone(), true));
                }
            }
            if let Some(cr) = &self.create {
                if !cr.ev.all_day && cr.ev.start < d1 && cr.ev.end > d0 {
                    evs.push((cr.ev.clone(), true));
                }
            }
            if let Some(Drag::Create { draft, .. }) = &self.drag {
                if draft.start < d1 && draft.end > d0 {
                    evs.push((draft.clone(), true));
                }
            }
            evs.sort_by(|a, b| {
                a.0.start
                    .cmp(&b.0.start)
                    .then(b.0.minutes().cmp(&a.0.minutes()))
                    .then(a.0.calendar.cmp(&b.0.calendar))
                    .then(b.0.title.cmp(&a.0.title))
            });
            // each event loses 1 px at top and bottom and is at least 12 px tall
            let boxes: Vec<(f32, f32)> = evs
                .iter()
                .map(|(e, _)| {
                    let s = (e.start.max(d0) - d0).num_minutes();
                    let en = (e.end.min(d1) - d0).num_minutes();
                    let (t, b) = (y_of(s), y_of(en));
                    let h = (b - t).max(0.0);
                    let gap = EVENT_EDGE_GAP.min(h / 4.0);
                    (t + gap, t + gap + (h - 2.0 * gap).max(EVENT_MIN_H))
                })
                .collect();
            let cw = col_w - 8.0;
            let lay = overlap_layout(&boxes, OVERLAP_INSET / col_w);
            let mut day_items: Vec<(usize, Item)> = vec![];
            for (k, (e, draft)) in evs.iter().enumerate() {
                let (top_y, bottom_y) = boxes[k];
                let (left, width, layer) = lay[k];
                let x = x0 + di as f32 * col_w + left * cw;
                let mut task = e.task_id.and_then(|t| self.data.task(t)).map(|t| (t.id, t.done));
                // a block still being drawn or named looks like what it will be
                let editing = self.editor.as_ref().is_some_and(|ed| ed.ev.id == e.id);
                if *draft && !editing && self.create.as_ref().is_none_or(|c| c.kind == CreateKind::Task) {
                    task = Some((0, false));
                }
                let mut target = Rect::from_min_max(pos2(x, top_y), pos2(x + width * cw, bottom_y));
                if !*draft {
                    let dragging = matches!(&self.drag, Some(Drag::Move { id, .. }) | Some(Drag::Resize { id }) if *id == e.id);
                    // a zoom or a new column width moves every block at once, like the frames
                    let secs = if regrid { 0.0 } else if dragging { ANIM_FAST_S * 0.6 } else { ANIM_S };
                    let key = Id::unique(("block", e.id, (*day - e.start.date()).num_days()));
                    let ax = ctx.animate_value_with_time(key.with("x"), target.left() - x0, secs);
                    let aw = ctx.animate_value_with_time(key.with("w"), target.width(), secs);
                    let at = ctx.animate_value_with_time(key.with("t"), target.top() - top, secs);
                    let ab = ctx.animate_value_with_time(key.with("b"), target.bottom() - top, secs);
                    target = Rect::from_min_max(pos2(x0 + ax, top + at), pos2(x0 + ax + aw, top + ab));
                }
                day_items.push((
                    layer,
                    Item {
                        id: e.id,
                        rect: target,
                        // a block without a name says what it is
                        title: if !e.title.is_empty() {
                            e.title.clone()
                        } else if task.is_some() || self.create.as_ref().is_some_and(|c| c.ev.id == e.id && c.kind == CreateKind::Task) {
                            "New task".into()
                        } else {
                            "New Event".into()
                        },
                        sub: format!("{} – {}", p.fmt_time(e.start.time()), p.fmt_time(e.end.time())),
                        color: rgb(self.data.event_color(e)),
                        task,
                        draft: *draft,
                        past: p.dim_past && e.end < now,
                    },
                ));
            }
            day_items.sort_by_key(|x| x.0);
            items.extend(day_items.into_iter().map(|x| x.1));
        }
        let hover_id = resp.hover_pos().and_then(|hp| items.iter().rev().find(|it| !it.draft && it.rect.contains(hp))).map(|it| it.id);
        for it in &items {
            let hovered = !it.draft && self.drag.is_none() && self.drag_task.is_none() && hover_id == Some(it.id);
            let glow = ctx.animate_bool_with_time(Id::unique(("glow", it.id)), hovered, ANIM_FAST_S);
            draw_item(&painter, &self.pal, it, self.selected == Some(it.id) && !it.draft, glow);
        }

        if let Some(i) = days.iter().position(|d| *d == today) {
            let m = now.hour() as i64 * 60 + now.minute() as i64;
            if m >= gs && m <= ge {
                let y = y_of(m);
                let xa = x0 + i as f32 * col_w;
                painter.line_segment([pos2(xa, y), pos2(xa + col_w, y)], Stroke::new(1.5, self.pal.now));
                painter.circle_filled(pos2(xa, y), 4.0, self.pal.now);
            }
        }

        let right = rect.right();
        let bottom = rect.bottom();
        let days_v = days.to_vec();
        let pos_to = move |pos: Pos2| -> NaiveDateTime {
            let x = pos.x.clamp(x0, right - 1.0);
            let di = (((x - x0) / col_w) as usize).min(n - 1);
            let y = pos.y.clamp(top, bottom);
            let m = gs + ((y - top) / hh * 60.0) as i64;
            midnight(days_v[di]) + Duration::minutes(m.clamp(gs, ge))
        };
        let hit = |pos: Pos2| items.iter().rev().find(|it| !it.draft && it.rect.contains(pos));
        let snap = p.snap_min;

        if let Some(hp) = resp.hover_pos() {
            if self.drag_task.is_none() {
                if let Some(it) = hit(hp) {
                    ctx.set_cursor_icon(if hp.y > it.rect.bottom() - 6.0 {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::PointingHand
                    });
                }
            }
        }

        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                if let Some(it) = hit(o).filter(|it| !self.data.event(it.id).is_some_and(|e| self.data.locked(e).is_some())) {
                    let id = it.id;
                    let resize = o.y > it.rect.bottom() - 6.0;
                    self.snapshot();
                    self.selected = Some(id);
                    if resize {
                        self.drag = Some(Drag::Resize { id });
                    } else if let Some(e) = self.data.event(id) {
                        let grab_min = (pos_to(o) - e.start).num_minutes();
                        self.drag = Some(Drag::Move { id, grab_min });
                    }
                } else if o.x >= x0 {
                    let anchor = snap_dt(pos_to(o), snap, -1);
                    self.editor = None;
                    self.create = None;
                    self.selected = None;
                    self.drag = Some(Drag::Create {
                        anchor,
                        draft: Event {
                            id: 0,
                            title: String::new(),
                            start: anchor,
                            end: anchor + Duration::minutes(snap),
                            all_day: false,
                            calendar: self.data.first_own_calendar(),
                            notes: String::new(),
                            task_id: None,
                        color: None,
                        ..Default::default()
                        },
                    });
                }
            }
        }
        if resp.dragged_by(PointerButton::Primary) {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = pos_to(pos);
                match &mut self.drag {
                    Some(Drag::Move { id, grab_min }) => {
                        let ns = snap_dt(t - Duration::minutes(*grab_min), snap, 0);
                        if let Some(e) = self.data.event_mut(*id) {
                            let len = e.end - e.start;
                            e.start = ns;
                            e.end = ns + len;
                        }
                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    Some(Drag::Resize { id }) => {
                        if let Some(e) = self.data.event_mut(*id) {
                            e.end = snap_dt(t, snap, 0).max(e.start + Duration::minutes(snap));
                        }
                        ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    Some(Drag::Create { anchor, draft }) => {
                        let t2 = snap_dt(t, snap, 1);
                        if t2 > *anchor {
                            draft.start = *anchor;
                            draft.end = t2;
                        } else {
                            draft.start = snap_dt(t, snap, -1);
                            draft.end = *anchor + Duration::minutes(snap);
                        }
                    }
                    None => {}
                }
            }
        }
        if resp.drag_stopped() {
            match self.drag.take() {
                Some(Drag::Create { mut draft, .. }) => {
                    draft.id = self.data.new_id();
                    // the create window sits beside the drawn block
                    let anchor = items
                        .iter()
                        .filter(|it| it.draft && it.id == 0)
                        .map(|it| it.rect)
                        .reduce(|a, b| a.union(b))
                        .unwrap_or_else(|| Rect::from_center_size(rect.center(), vec2(120.0, 40.0)));
                    self.create = Some(CreateEdit { ev: draft, kind: CreateKind::Task, anchor_rect: anchor, focus: true });
                }
                Some(Drag::Move { id, .. }) => {
                    let sched = self.data.event(id).and_then(|e| e.task_id.map(|t| (t, e.start.date())));
                    if let Some((tid, day)) = sched {
                        self.after_task_scheduled(&ctx, tid, day);
                    }
                    self.commit();
                }
                Some(Drag::Resize { .. }) => self.commit(),
                None => {}
            }
        }

        let hit_info = |pos: Pos2| hit(pos).map(|it| (it.id, it.task, it.rect));
        // test aid for pictures: open the editor on the Nth visible block, as
        // a double-click would (KALENDAR_SHOW_EDIT=N; only in picture mode)
        if self.shot.is_some() && self.frames == 30 {
            if let Some(n) = std::env::var("KALENDAR_SHOW_EDIT").ok().and_then(|v| v.parse::<usize>().ok()) {
                let mut real: Vec<_> = items.iter().filter(|it| !it.draft).map(|it| (it.id, it.rect)).collect();
                real.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()).then(a.1.top().total_cmp(&b.1.top())));
                if let Some((id, r)) = real.get(n.min(real.len().saturating_sub(1))).copied() {
                    self.edit_event_at(id, Some(r));
                }
            }
        }
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some((id, task, r)) = hit_info(pos) {
                    match task {
                        Some((tid, _)) if check_rect(r.shrink2(vec2(1.0, 0.0))).expand(3.0).contains(pos) => {
                            self.toggle_task(tid)
                        }
                        _ => self.selected = Some(id),
                    }
                } else if self.editor.is_some() || self.create.is_some() {
                    self.editor = None;
                    self.create = None;
                } else if self.selected.is_some() {
                    self.selected = None;
                } else if pos.x >= x0 {
                    // one click makes a draft one grid slot long
                    let start = snap_dt(pos_to(pos), snap, -1);
                    let di = days.iter().position(|d| *d == start.date()).unwrap_or(0);
                    let s = (start - midnight(start.date())).num_minutes();
                    let block = Rect::from_min_max(
                        pos2(x0 + di as f32 * col_w, y_of(s)),
                        pos2(x0 + (di + 1) as f32 * col_w - 8.0, y_of(s + p.slot_min)),
                    );
                    self.open_create(start, p.slot_min, block);
                }
            }
        }
        // the frame under a point, when no event is there
        let frame_at = |pos: Pos2| frame_boxes.iter().rev().find(|b| b.rect.contains(pos));
        if resp.double_clicked() {
            if let Some((id, _, r)) = resp.interact_pointer_pos().and_then(hit_info) {
                self.edit_event_at(id, Some(r));
            } else if let Some(b) = resp.interact_pointer_pos().and_then(|q| frame_boxes.iter().find(|b| b.label.expand(2.0).contains(q))) {
                // the frame's name label opens its editor
                self.edit_frame(b.id);
            }
        }
        if resp.secondary_clicked() {
            let pos = resp.interact_pointer_pos();
            self.ctx_target = pos.and_then(hit_info).map(|x| x.0);
            self.ctx_time = pos.map(|q| snap_dt(pos_to(q), snap, -1));
            self.ctx_frame = if self.ctx_target.is_none() { pos.and_then(frame_at).map(|b| (b.id, b.day)) } else { None };
            if self.ctx_target.is_some() {
                self.selected = self.ctx_target;
            }
        }
        let target = self.ctx_target;
        let at = self.ctx_time;
        let in_frame = self.ctx_frame.and_then(|(id, day)| self.data.frame(id).map(|f| (id, day, f.name.clone())));
        resp.context_menu(|ui| {
            if let Some(id) = target {
                self.block_menu(ui, id);
            } else if let Some(t) = at {
                ui.set_min_width(190.0);
                if self.clipboard.is_some() && ui.button(format!("{}  Paste here", icons::PASTE)).clicked() {
                    self.paste_at(t, Some(false));
                    ui.close();
                }
                if ui.button(format!("{}  Create new event", icons::CALENDAR_PLUS)).clicked() {
                    self.open_new_event(t, NEW_EVENT_MIN, false);
                    ui.close();
                }
                let ctx = ui.ctx().clone();
                if let Some((fid, day, name)) = &in_frame {
                    ui.separator();
                    ui.label(RichText::new(format!("Frame: {name}")).size(11.5).color(self.pal.muted));
                    if ui.button(format!("{}  Edit frame…", icons::PEN)).clicked() {
                        self.edit_frame(*fid);
                        ui.close();
                    }
                    if ui.button(format!("{}  Skip this day", icons::BAN)).clicked() {
                        self.skip_frame_day(&ctx, *fid, *day);
                        ui.close();
                    }
                    if ui.button(RichText::new(format!("{}  Delete frame", icons::TRASH)).color(Color32::from_rgb(0xeb, 0x62, 0x62))).clicked() {
                        self.delete_frame(&ctx, *fid);
                        ui.close();
                    }
                } else if ui.button(format!("{}  New frame here", icons::CALENDAR_DAYS)).clicked() {
                    let s = (t - midnight(t.date())).num_minutes();
                    self.open_new_frame(t.date(), s, (s + 60).min(24 * 60), self.data.first_own_calendar(), String::new());
                    ui.close();
                }
            }
        });

        // a task dragged from the list
        if let Some(tid) = self.drag_task {
            if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
                if rect.intersect(ui.clip_rect()).contains(pos) && pos.x >= x0 {
                    let start = snap_dt(pos_to(pos), snap, 0);
                    let mins = self.data.task(tid).and_then(|t| t.duration_min).unwrap_or(p.default_task_min);
                    let di = days.iter().position(|d| *d == start.date()).unwrap_or(0);
                    let s = (start - midnight(start.date())).num_minutes();
                    let r = Rect::from_min_max(
                        pos2(x0 + di as f32 * col_w, y_of(s)),
                        pos2(x0 + (di + 1) as f32 * col_w - 8.0, y_of(s + mins)),
                    );
                    // over a frame the ghost takes its colour, dimmed when the
                    // frame would not take this task; the drop works either way
                    let over = frame_at(pos).filter(|b| b.day == start.date());
                    let fits = over.is_none_or(|b| {
                        let f = self.data.frame(b.id);
                        let t = self.data.task(tid);
                        f.zip(t).is_none_or(|(f, t)| f.matches(t, p.default_task_min))
                    });
                    let bg = self.pal.bg;
                    let (fill, line, text) = match over {
                        Some(b) if fits => (mix(bg, b.color, 0.35), b.color, self.pal.text),
                        Some(b) => (mix(bg, b.color, 0.10), mix(bg, b.color, 0.40), self.pal.muted),
                        None => (mix(bg, self.pal.accent, 0.25), self.pal.accent, self.pal.text),
                    };
                    fill_squircle(&painter, r, RADIUS, fill);
                    stroke_squircle(&painter, r.shrink(0.5), RADIUS, Stroke::new(1.0, line));
                    painter.text(r.left_top() + vec2(8.0, 4.0), Align2::LEFT_TOP, p.fmt_time(start.time()), FontId::proportional(11.0), text);
                    if !fits {
                        // shown under the title by ui_drag_ghost, which runs after the grid
                        ctx.data_mut(|d| d.insert_temp(Id::unique("drag_misfit"), true));
                    }
                    if ctx.input(|i| i.pointer.any_released()) {
                        self.schedule_task(&ctx, tid, start);
                        self.drag_task = None;
                    }
                }
            }
        }
    }

    /// The right-click menu of a calendar block: colours, copy, the planner
    /// actions of a task session, and the red actions at the bottom.
    pub fn block_menu(&mut self, ui: &mut egui::Ui, id: u64) {
        let ctx = ui.ctx().clone();
        let Some(ev) = self.data.event(id).cloned() else {
            ui.close();
            return;
        };
        let red = Color32::from_rgb(0xeb, 0x62, 0x62);
        let muted = self.pal.muted;
        if self.data.locked(&ev).is_some() {
            // a subscribed event can only be copied
            ui.set_min_width(260.0);
            let cal = self.data.calendar(ev.calendar).map(|c| c.name.clone()).unwrap_or_default();
            ui.label(RichText::new(format!("From {cal} (read-only)")).color(muted));
            ui.separator();
            if ui.button("Copy event").clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            return;
        }
        ui.set_min_width(330.0);
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        ui.spacing_mut().button_padding = vec2(10.0, 7.0);
        // colour dots: "no colour" first, then the palette
        ui.add_space(4.0);
        let mut pick: Option<Option<[u8; 3]>> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            ui.add_space(8.0);
            let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            let c = if resp.hovered() { self.pal.text } else { muted };
            ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(2.0, c));
            ui.painter().line_segment([r.center() + vec2(-6.0, -6.0), r.center() + vec2(6.0, 6.0)], Stroke::new(2.0, c));
            if resp.on_hover_text("Calendar colour").clicked() {
                pick = Some(None);
            }
            for (i, (col, name)) in EVENT_COLORS.iter().enumerate() {
                if i == 8 {
                    ui.end_row();
                    ui.add_space(8.0);
                }
                let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                let grow = ctx.animate_bool_with_time(Id::unique(("dot", i)), resp.hovered(), ANIM_FAST_S);
                ui.painter().circle_filled(r.center(), 10.0 + 2.0 * grow, rgb(*col));
                if ev.color == Some(*col) {
                    ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(1.5, self.pal.text));
                }
                if resp.on_hover_text(*name).clicked() {
                    pick = Some(Some(*col));
                }
            }
        });
        if let Some(c) = pick {
            self.snapshot();
            if let Some(e) = self.data.event_mut(id) {
                e.color = c;
            }
            self.commit();
            ui.close();
        }
        ui.separator();
        let big = |t: &str| RichText::new(t).size(15.0);
        if let Some(tid) = ev.task_id {
            if ui.button(big(&format!("{}   Copy task session", icons::COPY))).clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("Planner").size(14.0).color(muted));
            });
            ui.menu_button(big(&format!("{}   Replan session", icons::REFRESH)), |ui| {
                ui.set_min_width(300.0);
                let row = |ui: &mut egui::Ui, label: &str, keys: &str, enabled: bool| -> bool {
                    let mut hit = false;
                    ui.horizontal(|ui| {
                        hit = ui.add_enabled(enabled, egui::Button::new(label).frame(false)).clicked();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(keys).monospace().size(11.0).color(muted));
                        });
                    });
                    hit
                };
                if row(ui, "Reschedule this session", "Shift+R", true) {
                    self.reschedule_session(&ctx, id);
                    ui.close();
                }
                if row(ui, "Complete & schedule more time", "Shift+F", true) {
                    self.follow_up(&ctx, id);
                    ui.close();
                }
                if row(ui, "Split & reschedule remaining time", "Shift+S", self.split_time(id).is_some()) {
                    self.split_session(&ctx, id);
                    ui.close();
                }
                ui.separator();
                if row(ui, "Unschedule session", "Del", true) {
                    self.delete_event(&ctx, id);
                    ui.close();
                }
            });
            let mut flexible = self.data.task(tid).map(|t| t.flexible).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(big(&format!("{}   Flexible task", icons::SHUFFLE)));
                ui.label(RichText::new("(i)").size(11.0).color(muted)).on_hover_text(
                    "The planner may move flexible tasks to make room for more important ones.",
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(6.0);
                    if ui.checkbox(&mut flexible, "").changed() {
                        self.snapshot();
                        if let Some(t) = self.data.task_mut(tid) {
                            t.flexible = flexible;
                        }
                        self.commit();
                    }
                });
            });
            ui.separator();
            if ui.button(big(&format!("{}   Unschedule session", icons::X)).color(red)).clicked() {
                self.delete_event(&ctx, id);
                ui.close();
            }
            if ui.button(big(&format!("{}   Unschedule and delete task", icons::TRASH)).color(red)).clicked() {
                self.delete_task(&ctx, tid);
                ui.close();
            }
        } else {
            if ui.button(big(&format!("{}   Copy event", icons::COPY))).clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            ui.separator();
            if ui.button(big(&format!("{}   Delete event", icons::TRASH)).color(red)).clicked() {
                self.delete_event(&ctx, id);
                ui.close();
            }
        }
        ui.add_space(2.0);
    }

    // ---------------------------------------------------------------- month

    pub fn ui_month(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let days = self.visible_days();
        let cols = if self.data.prefs.weekends_visible { 7 } else { 5 };
        let rows = (days.len() / cols).max(1);
        let p = self.data.prefs.clone();
        let today = Local::now().date_naive();
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click());
        let painter = ui.painter_at(rect);
        let head_h = 28.0;
        let cw = rect.width() / cols as f32;
        let ch = (rect.height() - head_h) / rows as f32;
        for c in 0..cols {
            painter.text(
                pos2(rect.left() + c as f32 * cw + 8.0, rect.top() + head_h / 2.0),
                Align2::LEFT_CENTER,
                days[c].format("%a").to_string(),
                FontId::proportional(12.0),
                self.pal.muted,
            );
        }
        #[derive(Clone, Copy)]
        enum Hit {
            Ev(u64),
            Day(NaiveDate),
        }
        let mut hits: Vec<(Rect, Hit)> = vec![];
        let mut cells: Vec<(Rect, NaiveDate)> = vec![];
        let fit = (((ch - 28.0) / 19.0).floor() as usize).max(1);
        let limit = p.day_max_events.min(fit);
        for (i, d) in days.iter().enumerate() {
            let (r, c) = (i / cols, i % cols);
            let cell = Rect::from_min_size(pos2(rect.left() + c as f32 * cw, rect.top() + head_h + r as f32 * ch), vec2(cw, ch));
            cells.push((cell, *d));
            if *d == today {
                painter.rect_filled(cell, 0.0, self.pal.today);
            } else if d.weekday().num_days_from_monday() >= 5 {
                painter.rect_filled(cell, 0.0, self.pal.weekend);
            }
            painter.rect_stroke(cell, 0.0, Stroke::new(0.5, self.pal.border), StrokeKind::Middle);
            let other = d.month() != self.anchor.month();
            let num = if d.day() == 1 { d.format("%b %-d").to_string() } else { d.day().to_string() };
            let num_rect = Rect::from_min_size(cell.left_top() + vec2(4.0, 3.0), vec2(if d.day() == 1 { 48.0 } else { 24.0 }, 20.0));
            if *d == today {
                painter.rect_filled(num_rect, 6.0, self.pal.accent);
                painter.text(num_rect.center(), Align2::CENTER_CENTER, num, FontId::proportional(12.5), Color32::WHITE);
            } else {
                let c = if other { self.pal.muted } else { self.pal.text };
                painter.text(num_rect.center(), Align2::CENTER_CENTER, num, FontId::proportional(12.5), c);
            }
            hits.push((num_rect, Hit::Day(*d)));
            let mut evs: Vec<Event> = self.data.all_day_on(*d).into_iter().cloned().collect();
            evs.extend(self.data.timed_on(*d).into_iter().filter(|e| e.start.date() == *d).cloned());
            if let Some(ed) = &self.editor {
                if ed.is_new && ed.ev.start.date() == *d {
                    evs.push(ed.ev.clone());
                }
            }
            let show = if evs.len() > limit { limit.saturating_sub(1) } else { evs.len() };
            let clip = painter.with_clip_rect(cell.shrink(1.0));
            for (k, e) in evs.iter().take(show).enumerate() {
                let er = Rect::from_min_size(cell.left_top() + vec2(3.0, 26.0 + k as f32 * 19.0), vec2(cw - 6.0, 17.0));
                let color = rgb(self.data.event_color(e));
                let title = if e.title.is_empty() { "New Event" } else { &e.title };
                let tclip = painter.with_clip_rect(er.intersect(cell));
                if e.all_day {
                    clip.rect_filled(er, 4.0, mix(self.pal.bg, color, 0.55));
                    tclip.text(er.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(11.5), self.pal.text);
                } else {
                    clip.circle_filled(er.left_center() + vec2(6.0, 0.0), 3.5, color);
                    let label = format!("{}  {}", p.fmt_time(e.start.time()), title);
                    let done = e.task_id.and_then(|t| self.data.task(t)).map(|t| t.done).unwrap_or(false);
                    let c = if done { self.pal.muted } else { self.pal.text };
                    tclip.text(er.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(11.5), c);
                }
                if self.selected == Some(e.id) {
                    clip.rect_stroke(er, 4.0, Stroke::new(1.0, self.pal.text), StrokeKind::Inside);
                }
                hits.push((er, Hit::Ev(e.id)));
            }
            if evs.len() > show {
                let er = Rect::from_min_size(cell.left_top() + vec2(3.0, 26.0 + show as f32 * 19.0), vec2(cw - 6.0, 17.0));
                clip.text(
                    er.left_center() + vec2(6.0, 0.0),
                    Align2::LEFT_CENTER,
                    format!("+{} more", evs.len() - show),
                    FontId::proportional(11.5),
                    self.pal.muted,
                );
                hits.push((er, Hit::Day(*d)));
            }
        }
        let cell_at = |pos: Pos2| cells.iter().find(|(r, _)| r.contains(pos)).map(|(_, d)| *d);
        let hit_at = |pos: Pos2| hits.iter().rev().find(|(r, _)| r.contains(pos)).map(|(_, h)| *h);
        let hit_rect = |pos: Pos2| hits.iter().rev().find(|(r, _)| r.contains(pos)).map(|(r, h)| (*r, *h));
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                match hit_at(pos) {
                    Some(Hit::Ev(id)) => self.selected = Some(id),
                    Some(Hit::Day(d)) => {
                        self.anchor = d;
                        self.view = View::Days(1);
                    }
                    None => {
                        if self.editor.is_some() {
                            self.editor = None;
                        } else if self.selected.is_some() {
                            self.selected = None;
                        } else if let Some(d) = cell_at(pos) {
                            self.open_new_event(midnight(d), 1440, true);
                        }
                    }
                }
            }
        }
        if resp.double_clicked() {
            if let Some((r, Hit::Ev(id))) = resp.interact_pointer_pos().and_then(hit_rect) {
                self.edit_event_at(id, Some(r));
            }
        }
        if let Some(tid) = self.drag_task {
            if let Some(d) = ctx.input(|i| i.pointer.latest_pos()).and_then(cell_at) {
                if let Some((r, _)) = cells.iter().find(|(_, x)| *x == d) {
                    painter.rect_stroke(*r, 0.0, Stroke::new(2.0, self.pal.accent), StrokeKind::Inside);
                }
                if ctx.input(|i| i.pointer.any_released()) {
                    self.snapshot();
                    if let Some(t) = self.data.task_mut(tid) {
                        t.due = Some(d);
                    }
                    self.commit();
                    self.say(&ctx, format!("Due date set to {}", d.format("%b %-d")), true);
                    self.drag_task = None;
                }
            }
        }
    }

    // --------------------------------------------------------------- agenda

    pub fn ui_agenda(&mut self, ui: &mut egui::Ui) {
        let days = self.visible_days();
        let today = Local::now().date_naive();
        let p = self.data.prefs.clone();
        let (muted, accent, border) = (self.pal.muted, self.pal.accent, self.pal.border_minor);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(Margin::symmetric(24, 12)).show(ui, |ui| {
                ui.label(RichText::new(format!("Week of {}", days[0].format("%B %-d"))).color(muted));
                for d in &days {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(d.format("%A, %B %-d").to_string()).size(15.0).strong());
                        if *d == today {
                            egui::Frame::new().fill(accent).corner_radius(8).inner_margin(Margin::symmetric(8, 1)).show(ui, |ui| {
                                ui.label(RichText::new("Today").size(11.0).color(Color32::WHITE));
                            });
                        }
                        if ui.small_button("+").on_hover_text("New event").clicked() {
                            self.open_new_event(midnight(*d) + Duration::hours(NEW_EVENT_NOON_H), NEW_EVENT_MIN, false);
                        }
                    });
                    let mut evs: Vec<Event> = self.data.all_day_on(*d).into_iter().cloned().collect();
                    evs.extend(self.data.timed_on(*d).into_iter().cloned());
                    if evs.is_empty() {
                        ui.label(RichText::new("Nothing planned").color(muted));
                    }
                    for e in evs {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.5, rgb(self.data.event_color(&e)));
                            let when = if e.all_day {
                                "All day".to_string()
                            } else {
                                format!("{} – {}", p.fmt_time(e.start.time()), p.fmt_time(e.end.time()))
                            };
                            ui.add_sized([150.0, 16.0], egui::Label::new(RichText::new(when).color(muted)).halign(Align::LEFT));
                            let task = e.task_id.and_then(|t| self.data.task(t)).map(|t| (t.id, t.done));
                            if let Some((tid, mut done)) = task {
                                if ui.checkbox(&mut done, "").changed() {
                                    self.toggle_task(tid);
                                }
                            }
                            let sel = self.selected == Some(e.id);
                            let title = if e.title.is_empty() { "(No title)" } else { &e.title };
                            let r = ui.selectable_label(sel, title);
                            if r.clicked() {
                                self.selected = Some(e.id);
                            }
                            if r.double_clicked() {
                                self.edit_event_at(e.id, Some(r.rect));
                            }
                        });
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, border));
                }
            });
        });
    }
}
