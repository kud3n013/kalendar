//! The simple task planner: finding free time and fitting tasks into it.

use super::*;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};

impl Data {
    /// Free gaps inside working hours of one day, from `from` on.
    pub(super) fn free_gaps(&self, day: NaiveDate, from: NaiveDateTime) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            return vec![];
        }
        let midnight = day.and_hms_opt(0, 0, 0).unwrap();
        let ws = (midnight + Duration::minutes(WORK_START_MIN)).max(from);
        let we = midnight + Duration::minutes(WORK_END_MIN);
        self.free_gaps_in(ws, we)
    }

    /// Stretches of [a, b) not covered by any timed event, on any calendar.
    /// Frames are not events and never count as busy.
    pub(super) fn free_gaps_in(&self, a: NaiveDateTime, b: NaiveDateTime) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        if a >= b {
            return vec![];
        }
        let mut busy: Vec<(NaiveDateTime, NaiveDateTime)> = self
            .events
            .iter()
            .filter(|e| !e.all_day && e.start < b && e.end > a)
            .map(|e| (e.start, e.end))
            .collect();
        busy.sort();
        let mut gaps = vec![];
        let mut cur = a;
        for (s, e) in busy {
            if s > cur {
                gaps.push((cur, s));
            }
            cur = cur.max(e);
        }
        if cur < b {
            gaps.push((cur, b));
        }
        gaps
    }

    /// Frames are in use: switched on and at least one of them enabled.
    pub(super) fn frames_active(&self) -> bool {
        self.prefs.use_frames && self.frames.iter().any(|f| f.enabled)
    }

    /// Where a task may go over `days` days from `day0`, at or after `from`:
    /// free time inside matching frames, and free working time outside the
    /// exclusive frames it does not match (empty when that is not allowed).
    /// Without a task, plain free working time.
    pub(super) fn task_windows(&self, t: Option<&Task>, day0: NaiveDate, from: NaiveDateTime, days: i64, own_days: i64) -> (Vec<Window>, Vec<Window>, Vec<Window>) {
        let task_cal = self.task_calendar();
        let frames = self.frames_active() && t.is_some();
        let (mut own, mut open, mut outside) = (vec![], vec![], vec![]);
        // `days` for frames open to anything and for free time; a task's
        // own frames are looked for further ahead, over `own_days`
        for d in 0..days.max(own_days) {
            let day = day0 + Duration::days(d);
            if t.and_then(|t| t.earliest).is_some_and(|e| day < e) {
                continue;
            }
            // first what is closed to this task: frames that keep others out
            let mut cuts = vec![];
            let mut takes = vec![];
            if let Some(t) = t.filter(|_| frames) {
                for o in self.frames_on(day) {
                    let Some(f) = self.frame(o.frame) else { continue };
                    if f.matches(t, self.prefs.default_task_min) {
                        takes.push((o, f));
                    } else if f.exclusive {
                        cuts.push((o.start, o.end));
                    }
                }
            }
            // then the frames that take it, less any closed time they share
            for (o, f) in takes {
                let asks = !f.filter.lists.is_empty() || !f.filter.tags.is_empty();
                for (s, e) in subtract(self.free_gaps_in(o.start.max(from), o.end), &cuts) {
                    let w = Window { start: s, end: e, calendar: f.calendar, spec: f.filter.specificity() };
                    if asks {
                        own.push(w);
                    } else if d < days {
                        open.push(w);
                    }
                }
            }
            if d < days && (!frames || self.prefs.plan_outside_frames) {
                for (s, e) in subtract(self.free_gaps(day, from), &cuts) {
                    outside.push(Window { start: s, end: e, calendar: task_cal, spec: 0 });
                }
            }
        }
        // earliest first; where two frames start together the more specific wins
        own.sort_by_key(|w| (w.start, std::cmp::Reverse(w.spec), w.end));
        open.sort_by_key(|w| (w.start, w.end));
        (own, open, outside)
    }

    /// The frame that keeps this task out of a stretch of time, if one does:
    /// it runs then, takes only its own kind, and this task is not that.
    pub fn frame_keeping_out(&self, t: &Task, start: NaiveDateTime, end: NaiveDateTime) -> Option<&Frame> {
        let mut day = start.date();
        while day <= end.date() {
            for o in self.frames_on(day) {
                if o.start < end && o.end > start {
                    if let Some(f) = self.frame(o.frame).filter(|f| f.exclusive && !f.matches(t, self.prefs.default_task_min)) {
                        return Some(f);
                    }
                }
            }
            day += Duration::days(1);
        }
        None
    }

    /// First place `mins` minutes long at or after `from`, looking up to
    /// PLAN_MAX_SPAN_DAYS days ahead. With a task, its frames come first.
    pub fn next_free_slot(&self, from: NaiveDateTime, mins: i64, task: Option<&Task>) -> Option<NaiveDateTime> {
        let m = from.hour() as i64 * 60 + from.minute() as i64;
        let from = from.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes((m + 14) / 15 * 15);
        let (own, open, outside) = self.task_windows(task, from.date(), from, PLAN_MAX_SPAN_DAYS, PLAN_OWN_FRAME_DAYS);
        [own, open, outside]
            .iter()
            .find_map(|ws| ws.iter().find(|w| (w.end - w.start).num_minutes() >= mins).map(|w| w.start))
    }

    /// Fits open, unscheduled tasks into matching frames, else into free
    /// working time, over the next days. Returns (sessions made, tasks left
    /// behind).

    pub fn plan_tasks(&mut self, now: NaiveDateTime) -> (usize, usize) {
        // sessions of flexible tasks that have not started may be moved
        let movable: Vec<u64> = self.tasks.iter().filter(|t| t.flexible && !t.done).map(|t| t.id).collect();
        self.events.retain(|e| !(e.start > now && e.task_id.map(|t| movable.contains(&t)).unwrap_or(false)));
        let snap = 15;
        let mins = now.hour() as i64 * 60 + now.minute() as i64;
        let from = now.date().and_hms_opt(0, 0, 0).unwrap() + Duration::minutes((mins + snap - 1) / snap * snap);
        let mut todo: Vec<Task> = self
            .tasks
            .iter()
            .filter(|t| !t.done && !self.task_is_scheduled(t.id))
            .filter(|t| t.due.is_some() || self.prefs.plan_without_due)
            .cloned()
            .collect();
        todo.sort_by_key(|t| {
            (
                t.due.unwrap_or(NaiveDate::MAX),
                if t.priority == 0 { 10 } else { t.priority },
            )
        });
        let (mut made, mut left) = (0, 0);
        for t in todo {
            let need = t.duration_min.unwrap_or(self.prefs.default_task_min);
            let can_split = self.prefs.split_long_tasks && need >= PLAN_CHUNK_FROM;
            // its own frames first, as far as two weeks ahead; then frames
            // open to any task, then free working time, over the next days
            let (own, open, outside) = self.task_windows(Some(&t), now.date(), from, PLAN_SPAN_DAYS, PLAN_OWN_FRAME_DAYS);
            let Some(placed) = fit(&own, need, can_split).or_else(|| fit(&open, need, can_split)).or_else(|| fit(&outside, need, can_split)) else {
                left += 1;
                continue;
            };
            for (s, e, cal) in placed {
                let id = self.new_id();
                self.events.push(Event {
                    id,
                    title: t.title.clone(),
                    notes: t.notes.clone(),
                    start: s,
                    end: e,
                    calendar: cal,
                    task_id: Some(t.id),
                    ..Default::default()
                });
                made += 1;
                if self.prefs.auto_due {
                    if let Some(tt) = self.task_mut(t.id) {
                        if tt.due.is_none() {
                            tt.due = Some(s.date());
                        }
                    }
                }
            }
        }
        (made, left)
    }
}

/// A stretch of time a task may be placed in, and the calendar it goes to.
#[derive(Clone, Copy, Debug)]
pub(super) struct Window {
    start: NaiveDateTime,
    end: NaiveDateTime,
    calendar: u64,
    /// how specific the frame's filter is; 0 outside frames
    spec: usize,
}

type Span = (NaiveDateTime, NaiveDateTime);

/// `gaps` with the `cuts` taken out.
pub(super) fn subtract(gaps: Vec<Span>, cuts: &[Span]) -> Vec<Span> {
    let mut out = gaps;
    for &(cs, ce) in cuts {
        out = out
            .into_iter()
            .flat_map(|(s, e)| {
                if ce <= s || cs >= e {
                    return vec![(s, e)];
                }
                let mut v = vec![];
                if cs > s {
                    v.push((s, cs));
                }
                if ce < e {
                    v.push((ce, e));
                }
                v
            })
            .collect();
    }
    out
}

/// Places `need` minutes in the windows: the first one with room for all of
/// it, else (when splitting is allowed) chunks of at least PLAN_MIN_CHUNK
/// across the windows in order, never leaving a smaller remainder.
pub(super) fn fit(windows: &[Window], need: i64, can_split: bool) -> Option<Vec<(NaiveDateTime, NaiveDateTime, u64)>> {
    if let Some(w) = windows.iter().find(|w| (w.end - w.start).num_minutes() >= need) {
        return Some(vec![(w.start, w.start + Duration::minutes(need), w.calendar)]);
    }
    if !can_split {
        return None;
    }
    let mut left = need;
    let mut placed: Vec<(NaiveDateTime, NaiveDateTime, u64)> = vec![];
    for w in windows {
        // windows of overlapping frames may share time already taken
        let taken: Vec<Span> = placed.iter().map(|p| (p.0, p.1)).collect();
        for (s, e) in subtract(vec![(w.start, w.end)], &taken) {
            let take = (e - s).num_minutes().min(left);
            if take < PLAN_MIN_CHUNK || (left - take != 0 && left - take < PLAN_MIN_CHUNK) {
                continue;
            }
            placed.push((s, s + Duration::minutes(take), w.calendar));
            left -= take;
            if left == 0 {
                return Some(placed);
            }
        }
    }
    None
}
