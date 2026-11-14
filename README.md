# Kalendar

A fast desktop calendar and task planner for Linux, written in Rust with
[egui](https://github.com/emilk/egui). Everything is local: one binary, one
JSON data file, no account.

## Features

- **Calendar views**: 1–9 days, week, two weeks, month, and agenda; ISO week
  numbers; a live clock line; Ctrl+wheel zoom; Shift+wheel to pan by days.
- **Direct editing**: click or drag to create events, drag to move, drag an
  edge to resize, with 15-minute snapping and side-by-side layout of
  overlapping events.
- **Tasks**: an Inbox plus your own coloured lists, tags with filter chips,
  priority, duration, due dates (with repeats), earliest-start dates, and
  pinned Overdue / Due today / Due tomorrow / Due soon sections.
- **Time blocking**: drag a task onto the calendar to book a session; a task
  can have several sessions; reschedule, follow-up, and split actions move
  work to the next free slot.
- **Frames**: blocks of time set aside for certain kinds of work, such as
  "Deep work" on weekday mornings or "Quick wins" for tasks under 30 minutes.
  A frame repeats on chosen days and can take only tasks from certain lists,
  with certain tags, or up to a length. Make one from the + menu (`B`) or by
  right-clicking the calendar.
- **Planner**: one click fits open tasks into your frames, or free working
  time (Mon–Fri, 08:00–18:00), splitting long tasks into chunks.
- **Quick entry**: press `Ctrl+T` and type
  `Write report #deep-work !high >Work 90m .tomorrow` to set tags, priority,
  list, duration, and due date in one line.
- **Calendar subscriptions**: subscribe to read-only `.ics` feeds
  (`https://` or `webcal://`), refreshed automatically.
- **Comfort**: command bar (Ctrl+K), search, undo with a grace toast,
  light/dark themes, a task panel that can sit on the left or the right,
  and a full keyboard shortcut set (press `.`). Every shortcut can be
  changed in Settings → Shortcuts; `Ctrl+,` opens settings.

## Build and run

Requires a Rust toolchain (edition 2021).

```
cargo build --release
./target/release/kalendar
```

`cargo test` runs the unit tests.

## Data and privacy

Kalendar follows the XDG base directories:

| What | Where |
| --- | --- |
| Events, tasks, calendars | `$XDG_DATA_HOME/kalendar/data.json` (usually `~/.local/share/kalendar/data.json`) |
| Settings and shortcuts | `$XDG_CONFIG_HOME/kalendar/config.json` (usually `~/.config/kalendar/config.json`) |

Older versions kept settings inside `data.json`; they are read from there
once and then saved to `config.json`. Nothing is sent anywhere; the only
network access is fetching the `.ics` feeds you subscribe to.

Note that subscription links are stored in that file in plain text. Some
providers (for example Proton Calendar) embed the access key in the link
itself, so treat the data file as private.

A portable mode is supported: if a folder named `kalendar.home` exists next
to the binary, data and settings are kept inside it instead.

## License

GPL-2.0 — see [LICENSE](LICENSE).

The bundled [Lucide](https://lucide.dev) icon font in `assets/lucide/` is
ISC-licensed — see [assets/lucide/LICENSE](assets/lucide/LICENSE).
