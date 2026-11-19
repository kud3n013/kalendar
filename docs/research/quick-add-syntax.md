# Quick-add syntax in task apps, and presets for katlendar

Research notes, gathered 2026-11-16. Scope: how popular task apps read a typed line in their quick-add
field (dates, times, durations, repeats, priority, tags, lists), so that katlendar's quick task field
(`src/model/quick.rs`) can offer **presets** that feel familiar to people coming from those apps.

Citation conventions used below:

- A bare link after a claim is the **primary source** (vendor help centre, vendor guide or blog, staff
  answer) and was read for this note.
- **(secondary)** means the claim comes from a third party (blog, review, community post), not the vendor.
- **(unverified)** means it comes from memory or a search snippet only, and needs checking in the app
  before code relies on it.
- Several help centres were read through their raw HTML because the page renderer refused or truncated
  them: TickTick (articles are embedded as JSON in `help.ticktick.com/articles/…`) and ClickUp (the
  normal fetcher got HTTP 403). Images on those pages (TickTick puts its recognition tables in pictures)
  were **not** read.

---

## 0. What katlendar built from this

- Presets in Settings → Quick add: Katlendar (like Morgen, the default), Todoist (`#List`, `@tag`/`%tag`,
  `p1`–`p4`), TickTick (`#tag`, `~List`/`^List`, `!high`), Plain words (like Things: no marks), and My own.
  Changing any mark switches to My own.
- Plain dates, times and repeats are read in every preset (switchable): `tomorrow`, `fri`, `next fri`,
  `in 3 days`, `12/4` (day-first by default, switchable), `dec 3`, `at 5pm`, `@12pm`, `@24`, `5-6pm`,
  `every monday`.
- A typed time puts the task straight on the calendar as a block; a date alone sets the due date.
  This replaces the `scheduled` field §5 suggests.
- Quotes keep words in the title. Users can add their own abbreviations (`eod` → `at 17:00`).
- Not built yet: Todoist `{deadline}`, `^` not-before, `\` escape, clickable chips.

## 1. TL;DR

- **Only a few apps document an inline token language for the title field.** Todoist is by far the
  best documented. Akiflow, Sunsama and Morgen publish short symbol tables. TickTick documents date
  recognition and `#` tags, and its list/priority symbols only for Google Calendar event titles.
  Microsoft To Do and Any.do recognise dates and times only (Any.do also `#` tags).
- **Things 3, ClickUp, OmniFocus and Motion do not parse the title in quick add.** They parse
  natural language **inside the date field** instead (Things, ClickUp, OmniFocus), or not at all
  (Motion: form fields and AI chat). OmniFocus additionally imports TaskPaper text with `@due(…)` tags.
- **The same symbol means different things across apps.** `#` is a project in Todoist and Akiflow, a
  tag in TickTick and Any.do, a channel in Sunsama. `>` is a list in Morgen, a time slot in Akiflow and
  "make subtask" in Sunsama. `!` is priority in Akiflow, Sunsama, Morgen and TickTick, but a
  *reminder* in Todoist. That is the strongest argument for presets rather than one fixed grammar.
- **Every app that parses the title gives a per-word "don't parse this" escape, and none documents a
  typed escape character.** Todoist and TickTick: click the highlighted word or press Backspace/Delete.
  Microsoft To Do: Backspace on the highlight. Akiflow: `ESC`. Any.do: click the blue suggestion.
  Everyone also has a global off switch.
- **Feedback is highlighting in the title** (Todoist, TickTick blue, Any.do blue, To Do), sometimes with
  a suggestion row or chips (Things popover list, Sunsama sparkle suggestions).
- **Scheduled vs deadline is split three ways:** a *plan/start* date (Todoist date, Sunsama `@`,
  OmniFocus defer/planned, Things "When"), a *deadline* (Todoist `{…}`, Akiflow `<`, Things Deadline,
  OmniFocus due, Motion due/hard deadline), and a *time slot on the calendar* (Akiflow `>`, Morgen
  "include a time", Todoist time + `for` duration).
- **katlendar's current syntax** (`#tag`, `!high|!m|!l`, `>List`, `30m`/`1h30m`, `.today`/`.tomorrow`/
  `.2026-11-30`) is already close to Morgen's command bar (`>list`, `!`, `.` + due date). Keep it as the
  default and add Todoist, TickTick and Plain presets (§5).

---

## 2. Comparison

"—" means no inline token is documented; the attribute is set with a button, picker or field instead.

| Info kind | Todoist | TickTick | Morgen | Akiflow | Sunsama | Microsoft To Do | Any.do | Things 3 | ClickUp | OmniFocus | Motion |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Where parsing happens | Title | Title | Command bar | Command bar | Add Task modal | Title (Windows ≥2.66, iOS) | Title (Quick Add bar) | When / Deadline fields only | Date fields only (+ email subject) | Date fields; TaskPaper paste | None documented |
| Date words | `today`, `tod`, `tom`, `next fri`, `in 5 days`, `+5 days`, `27th`, `mid January`, `end of month`, `someday` … | Natural language, e.g. `Meeting at 9 tomorrow` | `.` + due date | `tomorrow`, `Monday`, `6/6`, `June 6` | `@tomorrow`, `@Friday`, `@July12`, `@next week` | `tomorrow`, `Apr 27`, `Friday` | `tomorrow`, `next week`, `Friday`, `in 2 hours`, `on January 12` | `tod`, `17d`, `jul9 + 17d`, `3rd wed jul`, `last fri jul` | `tod`, `tom`, `next week`, `2 Fridays from now`, `in 2 days no weekends` | `2d`, `-3w`, `tomorrow`, `next Thursday`, `2w sat` | — |
| Numeric dates | `27/1`, `01/27/2023`, `27/01/2023`, `2023-01-27` | (unverified) | (not documented) | `6/6` | — | `Apr 27` only shown | — | `10-3`; order follows device region | Uses the user's date-format setting; overrides when the month is impossible | System region format, e.g. `5/23/08 10a` | — |
| Time | `at 10`, `16:00`, `7pm`, `1900`, `in the morning` | `at 9`, `9pm` | `3pm` | `3pm` (secondary) | — | `5pm`, `noon` | `9am` | `2p`, `8am`, `10-3 9` | `@ 2pm`, `at`, `1530`, `lunch` | `5p`, `14:00`, `4d @ 5p` | — |
| Time puts it on the calendar? | Yes with a duration (time-blocking) | Sets a reminder time | Yes (Pro) | `>` = time slot | — (planned time, not a slot) | Sets a reminder | Sets date+time | Sets a reminder | Sets due/start time | Sets defer/due time | Auto-scheduler decides |
| Duration | `for 30m`, `for 1h15m`, `from 21:00 to 22:00`, `9:30-10:00` | — | `for 2hours` (events); `[30m]` in a synced task's title | `=` + duration | `~30min` | — | — | — | — (time estimate field) | `@estimate(2h)` (TaskPaper) | Duration field |
| Repeat | `every mon`, `every! 3 days`, `after 10 days`, `starting`/`ending` | Recognised (details in images) | — | `Every Week` | — (menu) | `daily`, `every Monday`, `daily 5pm` | — | — | — (date picker) | `@repeat-rule(FREQ=…)` | Schedule field |
| Priority | `p1`, `p2`, `p3` (p4 = none) | `!` + number or word (Google event titles) | `!` | `!` | `!` then pick | — | — | — | — (flag icon) | `@flagged` | ASAP/High/Medium/Low field |
| Tag / label | `%label` (`@label` being retired) | `#tag` | — | `*tag` | — | — | `#tag` | — (tag field) | `<tag name>` (email only) | `@tags(a, b)` | — |
| Project / list | `#Project`, `/Section` | `~List` (Google event titles) | `>List`, `/Calendar` | `#Project`, `\|Calendar` | `#channel`, `$objective`, `<folder>` | — | — (list icon) | — | — | Project: line ending in `:` | — |
| Deadline vs date | `{date}` = deadline, plain date = when you plan to work | Due date only | `.` = due; time = scheduled | `<` = deadline, `>` = time slot | `@` = start date (no due date) | Due date vs reminder | — | When vs Deadline fields | Start vs due date fields | Defer / Planned / Due | Start date, Due date + Hard deadline toggle |
| Other | `!14:00` reminder, `+name` assignee, `no date` | Remove date text from title (setting) | `;location`, `@invitee` (events) | `//` description, `@` guests | `>` merge as subtask | Remove date text from title (setting) | — | — | — | — | AI chat |
| Stop one word being parsed | Click highlight / Backspace | Click highlight / Delete | (not documented) | `ESC` | — | Backspace on highlight | Click the blue suggestion | n/a | n/a | n/a | n/a |
| Global off switch | Settings › General › Smart date recognition | Settings › General › Smart Recognition › Smart Date Parsing | — | — | Settings › AI (suggestions) | Settings › Recognize dates and times in task titles | Settings › Time detection | n/a | n/a | n/a | n/a |
| How parsed parts are shown | Highlighted words | Blue highlight | (not documented) | Symbol list under the bar | Sparkle suggestions | Highlighted words | Blue highlight + suggestion | Resolved date / suggestion list | Dropdown of matches | Field shows resolved date | n/a |

Sources for every cell are in §4.

---

## 3. Patterns worth copying

1. **Prefix sigils for things with a name, natural language for time.** Every documented grammar uses a
   one-character prefix for named objects (project, list, tag, calendar, priority) and free words for
   dates and times. Names come from a known set, so a sigil + autocomplete is unambiguous; dates are
   open-ended, so they get a parser.
2. **Ambiguous dates resolve to the future.** ClickUp: on July 10, `July 9` means next year, and a time
   that has passed means tomorrow
   (https://help.clickup.com/hc/en-us/articles/6308688267671-Natural-language-processing-for-dates-and-times).
   TickTick: an ambiguous date is "the most recent valid date and time", e.g. at 4pm `remind me at 9pm`
   is 9pm today (https://help.ticktick.com/articles/7081924556310446080). Todoist: `6pm` is today, or
   tomorrow if 6pm has passed (https://www.todoist.com/help/articles/introduction-to-dates-and-time-q7VobO).
3. **Numeric order follows a user setting, with a sanity override.** ClickUp uses the user's date-format
   preference and the typed order, but `24/10/2024` under `mm/dd/yyyy` becomes 24 October because 24
   cannot be a month (ClickUp page above). Things follows the device region
   (https://culturedcode.com/things/support/articles/9780167/); OmniFocus the system Language & Region
   format (https://support.omnigroup.com/documentation/omnifocus/universal/4.7.1/en/inspector/).
   ISO `YYYY-MM-DD` is unambiguous everywhere and Todoist accepts it.
4. **Un-recognising a word is a click or Backspace on its highlight**, not a typed escape. That works
   because the parse is live and shown in the field.
5. **Optionally strip the date words from the title.** TickTick and Microsoft To Do both make this a
   setting (TickTick article above;
   https://support.microsoft.com/en-us/office/smart-due-date-reminder-recognition-in-microsoft-to-do-b8cde14b-28da-42e1-9ce8-d75cc3717993).
   Any.do leaves them in and tells users to delete them by hand
   (https://support.any.do/en/articles/8637042-smart-type-natural-language-input). Sigil tokens are
   always removed in every app that has them.

---

## 4. Per-app findings

### 4.1 Todoist

Best-documented grammar of the set. Main pages:
quick add (https://www.todoist.com/help/articles/use-task-quick-add-in-todoist-va4Lhpzz),
dates (https://www.todoist.com/help/articles/introduction-to-dates-and-time-q7VobO),
repeats (https://www.todoist.com/help/articles/introduction-to-recurring-dates-YUYVJJAV),
duration (https://todoist.com/help/articles/set-a-task-duration-L1kYkZv8d),
deadlines (https://www.todoist.com/help/articles/introduction-to-deadlines-in-todoist-uMqbSLM6U?lang=en),
recognition switch (https://todoist.com/help/articles/turn-smart-date-recognition-on-or-off-63WfIr).

**Tokens (quick add page)**

| Token | Meaning | Example |
|---|---|---|
| natural language | date / time / repeat | `tomorrow at 4 PM`, `every other Tuesday starting March 3` |
| `{…}` | deadline | `{march 30}` |
| `%name` | label | `%email`; `@` still works but "is planned to be retired by the end of 2026" |
| `p1` `p2` `p3` | priority (p4 = default/none; the page lists only p1–p3) | `p1` |
| `!…` | reminder | `!14:00`, `!30 min before` |
| `+name` | assignee (shared projects; give `#project` first) | `+Lucile` |
| `#name` | project | `#Work` |
| `/name` | section, after the project | `#Work /Admin` |
| `for …` | duration, `h` and `m` units | `for 1h15m` |

**Dates** (dates page)

- Words: `today`/`tod`, `tomorrow`/`tom`, `next week` (next Monday by default, configurable),
  `next month`, `Friday` (next occurrence), `next Friday` (the one after), `this weekend`,
  `next weekend`, `27th`, `mid January`, `end of month`, `later this week`, `next year` (1 Jan),
  `someday`, `3rd friday jan`, `50 days before new year's eve`, `6 weeks before 21 Jul`,
  `28 days after 21 July`, `in 5 days`, `+5 days`, `in 3 weeks`, `in 2 hours`, `no date`/`no due date`.
- Numeric: `jan 27`, `27 jan`, `27/1` (day first), `01/27/2023` (month first), `27/01/2023` (day first),
  `2023/01/27`, `2023-01-27`. How an ambiguous `03/04` is resolved is not stated.
- Locale: Czech and Turkish dates "aren't supported yet"; the page does not otherwise explain locale.

**Times** (dates page): `today at 10`, `tomorrow at 16:00`, `Fri 7pm`, `6pm` (today, or tomorrow if past),
`Fri at 1900`, `in the morning`/`afternoon`/`evening` (9 AM / noon / 7 PM), `tom night` (10 PM).
The scheduler's separate Time field does not accept colon-less `1300`.

**Duration** (duration page): `for` + `h`/`m`, combinable (`for 1h15m`). Works without a date
(`Write paper for 1h`). To schedule as well, put the date and time first: `Tomorrow 13:00 for 10m`,
`Every Monday at 16:00 for 1h15m`. Ranges also work: `from 21:00 to 22:00`, `9:30-10:00`. Max 24 h.
The time-blocking page (https://todoist.com/help/articles/time-blocking-in-todoist-d6Pf1uTpc) is the
calendar use of this; not re-read here.

**Repeats** (repeats page)

- `every day`/`daily`, `every weekday`/`every workday`, `weekly`, `monthly`, `yearly`,
  `every quarter`, `every other fri`, `every 3 workday`, `every mon, fri at 20:00`, `every 2, 15, 27`,
  `every 14 jan, 14 apr`, `every last workday`, `every 1st wed jan, 3rd thu jul`, `every hour`,
  `every 12 hours starting at 9pm`.
- `every!` repeats from completion (`every! 2 months`); `after 10 days` becomes `every! 10 days`.
- `starting …`/`from …` sets the first occurrence; `ending …`/`until …`/`for 3 weeks` the last.
- One `every` per date; `every day except` and mixed per-day times are unsupported.

**Date vs deadline** (deadlines page): "dates schedule when you plan to work on a task, deadlines
represent the hard cutoff". Deadline syntax is curly brackets, e.g. `{next Friday}`. Pro/Business only.
The deadlines page says deadlines don't support recurring tasks, yet the repeats page shows
`every 2 days at 12:00 {Feb 28}`; the two pages disagree.

**Escaping and feedback**

- Recognised words are highlighted. Click the highlighted word, or press Delete/Backspace right after it
  is highlighted, and it "becomes a part of the task name" (recognition switch page; dates page).
- Global switch: Settings › General › Smart date recognition. With it off, dates and deadlines can only
  be set with the scheduler (same page).
- No typed escape character is documented.

### 4.2 TickTick

Read from the help centre's embedded article JSON (see conventions). Pages:
Add Tasks (https://help.ticktick.com/articles/7055782422935240704),
Smart Recognition (https://help.ticktick.com/articles/7081924556310446080, modified 2026-11-14),
Tags (https://help.ticktick.com/articles/7055782255804809216),
Recurring (https://help.ticktick.com/articles/7055782206349770752),
Google Calendar (https://help.ticktick.com/articles/7055781593733922816).

- **Dates and times:** "TickTick supports recognizing dates and times in tasks", example
  `Meeting at 9 tomorrow`; a recognised time sets the reminder (Add Tasks). Ambiguous entries resolve to
  the most recent valid date and time (Smart Recognition). The Smart Recognition page also covers
  repeat, early-reminder and postponed-reminder phrases, but only as screenshots, not read here.
- **Strip date text:** "You can also customise whether the date is removed from the task text or not"
  (Smart Recognition).
- **Tags:** "type # to quickly create a new tag or choose from existing tags" (Tags).
- **Priority and list:** documented only as shortcuts in **Google Calendar event titles** that sync into
  TickTick: `!1` or `!High priority`, `#Tag name`, `~List name` (the list one only when integrated with the
  "All" list) (Google Calendar page). Whether the in-app quick add accepts `!` and `~` is **unverified**.
  A third-party guide gives `^list` for the in-app quick add (secondary:
  https://curtismchale.ca/2020/08/10/ticktick-quick-add-syntax). Numeric priority meaning is unclear: the
  URL scheme uses 0 none, 1 low, 3 medium, 5 high (https://help.ticktick.com/articles/7055781515422072832,
  seen via search summary, unverified).
- **Duration:** nothing documented for quick add.
- **Repeats:** set from "Date & Repeat"; custom repeat is by due date, by completion date or specific
  dates (Recurring). Typed repeat phrases are shown only as images.
- **Escape and feedback:** recognised keywords are blue-highlighted; tap one and "it will go back to plain
  text" (https://blog.ticktick.com/2020/12/08/20-lesser-known-ticktick-features/). Pressing Delete or
  clicking the recognised time cancels it (Add Tasks). Off switch: Settings › General › Smart Recognition
  › Smart Date Parsing (Add Tasks).

### 4.3 Morgen

Command-bar guide (https://www.morgen.so/guides/create-events-and-tasks-with-command-bar):

| Token | Tasks | Events |
|---|---|---|
| `>name` | task list | — |
| `/name` | calendar | calendar |
| `!` | priority (levels not documented) | — |
| `.` + date | due date | — |
| a time, e.g. `3pm` | scheduled on the calendar (Morgen Pro) | start time |
| `for 2hours` | — | duration (default 1 h) |
| `;place` | — | location |
| `@name` | — | invitee |

- Tags, repeats, highlighting and escaping are not documented.
- **Duration tags in synced task titles:** for sources without an estimate field, add `[Xm]` or `[Xh]`
  (e.g. `[30m]`, `[2h]`) to the task's title or notes; otherwise a default from the AI Planner
  preferences is used (https://www.morgen.so/guides/troubleshooting-the-ai-planner). The changelog notes
  the duration is now displayed when such a tag is present (seen via search summary:
  https://changelog.morgen.so/rescheduling-assistant-rsvp-inbox-more-308543, unverified).
- katlendar's `>List` and `.date` already match Morgen.

### 4.4 Akiflow

Command bar article (https://product.akiflow.com/articles/6483573-command-bar):

| Token | Meaning |
|---|---|
| `>` | assign a task to a time slot (scheduled) |
| `=` | duration (task or event) |
| `#` | project |
| `*` | tags |
| `<` | deadline |
| `!` | priority |
| `\|` | calendar (for tasks this locks it and changes the event calendar) |
| `@` | event guests |
| `//` | description |
| `ESC` | remove natural-language recognition |

- Natural language examples: `6/6`, `June 6`, `Monday`, `Tomorrow`, `Every Week`.
- The special characters are listed at the bottom of the command bar as you type.
- A plain `for 1h` duration example appears only in a secondary source and a user request asks for
  `20 minutes` without `=` (https://akiflow.featurebase.app/p/better-nlp-for-task-creation, community),
  so treat `=` as the reliable form.

### 4.5 Sunsama

Creating Tasks (https://help.sunsama.com/docs/usage-guides/tasks/creating-tasks):

| Token | Meaning | Example |
|---|---|---|
| `@` | start date: the day whose list the task appears on | `@next week`, `@Friday`, `@July12`, `@backlog` (search summary) |
| `~` | planned time (estimate) | `~30min` |
| `#` | channel, pick with Enter | |
| `$` | weekly objective | |
| `!` | priority; typing a number picks that row | |
| `>` | merge into an existing task as a subtask | |
| `<` | backlog folder (backlog only) | |

- AI suggests channel, planned time and objective as you type; suggestions show a sparkle and apply on
  create; each kind can be turned off in Settings › AI.
- No due-date token (only start date). Repeats are set afterwards from the task's menu.
- No escape is documented.

### 4.6 Microsoft To Do

Smart due date, reminder recognition
(https://support.microsoft.com/en-us/office/smart-due-date-reminder-recognition-in-microsoft-to-do-b8cde14b-28da-42e1-9ce8-d75cc3717993;
announcement: https://techcommunity.microsoft.com/blog/to-doblog/smart-due-date-and-reminder-recognition-in-microsoft-to-do/3273521):

- Due date: `tomorrow`, `Apr 27`, `Friday`. Reminder (date + time): `tomorrow 5pm`, `Apr 27 10am`,
  `friday noon`. Repeat: `daily`, `every Monday`, `daily 5pm`.
- Windows app ≥ 2.66 (English only) and iOS. A 2025 Microsoft Q&A answer says web and Mac do not do it
  (https://learn.microsoft.com/en-us/answers/questions/5456901/microsoft-to-do-for-mac-natural-language-processin,
  staff/moderator status not checked).
- Recognised text is highlighted while typing and applied on save. Backspace on a highlighted word
  un-recognises it. Settings: "Recognize dates and times in task titles" and "Remove dates and times from
  task titles once recognized" (off by default, so text stays).
- No tag, list, priority or duration tokens.

### 4.7 Any.do

Smart Type (https://support.any.do/en/articles/8637042-smart-type-natural-language-input):

- Examples: `Call Rene tomorrow`, `Pay rent next week`, `Submit report Friday at 9am`,
  `Dentist in 2 hours`, `Meeting with shareholders on January 12`.
- Languages: English, French, German, Spanish, Hebrew, Portuguese, Chinese, Italian, Korean, Arabic.
- `#` picks an existing tag; `#NewTag` creates one (personal tasks).
- Recognised text is blue; click the blue suggestion below the title to drop it for this task.
  Off switch: Settings › Preferences › Time Detection (mobile), Settings › Time Settings › Time detection
  (web/desktop). The date words can remain in the title.
- Advice: several numbers in a title confuse recognition, so write them as words. (A parser lesson:
  require a date context for bare numbers.)
- No list, priority, duration or repeat tokens.

### 4.8 Things 3

- **No title parsing.** Natural language works in the **When** field (start date + reminder) and the
  **Deadline** field (https://culturedcode.com/things/support/articles/9780167/).
- Examples: `tod…`, `10-3` (Oct 3), `10-3 9`, `2p` (2 PM reminder), `17d` (`w`, `mo`, `y` also),
  `17d from jul9`, `jul9 + 17d`, `3rd wed jul`, `3rd wed jul 2026`, `last fri jul`, `tues 8am`;
  partial words are accepted and `wed jul…` offers a list of matching Wednesdays.
- Numeric order follows the device region. Languages: English, German, French, Italian, Spanish,
  Russian, Chinese, Japanese. Reminders attach only to start dates, not deadlines.
- Tags and list are set with shortcuts and pickers, which also work in Quick Entry
  (https://culturedcode.com/things/support/articles/2249437/;
  https://culturedcode.com/things/support/articles/2785159/, read via search summary).

### 4.9 ClickUp

- **No title parsing** in task creation; priority is a flag icon, tags a tag icon, dates a calendar
  icon (https://help.clickup.com/hc/en-us/articles/6309647350807-Create-a-task).
- Natural language works **in date fields**
  (https://help.clickup.com/hc/en-us/articles/6308688267671-Natural-language-processing-for-dates-and-times):
  - Relative: `Tod`/`Today`, `Tom`/`Tomorrow`, `Yesterday`, `Last Friday`, `Next week` (next Monday),
    `5 days ago`, `2 Fridays from now`, `Later` (in 2 h), `Later this week` (in 2 days),
    `This weekend`, `Next weekend`, `Next month` (1st), `Next year` (Jan 1).
  - Units: `d`/`days`, `w`/`wk`/`week`, `mo`/`month`, `y`/`yrs`/`years`, `h`/`hrs`/`hours`,
    `m`/`mins`/`minutes` (**`m` means months** when setting task start/due dates).
  - Parts of day: `Morning` 8 am, `Noon`/`lunch` 12 pm, `Afternoon` 2 pm, `Evening` 5 pm.
  - `@` or `at` for a time (`Today @ 2pm`), 4-digit 24 h `1530`, `no weekends` modifier
    (`in 2 days no weekends`), full timestamps.
  - Results default to the future; numeric order from the user's date-format preference with an
    impossible-month override (see §3).
  - Matches appear in a dropdown; Enter accepts.
- Email-to-task subject tokens
  (https://help.clickup.com/hc/en-us/articles/6309707288599-Create-tasks-and-comments-via-email):
  `<tag name>`, `<due next Friday>`, `<due mm/dd/yyyy at 2pm>`, `<assign me>`.

### 4.10 OmniFocus

- **Date fields** accept (https://support.omnigroup.com/documentation/omnifocus/universal/4.7.1/en/inspector/):
  `2 days`, `-3 weeks`, `1 year 1 month`, `2d`, `-3w`, `1h`, `1y1m`, `yesterday`, `tomorrow`, `tonight`,
  `next Thursday` (this/next/last are calendar-week based: `next Friday` is always in the following week),
  `fri`, `september`, `2028`, region-format short dates `5/23/08 10a`, mixed `2w sat`, `4d @ 5p`, `aug 6`,
  bare `now`, `9`, `14:00`, `tom`.
- Three dates: **Defer** (available from), **Planned** (scheduled for work), **Due** (must be done).
  A date without a time gets the per-field default time from settings. Estimated Duration is a text field.
- Natural-language date entry was added to iPhone/iPad/Vision (search summary of the 4.3.3 manual,
  unverified).
- **TaskPaper import** (https://support.omnigroup.com/omnifocus-taskpaper-reference, last modified
  Aug 5, 2026): project = line ending in `:`, task = line starting with `- `, tags `@defer(date)`,
  `@planned(date)`, `@due(date)`, `@estimate(2h)`, `@tags(work, online)`, `@flagged`,
  `@repeat-rule(FREQ=WEEKLY;INTERVAL=1)`, `@repeat-schedule-type(from-assigned|from-completion)`;
  date examples `2026-04-19 5pm`, `next Thursday -3d`. This is an import format, not quick add.

### 4.11 Motion

- **No inline syntax documented.** Tasks are created from `+ New` with fields for duration, deadline and
  priority (https://www.usemotion.com/help/getting-started/quickstart.md). Fields: Duration (with
  "Min chunk"), Start Date ("the earliest time the task can be started"), Due Date with a Hard Deadline
  toggle, Priority ASAP/High/Medium/Low, Schedule (https://www.usemotion.com/help/project-management/task.md).
- Plain-language creation goes through AI chat, Siri and email forwarding instead (search summaries of
  Motion help, unverified). Skipped for presets.

---

## 5. Recommended presets for katlendar

Today `parse_quick` reads whitespace-separated words and picks out sigil words; everything else is the
title. Presets only change **which sigil means what** and **which natural-language extras are on**. They
map onto the existing `Task` fields (`src/model/task.rs`): `tags`, `priority` (iCal 1/5/9),
`list`, `duration_min`, `due` (deadline), `earliest` (not before), `repeat`
(None/Daily/Weekdays/Weekly/Monthly). katlendar has no scheduled-time field, so a typed time can only
become a planner hint or be ignored until such a field exists.

All presets share the date/time phrase parser in §5.6. In every preset, a token the parser can't resolve
stays in the title (as `.foo` does today).

### 5.1 katlendar (default, current syntax, extended)

| Kind | Tokens |
|---|---|
| Tag | `#tag` |
| Priority | `!high`/`!h`/`!1`/`!`, `!medium`/`!m`/`!5`, `!low`/`!l`/`!9` (as now) |
| List | `>List` |
| Duration | `30m`, `2h`, `1h30m` (as now) |
| Due (deadline) | `.today`, `.tomorrow`, `.2026-11-30`; extend to `.fri`, `.next-fri`, `.in-3d`, `.3d`, `.30/11`, `.nov30` (hyphen joins multi-word phrases so the token stays one word) |
| Not before (`earliest`) | `^tomorrow`, `^2026-12-01` (new; `^` unused elsewhere in katlendar) |
| Repeat | `*daily`, `*weekdays`, `*weekly`, `*monthly` (new; one word each, matches `Repeat`) |
| Escape | `\#notatag` keeps `#notatag` in the title; clicking a chip under the field undoes that token |

Rationale: matches Morgen's `>list` and `.due` (§4.3) and Akiflow's `!` priority (§4.4); no free-text
date words are parsed, so a title like "Report for Friday" is never rewritten.

### 5.2 Todoist

| Kind | Tokens |
|---|---|
| Project → katlendar list | `#List` (Todoist's `/Section` ignored, left in the title) |
| Label → tag | `@tag` and `%tag` (Todoist is moving from `@` to `%`) |
| Priority | `p1` → 1 (high), `p2` → 3 (high band), `p3` → 5 (medium), `p4` → 0 (none). Todoist has four levels and katlendar three bands, so `p2` lands in "High"; alternatively map `p2` → 5 and `p3` → 9 |
| Date | free natural language anywhere in the line (§5.6): `today`, `tom`, `next fri`, `in 3 days`, `27/1`, `jan 27` |
| Time | `at 5pm`, `17:00`, `5pm`, `in the morning` |
| Duration | `for 30m`, `for 1h15m`, `from 9:30 to 10:00`, `9:30-10:00` |
| Deadline | `{next friday}`; when a deadline is given, the plain date goes to `earliest`, otherwise the plain date is `due` (Todoist's plain date is a plan date, but katlendar has only `due` as the primary date) |
| Repeat | `every day`/`daily`, `every weekday`/`every workday`, `every week`/`weekly`, `every month`/`monthly`; other `every …` forms stay in the title until `Repeat` grows |
| Not parsed | `!reminder`, `+assignee`: left in the title |
| Escape | click the highlighted phrase (chip) to return it to the title, as in Todoist |

### 5.3 TickTick

| Kind | Tokens |
|---|---|
| Tag | `#tag` |
| List | `~List` (TickTick's documented Google Calendar shortcut); also accept `^List` (third-party, see §4.2) |
| Priority | `!high`/`!medium`/`!low`/`!none`; numeric `!1` high, `!2` medium, `!3` low (katlendar's choice; TickTick's own numeric meaning is unverified) |
| Date + time | free natural language (§5.6), ambiguous → nearest future (TickTick rule) |
| Duration | none in TickTick; keep katlendar's bare `30m`/`1h30m` |
| Repeat | `daily`, `every day`, `every weekday`, `weekly`, `every week`, `monthly`, `every month` |
| Option | "Remove date text from title" toggle, matching TickTick's setting (default on) |
| Escape | Delete/Backspace right after a recognised phrase, or click its chip |

### 5.4 Things-ish / Plain

For people who don't want symbols: Things, ClickUp and OmniFocus never touch the title.

| Kind | Tokens |
|---|---|
| Tags, list, priority | none (set in the dialog) |
| Date | only a **trailing** date phrase is read, e.g. `Call mum tomorrow`, `Pay rent next fri`, `Taxes 30/11`; a date mid-title is left alone |
| Due vs not-before | trailing `by <date>` / `due <date>` → `due`; trailing `on <date>`/`<date>` → `earliest` (Things' When) unless no other date, then `due` |
| Duration | trailing `for 30m`/`for 1h` only |
| Escape | wrap the title in quotes `"Meet Friday"` to disable parsing |

### 5.5 Morgen-compatible (optional)

Same as 5.1 but `!` without a level means high, `/Calendar` is reserved (left in the title) and
`[30m]`/`[2h]` is read as duration, so tasks copied from Morgen-synced sources keep their estimates.

### 5.6 Date and time phrases the parser should support

Drawn from Todoist, ClickUp, Things, OmniFocus, Microsoft To Do and Any.do (sources in §4).

**Dates**

- Today/tomorrow/yesterday: `today`, `tod`, `tomorrow`, `tom`, `tmr`(unverified in any app),
  `yesterday`.
- Weekdays: `mon` … `sun`, full names, `tues`/`thurs`; `fri` = next occurrence (Todoist; whether today
  counts when today is Friday is not documented, so pick and test); `next fri` = the one after (Todoist) **or** Friday of next
  week (OmniFocus). Pick one and document it; Todoist's reading is the most common.
- `this weekend`, `next weekend`, `next week` (next Monday), `next month` (1st, ClickUp; Todoist keeps
  the day of month), `next year` (Jan 1), `end of month`, `mid <month>`, `later this week`.
- Offsets: `in 3 days`, `in 2 weeks`, `in 1 month`, `+3d`, `3d`, `2w`, `1mo`, `1y` (`m` stays
  minutes for durations; use `mo` for months, as Things and ClickUp's date fields do).
- Ordinals: `27th`, `3rd wed jul`, `last fri jul`, `jan 27`, `27 jan`, `january 27`, `27 january 2027`.
- Numeric: `2026-11-30` (always), `30/11`, `11/30`, `30.11.`, `30.11.2026`. Order from a setting that
  defaults to the system locale; override when only one reading is valid (ClickUp rule); dots always
  day-first.
- Bare dates resolve to the **next** such date (ClickUp, TickTick).
- `no date` clears the date (Todoist).

**Times**

- `5pm`, `5 pm`, `5p`, `5:30pm`, `17:00`, `17.00`, `1700` (4 digits only next to a date or after
  `at`/`@`), `at 5`, `@5pm`, `noon`, `midnight`.
- Parts of day: `morning` 9:00, `afternoon` 12:00–14:00, `evening` 17:00–19:00, `night` 22:00 (Todoist
  and ClickUp differ; make them settings).
- A bare time with no date means today, or tomorrow if already past (Todoist, ClickUp).

**Durations**

- `30m`, `45min`, `1h`, `1.5h`, `1h30m`, `90m`, `for 30m`, `for 1h15m`, `~30m` (Sunsama), `=30m`
  (Akiflow), `[30m]` (Morgen), ranges `9:30-10:00` and `from 21:00 to 22:00` (Todoist).

**Repeats** (limited to katlendar's `Repeat` today)

- `daily`/`every day`, `every weekday`/`every workday`/`weekdays`, `weekly`/`every week`,
  `monthly`/`every month`. Later: `every mon`, `every other week`, `every 2 weeks`, `every!` (from
  completion, Todoist).

### 5.7 Feedback and escaping in the egui field

- Colour each recognised token in the text field and show one chip per parsed attribute under it
  (date, list, tags, priority, duration), which is what Todoist, TickTick, Any.do and To Do do with
  highlighting.
- Clicking a chip, or Backspace right after a just-recognised phrase, turns it back into title text
  and remembers that span for this entry (Todoist, TickTick, To Do).
- `Esc` while the suggestion popup is open cancels recognition of the current token (Akiflow).
- Typed escape, which no surveyed app documents: a leading backslash (`\#5`, `\.net`), or quote the
  whole title.
- Settings: preset picker, "recognise dates in plain words" on/off, "remove date words from title"
  on/off (TickTick, To Do), numeric date order (auto / D/M / M/D).

---

## 6. Gaps and open questions

1. TickTick's in-app symbols for list and priority are documented only for Google Calendar titles; test
   `~`, `^` and `!` in the quick add bar before shipping that preset as "TickTick".
2. Todoist's deadline/recurrence pages contradict each other (§4.1); irrelevant for katlendar unless
   `{…}` on a repeating task needs a rule.
3. ClickUp and Motion have no title grammar, so no preset is named after them. Things gets the
   "Things-ish / Plain" preset only by analogy.
4. katlendar has no scheduled time on tasks. Todoist, Morgen and Akiflow all let a typed time put the
   task on the calendar; adding a `scheduled: Option<NaiveDateTime>` would be a model change.
5. `m` = minutes vs months: ClickUp's date fields read `m` as months; katlendar uses `m` as minutes in
   durations. Keep `m` = minutes and use `mo` for months everywhere.
