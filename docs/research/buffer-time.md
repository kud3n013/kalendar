# Buffer time for katlendar

Research notes, gathered 2026-11-20. Scope: how Morgen and Akiflow keep time free before and after
events (travel, traffic, a breather), and what katlendar takes from them.

Citation conventions used below:

- A bare link after a claim is the **primary source** (vendor docs) and was read for this note.
- **(unverified)** means it comes from a search snippet only.

---

## 1. TL;DR

- **Morgen has two workflows: Buffer Time and Automated Travel Time.** Both write real, busy events
  into a calendar. Buffers follow their event when it moves and go when it is cancelled.
- **Akiflow has only manual, per-event Travel Time.** It has no automatic buffers. Automatic
  "preparation time" is an open feature request.
- **katlendar's take:** buffers are switched on per calendar, with before and after minutes. They
  are computed, not stored: nothing is written to data.json or pushed to a server. They are drawn
  like frames, and by default the planner treats them as blocked.

## 2. Morgen

### Buffer Time workflow

https://www.morgen.so/guides/how-to-use-buffer-time-workflow

- Turned on by adding the Buffer Time workflow from the Morgen Assist gallery. After that it runs
  by itself.
- You choose **which calendars** get buffers.
- Buffers go **before or after** an event. Lengths can depend on meeting length, for example
  5 minutes after short meetings and 15 after long ones.
- Buffers are **real calendar events marked Busy**, so nothing else is booked over them.
- They are added **only in free time** and never over an existing event.
- They apply only to events with at least one other attendee.
- If the event moves, the buffer moves with it. If the event is cancelled, the buffer is deleted.
- Works with Google, Outlook, iCloud and Fastmail calendars.

### Automated Travel Time workflow

https://www.morgen.so/guides/auto-schedule-travel-time

- Settings are a target calendar, a departure place, one-way or round trip, a travel mode and
  extra padding of up to 30 minutes.
- It applies to events with an address. Times come from Google Maps.
- Travel goes before the event, and also after it on a round trip. Travel blocks are real events
  on the target calendar.
- It is skipped when another event already fills the travel window, when travel would take over
  5 hours, or when the address is the default place.
- If the event moves, travel moves with it. If the event is deleted, travel goes too.

Marketing page, which adds no mechanics: https://www.morgen.so/auto-schedule-buffer-travel

Neither guide says whether Morgen's task auto-scheduler avoids buffers. Because they are busy
events, it presumably does **(unverified)**.

## 3. Akiflow

### Travel Time

https://product.akiflow.com/en/help/articles/9727858-travel-time

- Manual only. You add it to one event at a time from the event's location row.
- Presets are 15, 30 and 45 minutes or 1 hour. You can also type 1 minute to 24 hours.
- One-way only: it always ends when the event starts. There is no "after".
- Timed events only, not all-day events.
- On Google and Outlook, Akiflow writes a separate private "Travel time" event, marked Busy, on the
  same calendar. On iCloud it uses Apple's own travel time.
- It moves with its event and goes when the event is deleted. It may overlap other events.

### Not documented

- Events management (https://product.akiflow.com/en/help/articles/0227271-events-management) and
  task planning (https://product.akiflow.com/en/help/articles/8286936-task-planning) say nothing
  about buffers or gaps.
- Automatic buffers per event type are only a feature request:
  https://akiflow.featurebase.app/p/smart-event-preparation

## 4. What katlendar does

| | Morgen | Akiflow | katlendar |
|---|---|---|---|
| Turned on | per workflow and calendar | per event, by hand | per calendar, in Settings > Planner |
| Before / after | either, length by meeting length | before only | both, minutes each |
| Stored as | real busy events | real busy event | computed when drawn and when planning; never synced |
| Over other events | never | allowed | allowed; drawn under events |
| Tasks | (unverified) | not said | kept out by default; a setting lets tasks in |

- **Which events get buffers:** timed, not skipped events on a calendar that has buffers on.
  Task sessions are excluded, because a task's buffers would push the planner around itself.
- **Drawing:** like a frame, a tinted band with a line of the event's colour on the side away from
  the event. It is only painted, so clicks and drags still reach the grid.
- **Planner:** with "Keep tasks out of buffers" on, buffer time is cut from frames and from free
  working time, just as an exclusive frame is.
- **Not done yet:** buffer length by meeting length, attendee-only buffers and travel time
  computed from an address. These would need a maps service, which katlendar avoids.
