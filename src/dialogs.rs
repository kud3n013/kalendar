//! Pop-up windows: editors, preferences, shortcuts, command bar, messages.

use crate::strings::{tr, trf};
use crate::ids;
use crate::motion;
use crate::theme;
use crate::icons;
use crate::model::*;
use crate::views::midnight;
use crate::{mix, rgb, App, Cmd, CreateKind, EventEdit, TaskEdit, COMMANDS};
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

/// Hours and minutes to drag, in 5-minute steps, kept between two bounds.
fn time_drag(ui: &mut egui::Ui, m: &mut i64, lo: i64, hi: i64, hour12: bool, muted: egui::Color32) {
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
}

/// How a date picker looks: the week's first day and a few colours.
#[derive(Clone, Copy)]
pub(crate) struct PickLook {
    pub first_day: u8,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub side: egui::Color32,
    pub accent: egui::Color32,
}

impl App {
    pub(crate) fn pick_look(&self) -> PickLook {
        PickLook { first_day: self.data.prefs.first_day, text: self.pal.text, muted: self.pal.muted, side: self.pal.side, accent: self.pal.accent }
    }
}

/// A calendar button that drops down a month to click a day in, the way
/// calendar apps pick dates. `cur` is the day shown picked (and the month
/// it opens on, else today's). Gives back the day clicked.
pub(crate) fn date_pick(ui: &mut egui::Ui, salt: &str, cur: Option<NaiveDate>, look: PickLook) -> Option<NaiveDate> {
    use chrono::Datelike;
    let b = ui.add(egui::Button::new(RichText::new(icons::CALENDAR).size(theme::size::BODY)).min_size(egui::vec2(24.0, 0.0))).on_hover_text(tr("Pick a date"));
    let id = ids::date_pick(salt);
    let today = Local::now().date_naive();
    let open_on = cur.unwrap_or(today).with_day(1).unwrap_or(today);
    // the month shown starts at the picked day's each time it opens
    if b.clicked() {
        ui.data_mut(|d| d.insert_temp(id, open_on));
    }
    let mut out = None;
    egui::Popup::from_toggle_button_response(&b).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let first = ui.data(|d| d.get_temp::<NaiveDate>(id)).unwrap_or(open_on);
        let back = (first.weekday().num_days_from_sunday() as i64 - look.first_day as i64).rem_euclid(7);
        let start = first - Duration::days(back);
        let mut step = 0;
        ui.set_width(7.0 * 30.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(first.format("%B %Y").to_string()).font(theme::bold(theme::size::BODY)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new(icons::CHEVRON_RIGHT).size(theme::size::BODY)).frame(false)).on_hover_text(tr("Next month")).clicked() {
                    step = 1;
                }
                if ui.add(egui::Button::new(RichText::new(icons::CHEVRON_LEFT).size(theme::size::BODY)).frame(false)).on_hover_text(tr("Previous month")).clicked() {
                    step = -1;
                }
            });
        });
        egui::Grid::new(id.with("grid")).spacing([0.0, 0.0]).min_col_width(30.0).show(ui, |ui| {
            for i in 0..7 {
                let d = start + Duration::days(i);
                ui.vertical_centered(|ui| ui.label(RichText::new(&d.format("%a").to_string()[..2]).size(theme::size::MICRO).color(look.muted)));
            }
            ui.end_row();
            for i in 0..42 {
                let d = start + Duration::days(i);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(30.0, 26.0), egui::Sense::click());
                let picked = Some(d) == cur;
                if picked {
                    ui.painter().circle_filled(r.center(), 11.0, look.accent);
                } else if resp.hovered() {
                    ui.painter().circle_filled(r.center(), 11.0, mix(look.side, look.text, 0.14));
                }
                if d == today && !picked {
                    ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(1.0, theme::DANGER));
                }
                let c = if picked { egui::Color32::WHITE } else if d.month() != first.month() { look.muted } else { look.text };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(theme::size::SMALL), c);
                if resp.clicked() {
                    out = Some(d);
                }
                if i % 7 == 6 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button(tr("Today")).clicked() {
                out = Some(today);
            }
        });
        if step != 0 {
            let m = chrono::Months::new(1);
            let to = if step > 0 { first.checked_add_months(m) } else { first.checked_sub_months(m) }.unwrap_or(first);
            ui.data_mut(|d| d.insert_temp(id, to));
        }
        if out.is_some() {
            ui.close();
        }
    });
    out
}

/// Minutes after midnight as the clock shows them, e.g. 13:30 or 1:30 PM.
fn clock(m: i64, hour12: bool) -> String {
    let (h, mm) = (m.rem_euclid(24 * 60) / 60, m % 60);
    if hour12 {
        format!("{}:{:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, mm, if h < 12 { "AM" } else { "PM" })
    } else {
        format!("{h:02}:{mm:02}")
    }
}

/// A clock button that drops down a list of times every quarter hour,
/// from `lo` to `hi` minutes after midnight (`hi` past a day wraps), the
/// way calendar apps pick times. With `from` (an end time) each row also
/// says how long that makes it. Gives back the minutes picked.
pub(crate) fn time_pick(ui: &mut egui::Ui, salt: &str, cur: i64, lo: i64, hi: i64, from: Option<i64>, hour12: bool) -> Option<i64> {
    let b = ui.add(egui::Button::new(RichText::new(icons::CLOCK).size(theme::size::BODY)).min_size(egui::vec2(24.0, 0.0))).on_hover_text(tr("Pick a time"));
    let id = ids::time_pick(salt);
    if b.clicked() {
        ui.data_mut(|d| d.insert_temp(id, true));
    }
    // an end past midnight sits a day on in the list
    let cur = if cur < lo { cur + 24 * 60 } else { cur };
    let mut out = None;
    egui::Popup::from_toggle_button_response(&b).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let jump = ui.data_mut(|d| d.remove_temp::<bool>(id)).unwrap_or(false);
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            ui.set_min_width(if from.is_some() { 150.0 } else { 100.0 });
            // a start on the quarter hour, an end a quarter after its start
            let first = if from.is_some() { lo } else { (lo + 14) / 15 * 15 };
            let mut m = first;
            while m <= hi {
                let mut text = clock(m, hour12);
                if let Some(f) = from {
                    let len = m - f;
                    let dur = if len < 60 {
                        trf("{} min", &[&len])
                    } else if len % 60 == 0 {
                        trf("{} h", &[&(len / 60)])
                    } else {
                        trf("{} h {} min", &[&(len / 60), &(len % 60)])
                    };
                    text = format!("{text}  ({dur})");
                }
                let on = m == cur;
                let r = ui.selectable_label(on, text);
                // opens on the time picked, or the nearest quarter to it
                if jump && (on || (m <= cur && cur < m + 15)) {
                    r.scroll_to_me(Some(Align::Center));
                }
                if r.clicked() {
                    out = Some(m);
                }
                m += 15;
            }
        });
        if out.is_some() {
            ui.close();
        }
    });
    out
}

/// A YYYY-MM-DD box with the date picker beside it.
pub(crate) fn date_field(ui: &mut egui::Ui, salt: &str, text: &mut String, width: f32, look: PickLook) {
    ui.add(egui::TextEdit::singleline(text).hint_text("YYYY-MM-DD").desired_width(width));
    let cur = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok();
    if let Some(d) = date_pick(ui, salt, cur, look) {
        *text = d.to_string();
    }
}

/// A "+" beside a dropdown: a small field pops up under it, and Enter
/// gives back the name typed there.
fn add_button(ui: &mut egui::Ui, salt: &str, hint: &str) -> Option<String> {
    let b = ui.add(egui::Button::new("+").min_size(egui::vec2(24.0, 0.0))).on_hover_text(hint);
    let id = ids::add_text(salt);
    let mut out = None;
    egui::Popup::from_toggle_button_response(&b).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(160.0));
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let name = text.trim().trim_start_matches('#').to_string();
            if !name.is_empty() {
                out = Some(name);
            }
            text.clear();
            ui.close();
        } else {
            r.request_focus();
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    });
    out
}

/// A dropdown where several things can be ticked; it stays open while
/// ticking. Gives back the place of the one that was clicked.
/// What `pick_many` gives back when its first row clears the picks.
const CLEAR: usize = usize::MAX;

fn pick_many(ui: &mut egui::Ui, salt: &str, none: &str, items: &[(String, egui::Color32, bool)]) -> Option<usize> {
    let on: Vec<&str> = items.iter().filter(|i| i.2).map(|i| i.0.as_str()).collect();
    let summary = match on.len() {
        0 => none.to_string(),
        1 | 2 => on.join(", "),
        n => format!("{}, {} +{}", on[0], on[1], n - 2),
    };
    let mut hit = None;
    egui::ComboBox::from_id_salt(salt).selected_text(summary).height(320.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
        // the first row clears every pick
        if ui.selectable_label(on.is_empty(), none).clicked() && !on.is_empty() {
            hit = Some(CLEAR);
        }
        if items.is_empty() {
            ui.label(RichText::new(tr("Nothing yet; add one with +")).weak());
        }
        for (i, (name, col, ticked)) in items.iter().enumerate() {
            if pick_row(ui, *ticked, name, *col).clicked() {
                hit = Some(i);
            }
        }
    });
    hit
}

/// One row of a dropdown, the same in every dropdown: the text in its
/// colour, lit while it is picked.
pub(crate) fn pick_row(ui: &mut egui::Ui, on: bool, text: &str, col: egui::Color32) -> egui::Response {
    ui.selectable_label(on, RichText::new(text).color(col))
}

/// The event and task editor windows share one size and one layout, so an
/// event, a task's session and a task look alike wherever they are opened.
const EDITOR_SIZE: [f32; 2] = [420.0, 540.0];
/// Width of the label column in an editor's field grid.
const EDITOR_LABEL_W: f32 = 96.0;

/// Puts the typing cursor in the field `r`, after its last letter, when
/// `pending` is set. It stays set until the field has the cursor, since a
/// window that has just opened may take a pass or two to get the keys.
fn grab_focus(ui: &egui::Ui, r: &egui::Response, len: usize, pending: &mut bool) {
    if !*pending {
        return;
    }
    if r.has_focus() {
        *pending = false;
        return;
    }
    r.request_focus();
    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), r.id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(len))));
    state.store(ui.ctx(), r.id);
    ui.ctx().request_repaint();
}

/// An editor's name field at the top; true when Enter was pressed in it.
fn editor_title(ui: &mut egui::Ui, text: &mut String, hint: &str, focus: &mut bool) -> bool {
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .font(FontId::proportional(theme::size::HEADING))
            .desired_width(f32::INFINITY),
    );
    grab_focus(ui, &r, text.chars().count(), focus);
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Which field an editor opens on: the name when it has none yet, else the
/// notes, so what is typed first goes where it is wanted. Gives (name, notes).
fn first_field(focus: bool, title: &str) -> (bool, bool) {
    let blank = title.trim().is_empty();
    (focus && blank, focus && !blank)
}

/// An editor's labelled fields: muted names in a column of their own.
fn editor_grid(ui: &mut egui::Ui, salt: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(salt).num_columns(2).spacing([12.0, 8.0]).show(ui, add);
}

fn editor_label(ui: &mut egui::Ui, text: &str, muted: egui::Color32) {
    ui.add_sized([EDITOR_LABEL_W, 20.0], egui::Label::new(RichText::new(text).color(muted)).halign(Align::LEFT));
}

/// The editor's notes, at the end of its fields; Ctrl+Enter saves.
fn editor_notes(ui: &mut egui::Ui, notes: &mut String, hint: &str, focus: &mut bool) -> bool {
    ui.add_space(4.0);
    let r = ui.add(egui::TextEdit::multiline(notes).hint_text(hint).desired_rows(3).desired_width(f32::INFINITY));
    grab_focus(ui, &r, notes.chars().count(), focus);
    ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
}

/// What was pressed at the foot of an editor window.
#[derive(Clone, Copy, Default)]
struct Pressed {
    save: bool,
    cancel: bool,
    delete: bool,
    skip: bool,
    /// while unsaved changes were asked about: drop them, or go on editing
    discard: bool,
    keep: bool,
}

/// Where an editor window keeps whether it is asking about unsaved changes.
fn asking_id(name: &str) -> egui::Id {
    egui::ViewportId::from_hash_of(name).0.with("ask")
}

/// Has the editor window `name` ask whether to drop its unsaved changes,
/// instead of closing.
pub(crate) fn ask_discard(ctx: &egui::Context, name: &str) {
    ctx.data_mut(|d| d.insert_temp(asking_id(name), true));
}

/// What an editor window offers besides Save and Cancel.
#[derive(Clone, Copy)]
struct Offer {
    delete: bool,
    /// Some(is it skipped now) shows Skip or Unskip
    skip: Option<bool>,
}

/// Save and Cancel, with Skip and Delete on the right, kept at the foot of
/// the editor window whatever its size.
fn editor_buttons(ui: &mut egui::Ui, offer: Offer, asking: bool) -> Pressed {
    let mut out = Pressed::default();
    if asking {
        // closing would lose what was typed: say so where the buttons were
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Discard unsaved changes?")).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                out.keep = ui.button(tr("Keep editing")).on_hover_text(tr("Esc")).clicked();
                out.discard = ui.button(tr("Discard")).clicked();
                out.save = ui.button(RichText::new(tr("Save")).strong()).on_hover_text(tr("Ctrl+Enter")).clicked();
            });
        });
        return out;
    }
    ui.horizontal(|ui| {
        out.save = ui.button(RichText::new(tr("Save")).strong()).on_hover_text(tr("Ctrl+Enter")).clicked();
        out.cancel = ui.button(tr("Cancel")).on_hover_text(tr("Esc")).clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if offer.delete {
                out.delete = ui.button(tr("Delete")).clicked();
            }
            if let Some(skipped) = offer.skip {
                let (text, tip) = if skipped {
                    (tr("Unskip"), tr("Show it as a usual event again"))
                } else {
                    (tr("Skip"), tr("Keep it on the calendar, greyed, and let other blocks go over it"))
                };
                out.skip = ui.button(text).on_hover_text(tip).clicked();
            }
        });
    });
    out
}

/// A dropdown where one thing is picked, drawn as `pick_many` draws its
/// rows. `items` are (value, text, colour).
fn pick_one(ui: &mut egui::Ui, salt: &str, items: &[(u64, String, egui::Color32)], current: &mut u64) {
    let shown = items.iter().find(|i| i.0 == *current).map(|i| i.1.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(salt).selected_text(shown).height(320.0).show_ui(ui, |ui| {
        for (v, text, col) in items {
            if pick_row(ui, *v == *current, text, *col).clicked() {
                *current = *v;
            }
        }
    });
}

/// A row of colour swatches: the calendar's own first, then the event
/// colours. `color` is None for the calendar's.
fn color_row(ui: &mut egui::Ui, cal_color: [u8; 3], color: &mut Option<[u8; 3]>) {
    swatches(ui, cal_color, tr("Calendar colour"), color);
}

/// The repeat dropdown: the fixed patterns and Custom, which starts on
/// the weekday of `day`; returns whether the pick changed.
fn repeat_combo(ui: &mut egui::Ui, id: &str, r: &mut Repeat, day: NaiveDate) -> bool {
    let was = *r;
    let shown = if matches!(r, Repeat::Custom(_)) { tr("Custom").to_string() } else { r.label() };
    egui::ComboBox::from_id_salt(id).selected_text(shown).show_ui(ui, |ui| {
        for v in Repeat::ALL {
            ui.selectable_value(r, v, v.label());
        }
        let custom = matches!(r, Repeat::Custom(_));
        if ui.selectable_label(custom, tr("Custom")).clicked() && !custom {
            *r = Repeat::custom_from(day);
        }
    });
    *r != was
}

/// Mo to Su as toggles over a weekday mask: bit 0 Monday to bit 6 Sunday.
pub(crate) fn weekday_chips(ui: &mut egui::Ui, days: &mut u8) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, d) in [tr("Mo"), tr("Tu"), tr("We"), tr("Th"), tr("Fr"), tr("Sa"), tr("Su")].iter().enumerate() {
            let bit = 1u8 << i;
            if ui.selectable_label(*days & bit != 0, *d).clicked() {
                *days ^= bit;
            }
        }
    });
}

/// The weekday row under a Custom repeat, in a two-column grid.
/// `editor` lays the label out as an editor window's labels, else as the
/// flyout's.
fn custom_days_row(ui: &mut egui::Ui, r: &mut Repeat, muted: egui::Color32, editor: bool) {
    if let Repeat::Custom(days) = r {
        if editor {
            editor_label(ui, tr("On"), muted);
        } else {
            ui.label(RichText::new(tr("On")).color(muted));
        }
        weekday_chips(ui, days);
        ui.end_row();
    }
}

/// A row of colour dots: the first, `base`, for None, then the palette.
pub(crate) fn swatches(ui: &mut egui::Ui, base: [u8; 3], base_tip: &str, color: &mut Option<[u8; 3]>) {
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
        if swatch(ui, base, color.is_none(), base_tip) {
            *color = None;
        }
        ui.add_space(6.0);
        for (col, cname) in EVENT_COLORS.iter() {
            if swatch(ui, *col, *color == Some(*col), tr(cname)) {
                *color = Some(*col);
            }
        }
    });
}

/// An event's notes as the place on their first line ("Where: …") and the
/// rest, both as typed: nothing is trimmed, so a space being typed stays.
fn split_place(notes: &str) -> (String, String) {
    match notes.strip_prefix("Where: ") {
        Some(rest) => match rest.split_once('\n') {
            Some((place, more)) => (place.to_string(), more.to_string()),
            None => (rest.to_string(), String::new()),
        },
        None => (String::new(), notes.to_string()),
    }
}

/// The other way from [`split_place`]: no place leaves the notes as they are.
fn join_place(place: &str, notes: &str) -> String {
    match (place.is_empty(), notes.is_empty()) {
        (true, _) => notes.to_string(),
        (false, true) => format!("Where: {place}"),
        (false, false) => format!("Where: {place}\n{notes}"),
    }
}

/// Starts the part of a window that changes with what is being made. It
/// comes back as a `Ui` of its own: the fields drawn in it fade in after
/// `kind` changes, and the room they take glides to their height.
fn morph_begin(ui: &mut egui::Ui, kind: u8) -> egui::Ui {
    let ctx = ui.ctx().clone();
    let id = ui.scope_id().with("morph");
    let frame = ctx.cumulative_frame_nr();
    let now = ctx.input(|i| i.time);
    // (height of the fields, the frame they were drawn in, the kind, when it changed)
    let last = ctx.data(|d| d.get_temp::<(f32, u64, u8, f64)>(id)).filter(|l| l.1 + 1 >= frame);
    let since = match last {
        Some(l) if l.2 == kind => l.3,
        Some(_) => now,
        None => now - 10.0,
    };
    let secs = motion::LONG.secs();
    let h = last.map(|l| ctx.animate_value_with_time(id.with("h"), l.0, secs));
    ctx.data_mut(|d| d.insert_temp(id.with("now"), (frame, kind, since)));
    let top = ui.cursor().min;
    let w = ui.available_width();
    let rect = match h {
        Some(h) => ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover()).0,
        None => egui::Rect::from_min_size(top, egui::vec2(w, 0.0)),
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(w, f32::INFINITY))));
    if h.is_some() {
        child.set_clip_rect(rect.expand2(egui::vec2(4.0, 0.0)).intersect(ui.clip_rect()));
    }
    let fade = if secs > 0.0 { (((now - since) as f32) / secs).clamp(0.0, 1.0) } else { 1.0 };
    if fade < 1.0 {
        child.set_opacity(fade);
        ctx.request_repaint();
    }
    if h.is_none() {
        // nothing was set aside yet: the fields take their room as they come
        ctx.data_mut(|d| d.insert_temp(id.with("grow"), true));
    }
    child
}

/// Ends what `morph_begin` began, with the `Ui` it gave.
fn morph_end(outer: &mut egui::Ui, child: &egui::Ui) {
    let ctx = outer.ctx().clone();
    let id = outer.scope_id().with("morph");
    let real = child.min_rect().height();
    if ctx.data_mut(|d| d.remove_temp::<bool>(id.with("grow"))).is_some() {
        outer.allocate_space(egui::vec2(child.min_rect().width(), real));
        // a window that just opened is its own size at once
        ctx.animate_value_with_time(id.with("h"), real, 0.0);
    }
    if let Some((frame, kind, since)) = ctx.data(|d| d.get_temp::<(u64, u8, f64)>(id.with("now"))) {
        ctx.data_mut(|d| d.insert_temp(id, (real, frame, kind, since)));
    }
}

impl App {
    /// The task lists as dropdown rows: (id, icon and name, colour), the
    /// Inbox first. `writable` leaves out lists that cannot take a task.
    pub(crate) fn list_items(&self, writable: bool) -> Vec<(u64, String, egui::Color32)> {
        let ids = std::iter::once(0).chain(self.data.lists.iter().filter(|l| !writable || !l.read_only).map(|l| l.id));
        ids.map(|id| {
            let (_, col, icon) = self.data.list_look(id);
            let name = if id == 0 { self.data.prefs.inbox_name() } else { self.data.list_look(id).0 };
            let icon = if icon.is_empty() { icons::CIRCLE.to_string() } else { icon };
            (id, format!("{icon} {name}"), rgb(col))
        })
        .collect()
    }

    pub fn ui_dialogs(&mut self, ctx: &egui::Context) {
        let had_frame = self.frame_editor.is_some();
        self.dlg_create(ctx);
        self.dlg_event(ctx);
        self.dlg_task(ctx);
        // a block window that has just turned to Frame was drawn in this pass
        // already, under the same id; the frame fields come in the next pass
        if had_frame || self.frame_editor.as_ref().is_none_or(|f| f.from_block.is_none()) {
            self.dlg_frame(ctx);
        }
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
        let mut new_list: Option<String> = None;
        let mut all_tags: Vec<String> = self.data.tasks.iter().flat_map(|t| t.tags.iter().cloned()).chain(c.task.tags.iter().cloned()).collect();
        all_tags.sort_by_key(|t| t.to_lowercase());
        all_tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let muted = self.pal.muted;
        let p = self.data.prefs.clone();
        let look = self.pick_look();
        let w = 380.0;
        let pos = place_beside(ctx, ids::create_popup(), c.anchor_rect, egui::vec2(w + 20.0, 400.0));
        let cal_color = self.data.calendar_color(c.ev.calendar);
        let today = Local::now().date_naive();
        egui::Window::new("create")
            .id(ids::create_popup())
            .title_bar(false)
            .resizable(false)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_width(w);
                // what the block becomes: a task session, a plain event or a frame
                ui.horizontal(|ui| {
                    for (kind, label) in [(CreateKind::Task, tr("Task")), (CreateKind::Event, tr("Event")), (CreateKind::Frame, tr("Frame"))] {
                        let r = ui.selectable_label(c.kind == kind, RichText::new(label).size(theme::size::CAPTION));
                        let r = if kind == CreateKind::Frame { r.on_hover_text(tr("Reserve this time every week for a kind of work")) } else { r };
                        if r.clicked() {
                            c.kind = kind;
                            to_frame = kind == CreateKind::Frame;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(c.ev.start.format("%a, %b %-d").to_string()).size(theme::size::SMALL).color(muted));
                    });
                });
                let is_task = c.kind != CreateKind::Event;
                let hint = if is_task { tr("New Task") } else { tr("New Event") };
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut c.ev.title)
                        .hint_text(hint)
                        .font(FontId::proportional(theme::size::HEADING))
                        .desired_width(f32::INFINITY),
                );
                if c.focus {
                    resp.request_focus();
                    c.focus = false;
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                // the fields below change with the kind: they fade in and the
                // window grows or shrinks to them
                let outer_ui = ui;
                let mut morph = morph_begin(outer_ui, c.kind as u8);
                let ui = &mut morph;
                ui.add_space(4.0);
                let label = |ui: &mut egui::Ui, t: &str| {
                    ui.label(RichText::new(t).color(muted));
                };
                egui::Grid::new("creategrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    // every calendar that takes changes, one's own and synced ones
                    label(ui, tr("Calendar"));
                    let cname = |id: u64| self.data.calendar(id).map(|c| c.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("ccal").selected_text(cname(c.ev.calendar)).height(320.0).show_ui(ui, |ui| {
                        for cal in self.data.calendars.iter().filter(|cal| cal.writable()) {
                            ui.selectable_value(&mut c.ev.calendar, cal.id, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color)));
                        }
                    });
                    ui.end_row();

                    label(ui, tr("Colour"));
                    color_row(ui, cal_color, &mut c.ev.color);
                    ui.end_row();

                    // hours, in 5-minute steps, within the block's day
                    label(ui, tr("Time"));
                    ui.horizontal(|ui| {
                        let day = midnight(c.ev.start.date());
                        let mut s = (c.ev.start - day).num_minutes();
                        let mut e = (c.ev.end - day).num_minutes().min(24 * 60);
                        let len = e - s;
                        time_drag(ui, &mut s, 0, 24 * 60 - 5, p.hour12, muted);
                        if let Some(v) = time_pick(ui, "cstart", s, 0, 24 * 60 - 15, None, p.hour12) {
                            // a new start keeps the block's length
                            (s, e) = (v, (v + len).min(24 * 60));
                        }
                        ui.label(RichText::new("to").color(muted));
                        time_drag(ui, &mut e, s + 5, 24 * 60, p.hour12, muted);
                        if let Some(v) = time_pick(ui, "cend", e, s + 15, 24 * 60, Some(s), p.hour12) {
                            e = v;
                        }
                        c.ev.start = day + Duration::minutes(s);
                        c.ev.end = day + Duration::minutes(e.max(s + 5));
                    });
                    ui.end_row();

                    label(ui, tr("Repeat"));
                    ui.horizontal(|ui| {
                        if repeat_combo(ui, "crep", &mut c.repeat, c.ev.start.date()) && c.repeat != Repeat::None {
                            c.until_text = c.repeat.first_until(c.ev.start.date()).to_string();
                        }
                        if c.repeat != Repeat::None {
                            if is_task {
                                ui.label(RichText::new("when completed").color(muted)).on_hover_text(tr("Ticking the task off leaves a fresh one, due the next time"));
                            } else {
                                ui.label(RichText::new("until").color(muted));
                                date_field(ui, "cuntil", &mut c.until_text, 100.0, look);
                            }
                        }
                    });
                    ui.end_row();
                    custom_days_row(ui, &mut c.repeat, muted, false);

                    if is_task {
                        label(ui, tr("List"));
                        let items = self.list_items(true);
                        ui.horizontal(|ui| {
                            pick_one(ui, "clist", &items, &mut c.task.list);
                            if let Some(name) = add_button(ui, "clist", tr("New list")) {
                                new_list = Some(name);
                            }
                        });
                        ui.end_row();

                        label(ui, tr("Tags"));
                        ui.horizontal(|ui| {
                            let text = ui.visuals().text_color();
                            let items: Vec<(String, egui::Color32, bool)> = all_tags.iter().map(|t| (format!("#{t}"), text, c.task.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))).collect();
                            if let Some(i) = pick_many(ui, "ctags", tr("No tags"), &items) {
                                if i == CLEAR {
                                    c.task.tags.clear();
                                } else {
                                    let tag = &all_tags[i];
                                    if items[i].2 {
                                        c.task.tags.retain(|t| !t.eq_ignore_ascii_case(tag));
                                    } else {
                                        c.task.tags.push(tag.clone());
                                    }
                                }
                            }
                            if let Some(tag) = add_button(ui, "ctag", tr("New tag")) {
                                if !c.task.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                                    c.task.tags.push(tag);
                                }
                            }
                        });
                        ui.end_row();

                        label(ui, tr("Due"));
                        ui.horizontal(|ui| {
                            date_field(ui, "cdue", &mut c.due_text, 100.0, look);
                            if ui.small_button(tr("Today")).clicked() {
                                c.due_text = today.to_string();
                            }
                            if ui.small_button(tr("That day")).on_hover_text(tr("The day of this block")).clicked() {
                                c.due_text = c.ev.start.date().to_string();
                            }
                        });
                        ui.end_row();

                        label(ui, tr("Priority"));
                        egui::ComboBox::from_id_salt("cprio").selected_text(priority_name(c.task.priority)).show_ui(ui, |ui| {
                            for v in [0u8, 1, 5, 9] {
                                ui.selectable_value(&mut c.task.priority, v, priority_name(v));
                            }
                        });
                        ui.end_row();
                    }

                    label(ui, tr("Notes"));
                    ui.add(egui::TextEdit::multiline(&mut c.ev.notes).hint_text(tr("Add a note")).desired_rows(3).desired_width(f32::INFINITY));
                    ui.end_row();
                });
                morph_end(outer_ui, &morph);
                let ui = outer_ui;
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(tr("Save")).strong()).on_hover_text(tr("Enter")).clicked() {
                        save = true;
                    }
                    if ui.button(tr("Cancel")).on_hover_text(tr("Esc")).clicked() {
                        cancel = true;
                    }
                });
                if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
            });
        if let Some(name) = new_list {
            c.task.list = self.new_list(&name);
            self.commit();
        }
        // a due date that cannot be read keeps the window open
        let due = match c.due_text.trim() {
            "" => None,
            t => match NaiveDate::parse_from_str(t, "%Y-%m-%d") {
                Ok(d) => Some(d),
                Err(_) => {
                    if save && c.kind == CreateKind::Task {
                        self.warn(ctx, tr("The due date should look like 2026-11-30"));
                        save = false;
                    }
                    None
                }
            },
        };
        // an event repeats up to a day, which has to be readable
        let mut until: Option<NaiveDate> = None;
        if save && c.kind == CreateKind::Event && c.repeat != Repeat::None {
            match NaiveDate::parse_from_str(c.until_text.trim(), "%Y-%m-%d") {
                Ok(d) => until = Some(d),
                Err(_) => {
                    self.warn(ctx, tr("The last day should look like 2026-11-30"));
                    save = false;
                }
            }
        }
        if to_frame {
            let day = c.ev.start.date();
            let s = (c.ev.start - midnight(day)).num_minutes();
            let e = if c.ev.end.date() == day { (c.ev.end - midnight(day)).num_minutes() } else { 24 * 60 };
            self.open_new_frame(day, s, e, c.ev.calendar, c.ev.title.clone());
            // the frame's fields take over this window, beside the same block
            if let Some(fe) = &mut self.frame_editor {
                fe.from_block = Some((c.ev.clone(), c.anchor_rect));
            }
            // run the pass again at once, so no frame shows without the window
            ctx.request_discard("the block window turned to Frame");
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
                // and a task always has a name to show in the lists
                if ev.title.trim().is_empty() {
                    ev.title = tr("New task").into();
                }
                let task_id = self.data.new_id();
                self.data.tasks.insert(
                    0,
                    Task {
                        id: task_id,
                        title: ev.title.clone(),
                        notes: ev.notes.clone(),
                        duration_min: Some(ev.minutes()),
                        // a repeat counts from the due date: without one, the block's day
                        due: due.or((c.repeat != Repeat::None).then_some(day)),
                        repeat: c.repeat,
                        ..c.task.clone()
                    },
                );
                ev.task_id = Some(task_id);
                self.data.events.push(ev);
                self.after_task_scheduled(ctx, task_id, day);
            } else {
                // a repeating event is laid out as one event per time, so each
                // can be moved or removed alone and all of them sync like any other
                let times = until.map(|u| c.repeat.days_after(day, u)).unwrap_or_default();
                for d in &times {
                    let mut copy = ev.clone();
                    copy.id = self.data.new_id();
                    copy.start = ev.start + (*d - day);
                    copy.end = ev.end + (*d - day);
                    self.data.events.push(copy);
                }
                self.data.events.push(ev);
                if !times.is_empty() {
                    self.say(ctx, trf("Added {} events", &[&(times.len() + 1)]), true);
                }
            }
            self.selected = Some(id);
            self.commit();
        } else if !cancel {
            self.create = Some(c);
        }
    }

    /// The frame editor, in a window of its own as the event and task
    /// editors are: name, calendar, colour, hours, weekdays, repeat, end
    /// date, which tasks it takes, and whether it keeps others out.
    fn dlg_frame(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.frame_editor.take() else { return };
        let title = if ed.is_new { tr("New frame") } else { tr("Frame") };
        let key = ids::frame_editor().with(ed.frame.id);
        let at = self.editor_spot(ctx, key, ed.from_block.as_ref().map(|b| b.1));
        let mut new_list: Option<String> = None;
        let focus = ed.focus;
        let is_new = ed.is_new;
        let mut back: Option<CreateKind> = None;
        // a frame being made stays in the flyout beside its block (or the
        // centre); only one already made gets a window of its own
        let p = if ed.is_new && ed.from_block.is_some() {
            self.frame_flyout(ctx, &mut ed, &mut new_list, &mut back)
        } else {
            {
            let ed = std::cell::RefCell::new(&mut ed);
            self.edit_window(
                ctx,
                "frame_editor",
                title,
                at,
                focus,
                Offer { delete: !is_new, skip: None },
                |app| ed.borrow().changed(app.data.frame(ed.borrow().frame.id)),
                |app, ui| app.frame_form(ui, &mut ed.borrow_mut(), &mut new_list),
            )
        }
        };
        let Pressed { save, cancel, delete, .. } = p;
        if let Some(name) = new_list {
            // a list added from here is taken by the frame at once
            let id = self.new_list(&name);
            ed.frame.filter.lists.push(id);
            self.commit();
        }
        if let (Some(kind), Some((mut ev, rect))) = (back, ed.from_block.clone()) {
            // back to a task or an event, keeping the name and the calendar
            ev.title = ed.frame.name.clone();
            ev.calendar = ed.frame.calendar;
            self.create = Some(crate::CreateEdit::new(ev, kind, rect));
            // the block window ran earlier in this pass; run it again at once
            ctx.request_discard("the frame window turned back to a block");
            return;
        }
        if save {
            let text = ed.until_text.trim();
            let until = if text.is_empty() {
                None
            } else if let Ok(d) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                Some(d)
            } else {
                self.warn(ctx, tr("The end date should look like 2026-12-31"));
                return self.frame_editor = Some(ed);
            };
            if ed.frame.days == 0 {
                self.warn(ctx, tr("Pick at least one day"));
                return self.frame_editor = Some(ed);
            }
            let mut f = ed.frame;
            f.until = until;
            if f.name.trim().is_empty() {
                f.name = tr("Frame").into();
            }
            // never a subscribed calendar
            if self.data.calendar(f.calendar).is_none_or(|c| !c.is_local()) {
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
            return self.frame_editor = Some(ed);
        }
        ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(key));
    }

    /// A new frame's in-app flyout: in the very window a drawn block was set
    /// up in, with the Task and Event tabs to go back, else in the centre.
    fn frame_flyout(&mut self, ctx: &egui::Context, ed: &mut crate::FrameEdit, new_list: &mut Option<String>, back: &mut Option<CreateKind>) -> Pressed {
        let mut p = Pressed::default();
        let muted = self.pal.muted;
        let win_id = if ed.from_block.is_some() { ids::create_popup() } else { ids::frame_editor() };
        let win = egui::Window::new(tr("New frame")).id(win_id).collapsible(false).resizable(false);
        let win = match &ed.from_block {
            Some((_, r)) => win.title_bar(false).fixed_pos(place_beside(ctx, win_id, *r, egui::vec2(400.0, 400.0))),
            None => win.anchor(Align2::CENTER_CENTER, [0.0, -30.0]),
        };
        let at = |m: i64| chrono::NaiveTime::from_num_seconds_from_midnight_opt((m.clamp(0, 24 * 60 - 1) * 60) as u32, 0).unwrap_or_default();
        let hours = format!("{} – {}", self.data.prefs.fmt_time(at(ed.frame.start_min)), self.data.prefs.fmt_time(at(ed.frame.end_min)));
        win.show(ctx, |ui| {
            ui.set_width(380.0);
            if ed.from_block.is_some() {
                ui.horizontal(|ui| {
                    for (kind, label) in [(CreateKind::Task, tr("Task")), (CreateKind::Event, tr("Event")), (CreateKind::Frame, tr("Frame"))] {
                        if ui.selectable_label(kind == CreateKind::Frame, RichText::new(label).size(theme::size::CAPTION)).clicked() && kind != CreateKind::Frame {
                            *back = Some(kind);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&hours).size(theme::size::SMALL).color(muted));
                    });
                });
            }
            // the fields fade in and the window grows or shrinks to them
            let mut morph = morph_begin(ui, 2);
            p.save = self.frame_form(&mut morph, ed, new_list);
            morph_end(ui, &morph);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new(tr("Save")).strong()).on_hover_text(tr("Ctrl+Enter")).clicked() {
                    p.save = true;
                }
                if ui.button(tr("Cancel")).on_hover_text(tr("Esc")).clicked() {
                    p.cancel = true;
                }
            });
        });
        p
    }

    /// The frame editor's fields; returns whether to save.
    fn frame_form(&mut self, ui: &mut egui::Ui, ed: &mut crate::FrameEdit, new_list: &mut Option<String>) -> bool {
        let muted = self.pal.muted;
        let look = self.pick_look();
        let cal_color = self.data.calendar_color(ed.frame.calendar);
        // every tag in use, plus the ones the frame already asks for
        let mut all_tags: Vec<String> = self.data.tasks.iter().flat_map(|t| t.tags.iter().cloned()).collect();
        all_tags.extend(ed.frame.filter.tags.iter().cloned());
        all_tags.sort_by_key(|t| t.to_lowercase());
        all_tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let lists = self.list_items(false);
        let hour12 = self.data.prefs.hour12;
        let save = editor_title(ui, &mut ed.frame.name, tr("Frame name, e.g. Deep work"), &mut ed.focus);
        ui.add_space(4.0);
                let label = |ui: &mut egui::Ui, t: &str| {
                    ui.label(RichText::new(t).color(muted));
                };
                egui::Grid::new("framegrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    label(ui, tr("Calendar"));
                    let cname = |id: u64| self.data.calendar(id).map(|c| c.name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("fcal").selected_text(cname(ed.frame.calendar)).show_ui(ui, |ui| {
                        for cal in self.data.calendars.iter().filter(|c| c.is_local()) {
                            ui.selectable_value(&mut ed.frame.calendar, cal.id, RichText::new(format!("{} {}", icons::CIRCLE, cal.name)).color(rgb(cal.color)));
                        }
                    });
                    ui.end_row();

                    label(ui, tr("Colour"));
                    color_row(ui, cal_color, &mut ed.frame.color);
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
                    label(ui, tr("Time"));
                    ui.horizontal(|ui| {
                        time(ui, &mut ed.frame.start_min, 0, 24 * 60 - 15);
                        if let Some(v) = time_pick(ui, "fstart", ed.frame.start_min, 0, 24 * 60 - 15, None, hour12) {
                            let len = ed.frame.end_min - ed.frame.start_min;
                            ed.frame.start_min = v;
                            ed.frame.end_min = (v + len).min(24 * 60);
                        }
                        ui.label(RichText::new("to").color(muted));
                        let lo = ed.frame.start_min + 15;
                        time(ui, &mut ed.frame.end_min, lo, 24 * 60);
                        if let Some(v) = time_pick(ui, "fend", ed.frame.end_min, lo, 24 * 60, Some(ed.frame.start_min), hour12) {
                            ed.frame.end_min = v;
                        }
                    });
                    ui.end_row();

                    label(ui, tr("Days"));
                    weekday_chips(ui, &mut ed.frame.days);
                    ui.end_row();

                    label(ui, tr("Repeat"));
                    ui.horizontal(|ui| {
                        ui.label("every");
                        ui.add(egui::DragValue::new(&mut ed.frame.every_weeks).range(1..=8).speed(0.05));
                        ui.label(if ed.frame.every_weeks == 1 { "week" } else { "weeks" });
                        ui.label(RichText::new(format!("from {}", ed.frame.from.format("%b %-d"))).color(muted));
                    });
                    ui.end_row();

                    label(ui, tr("Until"));
                    ui.horizontal(|ui| {
                        date_field(ui, "funtil", &mut ed.until_text, 100.0, look);
                        if !ed.until_text.is_empty() && ui.small_button(tr("Forever")).clicked() {
                            ed.until_text.clear();
                        }
                    });
                    ui.end_row();

                    if !ed.frame.skip.is_empty() {
                        label(ui, tr("Skipped"));
                        ui.horizontal(|ui| {
                            let n = ed.frame.skip.len();
                            ui.label(trf("{} day{}", &[&n, &if n == 1 { "" } else { "s" }]));
                            if ui.small_button(tr("Bring back")).clicked() {
                                ed.frame.skip.clear();
                            }
                        });
                        ui.end_row();
                    }
                });

                ui.add_space(6.0);
                ui.label(RichText::new(tr("Tasks it takes")).strong());
                ui.label(RichText::new(tr("With no list and no tags it takes any task.")).size(theme::size::TINY).color(muted));
                ui.add_space(2.0);
                egui::Grid::new("framefilter").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    label(ui, tr("Lists"));
                    ui.horizontal(|ui| {
                        let items: Vec<(String, egui::Color32, bool)> = lists.iter().map(|(id, text, col)| (text.clone(), *col, ed.frame.filter.lists.contains(id))).collect();
                        if let Some(i) = pick_many(ui, "flists", tr("No list"), &items) {
                            if i == CLEAR {
                                ed.frame.filter.lists.clear();
                            } else {
                                let id = lists[i].0;
                                if items[i].2 {
                                    ed.frame.filter.lists.retain(|l| *l != id);
                                } else {
                                    ed.frame.filter.lists.push(id);
                                }
                            }
                        }
                        if let Some(name) = add_button(ui, "flist", tr("New list")) {
                            *new_list = Some(name);
                        }
                    });
                    ui.end_row();

                    label(ui, tr("Tags"));
                    ui.horizontal(|ui| {
                        let text = ui.visuals().text_color();
                        let items: Vec<(String, egui::Color32, bool)> = all_tags.iter().map(|t| (format!("#{t}"), text, ed.frame.filter.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))).collect();
                        if let Some(i) = pick_many(ui, "ftags", tr("No tags"), &items) {
                            if i == CLEAR {
                                ed.frame.filter.tags.clear();
                            } else {
                                let tag = &all_tags[i];
                                if items[i].2 {
                                    ed.frame.filter.tags.retain(|t| !t.eq_ignore_ascii_case(tag));
                                } else {
                                    ed.frame.filter.tags.push(tag.clone());
                                }
                            }
                        }
                        if let Some(tag) = add_button(ui, "ftag", tr("New tag")) {
                            if !ed.frame.filter.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                                ed.frame.filter.tags.push(tag);
                            }
                        }
                    });
                    ui.end_row();

                    if ed.frame.filter.tags.len() > 1 {
                        label(ui, tr("Match"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut ed.frame.filter.tags_all, false, tr("Any tag"));
                            ui.selectable_value(&mut ed.frame.filter.tags_all, true, tr("All tags"));
                        });
                        ui.end_row();
                    }

                    label(ui, tr("Up to"));
                    let dur = |d: Option<i64>| d.map(fmt_duration).unwrap_or_else(|| tr("Any length").into());
                    egui::ComboBox::from_id_salt("fmax").selected_text(dur(ed.frame.filter.max_min)).show_ui(ui, |ui| {
                        for v in [None, Some(15), Some(30), Some(45), Some(60), Some(90), Some(120), Some(180), Some(240)] {
                            ui.selectable_value(&mut ed.frame.filter.max_min, v, dur(v));
                        }
                    });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.checkbox(&mut ed.frame.exclusive, tr("Keep other tasks out"))
                    .on_hover_text(tr("The planner puts no other task here"));
                ui.checkbox(&mut ed.frame.enabled, tr("Enabled"));
        save | ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
    }

    /// Ctrl+T: a bar in the middle, like the command bar, for a new task.
    fn dlg_new_task(&mut self, ctx: &egui::Context) {
        if !self.new_task_open {
            return;
        }
        let muted = self.pal.muted;
        let focus = std::mem::take(&mut self.new_task_focus);
        let list_name = match self.data.lists.iter().find(|l| l.id == self.new_task_list) {
            Some(l) => l.name.clone(),
            None => {
                self.new_task_list = 0;
                "your Inbox".to_string()
            }
        };
        let mut close = false;
        let hint = self.data.prefs.quick.tokens().hint();
        let accent = self.pal.accent;
        egui::Window::new("newtask")
            .id(ids::new_task())
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icons::SQUARE_CHECK).size(theme::size::LARGE).color(muted));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.new_task_text)
                            .hint_text(hint)
                            .font(FontId::proportional(theme::size::LARGE))
                            .desired_width(f32::INFINITY),
                    );
                    if focus {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let t = std::mem::take(&mut self.new_task_text);
                            if !t.trim().is_empty() {
                                self.add_task(ctx, &t, self.new_task_list);
                                close = true;
                            }
                        } else if ui.input(|i| i.pointer.any_click()) {
                            close = true;
                        }
                    }
                });
                // what the typed text will become, read live
                let now = chrono::Local::now().naive_local();
                let q = parse_quick(&self.new_task_text, now.date(), &self.data.prefs.quick);
                let chips = q.chips(now, self.data.prefs.hour12);
                if !chips.is_empty() {
                    ui.label(RichText::new(chips.join("   ")).size(theme::size::CAPTION).color(accent));
                }
                let lead = if q.start(now).is_some() {
                    trf("Enter puts it on the calendar and in {}  ·  Esc closes", &[&list_name])
                } else {
                    trf("Enter adds the task to {}  ·  Esc closes", &[&list_name])
                };
                ui.label(RichText::new(lead).size(theme::size::TINY).color(muted));
            });
        if close {
            self.new_task_open = false;
        }
    }

    fn dlg_event(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.editor.take() else { return };
        self.sync_event_edit(&mut ed);
        let is_task = ed.ev.task_id.is_some();
        let title = if ed.is_new && is_task {
            tr("New task")
        } else if ed.is_new {
            tr("New event")
        } else if is_task {
            tr("Task session")
        } else {
            tr("Event")
        };
        let at = self.editor_spot(ctx, ids::event_editor().with(ed.ev.id), ed.anchor_rect);
        let stored = (!ed.is_new).then(|| self.data.event(ed.ev.id)).flatten();
        let offer = Offer { delete: stored.is_some_and(|e| self.data.locked(e).is_none()), skip: stored.filter(|e| e.task_id.is_none()).map(|e| e.skipped) };
        let focus = ed.focus;
        let pressed = {
            let ed = std::cell::RefCell::new(&mut ed);
            self.edit_window(ctx, "event_editor", title, at, focus, offer, |_| ed.borrow().changed(), |app, ui| app.event_form(ui, &mut ed.borrow_mut()))
        };
        self.finish_event_edit(ctx, ed, pressed);
    }

    /// Where an editor window opens: beside the block it came from, else at
    /// the main window's top right. Kept for as long as it edits that item.
    fn editor_spot(&self, ctx: &egui::Context, key: egui::Id, anchor: Option<egui::Rect>) -> egui::Pos2 {
        if let Some(p) = ctx.data(|d| d.get_temp::<egui::Pos2>(key)) {
            return p;
        }
        let main = ctx.input(|i| i.viewport().inner_rect).unwrap_or(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)));
        let p = match anchor {
            Some(r) => main.min + egui::vec2(r.right() + 12.0, r.top().max(40.0)),
            None => egui::pos2(main.right() - 400.0, main.top() + 64.0),
        };
        ctx.data_mut(|d| d.insert_temp(key, p));
        p
    }

    /// Shows an editor in a window of its own, so the person can work in the
    /// main window and come back to it; where the system can't open one it
    /// sits inside the main window. Closing it or Esc cancels. While `focus`
    /// is set the window asks the system for the keys, so typing goes in.
    #[allow(clippy::too_many_arguments)]
    fn edit_window(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        title: &str,
        at: egui::Pos2,
        focus: bool,
        offer: Offer,
        changed: impl Fn(&App) -> bool,
        mut form: impl FnMut(&mut App, &mut egui::Ui) -> bool,
    ) -> Pressed {
        let ask = asking_id(name);
        let asking = ctx.data(|d| d.get_temp::<bool>(ask)).unwrap_or(false);
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id("katlendar")
            .with_position(at)
            .with_inner_size(EDITOR_SIZE)
            .with_min_inner_size([380.0, 300.0])
            .with_window_level(egui::WindowLevel::AlwaysOnTop);
        let mut out = Pressed::default();
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(name), builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                self.own_windows = false;
                ui.set_width(EDITOR_SIZE[0] - 28.0);
                let save = form(self, ui);
                ui.add_space(4.0);
                out = editor_buttons(ui, offer, asking);
                out.save |= save;
                return;
            }
            self.own_windows = true;
            let ctx = ui.ctx().clone();
            if focus {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            // the buttons sit at the foot of the window; the fields scroll above them
            let foot = egui::Panel::bottom(ids::editor_buttons())
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(self.pal.bg).inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 14 }))
                .show(ui, |ui| out = editor_buttons(ui, offer, asking));
            let mut save = false;
            let mut content = egui::Vec2::ZERO;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(self.pal.bg).inner_margin(egui::Margin { left: 14, right: 14, top: 14, bottom: 0 })).show(ui, |ui| {
                content = egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        save = form(self, ui);
                    })
                    .content_size;
            });
            out.save |= save;
            // a window just opened grows to show all its fields, the notes
            // at the end too, within the screen; after that its size is the
            // person's
            // (egui forgets a closed window, so its passes count from 0 again;
            // the system may not tell where a window is, as on Wayland, but
            // its size is the size of what is drawn in it)
            if ctx.cumulative_pass_nr() < 6 {
                let want = egui::vec2(content.x + 28.0, content.y + 14.0 + foot.response.rect.height()).max(EDITOR_SIZE.into());
                let screen = ctx.input(|i| i.viewport().monitor_size);
                let want = screen.map_or(want, |s| want.min(s - egui::vec2(40.0, 80.0)));
                let now = ctx.viewport_rect().size();
                if want.x > now.x + 0.5 || want.y > now.y + 0.5 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
                    if let Some(s) = screen {
                        // kept on the screen, as far as the system lets it be moved
                        let at = egui::pos2(at.x.min(s.x - want.x - 20.0).max(0.0), at.y.min(s.y - want.y - 60.0).max(0.0));
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(at));
                    }
                }
            }
            let (close, esc) = ctx.input(|i| (i.viewport().close_requested(), i.key_pressed(egui::Key::Escape)));
            if close {
                // the window stays until the unsaved changes are asked about
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            out.cancel |= close;
            out.keep |= asking && esc;
            out.cancel |= esc && !asking;
        });
        // leaving with changes not saved asks first; while it asks, closing
        // again only keeps the question up
        if asking {
            if out.discard {
                out.cancel = true;
            } else if out.cancel || out.keep {
                out.cancel = false;
                if out.keep {
                    ctx.data_mut(|d| d.insert_temp(ask, false));
                }
            }
        } else if out.cancel && changed(self) {
            out.cancel = false;
            ask_discard(ctx, name);
        }
        if out.save || out.cancel || out.delete {
            ctx.data_mut(|d| d.remove_temp::<bool>(ask));
        }
        out
    }

    /// Takes into the editor what changed in the stored event since it last
    /// looked (a drag, a resize, undo), keeping the person's other edits.
    fn sync_event_edit(&self, ed: &mut EventEdit) {
        if let Some(t) = ed.task.as_mut() {
            self.sync_task_edit(t);
        }
        let Some(base) = ed.base.as_ref() else { return };
        let Some(now) = self.shown_event(ed.ev.id) else { return };
        if (now.start, now.end, now.all_day) != (base.start, base.end, base.all_day) {
            (ed.ev.start, ed.ev.end, ed.ev.all_day) = (now.start, now.end, now.all_day);
        }
        if now.title != base.title {
            ed.ev.title = now.title.clone();
        }
        if now.notes != base.notes {
            ed.ev.notes = now.notes.clone();
        }
        if now.calendar != base.calendar {
            ed.ev.calendar = now.calendar;
        }
        if now.color != base.color {
            ed.ev.color = now.color;
        }
        ed.base = Some(now);
    }

    /// The event editor's fields; returns whether to save.
    fn event_form(&mut self, ui: &mut egui::Ui, ed: &mut EventEdit) -> bool {
        let is_task = ed.ev.task_id.is_some();
        let hour12 = self.data.prefs.hour12;
        let look = self.pick_look();
        let muted = self.pal.muted;
        // a read-only event: only Skip and the side notes can change
        let locked = if ed.is_new { None } else { self.data.locked(&ed.ev) };
        if let Some(why) = locked {
            ui.label(RichText::new(why).size(theme::size::TINY).color(muted));
            ui.add_space(2.0);
        }
        let mut save = false;
        // a read-only event's name can't take the cursor: its side notes do
        let (mut to_title, mut to_notes) = first_field(ed.focus, &ed.ev.title);
        if locked.is_some() {
            (to_title, to_notes) = (false, ed.focus);
        }
        // the place is the first line of the notes ("Where: …"), as servers
        // get it; a task's block has no place of its own
        let (mut place, mut rest) = if is_task { (String::new(), ed.ev.notes.clone()) } else { split_place(&ed.ev.notes) };
        ui.add_enabled_ui(locked.is_none(), |ui| {
            save |= editor_title(ui, &mut ed.ev.title, if is_task { tr("New Task") } else { tr("New Event") }, &mut to_title);
            let was_all_day = ed.ev.all_day;
            ui.checkbox(&mut ed.ev.all_day, tr("All day"));
            if ed.ev.all_day && !was_all_day {
                ed.ev.start = midnight(ed.ev.start.date());
                ed.ev.end = ed.ev.start + Duration::days(1);
            } else if !ed.ev.all_day && was_all_day {
                ed.ev.start = midnight(ed.ev.start.date()) + Duration::hours(NEW_EVENT_NOON_H);
                ed.ev.end = ed.ev.start + Duration::minutes(NEW_EVENT_MIN);
            }
            editor_grid(ui, "eventgrid", |ui| {
                editor_label(ui, tr("Date"), muted);
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
                    // moves the whole event, keeping its times and length
                    if let Some(d) = date_pick(ui, "edate", Some(ed.ev.start.date()), look) {
                        let by = d - ed.ev.start.date();
                        ed.ev.start += by;
                        ed.ev.end += by;
                    }
                });
                ui.end_row();
                if ed.ev.all_day {
                    let mut n = (ed.ev.end.date() - ed.ev.start.date()).num_days().max(1);
                    editor_label(ui, tr("Days"), muted);
                    ui.add(egui::DragValue::new(&mut n).range(1..=60));
                    ui.end_row();
                    ed.ev.end = midnight(ed.ev.start.date()) + Duration::days(n);
                } else {
                    let len = ed.ev.end - ed.ev.start;
                    let (mut sh, mut sm) = (ed.ev.start.hour(), ed.ev.start.minute());
                    let (mut eh, mut em) = (ed.ev.end.hour(), ed.ev.end.minute());
                    let start_m = (sh * 60 + sm) as i64;
                    // the end's list runs a day on from the start, as it may end tomorrow
                    let time = |ui: &mut egui::Ui, h: &mut u32, m: &mut u32, after: Option<String>| -> bool {
                        let mut ch = false;
                        ui.horizontal(|ui| {
                            let end = after.is_some();
                            let (salt, lo, hi, from) = if end { ("eend", start_m + 15, start_m + 24 * 60, Some(start_m)) } else { ("estart", 0, 24 * 60 - 15, None) };
                            ch |= ui.add(egui::DragValue::new(h).range(0..=23).speed(0.1)).changed();
                            ui.label(":");
                            ch |= ui
                                .add(egui::DragValue::new(m).range(0..=59).speed(0.25).custom_formatter(|v, _| format!("{:02}", v as u32)))
                                .changed();
                            if let Some(v) = time_pick(ui, salt, (*h * 60 + *m) as i64, lo, hi, from, hour12) {
                                let v = v.rem_euclid(24 * 60) as u32;
                                (*h, *m) = (v / 60, v % 60);
                                ch = true;
                            }
                            if hour12 {
                                let h12 = if *h % 12 == 0 { 12 } else { *h % 12 };
                                let ap = if *h < 12 { "AM" } else { "PM" };
                                ui.label(RichText::new(format!("{h12}:{:02} {ap}", *m)).color(muted));
                            }
                            if let Some(a) = after {
                                ui.label(RichText::new(a).color(muted).size(theme::size::TINY));
                            }
                        });
                        ch
                    };
                    editor_label(ui, tr("Start"), muted);
                    if time(ui, &mut sh, &mut sm, None) {
                        ed.ev.start = ed.ev.start.date().and_hms_opt(sh, sm, 0).unwrap();
                        ed.ev.end = ed.ev.start + len;
                    }
                    ui.end_row();
                    let mins = ed.ev.minutes();
                    let length = if mins % 60 == 0 {
                        trf("{} h", &[&(mins / 60)])
                    } else if mins > 60 {
                        trf("{} h {} min", &[&(mins / 60), &(mins % 60)])
                    } else {
                        trf("{} min", &[&mins])
                    };
                    editor_label(ui, tr("End"), muted);
                    if time(ui, &mut eh, &mut em, Some(length)) {
                        let mut e = ed.ev.start.date().and_hms_opt(eh, em, 0).unwrap();
                        if e <= ed.ev.start {
                            e += Duration::days(1);
                        }
                        ed.ev.end = e;
                    }
                    ui.end_row();
                }
                editor_label(ui, tr("Calendar"), muted);
                let name = self.data.calendar(ed.ev.calendar).map(|c| c.name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("cal").selected_text(name).show_ui(ui, |ui| {
                    for c in self.data.calendars.iter().filter(|c| c.writable()) {
                        ui.selectable_value(&mut ed.ev.calendar, c.id, RichText::new(format!("{} {}", icons::CIRCLE, c.name)).color(rgb(c.color)));
                    }
                });
                ui.end_row();
                if !is_task {
                    // a task's block takes its list's colour and its task's repeat
                    editor_label(ui, tr("Colour"), muted);
                    color_row(ui, self.data.calendar_color(ed.ev.calendar), &mut ed.ev.color);
                    ui.end_row();
                    editor_label(ui, tr("Location"), muted);
                    ui.add(egui::TextEdit::singleline(&mut place).hint_text(tr("Add a place")).desired_width(f32::INFINITY));
                    ui.end_row();
                    editor_label(ui, tr("Repeat"), muted);
                    ui.horizontal(|ui| {
                        if repeat_combo(ui, "erep", &mut ed.repeat, ed.ev.start.date()) && ed.repeat != Repeat::None {
                            ed.until_text = ed.repeat.first_until(ed.ev.start.date()).to_string();
                        }
                        if ed.repeat != Repeat::None {
                            ui.label(RichText::new(tr("until")).color(muted));
                            date_field(ui, "euntil", &mut ed.until_text, 100.0, look);
                        }
                    })
                    .response
                    .on_hover_text(tr("Saving adds a copy of this event each time, up to the last day"));
                    ui.end_row();
                    custom_days_row(ui, &mut ed.repeat, muted, true);
                }
        });
        // a task's block shows the whole task, as its own editor does
        if let Some(t) = ed.task.as_mut() {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            self.task_fields(ui, t);
        }
        });
        if locked.is_none() {
            save |= editor_notes(ui, &mut rest, tr("Notes"), &mut to_notes);
            ed.ev.notes = if is_task { rest } else { join_place(&place, &rest) };
        } else if !ed.ev.notes.trim().is_empty() {
            // its own notes, to read and copy from
            ui.add_space(4.0);
            let mut notes = ed.ev.notes.as_str();
            ui.add(egui::TextEdit::multiline(&mut notes).desired_rows(2).desired_width(f32::INFINITY));
        }
        if locked.is_some() || !ed.ev.side_notes.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Side notes · kept in this app only")).size(theme::size::TINY).color(muted));
            save |= editor_notes(ui, &mut ed.ev.side_notes, tr("Your own notes on this event"), &mut to_notes);
        }
        ed.focus = to_title || to_notes;
        save
    }

    fn finish_event_edit(&mut self, ctx: &egui::Context, mut ed: EventEdit, p: Pressed) {
        let Pressed { save, cancel, delete, skip, .. } = p;
        if skip {
            // the editor stays open, showing Unskip
            self.toggle_skip(ctx, ed.ev.id);
            return self.editor = Some(ed);
        }
        if save && ed.task.as_mut().is_some_and(|t| t.read_text().is_err()) {
            self.warn(ctx, tr("The due date should look like 2026-11-30"));
            return self.editor = Some(ed);
        }
        // a repeat is laid out as copies of the event up to a last day
        let mut copies: Vec<NaiveDate> = Vec::new();
        if save && ed.repeat != Repeat::None && ed.ev.task_id.is_none() && self.data.locked(&ed.ev).is_none() {
            match NaiveDate::parse_from_str(ed.until_text.trim(), "%Y-%m-%d") {
                Ok(until) => copies = ed.repeat.days_after(ed.ev.start.date(), until),
                Err(_) => {
                    self.warn(ctx, tr("The last day should look like 2026-11-30"));
                    return self.editor = Some(ed);
                }
            }
        }
        if save && self.data.locked(&ed.ev).is_some() {
            // only the side notes are its own here
            if self.data.event(ed.ev.id).is_some_and(|e| e.side_notes != ed.ev.side_notes) {
                self.snapshot();
                if let Some(e) = self.data.event_mut(ed.ev.id) {
                    e.side_notes = ed.ev.side_notes.clone();
                }
                self.commit();
            }
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
            // a block's name and note are its task's, and so its other blocks'
            let edited = ed.task.as_ref().map(|t| t.task.clone());
            // a new task made from the calendar is kept now
            if let (Some(tid), Some(new)) = (ev.task_id, edited.as_ref()) {
                if self.data.task(tid).is_none() {
                    if ev.title.trim().is_empty() {
                        ev.title = tr("New task").into();
                    }
                    let duration_min = new.duration_min.or(Some(ev.minutes()));
                    self.data.tasks.insert(0, Task { title: ev.title.clone(), notes: ev.notes.clone(), duration_min, ..new.clone() });
                }
            }
            if let Some(t) = ev.task_id.and_then(|t| self.data.task_mut(t)) {
                if let Some(mut n) = edited {
                    (n.title, n.notes) = (t.title.clone(), t.notes.clone());
                    t.take_edits(n);
                }
                if !ev.title.trim().is_empty() {
                    t.title = ev.title.clone();
                }
                t.notes = ev.notes.clone();
            }
            let day = ev.start.date();
            for d in &copies {
                let mut copy = ev.clone();
                copy.detach();
                copy.skipped = false;
                copy.id = self.data.new_id();
                (copy.start, copy.end) = (ev.start + (*d - day), ev.end + (*d - day));
                self.data.events.push(copy);
            }
            if !copies.is_empty() {
                self.say(ctx, trf("Added {} events", &[&copies.len()]), true);
            }
            if let Some(slot) = self.data.event_mut(id) {
                slot.take_edits(ev);
            } else {
                self.data.events.push(ev);
            }
            if let Some((t, _)) = sched {
                self.data.sync_blocks(t);
            }
            if let Some((t, day)) = sched {
                self.after_task_scheduled(ctx, t, day);
            }
            self.selected = Some(id);
            self.commit();
        } else if delete {
            self.delete_event(ctx, ed.ev.id);
        } else if !cancel {
            return self.editor = Some(ed);
        }
        ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(ids::event_editor().with(ed.ev.id)));
    }

    fn dlg_task(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.task_editor.take() else { return };
        self.sync_task_edit(&mut ed);
        let at = self.editor_spot(ctx, ids::task_editor().with(ed.task.id), None);
        let focus = ed.focus;
        let p = {
            let ed = std::cell::RefCell::new(&mut ed);
            self.edit_window(ctx, "task_editor", tr("Task"), at, focus, Offer { delete: true, skip: None }, |_| ed.borrow().changed(), |app, ui| app.task_form(ui, &mut ed.borrow_mut()))
        };
        self.finish_task_edit(ctx, ed, p.save, p.cancel, p.delete);
    }

    /// As [`Self::sync_event_edit`], for a task: what changed elsewhere
    /// (ticked off, moved to a list, a due date from scheduling) comes in.
    fn sync_task_edit(&self, ed: &mut TaskEdit) {
        let Some(now) = self.data.task(ed.task.id).cloned() else { return };
        let (b, t) = (&ed.base, &mut ed.task);
        macro_rules! take {
            ($($f:ident),*) => { $( if now.$f != b.$f { t.$f = now.$f.clone(); } )* };
        }
        take!(title, notes, done, priority, duration_min, list, repeat, flexible, color);
        if now.due != b.due {
            ed.due_text = now.due.map(|d| d.to_string()).unwrap_or_default();
        }
        if now.tags != b.tags {
            ed.tags_text = now.tags.join(" ");
        }
        if now.earliest != b.earliest {
            ed.earliest_text = now.earliest.map(|d| d.to_string()).unwrap_or_default();
        }
        ed.base = now;
    }

    /// The task editor's fields; returns whether to save.
    fn task_form(&mut self, ui: &mut egui::Ui, ed: &mut TaskEdit) -> bool {
        let (mut to_title, mut to_notes) = first_field(ed.focus, &ed.task.title);
        let save = editor_title(ui, &mut ed.task.title, tr("New Task"), &mut to_title);
        self.task_fields(ui, ed);
        let save = save | editor_notes(ui, &mut ed.task.notes, tr("Notes"), &mut to_notes);
        ed.focus = to_title || to_notes;
        save
    }

    /// A task's own fields, below its name: in the task editor and in the
    /// editor of each of its blocks.
    fn task_fields(&mut self, ui: &mut egui::Ui, ed: &mut TaskEdit) {
        let look = self.pick_look();
        let today = Local::now().date_naive();
        let muted = self.pal.muted;
        ui.checkbox(&mut ed.task.done, tr("Completed"));
        editor_grid(ui, "taskgrid", |ui| {
            editor_label(ui, tr("Due"), muted);
            ui.horizontal(|ui| {
                date_field(ui, "tdue", &mut ed.due_text, 96.0, look);
                if ui.small_button(tr("Today")).clicked() {
                    ed.due_text = today.to_string();
                }
                if ui.small_button(tr("Tomorrow")).clicked() {
                    ed.due_text = (today + Duration::days(1)).to_string();
                }
                if ui.small_button(icons::X).on_hover_text(tr("No due date")).clicked() {
                    ed.due_text.clear();
                }
            });
            ui.end_row();
            editor_label(ui, tr("Priority"), muted);
            egui::ComboBox::from_id_salt("prio").selected_text(priority_name(ed.task.priority)).show_ui(ui, |ui| {
                for v in [0u8, 1, 5, 9] {
                    ui.selectable_value(&mut ed.task.priority, v, priority_name(v));
                }
            });
            ui.end_row();
            editor_label(ui, tr("Duration"), muted);
            let dur = |d: Option<i64>| d.map(|m| trf("{} min", &[&m])).unwrap_or_else(|| tr("No estimate").into());
            egui::ComboBox::from_id_salt("dur").selected_text(dur(ed.task.duration_min)).show_ui(ui, |ui| {
                for v in [None, Some(15), Some(30), Some(45), Some(60), Some(90), Some(120), Some(180), Some(240)] {
                    ui.selectable_value(&mut ed.task.duration_min, v, dur(v));
                }
            });
            ui.end_row();
            editor_label(ui, tr("Repeat due"), muted);
            let day = ed.task.due.unwrap_or_else(|| chrono::Local::now().date_naive());
            repeat_combo(ui, "rep", &mut ed.task.repeat, day);
            ui.end_row();
            custom_days_row(ui, &mut ed.task.repeat, muted, true);
            editor_label(ui, tr("Earliest start"), muted);
            ui.horizontal(|ui| date_field(ui, "tearliest", &mut ed.earliest_text, 96.0, look));
            ui.end_row();
            editor_label(ui, tr("List"), muted);
            let items = self.list_items(false);
            pick_one(ui, "tlist", &items, &mut ed.task.list);
            ui.end_row();
            editor_label(ui, tr("Colour"), muted);
            let base = self.data.list_color(Some(&ed.task), self.data.default_calendar());
            swatches(ui, base, tr("List colour"), &mut ed.task.color);
            ui.end_row();
            editor_label(ui, tr("Tags"), muted);
            ui.add(egui::TextEdit::singleline(&mut ed.tags_text).hint_text("admin deep-work").desired_width(200.0));
            ui.end_row();
            editor_label(ui, tr("Flexible"), muted);
            ui.checkbox(&mut ed.task.flexible, "");
            ui.end_row();
        });
        // time already planned for this task
        let planned: i64 = self.data.events.iter().filter(|e| e.task_id == Some(ed.task.id)).map(|e| e.minutes()).sum();
        if planned > 0 {
            let need = ed.task.duration_min.map(|d| (d - planned).max(0));
            let mut line = trf("Scheduled {}", &[&fmt_duration(planned)]);
            if let Some(n) = need {
                line += &format!("  ·  still needed {}", fmt_duration(n));
            }
            ui.label(RichText::new(line).size(theme::size::TINY).color(muted));
        }
    }

    fn finish_task_edit(&mut self, ctx: &egui::Context, mut ed: TaskEdit, save: bool, cancel: bool, delete: bool) {
        let key = ids::task_editor().with(ed.task.id);
        if save {
            if ed.read_text().is_err() {
                self.warn(ctx, tr("The due date should look like 2026-11-30"));
                self.task_editor = Some(ed);
                return;
            }
            self.snapshot();
            let id = ed.task.id;
            let n = ed.task;
            match self.data.task_mut(id) {
                Some(t) => t.take_edits(n),
                // gone while the window was open: the edit brings it back
                None => {
                    let mut n = n;
                    n.detach();
                    self.data.tasks.insert(0, n);
                }
            }
            self.data.sync_blocks(id);
            self.commit();
        } else if delete {
            self.snapshot();
            let id = ed.task.id;
            self.data.tasks.retain(|t| t.id != id);
            self.data.events.retain(|e| e.task_id != Some(id));
            self.commit();
            self.say(ctx, tr("Task deleted"), true);
        } else if !cancel {
            return self.task_editor = Some(ed);
        }
        ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(key));
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
            .id(ids::command())
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut q)
                        .hint_text(tr("Type a command or search events and tasks"))
                        .font(FontId::proportional(theme::size::LARGE))
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
                    for (cmd, label, _, _) in COMMANDS.iter().map(|c| (&c.0, tr(c.1), c.2, c.3)).filter(|c| c.1.to_lowercase().contains(&ql)) {
                        if matches!(cmd, Cmd::CommandBar | Cmd::Search) {
                            continue;
                        }
                        let keys = &self.bind_text(*cmd);
                        ui.horizontal(|ui| {
                            let clicked = ui.selectable_label(false, label).clicked();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(keys).color(muted).monospace().size(theme::size::MICRO));
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
        let warn = t.warn;
        // a warning's countdown line runs smoothly, so it repaints often
        ctx.request_repaint_after(std::time::Duration::from_millis(if warn { 33 } else { 300 }));
        let (text, undo, move_due) = (t.text.clone(), t.undo, t.move_due);
        let (born, until) = (if t.born < 0.0 { now } else { t.born }, t.until);
        if let Some(t) = &mut self.toast {
            t.born = born;
        }
        // fades and rises in, fades out
        let a_in = (((now - born) as f32) / motion::NORMAL.secs()).clamp(0.0, 1.0);
        let a_out = (((until - now) as f32) / motion::NORMAL.secs()).clamp(0.0, 1.0);
        let alpha = a_in.min(a_out);
        if alpha < 1.0 {
            ctx.request_repaint();
        }
        let (mut do_undo, mut do_move, mut dismiss) = (false, false, false);
        let fill = mix(self.pal.side, self.pal.text, 0.12);
        // a warning wears a bright orange edge and sign, so it is not missed
        let border = if warn { Stroke::new(2.0, theme::WARNING) } else { Stroke::new(1.0, self.pal.border) };
        let accent = self.pal.accent;
        let left = (((until - now) / (until - born).max(0.001)) as f32).clamp(0.0, 1.0);
        egui::Area::new(ids::toast())
            .anchor(Align2::CENTER_BOTTOM, [0.0, -24.0 + (1.0 - a_in) * 16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                let frame = egui::Frame::new()
                    .fill(fill)
                    .stroke(border)
                    .corner_radius(theme::CARD_RADIUS)
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if warn {
                                ui.label(RichText::new(icons::ALERT).size(theme::size::LARGE).color(theme::WARNING));
                            }
                            ui.label(text);
                            if undo && ui.button(tr("Undo")).clicked() {
                                do_undo = true;
                            }
                            if move_due.is_some() {
                                if ui.button(tr("Move due date")).clicked() {
                                    do_move = true;
                                }
                                if ui.button(tr("Keep")).clicked() {
                                    dismiss = true;
                                }
                            }
                        });
                    });
                // a warning shows the time left as a theme-coloured line that runs out
                if warn {
                    let r = frame.response.rect;
                    let inset = theme::CARD_RADIUS;
                    let y = r.bottom() - 4.0;
                    let x0 = r.left() + inset;
                    let x1 = x0 + (r.width() - 2.0 * inset).max(0.0) * left;
                    ui.painter().line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], Stroke::new(2.0, accent));
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn app_with_event() -> App {
        let mut app = App::new(Some(PathBuf::from("unused.png")));
        app.data.events.clear();
        let start = App::now();
        let calendar = app.data.first_own_calendar();
        app.data.events.push(Event { id: 700, title: "Talk".into(), start, end: start + Duration::hours(1), calendar, ..Default::default() });
        app
    }

    #[test]
    fn the_place_is_the_first_line_of_the_notes() {
        assert_eq!(split_place("Where: Room 4 \nBring notes"), ("Room 4 ".into(), "Bring notes".into()));
        assert_eq!(split_place("Bring notes"), (String::new(), "Bring notes".into()));
        for notes in ["Where: Room 4\nBring notes", "Where: Room 4", "Bring notes", ""] {
            let (place, rest) = split_place(notes);
            assert_eq!(join_place(&place, &rest), notes);
        }
        assert_eq!(join_place("", "Bring notes"), "Bring notes");
    }

    #[test]
    fn an_editor_opens_on_the_name_when_blank_else_the_notes() {
        assert_eq!(first_field(true, "  "), (true, false));
        assert_eq!(first_field(true, "Talk"), (false, true));
        assert_eq!(first_field(false, ""), (false, false));
        let mut app = app_with_event();
        app.edit_event(700);
        assert!(app.editor.as_ref().unwrap().focus);
    }

    #[test]
    fn saving_a_repeat_lays_out_copies_up_to_the_last_day() {
        let mut app = app_with_event();
        app.edit_event(700);
        let mut ed = app.editor.take().unwrap();
        let day = ed.ev.start.date();
        ed.repeat = Repeat::Weekly;
        ed.until_text = (day + Duration::weeks(3)).to_string();
        ed.ev.color = Some(EVENT_COLORS[0].0);
        let ctx = egui::Context::default();
        app.finish_event_edit(&ctx, ed, Pressed { save: true, ..Default::default() });
        let mut talks: Vec<_> = app.data.events.iter().filter(|e| e.title == "Talk").map(|e| (e.start.date(), e.color)).collect();
        talks.sort();
        let want: Vec<_> = (0..4).map(|w| (day + Duration::weeks(w), Some(EVENT_COLORS[0].0))).collect();
        assert_eq!(talks, want);
    }

    #[test]
    fn a_drag_reaches_an_open_event_editor_and_keeps_typed_text() {
        let mut app = app_with_event();
        app.edit_event(700);
        let mut ed = app.editor.take().unwrap();
        ed.ev.notes = "typed, not saved".into();
        // the block is dragged an hour later in the main window
        let moved = app.data.event(700).unwrap().start + Duration::hours(1);
        let e = app.data.event_mut(700).unwrap();
        (e.start, e.end) = (moved, moved + Duration::hours(1));
        app.sync_event_edit(&mut ed);
        assert_eq!(ed.ev.start, moved);
        assert_eq!(ed.ev.notes, "typed, not saved");
        // a later frame with nothing new changes nothing
        ed.ev.title = "Talk prep".into();
        app.sync_event_edit(&mut ed);
        assert_eq!(ed.ev.title, "Talk prep");
    }

    #[test]
    fn a_task_block_edits_its_whole_task() {
        let mut app = app_with_event();
        let id = app.data.new_id();
        app.data.tasks.push(Task { id, title: "Write".into(), ..Default::default() });
        app.data.events[0].task_id = Some(id);
        app.edit_event(700);
        let mut ed = app.editor.take().unwrap();
        let t = ed.task.as_mut().expect("a task block opens with its task");
        (t.task.priority, t.tags_text, t.due_text) = (1, "deep #admin".into(), "2026-12-01".into());
        ed.ev.title = "Write more".into();
        let ctx = egui::Context::default();
        app.finish_event_edit(&ctx, ed, Pressed { save: true, ..Default::default() });
        let t = app.data.task(id).unwrap();
        assert_eq!((t.priority, t.title.as_str()), (1, "Write more"));
        assert_eq!(t.tags, ["deep", "admin"]);
        assert_eq!(t.due, NaiveDate::from_ymd_opt(2026, 12, 1));
    }

    #[test]
    fn a_change_elsewhere_reaches_an_open_task_editor() {
        let mut app = app_with_event();
        let id = app.data.new_id();
        app.data.tasks.push(Task { id, title: "Write".into(), ..Default::default() });
        app.edit_task(id);
        let mut ed = app.task_editor.take().unwrap();
        ed.task.notes = "draft".into();
        let day = App::now().date();
        let t = app.data.task_mut(id).unwrap();
        (t.done, t.due) = (true, Some(day));
        app.sync_task_edit(&mut ed);
        assert!(ed.task.done);
        assert_eq!(ed.due_text, day.to_string());
        assert_eq!(ed.task.notes, "draft");
    }
    #[test]
    fn a_read_only_event_keeps_its_fields_and_takes_side_notes() {
        let mut app = app_with_event();
        let cal = 9901;
        app.data.calendars.push(Calendar { id: cal, name: "Feed".into(), url: Some("https://example.org/a.ics".into()), visible: true, ..Default::default() });
        app.data.events[0].calendar = cal;
        app.edit_event(700);
        let mut ed = app.editor.take().unwrap();
        (ed.ev.title, ed.ev.side_notes) = ("Changed".into(), "bring slides".into());
        let ctx = egui::Context::default();
        app.finish_event_edit(&ctx, ed, Pressed { save: true, ..Default::default() });
        let e = app.data.event(700).unwrap();
        assert_eq!((e.title.as_str(), e.side_notes.as_str()), ("Talk", "bring slides"));
        assert!(app.editor.is_none());
    }

    #[test]
    fn skip_keeps_the_editor_open_and_undoes() {
        let mut app = app_with_event();
        app.edit_event(700);
        let ed = app.editor.take().unwrap();
        let ctx = egui::Context::default();
        app.finish_event_edit(&ctx, ed, Pressed { skip: true, ..Default::default() });
        assert!(app.data.event(700).unwrap().skipped);
        assert!(app.editor.is_some());
        app.toggle_skip(&ctx, 700);
        assert!(!app.data.event(700).unwrap().skipped);
    }

    #[test]
    fn an_editor_knows_when_it_has_unsaved_changes() {
        let mut app = app_with_event();
        app.edit_event(700);
        let mut ed = app.editor.take().unwrap();
        assert!(!ed.changed(), "just opened");
        ed.ev.title.push('!');
        assert!(ed.changed());

        let task = Task { id: 5, title: "Read".into(), tags: vec!["a".into(), "b".into()], ..Default::default() };
        let mut t = crate::TaskEdit::of(&task);
        assert!(!t.changed());
        t.tags_text = "a  b ".into();
        assert!(!t.changed(), "the same tags, spaced otherwise");
        t.due_text = "2026-12-01".into();
        assert!(t.changed());

        let id = app.data.frames.first().map(|f| f.id);
        if let Some(id) = id {
            app.edit_frame(id);
            let mut f = app.frame_editor.take().unwrap();
            assert!(!f.changed(app.data.frame(id)));
            f.frame.name.push('!');
            assert!(f.changed(app.data.frame(id)));
        }
    }

    #[test]
    fn folded_lists_stay_folded_through_the_rail() {
        let mut app = App::new(Some(PathBuf::from("unused.png")));
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let frame = |app: &mut App, t: &mut f64| {
            *t += 0.05;
            let raw = egui::RawInput { time: Some(*t), screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| app.ui_sidebar(ui));
            out.textures_delta.clear();
        };
        for _ in 0..5 { frame(&mut app, &mut t); }
        let keys = app.section_keys(Local::now().date_naive());
        app.collapsed.extend(keys.clone());
        let before = app.collapsed.clone();
        // folded to the rail and opened again
        for _ in 0..5 { frame(&mut app, &mut t); }
        app.run(&ctx, Cmd::ToggleTasks);
        for _ in 0..40 { frame(&mut app, &mut t); }
        app.run(&ctx, Cmd::ToggleTasks);
        for _ in 0..40 { frame(&mut app, &mut t); }
        assert_eq!(before, app.collapsed);
    }
}
