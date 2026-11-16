//! What is drawn in the main window: toolbar, task list and calendar views.

use crate::strings::{tr, trf};
use crate::ids;
use crate::motion;
use crate::theme;
use crate::icons;
use crate::model::*;
use crate::{fill_squircle, mix, rgb, stroke_squircle, App, Cmd, CreateEdit, CreateKind, Drag, Palette};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Layout, Margin, PointerButton, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind,
};

#[derive(Clone)]
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
    /// left out on this day: shown grey, and takes no tasks
    skipped: bool,
}

/// Draws one calendar block. Plain events are filled with their colour and
/// carry dark text; task sessions are dark with a coloured bar and a box to
/// tick. `hover` is 0..1 and brightens the block.
fn draw_item(p: &egui::Painter, pal: &Palette, it: &Item, selected: bool, hover: f32) {
    let r = it.rect.shrink2(vec2(1.0, 0.0));
    let done = matches!(it.task, Some((_, true)));
    let is_task = it.task.is_some();
    // a finished task goes grey
    let grey = Item { color: mix(pal.muted, pal.bg, 0.35), ..it.clone() };
    let it = if done { &grey } else { it };
    let soft = mix(it.color, Color32::from_gray(if pal.dark { 228 } else { 245 }), 0.5);
    let (fill, text, sub) = if is_task {
        let base = mix(pal.bg, it.color, if it.draft || it.past { 0.08 } else { 0.14 });
        (mix(base, it.color, 0.10 * hover), if done || it.past { pal.muted } else { pal.text }, pal.muted)
    } else {
        let base = if selected { it.color } else { soft };
        let base = if it.draft || it.past { mix(pal.bg, base, 0.45) } else { base };
        let ink = theme::INK;
        (mix(base, it.color, 0.5 * hover), ink, mix(ink, base, 0.25))
    };
    fill_squircle(p, r, theme::RADIUS, fill);
    let edge = if selected { pal.text } else if is_task { mix(pal.bg, it.color, 0.75) } else { fill };
    stroke_squircle(p, r.shrink(0.5), theme::RADIUS, Stroke::new(if selected { 1.5 } else { 1.0 }, edge));
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
    let title = if it.title.is_empty() { tr("New Event") } else { &it.title };
    let title_font = theme::bold(theme::size::CAPTION);
    let mut ty = r.top() + 2.5;
    if tall {
        clip.text(pos2(x, r.top() + 3.0), Align2::LEFT_TOP, &it.sub, FontId::proportional(theme::size::TINY), sub);
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
    let font = if !tall && r.height() < 20.0 { theme::bold(theme::size::MICRO) } else { title_font };
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
    /// The name of the frame that keeps this task session out of where it
    /// now sits, if one does.
    fn session_kept_out(&self, id: u64) -> Option<String> {
        let e = self.data.event(id)?;
        let t = self.data.task(e.task_id?)?;
        self.data.frame_keeping_out(t, e.start, e.end).map(|f| f.name.clone())
    }

    pub fn ui_drag_ghost(&mut self, ctx: &egui::Context) {
        let Some(id) = self.drag_task else { return };
        let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
        let Some(t) = self.data.task(id) else { return };
        // Some(true): the frame under it keeps this task out altogether
        let refused = ctx.data_mut(|d| d.remove_temp::<bool>(ids::drag_misfit()));
        let misfit = refused.is_some();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, ids::ghost()));
        let h = if misfit { 50.0 } else { 34.0 };
        let r = Rect::from_min_size(pos + vec2(10.0, 8.0), vec2(230.0, h));
        fill_squircle(&p, r, theme::RADIUS, mix(self.pal.side, self.pal.text, 0.12));
        stroke_squircle(&p, r.shrink(0.5), theme::RADIUS, Stroke::new(1.0, rgb(self.data.list_look(t.list).1)));
        let clip = p.with_clip_rect(r.shrink(4.0));
        let title_y = if misfit { r.top() + 17.0 } else { r.center().y };
        clip.text(pos2(r.left() + 10.0, title_y), Align2::LEFT_CENTER, &t.title, FontId::proportional(theme::size::CAPTION), self.pal.text);
        if misfit {
            clip.text(
                pos2(r.left() + 10.0, r.bottom() - 14.0),
                Align2::LEFT_CENTER,
                if refused == Some(true) { tr("This frame keeps other tasks out") } else { tr("Not in this frame's filter") },
                FontId::proportional(theme::size::MICRO),
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
        hp.text(pos2(x0 - 8.0, hrect.top() + 50.0), Align2::RIGHT_TOP, "all-day", FontId::proportional(theme::size::MINI), self.pal.muted);
        hp.text(
            pos2(x0 - 8.0, hrect.top() + 22.0),
            Align2::RIGHT_CENTER,
            format!("W{}", days[0].iso_week().week()),
            FontId::proportional(theme::size::MINI),
            self.pal.muted,
        );
        for (i, d) in days.iter().enumerate() {
            let cx = x0 + i as f32 * col_w;
            // "Wed 11" centred over its day; today's number sits in a red disc
            let name = if col_w < 70.0 { d.format("%a").to_string()[..1].to_string() } else { d.format("%a").to_string() };
            let num = d.day().to_string();
            let is_today = *d == today;
            let gn = hp.layout_no_wrap(name, FontId::proportional(theme::size::UI), if is_today { self.pal.text } else { self.pal.muted });
            let gd = hp.layout_no_wrap(num, FontId::proportional(theme::size::UI), if is_today { Color32::WHITE } else { self.pal.muted });
            let disc = if is_today { 24.0 } else { gd.size().x };
            let total = gn.size().x + 6.0 + disc;
            let left = cx + (col_w - total) / 2.0;
            let cy = hrect.top() + 22.0;
            hp.galley(pos2(left, cy - gn.size().y / 2.0), gn.clone(), self.pal.muted);
            let dc = pos2(left + gn.size().x + 6.0 + disc / 2.0, cy);
            if is_today {
                hp.circle_filled(dc, 12.0, theme::DANGER);
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
            let title = if e.title.is_empty() { tr("New Event") } else { &e.title };
            hp.with_clip_rect(r.shrink(2.0)).text(
                r.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(theme::size::SMALL),
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
                let glow = ui.ctx().animate_bool_with_time(ids::due_glow(*tid), over, motion::HOVER.secs());
                hp.rect_filled(r, 4.0, mix(self.pal.bg, self.pal.text, 0.03 + 0.07 * glow));
                hp.rect_stroke(r, 4.0, Stroke::new(1.0, mix(self.pal.border, self.pal.text, 0.25)), StrokeKind::Inside);
                let c = hp.with_clip_rect(r.shrink(2.0));
                c.text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, icons::FLAG, FontId::proportional(theme::size::SMALL), self.pal.text);
                c.text(r.left_center() + vec2(22.0, 0.0), Align2::LEFT_CENTER, title, theme::bold(theme::size::SMALL), self.pal.text);
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
        // all-day blocks get the same right-click menu as timed ones
        if hresp.secondary_clicked() {
            let pos = hresp.interact_pointer_pos();
            self.ctx_target = pos.and_then(|q| chips.iter().find(|(_, r)| r.contains(q))).map(|(id, _)| *id);
            if self.ctx_target.is_some() {
                self.selected = self.ctx_target;
            }
        }
        if let Some(id) = self.ctx_target {
            context_menu(&hresp, |ui| self.block_menu(ui, id));
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
                let now_h = self.data.prefs.hour_height();
                let (from, at) = self.zoom_goal.map_or((now_h, now_h), |g| (g.0, g.2));
                let my = ctx.pointer_latest_pos().map_or(0.0, |p| (p.y - self.grid_scroll.1).max(0.0));
                self.zoom_goal = Some(((from * zoom).clamp(HOUR_H_MIN, HOUR_H_MAX).round(), my, at));
            }
            self.side_scroll += side;
            if self.side_scroll.abs() > 60.0 {
                self.anchor -= Duration::days(self.side_scroll.signum() as i64);
                self.side_scroll = 0.0;
            }
        }
        if let Some((goal, my, at)) = self.zoom_goal {
            // it ends on a whole number of points, close enough that the last
            // step cannot be seen
            let was = self.data.prefs.hour_height();
            let dt = ctx.input(|i| i.stable_dt).min(0.05);
            let mut h = if motion::on() { at + (goal - at) * (1.0 - (-dt * motion::ZOOM_RATE).exp()) } else { goal };
            if (goal - h).abs() < 0.02 {
                h = goal;
                self.zoom_goal = None;
            } else {
                self.zoom_goal = Some((goal, my, h));
                ctx.request_repaint();
            }
            self.data.prefs.hour_height_px = Some(h);
            let shown = self.data.prefs.hour_height();
            if shown != was {
                sa = sa.vertical_scroll_offset(((self.grid_scroll.0 + my) * shown / was - my).max(0.0));
            }
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
                    painter.text(pos2(x0 - 8.0, y), Align2::RIGHT_CENTER, p.fmt_hour(t), FontId::proportional(theme::size::MICRO), self.pal.muted);
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
            let shown: Vec<(&Frame, bool)> = shown.into_iter().map(|f| (f, false)).chain(self.data.frames_skipped_on(*day).into_iter().filter(|f| edited.is_none_or(|e| e.id != f.id)).map(|f| (f, true))).collect();
            for (f, skipped) in shown {
                let s = (f.start_min.max(gs)).min(ge);
                let e = (f.end_min.min(ge)).max(gs);
                if e <= s {
                    continue;
                }
                let col_l = x0 + di as f32 * col_w;
                let r = Rect::from_min_max(pos2(col_l + 2.0, y_of(s)), pos2(col_l + col_w - 2.0, y_of(e)));
                // a skipped day stays in view, greyed out
                let color = if skipped { mix(self.pal.muted, self.pal.bg, 0.45) } else { rgb(self.data.frame_color(f)) };
                // see-through, so the grid lines stay visible: lightened on a
                // dark theme, a plain tint on a light one
                let wash = if self.pal.dark { mix(color, Color32::WHITE, 0.25).gamma_multiply(0.14) } else { color.gamma_multiply(0.09) };
                fill_squircle(&painter, r, theme::RADIUS, wash);
                // a line of its colour along the top and the bottom
                if r.height() > 8.0 {
                    for y in [r.top() + 1.0, r.bottom() - 3.0] {
                        painter.rect_filled(Rect::from_min_max(pos2(r.left() + 4.0, y), pos2(r.right() - 4.0, y + 2.0)), 1.0, color);
                    }
                }
                let label = painter.with_clip_rect(r.intersect(painter.clip_rect())).text(
                    r.left_top() + vec2(6.0, 5.0),
                    Align2::LEFT_TOP,
                    if skipped { format!("{} (skipped)", f.name) } else { f.name.clone() },
                    FontId::proportional(theme::size::MICRO),
                    if skipped { mix(self.pal.muted, self.pal.bg, 0.3) } else { self.pal.muted },
                );
                frame_boxes.push(FrameBox { id: f.id, day: *day, rect: r, label, color, skipped });
            }
        }

        // has the grid itself changed size since the last frame?
        let grid_id = ids::grid_shape();
        // other days count too: the page slide carries the blocks then, so
        // they must not also glide on their own to where they now belong
        let shape = (hh, col_w, days[0], n);
        let regrid = ctx.data(|d| d.get_temp::<(f32, f32, NaiveDate, usize)>(grid_id)) != Some(shape);
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
                    let secs = if regrid { 0.0 } else if dragging { motion::FOLLOW.secs() } else { motion::NORMAL.secs() };
                    let key = ids::block((e.id, (*day - e.start.date()).num_days()));
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
                            tr("New task").into()
                        } else {
                            tr("New Event").into()
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
            let glow = ctx.animate_bool_with_time(ids::block_glow(it.id), hovered, motion::HOVER.secs());
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
                    self.close_block_window();
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
        // held at an edge, a drag carries on past it: up and down the day
        // scrolls, and a block being moved turns the page after a moment
        let edge_id = ids::drag_edge();
        let mut at_edge: Option<(i64, f64)> = None;
        if resp.dragged_by(PointerButton::Primary) && self.drag.is_some() {
            if let Some(pos) = ctx.pointer_latest_pos() {
                let view = rect.intersect(ui.clip_rect());
                let dt = ctx.input(|i| i.stable_dt).min(0.05);
                let zone = 26.0;
                let push = if pos.y < view.top() + zone {
                    (view.top() + zone - pos.y).min(zone * 2.0)
                } else if pos.y > view.bottom() - zone {
                    -(pos.y - (view.bottom() - zone)).min(zone * 2.0)
                } else {
                    0.0
                };
                if push != 0.0 {
                    ui.scroll_with_delta(vec2(0.0, push * 14.0 * dt));
                    ctx.request_repaint();
                }
                if matches!(self.drag, Some(Drag::Move { .. })) {
                    let dir = if pos.x > view.right() - zone { 1 } else if pos.x < x0 + zone { -1 } else { 0 };
                    if dir != 0 {
                        let now_s = ctx.input(|i| i.time);
                        let since = ctx.data(|d| d.get_temp::<(i64, f64)>(edge_id)).filter(|e| e.0 == dir).map_or(now_s, |e| e.1);
                        if now_s - since > motion::EDGE_HOLD_S {
                            self.step(dir);
                            // and again, a little slower, while it stays there
                            at_edge = Some((dir, now_s + motion::EDGE_REPEAT_S));
                        } else {
                            at_edge = Some((dir, since));
                        }
                        ctx.request_repaint();
                    }
                }
            }
        }
        match at_edge {
            Some(e) => {
                ctx.data_mut(|d| d.insert_temp(edge_id, e));
            }
            None => {
                ctx.data_mut(|d| d.remove_temp::<(i64, f64)>(edge_id));
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
                    self.close_block_window();
                    self.create = Some(CreateEdit::new(draft, CreateKind::Task, anchor));
                }
                Some(Drag::Move { id, .. }) | Some(Drag::Resize { id }) if self.session_kept_out(id).is_some() => {
                    // back to where it was: the drag began with a snapshot
                    let name = self.session_kept_out(id).unwrap_or_default();
                    if let Some(d) = self.undo.pop() {
                        self.data = d;
                    }
                    self.say(&ctx, trf("\"{}\" keeps other tasks out", &[&name]), false);
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
            if let Some(n) = crate::testaid::get().show_edit {
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
                } else if self.editor.is_some() || self.block_window_open() {
                    self.editor = None;
                    self.close_block_window();
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
        let in_frame = self.ctx_frame.and_then(|(id, day)| self.data.frame(id).map(|f| (id, day, f.name.clone(), f.skip.contains(&day))));
        context_menu(&resp, |ui| {
            if let Some(id) = target {
                self.block_menu(ui, id);
            } else if let Some(t) = at {
                let mut m = Menu::start(ui);
                if self.clipboard.is_some() && m.item(ui, icons::PASTE, tr("Paste here"), false).clicked() {
                    self.paste_at(t, Some(false));
                    ui.close();
                }
                if m.item(ui, icons::CALENDAR_PLUS, tr("Create new event"), false).clicked() {
                    self.open_new_event(t, NEW_EVENT_MIN, false);
                    ui.close();
                }
                let ctx = ui.ctx().clone();
                if let Some((fid, day, name, skipped)) = &in_frame {
                    m.line(ui);
                    m.heading(ui, &trf("Frame: {}", &[&name]));
                    if m.item(ui, icons::PEN, tr("Edit frame…"), false).clicked() {
                        self.edit_frame(*fid);
                        ui.close();
                    }
                    if *skipped {
                        if m.item(ui, icons::UNDO, tr("Use it this day again"), false).clicked() {
                            self.restore_frame_day(*fid, *day);
                            ui.close();
                        }
                    } else if m.item(ui, icons::BAN, tr("Skip this day"), false).clicked() {
                        self.skip_frame_day(&ctx, *fid, *day);
                        ui.close();
                    }
                    m.line(ui);
                    if m.item(ui, icons::TRASH, tr("Delete frame"), true).clicked() {
                        self.delete_frame(&ctx, *fid);
                        ui.close();
                    }
                } else if m.item(ui, icons::CALENDAR_DAYS, tr("New frame here"), false).clicked() {
                    let s = (t - midnight(t.date())).num_minutes();
                    self.open_new_frame(t.date(), s, (s + 60).min(24 * 60), self.data.first_own_calendar(), String::new());
                    ui.close();
                }
                m.end(ui);
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
                    // it glides from slot to slot like a block being moved;
                    // where it first shows, or the grid changes, it is just there
                    let key = ids::task_ghost(tid);
                    let frame = ctx.cumulative_frame_nr();
                    let seen = ctx.data(|d| d.get_temp::<u64>(key)).is_some_and(|f| f + 1 >= frame);
                    ctx.data_mut(|d| d.insert_temp(key, frame));
                    let secs = if regrid || !seen { 0.0 } else { motion::FOLLOW.secs() };
                    let ax = ctx.animate_value_with_time(key.with("x"), r.left() - x0, secs);
                    let at = ctx.animate_value_with_time(key.with("t"), r.top() - rect.top(), secs);
                    let ab = ctx.animate_value_with_time(key.with("b"), r.bottom() - rect.top(), secs);
                    let r = Rect::from_min_max(pos2(x0 + ax, rect.top() + at), pos2(x0 + ax + r.width(), rect.top() + ab));
                    // over a frame the ghost takes its colour, dimmed when the
                    // frame would not take this task; a frame that keeps
                    // other tasks out refuses the drop
                    let over = frame_at(pos).filter(|b| b.day == start.date() && !b.skipped);
                    let kept_out = self.data.task(tid).and_then(|t| self.data.frame_keeping_out(t, start, start + Duration::minutes(mins))).map(|f| f.name.clone());
                    let fits = over.is_none_or(|b| {
                        let f = self.data.frame(b.id);
                        let t = self.data.task(tid);
                        f.zip(t).is_none_or(|(f, t)| f.matches(t, p.default_task_min))
                    });
                    let bg = self.pal.bg;
                    let own = self.data.task(tid).map_or(self.pal.accent, |t| rgb(self.data.list_look(t.list).1));
                    let (fill, line, text) = match over {
                        Some(b) if fits => (mix(bg, own, 0.35), b.color, self.pal.text),
                        Some(b) => (mix(bg, b.color, 0.10), mix(bg, b.color, 0.40), self.pal.muted),
                        None => (mix(bg, own, 0.25), own, self.pal.text),
                    };
                    fill_squircle(&painter, r, theme::RADIUS, fill);
                    stroke_squircle(&painter, r.shrink(0.5), theme::RADIUS, Stroke::new(1.0, line));
                    painter.text(r.left_top() + vec2(8.0, 4.0), Align2::LEFT_TOP, p.fmt_time(start.time()), FontId::proportional(theme::size::MICRO), text);
                    if !fits || kept_out.is_some() {
                        // shown under the title by ui_drag_ghost, which runs after the grid
                        ctx.data_mut(|d| d.insert_temp(ids::drag_misfit(), kept_out.is_some()));
                    }
                    if ctx.input(|i| i.pointer.any_released()) {
                        match kept_out {
                            Some(name) => self.say(&ctx, trf("\"{}\" keeps other tasks out", &[&name]), false),
                            None => self.schedule_task(&ctx, tid, start),
                        }
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
        let muted = self.pal.muted;
        let mut m = Menu::start(ui);
        if self.data.locked(&ev).is_some() {
            // a subscribed event can only be copied
            let cal = self.data.calendar(ev.calendar).map(|c| c.name.clone()).unwrap_or_default();
            m.heading(ui, &trf("From {} (read-only)", &[&cal]));
            if m.item(ui, icons::COPY, tr("Copy event"), false).clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            m.end(ui);
            return;
        }
        // colour dots: "no colour" first, then the palette, in two rows of
        // seven, each only as wide as its dots, so the menu is as wide as
        // its longest line and no wider
        let mut pick: Option<Option<[u8; 3]>> = None;
        for row in 0..2 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.add_space(8.0);
                if row == 0 {
                    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                    let c = if resp.hovered() { self.pal.text } else { muted };
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(2.0, c));
                    ui.painter().line_segment([r.center() + vec2(-6.0, -6.0), r.center() + vec2(6.0, 6.0)], Stroke::new(2.0, c));
                    if resp.on_hover_text(tr("Calendar colour")).clicked() {
                        pick = Some(None);
                    }
                }
                let (from, to) = if row == 0 { (0, 6) } else { (6, EVENT_COLORS.len()) };
                for (i, (col, name)) in EVENT_COLORS.iter().enumerate().take(to).skip(from) {
                    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                    let grow = ctx.animate_bool_with_time(ids::colour_dot(i), resp.hovered(), motion::HOVER.secs());
                    ui.painter().circle_filled(r.center(), 9.0 + 2.0 * grow, rgb(*col));
                    if ev.color == Some(*col) {
                        ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(1.5, self.pal.text));
                    }
                    if resp.on_hover_text(*name).clicked() {
                        pick = Some(Some(*col));
                    }
                }
            });
        }
        if let Some(c) = pick {
            self.snapshot();
            if let Some(e) = self.data.event_mut(id) {
                e.color = c;
            }
            self.commit();
            ui.close();
        }
        m.line(ui);
        if m.item(ui, icons::PEN, tr("Edit"), false).clicked() {
            self.edit_event_at(id, None);
            ui.close();
        }
        if let Some(tid) = ev.task_id {
            if m.item(ui, icons::COPY, tr("Copy task session"), false).clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            m.line(ui);
            m.heading(ui, tr("Planner"));
            m.sub(ui, icons::REFRESH, tr("Replan session"), |ui| {
                let mut s = Menu::start(ui);
                // the keys sit at the far end, so the menu would take all
                // the width it is offered: hold it to its longest row
                let font = FontId::proportional(theme::size::UI);
                let longest = [tr("Reschedule this session"), tr("Complete & schedule more time"), tr("Split & reschedule remaining time")]
                    .iter()
                    .map(|l| ui.painter().layout_no_wrap(format!("{}   {l}", icons::CHECK), font.clone(), muted).size().x)
                    .fold(0.0, f32::max);
                let keys = ui.painter().layout_no_wrap("Shift+R".into(), FontId::monospace(theme::size::MICRO), muted).size().x;
                ui.set_width(longest + 2.0 * 10.0 + 16.0 + keys + 10.0 + 2.0 * 8.0);
                // each row keeps its keys at the far end
                let row = |ui: &mut egui::Ui, s: &Menu, icon: &str, label: &str, keys: &str, enabled: bool| -> bool {
                    ui.horizontal(|ui| {
                        let hit = s.item_if(ui, enabled, icon, label, false).clicked();
                        ui.add_space(16.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(10.0);
                            ui.label(RichText::new(keys).monospace().size(theme::size::MICRO).color(muted));
                        });
                        hit
                    })
                    .inner
                };
                if row(ui, &s, icons::CALENDAR_DAYS, tr("Reschedule this session"), tr("Shift+R"), true) {
                    self.reschedule_session(&ctx, id);
                    ui.close();
                }
                if row(ui, &s, icons::CHECK, tr("Complete & schedule more time"), tr("Shift+F"), true) {
                    self.follow_up(&ctx, id);
                    ui.close();
                }
                if row(ui, &s, icons::SHUFFLE, tr("Split & reschedule remaining time"), tr("Shift+S"), self.split_time(id).is_some()) {
                    self.split_session(&ctx, id);
                    ui.close();
                }
                s.line(ui);
                if row(ui, &s, icons::X, tr("Unschedule session"), tr("Del"), true) {
                    self.delete_event(&ctx, id);
                    ui.close();
                }
                s.end(ui);
            });
            let mut flexible = self.data.task(tid).map(|t| t.flexible).unwrap_or(false);
            // the tick sits right after the words: a right-aligned one would
            // stretch the menu to its widest
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(format!("{}   {}", icons::SHUFFLE, tr("Flexible task"))).size(theme::size::UI));
                ui.label(RichText::new("(i)").size(theme::size::MICRO).color(muted)).on_hover_text(
                    tr("The planner may move flexible tasks to make room for more important ones."),
                );
                ui.add_space(6.0);
                if ui.checkbox(&mut flexible, "").changed() {
                    self.snapshot();
                    if let Some(t) = self.data.task_mut(tid) {
                        t.flexible = flexible;
                    }
                    self.commit();
                }
            });
            m.line(ui);
            if m.item(ui, icons::X, tr("Unschedule session"), true).clicked() {
                self.delete_event(&ctx, id);
                ui.close();
            }
            if m.item(ui, icons::TRASH, tr("Unschedule and delete task"), true).clicked() {
                self.delete_task(&ctx, tid);
                ui.close();
            }
        } else {
            if m.item(ui, icons::COPY, tr("Copy event"), false).clicked() {
                self.selected = Some(id);
                self.run(&ctx, Cmd::Copy);
                ui.close();
            }
            m.line(ui);
            if m.item(ui, icons::TRASH, tr("Delete event"), true).clicked() {
                self.delete_event(&ctx, id);
                ui.close();
            }
        }
        m.end(ui);
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
                FontId::proportional(theme::size::SMALL),
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
                painter.text(num_rect.center(), Align2::CENTER_CENTER, num, FontId::proportional(theme::size::SMALL), Color32::WHITE);
            } else {
                let c = if other { self.pal.muted } else { self.pal.text };
                painter.text(num_rect.center(), Align2::CENTER_CENTER, num, FontId::proportional(theme::size::SMALL), c);
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
                let title = if e.title.is_empty() { tr("New Event") } else { &e.title };
                let tclip = painter.with_clip_rect(er.intersect(cell));
                if e.all_day {
                    clip.rect_filled(er, 4.0, mix(self.pal.bg, color, 0.55));
                    tclip.text(er.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(theme::size::TINY), self.pal.text);
                } else {
                    clip.circle_filled(er.left_center() + vec2(6.0, 0.0), 3.5, color);
                    let label = format!("{}  {}", p.fmt_time(e.start.time()), title);
                    let done = e.task_id.and_then(|t| self.data.task(t)).map(|t| t.done).unwrap_or(false);
                    let c = if done { self.pal.muted } else { self.pal.text };
                    tclip.text(er.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(theme::size::TINY), c);
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
                    FontId::proportional(theme::size::TINY),
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
        // a right click on a block opens the same menu as in the week
        if resp.secondary_clicked() {
            self.ctx_target = match resp.interact_pointer_pos().and_then(hit_at) {
                Some(Hit::Ev(id)) => Some(id),
                _ => None,
            };
            if self.ctx_target.is_some() {
                self.selected = self.ctx_target;
            }
        }
        if let Some(id) = self.ctx_target {
            context_menu(&resp, |ui| self.block_menu(ui, id));
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
                    self.say(&ctx, trf("Due date set to {}", &[&d.format("%b %-d")]), true);
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
                ui.label(RichText::new(trf("Week of {}", &[&days[0].format("%B %-d")])).color(muted));
                for d in &days {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(d.format("%A, %B %-d").to_string()).size(theme::size::UI).strong());
                        if *d == today {
                            egui::Frame::new().fill(accent).corner_radius(theme::RADIUS).inner_margin(Margin::symmetric(8, 1)).show(ui, |ui| {
                                ui.label(RichText::new(tr("Today")).size(theme::size::MICRO).color(Color32::WHITE));
                            });
                        }
                        if ui.small_button("+").on_hover_text(tr("New event")).clicked() {
                            self.open_new_event(midnight(*d) + Duration::hours(NEW_EVENT_NOON_H), NEW_EVENT_MIN, false);
                        }
                    });
                    let mut evs: Vec<Event> = self.data.all_day_on(*d).into_iter().cloned().collect();
                    evs.extend(self.data.timed_on(*d).into_iter().cloned());
                    if evs.is_empty() {
                        ui.label(RichText::new(tr("Nothing planned")).color(muted));
                    }
                    for e in evs {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.5, rgb(self.data.event_color(&e)));
                            let when = if e.all_day {
                                tr("All day").to_string()
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
                            context_menu(&r, |ui| self.block_menu(ui, e.id));
                        });
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, border));
                }
            });
        });
    }
}


/// A right-click menu on `resp`. Unlike egui's own, a second right-click on
/// the same widget while its menu is open moves the menu there instead of
/// just closing it: egui closes it this frame, and we open it again next.
pub fn context_menu(resp: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
    let ctx = &resp.ctx;
    let id = egui::Popup::default_response_id(resp);
    let again = id.with("again");
    let reopen = ctx.data_mut(|d| d.remove_temp::<bool>(again)).unwrap_or(false);
    if resp.secondary_clicked() && egui::Popup::is_id_open(ctx, id) {
        ctx.data_mut(|d| d.insert_temp(again, true));
        ctx.request_repaint();
    }
    let mut popup = egui::Popup::context_menu(resp);
    if reopen {
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    popup.show(add);
}

/// Every right-click menu is drawn the same way: this sets it up, adds its
/// rows and draws the lines between its parts.
pub struct Menu {
    /// heights of the lines between parts, drawn once the width is known:
    /// a separator would take all the room it is offered
    seps: Vec<f32>,
}

impl Menu {
    /// Sets the spacing and padding the menu's rows use.
    pub fn start(ui: &mut egui::Ui) -> Self {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        ui.spacing_mut().button_padding = vec2(10.0, 7.0);
        ui.add_space(2.0);
        Menu { seps: vec![] }
    }

    /// A row: an icon, then its words. A red one removes something.
    pub fn item(&self, ui: &mut egui::Ui, icon: &str, label: &str, red: bool) -> egui::Response {
        self.item_if(ui, true, icon, label, red)
    }

    /// A row that can be greyed out.
    pub fn item_if(&self, ui: &mut egui::Ui, enabled: bool, icon: &str, label: &str, red: bool) -> egui::Response {
        let w = words(icon, label, red);
        ui.add_enabled(enabled, egui::Button::new(w))
    }

    /// A row that opens a smaller menu beside it.
    pub fn sub(&self, ui: &mut egui::Ui, icon: &str, label: &str, add: impl FnOnce(&mut egui::Ui)) {
        ui.menu_button(words(icon, label, false), add);
    }

    /// A small grey heading over the next part.
    pub fn heading(&self, ui: &mut egui::Ui, text: &str) {
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let c = ui.visuals().weak_text_color();
            ui.label(RichText::new(text).size(theme::size::BODY).color(c));
        });
    }

    /// Leaves a gap where a line goes between two parts.
    pub fn line(&mut self, ui: &mut egui::Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(0.0, 7.0), Sense::hover());
        self.seps.push(r.center().y);
    }

    /// Draws the lines across the width the menu took.
    pub fn end(self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        let x = ui.min_rect().x_range();
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        for y in self.seps {
            ui.painter().hline(x, y, stroke);
        }
    }
}

fn words(icon: &str, label: &str, red: bool) -> RichText {
    let t = RichText::new(format!("{icon}   {label}")).size(theme::size::UI);
    if red {
        t.color(theme::DANGER)
    } else {
        t
    }
}
