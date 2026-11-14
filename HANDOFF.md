# Handoff: Frames for Kalendar

Goal: add **Frames** to Kalendar, modelled on Morgen's feature of the same
name, and make the planner ("Plan my tasks") place tasks inside them instead
of anywhere in 08:00–18:00.

This document has three parts: what a Frame is (reference), how it maps onto
the current code (design), and an ordered list of work with acceptance
criteria (instructions).

---

## 1. Reference: how Frames work in Morgen

Morgen describes a Frame as a recurring block of time dedicated to one
*category* of tasks — deep work, quick wins, life admin, learning, a side
project. Frames are a template for an "ideal ordinary week": they say what
kind of work should happen when, not which specific task.

Behaviour worth copying:

| Aspect | Morgen behaviour |
|---|---|
| Creation | Click an empty spot on the calendar, choose **Frame** in the event widget (or turn a new block into a Frame). Name it after the type of task it holds. |
| Calendar | Chosen at creation; any connected calendar. Planned task sessions go to that same calendar. |
| Time / days | Set by where the block is drawn, then by its recurrence. |
| Default recurrence | Drawn on a weekday → Mon–Fri. Drawn on Sat/Sun → weekly on that day. |
| Recurrence options | Presets (daily, weekly, monthly…) plus custom patterns ("every weekday but Wednesday", "every second Thursday", "until Jan 20"). |
| Filters | "Filtered tasks": restrict which tasks are eligible, by task source, list/project and tags. Due date and priority are deliberately *not* filters — the planner already weighs them. |
| Manual use | Drag a task from the panel onto a Frame. |
| Planner use | The AI Planner fills Frames with matching tasks using the filters, its priority score and availability. Without Frames it falls back to general availability, which gives poor timing. |
| Not embedded | Moving a Frame does **not** move the task sessions placed in it. |
| Flexibility | Frames are guidance, not hard blocks; meetings can land on them and the plan adapts. |
| Visibility | The planner only fills Frames on calendars that are currently visible. |
| Long tasks | With "split long tasks" on, a task longer than a Frame is spread over several Frames; with it off, it may fit nowhere. |
| Busy/free | A Frame can be marked busy or free for booking links / team availability. |
| Preview | Proposed sessions are shown over the Frames for review before they are written to the calendar. |

Not applicable to Kalendar: integrations as a filter (all tasks are local),
busy/free for booking links (no sharing), LLM-parsed recurrence text.

Sources: morgen.so/guides/how-to-use-frames,
morgen.so/guides/plan-your-day-using-the-ai-planner, morgen.so/frames.

---

## 2. Design for Kalendar

### 2.1 Current state (what exists today)

- `src/model.rs` — `Data { events, tasks, calendars, lists, prefs, next_id }`
  saved as one JSON file. Every struct uses `#[serde(default)]` for new
  fields, so old files keep loading. Keep that rule.
- Planner: `Data::plan_tasks(now)` sorts open, unscheduled tasks by
  `(due, priority)` and places each in the first gap from `free_gaps(day,
  from)`. `free_gaps` hard-codes Mon–Fri and `WORK_START_MIN..WORK_END_MIN`.
  Long tasks (`>= PLAN_CHUNK_FROM`) may split into chunks of at least
  `PLAN_MIN_CHUNK`, using placeholder events to keep gaps correct.
  `next_free_slot` (used by reschedule / follow-up / split) also goes through
  `free_gaps`.
- Calendars are referenced by **index** (`Event.calendar: usize`);
  `remove_calendar` re-indexes events. Remote (subscribed) calendars have
  `url: Some(_)` and are read-only.
- Grid drawing: `views.rs::grid_body` paints day backgrounds → grid lines →
  collects `Item`s for events/drafts → overlap layout → draws items →
  handles drag (`Drag::{Create, Move, Resize}`) → background context menu →
  task-drop ghost (`drag_task`).
- Drawing a block opens `CreateEdit` (`dialogs.rs::dlg_create`), which turns
  the block into a task session (`as_task: true`, default) or a plain event.
- Settings: `settings.rs`, page `Page::Planner` with three rows.
- Tests: only `ics.rs` has a `#[cfg(test)]` module. `model.rs` has none.

Small things noticed on the way (fix or leave, but know about them):
- `plan_tasks` has a duplicated `t.earliest` check inside the `'days2`
  loop (harmless, badly indented).
- `free_gaps` counts events on **hidden** calendars as busy. Probably
  intended (hidden ≠ free), keep it, but the frame code should make the
  same choice consciously.

### 2.2 Data model

Frames are a **template, not events**. Do not put them in `Data.events`;
that keeps `task_is_scheduled`, `free_gaps`, overlap layout, copy/paste and
ICS untouched. Occurrences are computed on demand.

Add to `model.rs`:

```rust
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Frame {
    pub id: u64,                 // from Data::new_id()
    pub name: String,
    pub calendar: usize,         // own calendar only (url == None)
    pub color: Option<[u8; 3]>,  // None → calendar colour
    pub start_min: i64,          // minutes from midnight, snapped to 15
    pub end_min: i64,            // > start_min, <= 24*60 (no midnight crossing in v1)
    pub days: u8,                // bit 0 = Monday … bit 6 = Sunday
    pub every_weeks: u8,         // 1 = every week, 2 = every other week …
    pub from: NaiveDate,         // first possible day; anchors every_weeks
    pub until: Option<NaiveDate>,
    pub skip: Vec<NaiveDate>,    // single occurrences removed ("skip this one")
    pub filter: FrameFilter,
    /// tasks that don't match the filter are never planned into this window
    pub exclusive: bool,
    pub enabled: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FrameFilter {
    pub lists: Vec<u64>,     // empty = any list (0 is the Inbox)
    pub tags: Vec<String>,   // empty = any tags
    pub tags_all: bool,      // false: task has any of the tags; true: all of them
    pub max_min: Option<i64>,// optional "quick wins": only tasks up to N minutes
}

pub struct FrameOcc { pub frame: u64, pub start: NaiveDateTime, pub end: NaiveDateTime }
```

Add `#[serde(default)] pub frames: Vec<Frame>` to `Data`.

Rules:
- An empty filter matches every task (a plain "Focus" frame).
- A task matches if `lists` is empty or contains `task.list`, **and** the tag
  rule holds, **and** `max_min` is unset or `duration >= …` fits. Compare tags
  case-insensitively, as the panel's filter chips do (check `panel.rs`).
- `exclusive` defaults to `true` in the editor (Morgen's "personal blocks off
  limits to work tasks"); a frame with an empty filter is effectively
  non-exclusive.
- Busy/free from Morgen is dropped; `exclusive` is the local analogue.

New `Data` methods:
- `frame(id)`, `frame_mut(id)`.
- `frame_occurs_on(&Frame, day) -> bool` — enabled, `day >= from`,
  `until` not passed, weekday bit set, not in `skip`, and
  `((day - week_start(from)).num_days() / 7) % every_weeks == 0`.
- `frames_on(day) -> Vec<FrameOcc>` — only frames whose calendar is visible,
  sorted by start.
- `Frame::matches(&Task, default_min) -> bool`.
- `remove_calendar` must also drop frames on that calendar and decrement
  `calendar` on frames above it, exactly like events.

### 2.3 Planner

Refactor first, behaviour second:

1. Split `free_gaps` into a generic
   `free_gaps_in(&self, a: NaiveDateTime, b: NaiveDateTime) -> Vec<(..)>`
   (busy events between a and b, clipped) and keep `free_gaps(day, from)` as
   the working-hours wrapper calling it. No behaviour change; add a test.
2. New pref `use_frames: bool` (default `true`) and
   `plan_outside_frames: bool` (default `true`).
3. In `plan_tasks`, build for each task a list of candidate windows over
   `PLAN_SPAN_DAYS`:
   - if `use_frames` and the task matches at least one frame occurrence:
     candidates = matching occurrences, each narrowed by `free_gaps_in`;
   - else if `plan_outside_frames`: candidates = the old working-hours gaps,
     **minus** every `exclusive` frame occurrence the task does not match;
   - else: the task is left behind (counted in `left`).
   - `t.earliest` and `from` (now, rounded up to 15) still apply.
4. Pick the earliest window that fits; ties go to the frame with the most
   specific filter (more lists/tags set). Splitting works the same as today
   but walks the candidate windows instead of `free_gaps`, so long tasks
   spread across several frames.
5. Placed sessions go to the **frame's calendar** (Morgen does this), falling
   back to `task_calendar()` when planned outside frames.
6. Frames themselves never count as busy for `free_gaps_in`; they are
   targets, not appointments.
7. `next_free_slot(from, mins)` (reschedule / follow-up / split) gets an
   optional `task: Option<&Task>` so it honours the same rules; update its
   three callers in `main.rs`.

Keep the undo toast after planning as the "preview" for v1. A real preview
(ghost sessions drawn over frames with Accept / Discard) is listed under
later work.

### 2.4 Calendar view

Drawing (in `grid_body`, after grid lines and **before** event items so
events sit on top):
- for each visible day, for each `frames_on(day)` occurrence clipped to
  `grid_start_h..grid_end_h`, paint a rect `col.left()+2 .. col.right()-2`
  with `fill_squircle`, fill `mix(pal.bg, colour, 0.10)` (dark theme: try
  0.16), a 3 px left stripe in the full colour, and the frame name in
  11 pt muted text at the top-left. A frame should read as background, never
  as an event.
- the frame is paint-only; it must not take clicks away from the existing
  drag-to-create logic. Drawing a block over a frame still creates an event.

Interaction:
- Right-click on empty grid inside a frame: the existing background menu
  gets a "Frame: <name>" section with **Edit frame…**, **Skip this day**,
  **Delete frame**. Find the occurrence under the pointer from
  `ctx_time` + column.
- Right-click on empty grid outside a frame: add **New frame here** (one hour
  from the clicked slot).
- Double-click on a frame's name label → frame editor.
- Dragging a task from the panel over a frame: tint the drop ghost with the
  frame colour; if the task does not match the frame's filter, show it
  dimmed with a tooltip ("Not in this frame's filter"). Still allow the drop.
- Moving / resizing frames by dragging is v2. In v1 time changes go through
  the editor. When it is added, remember: moving a frame does not move its
  sessions.
- Month, two-week and agenda views do not show frames.

### 2.5 Creating and editing

- `CreateEdit.as_task: bool` → `kind: CreateKind { Task, Event, Frame }`
  with a three-way segmented control in `dlg_create`. Choosing **Frame**
  turns the drawn block into a new `Frame`: times from the block, `days` =
  Mon–Fri if drawn on a weekday, else that weekday only; `from` = that day;
  calendar = the block's calendar if own, else `first_own_calendar()`; then
  open the frame editor.
- New `FrameEdit { frame: Frame, is_new: bool, until_text: String, focus: bool }`
  in `main.rs`, `App.frame_editor: Option<FrameEdit>`, and
  `dialogs.rs::dlg_frame` wired into `ui_dialogs`. Fields:
  name · calendar (own calendars only) · colour (`EVENT_COLORS` menu) ·
  from / to time · weekday chips M T W T F S S · "Every N weeks" ·
  until (blank = forever) · filter: list chips, tag chips (from existing
  tags), any/all toggle, optional max duration · exclusive toggle ·
  Delete (red) · Save. Follow the look of `dlg_event` / `dlg_task`.
- Every change goes through `snapshot()` … `commit()` so undo works.
- New `Cmd::NewFrame` ("Create a new frame", key `B`, section
  "Events and tasks") added to `Cmd`, `COMMANDS`, `run` and `keys`. Check
  `B` is still free first.

### 2.6 Settings

- `Page::Planner`: add rows "Plan into frames" (`use_frames`) and
  "When no frame fits" (Plan in working hours / Leave unplanned →
  `plan_outside_frames`). Update the page's explanatory line.
- Below them, a list of all frames (colour dot, name, days, time, filter
  summary) with Edit and an enable switch. Or a separate `Page::Frames` if
  the planner page gets crowded.

### 2.7 Sample data

`Data::sample` could add two frames ("Deep work" weekdays 09:00–11:00,
empty filter; "Quick wins" weekdays 16:00–17:00, `max_min: Some(30)`) so a
first start shows the feature. Optional; skip if it clutters the screenshot
mode (`App.shot`).

---

## 3. Instructions (ordered)

Do these as separate commits; run `cargo test` and `cargo clippy` after each.

1. **Model.** Add `Frame`, `FrameFilter`, `FrameOcc`, `Data.frames`,
   lookup helpers, `frame_occurs_on`, `frames_on`, `Frame::matches`, and the
   `remove_calendar` re-indexing.
   *Accept:* an existing `data.json` without `frames` loads unchanged and
   saves with `"frames": []`.
2. **Tests module in `model.rs`.** Cover: weekday bits, `every_weeks = 2`,
   `until`, `skip`, hidden calendar excluded, filter any/all/empty,
   `max_min`, calendar removal re-index.
3. **Refactor `free_gaps` → `free_gaps_in`.** No behaviour change.
   *Accept:* a test showing `plan_tasks` output identical with no frames.
4. **Planner with frames** (2.3), plus prefs `use_frames`,
   `plan_outside_frames`, and the `next_free_slot` change.
   *Accept tests:* matching task lands in its frame on the frame's calendar;
   non-matching task avoids an exclusive frame; with
   `plan_outside_frames = false` it is left behind; a 3 h task with split on
   spreads across two 2 h frames; `earliest` respected; existing sessions
   inside a frame shrink its room.
5. **Draw frames** in `grid_body` (2.4, drawing only). Check both themes, at
   5- and 60-minute resolution, with overlapping events on top.
6. **Create / edit** (2.5): `CreateKind`, `FrameEdit`, `dlg_frame`,
   `Cmd::NewFrame`, right-click menu entries, double-click to edit, skip day,
   delete with undo.
7. **Task drag feedback** over frames (tint / mismatch dim).
8. **Settings** rows and frame list (2.6).
9. **README.** Add a "Frames" bullet under Features and update the Planner
   bullet ("fits open tasks into your frames, or free working time").

Out of scope for v1, candidates for later:
- Drag to move/resize frames; "this occurrence only" time overrides.
- Planner preview (ghost sessions over frames, Accept / Discard).
- Frames crossing midnight.
- Configurable working hours/days (currently constants) — frames partly
  replace this need.
- Showing frames in month view (a coloured dot per day).
- ICS export of frames as recurring events.

## 4. Gotchas

- Calendar references are indices; any code that stores `calendar` must be
  updated in `remove_calendar`.
- Never allow a frame on a subscribed calendar: `replace_remote` would not
  touch it, but the editor must not offer remote calendars, and if a
  calendar is converted/removed handle it there.
- `#[serde(default)]` on every new struct and field, or old data files stop
  loading and `load_or_sample` silently falls back to sample data — that
  would look like data loss to the user.
- Keep painting order: frames below events, the clock line, drafts and the
  drop ghost above.
- `plan_tasks` deletes future sessions of `flexible` tasks before replanning;
  that stays, and those tasks become eligible for frames again.
