<p align="center"><img src="assets/icon.png" alt="Katlendar icon: a blue cat holding a pocket watch" width="160"></p>

# Katlendar

A fast desktop calendar and task planner for Linux, Windows and macOS,
written in Rust with [egui](https://github.com/emilk/egui). Everything is
local: one binary, one JSON data file, no account.

**Katlendar is in beta.** Website: <https://kud3n013.github.io/katlendar/>

## Download

Builds for every release are on the
[releases page](https://github.com/kud3n013/katlendar/releases):

| System | File |
| --- | --- |
| Debian, Ubuntu, Mint | `katlendar_<version>_amd64.deb` |
| Other Linux (x86_64) | `katlendar-<version>-linux-x86_64.tar.gz` |
| Windows 10 and 11 | `katlendar-<version>-windows-x86_64.zip` |
| macOS 11+ (Apple silicon and Intel) | `katlendar-<version>-macos-universal.dmg` |

The Windows and macOS builds are not signed by a paid certificate yet:
Windows SmartScreen may warn about an unknown publisher ("More info" →
"Run anyway"), and on macOS right-click the app and choose Open the first
time. `SHA256SUMS.txt` on each release lists the files' checksums.

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
- **Accounts**: two-way sync with Google, Microsoft 365 / Outlook.com,
  Nextcloud, CalDAV servers and Proton. Google and Microsoft need a
  sign-in registration of your own; see
  [Connecting Google and Microsoft](docs/connect-google-and-microsoft.md).
- **Comfort**: command bar (Ctrl+K), search, undo with a grace toast,
  light/dark themes, a task panel that can sit on the left or the right,
  and a full keyboard shortcut set (press `.`). Every shortcut can be
  changed in Settings → Shortcuts; `Ctrl+,` opens settings.

## Build and run

Requires a Rust toolchain (edition 2021).

```
cargo build --release
./target/release/katlendar
```

`cargo test` runs the unit tests. `katlendar --version` prints the
version and `katlendar --check-update` asks whether a newer one is out;
`katlendar --help` lists the other options.

### Making a release

Set `version` in `Cargo.toml`, commit, then push a matching tag:

```
git tag -a v0.1.0-beta.1 -m "Katlendar 0.1.0-beta.1"
git push origin v0.1.0-beta.1
```

The [Release workflow](.github/workflows/release.yml) builds the packages
for all three systems and publishes them on a GitHub release. A tag with a
pre-release part (`-beta.1`, `-rc.1`) is published as a pre-release.

## Data and privacy

Events, tasks and calendars are kept in `katlendar/data.json` and settings
and shortcuts in `katlendar/config.json`, inside:

| System | Data | Settings |
| --- | --- | --- |
| Linux | `$XDG_DATA_HOME` (usually `~/.local/share`) | `$XDG_CONFIG_HOME` (usually `~/.config`) |
| Windows | `%APPDATA%` | `%APPDATA%` |
| macOS | `~/Library/Application Support` | `~/Library/Application Support` |

`XDG_DATA_HOME` and `XDG_CONFIG_HOME` are honoured on every system when set.

Older versions kept settings inside `data.json`; they are read from there
once and then saved to `config.json`. Nothing is sent anywhere except to the
`.ics` feeds you subscribe to and the accounts you add; sign-ins are kept
in your system's keyring (Secret Service on Linux, Credential Manager on
Windows, the Keychain on macOS).

Katlendar also asks GitHub for the list of its releases when it starts and
twice a day after, so it can tell you when a newer version is out. Nothing
about you or your data is sent. Turn it off in Settings → Info → Check for
updates.

Note that subscription links are stored in that file in plain text. Some
providers (for example Proton Calendar) embed the access key in the link
itself, so treat the data file as private.

A portable mode is supported: if a folder named `katlendar.home` exists next
to the binary, data and settings are kept inside it instead.

## License

GPL-2.0 — see [LICENSE](LICENSE).

The bundled [Lucide](https://lucide.dev) icon font in `assets/lucide/` is
ISC-licensed — see [assets/lucide/LICENSE](assets/lucide/LICENSE).
