# Task-manager integrations for katlendar

Research notes, gathered 2026-11-20. Scope: how katlendar (a Rust/egui Linux desktop calendar and task
planner, GPL-2.0, local-first, one JSON data file, no account of its own) could sync its **tasks** with
hosted task managers and issue trackers. Calendars are covered in
[calendar-integrations.md](calendar-integrations.md). GitHub is covered in depth in
[github-integration.md](github-integration.md).

Citation conventions used below:

- A bare link after a claim is the **primary source** (vendor docs, schema, crate metadata) and was read for this note.
- **(secondary)** means the claim comes from a third party (blog, forum, aggregator), not the vendor.
- **(unverified)** means it comes from memory or a search snippet only, and needs checking before code relies on it.
- Crate versions and dates come from the crates.io API (`https://crates.io/api/v1/crates/<name>`), queried 2026-11-20.
- Code references (`src/...:line`) are to the working tree on 2026-11-20, which has uncommitted edits.

---

## 1. TL;DR

- **The sync plumbing exists, but tasks only travel as iCalendar VTODO today.**
  - `PushJob::SaveTask` always goes to the iCal-only `save_task()` (`src/sync/mod.rs:244`, `:283`).
    Events have a `save_native` hook. Tasks have none.
  - `FeedTask` carries only uid, title, notes, a date, priority, done, href and etag (`src/ics.rs:843`).
  - `Data::apply_tasks` copies only those fields (`src/model/data.rs:368`).
  - So step zero for every provider is a native task path, not a new provider.
- **Start with two providers that need no new app registration: Google Tasks and Microsoft To Do.**
  - katlendar already signs in to both (`src/sync/google.rs`, `src/sync/graph.rs`). Each needs one more
    OAuth scope and a JSON client.
  - Microsoft To Do has a real delta query, with start date, recurrence and categories:
    https://learn.microsoft.com/en-us/graph/api/todotask-delta?view=graph-rest-1.0
  - Google Tasks keeps only the date of `due`: "the time portion of the timestamp is discarded".
    It has `updatedMin` and `showDeleted` for incremental pulls:
    https://developers.google.com/workspace/tasks/reference/rest/v1/tasks
- **Todoist is the best third-party target.**
  - It has one unified API v1 with an incremental `sync_token`. It has dates, floating times, durations,
    deadlines, labels and recurrence: https://developer.todoist.com/api/v1/sync/overview.md
  - Personal tokens work today. Its OAuth also supports Dynamic Client Registration for public PKCE
    clients, so no client secret ships in the repo: https://developer.todoist.com/api/v1/authorization.md
- **Jira, Linear, ClickUp, GitLab and GitHub are "work assigned to me", not a to-do list.**
  - Pull a read-mostly filter (JQL, GraphQL filter, `assigned_to_me`) into a read-only katlendar list.
    The planner can then schedule time for that work.
  - Map "done" to a workflow transition only when the user picks one. Never send a blind `done = true`.
- **Use personal tokens first, OAuth second.**
  - Six providers need a client secret for OAuth: Jira 3LO, ClickUp, TickTick, Asana, Notion and Basecamp.
    A GPL desktop app cannot keep that secret.
  - All of them except Basecamp offer a personal token the user pastes. That matches the
    "bring your own client ID" route in calendar-integrations.md.
- **Stay blocking: hand-written ureq 3 on worker threads.**
  - Every vendor crate found is async (reqwest plus tokio), unofficial, or stale (section 6).
  - Each provider needs only 4 to 8 endpoints.
- **Webhooks are useless here.**
  - Todoist, ClickUp, Jira and the rest need a public HTTPS endpoint. A desktop app has none.
  - Poll with each provider's change cursor instead.
- **Out of scope:**
  - Things 3 and OmniFocus run only on Apple platforms and have no web API.
  - Any.do says it has no public REST API: https://www.any.do/developers
- **Vikunja needs no new code.** It already speaks CalDAV VTODO, which `src/sync/caldav.rs` handles:
  https://vikunja.io/docs/caldav/

### Phased plan

| Phase | What | katlendar files touched |
|---|---|---|
| 0 | `TaskProvider` trait. Native task push hook. Richer `FeedTask` (call it `RemoteTask`). Account `kind` enum. Account-wide task cursor. | `src/sync/mod.rs`, `src/ics.rs`, `src/model/task.rs`, `src/model/calendar.rs`, `src/model/data.rs`, `src/syncing/run.rs`, `src/syncing/take.rs`, `src/syncing/mod.rs` |
| 1 | Microsoft To Do over Graph. Google Tasks over its REST API. Both reuse the existing sign-in. | `src/sync/graph.rs` (or new `src/sync/mstodo.rs`), new `src/sync/gtasks.rs`, `src/sync/google.rs` (scope), `src/settings.rs` |
| 2 | Todoist with a pasted token, over the Sync endpoint | new `src/sync/todoist.rs`, `src/settings.rs` (account kinds near line 1040), `src/secrets.rs` (no change to the key scheme) |
| 3 | "Assigned to me" read-only lists: Jira Cloud (API token), Linear (API key), then GitLab. GitHub follows the other doc. | new `src/sync/jira.rs`, `src/sync/linear.rs`, `src/sync/gitlab.rs`, `src/settings.rs`, `src/views.rs` (link out, status chip) |
| 4 | TickTick and ClickUp with pasted tokens | new `src/sync/ticktick.rs`, `src/sync/clickup.rs` |
| 5 | OAuth where it needs no secret: Todoist (DCR or client metadata document), Linear (PKCE public client), GitLab (PKCE) | `src/sync/oauth.rs` (fixed port, JSON body, HTTP Basic), `src/settings.rs` |
| 6 | On demand: Asana, Trello, Notion, monday.com, Basecamp, Taskwarrior | one file each under `src/sync/` |

---

## 2. Comparison

"Token" means a personal token the user creates in the provider's settings and pastes into katlendar.

| Provider | API | Simplest auth for katlendar | OAuth without a secret? | Incremental pull | Due granularity | Priority | Recurrence via API | Rate limit (headline) | CalDAV/ICS | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| Microsoft To Do | Graph v1.0 REST | existing Microsoft sign-in + `Tasks.ReadWrite` | yes (existing public client) | delta query | date-time + zone | low/normal/high | yes (`patternedRecurrence`) | Graph limits (unverified) | no | **Phase 1** |
| Google Tasks | Tasks v1 REST | existing Google sign-in + `auth/tasks` | as for Calendar | `updatedMin` + `showDeleted` | date only | none | none | 50,000 queries/day | no | **Phase 1** |
| Todoist | API v1 (Sync + REST) | token | yes (DCR, public PKCE) | `sync_token` | date, floating or fixed time | 1–4 | yes (natural-language string) | 1000 partial syncs / 15 min | no | **Phase 2** |
| Jira Cloud | REST v3 + JQL | email + API token | no (3LO needs secret) | JQL `updated >=` | date | per-scheme names | no | points quota (3LO); tokens exempt | no | **Phase 3**, read-mostly |
| Linear | GraphQL | API key | yes (PKCE, secret optional) | `updatedAt` filter | date | 0–4 | no | 2,500 req/h (key) | no | **Phase 3**, read-mostly |
| GitHub | REST + GraphQL | see github-integration.md | see that doc | see that doc | Projects field | none | no | see that doc | no | see that doc |
| GitLab | REST v4 | PAT | yes (PKCE) | `updated_after` | date | none (labels) | no | 2,000 req/min (GitLab.com) | no | Phase 3, read-mostly |
| TickTick | Open API v1 | token | no (secret, Basic) | none found; full poll | date-time + zone, all-day flag | 0/1/3/5 | yes (RRULE) | not documented | no | Phase 4 |
| ClickUp | REST v2 | token | no (secret) | `date_updated_gt` | ms + "has time" flag | 1–4 | not in API schema | 100 req/min (lower plans) | no | Phase 4 |
| Asana | REST 1.0 | PAT | no (secret required) | events `sync` token | date or date-time | none (custom field) | no | 150/min free | no | Phase 6 |
| Trello | REST 1 | key + token | public clients only for Power-Ups | full board poll | date-time | none | no | 100 req/10 s per token | no | Phase 6 |
| Notion | REST, dated versions | PAT (one workspace) | no (secret, Basic) | `last_edited_time` filter | user schema | user schema | user schema | ~3 req/s | no | Phase 6, needs mapping UI |
| monday.com | GraphQL, quarterly versions | personal token | new OAuth 2.1 (unverified details) | activity logs (unverified) | board column | board column | no | 10M complexity/min (personal) | no | Phase 6 |
| Basecamp | REST (bc3-api) | OAuth only | no (secret) | ETag / `If-None-Match` | date | none | no | 429 + Retry-After | no | Phase 6 |
| Vikunja | REST + CalDAV | CalDAV token | n/a | CalDAV sync | DTSTART/DUE | PRIORITY | server to client only | per instance | **yes, CalDAV** | works today |
| Taskwarrior | TaskChampion replica protocol | sync server + secret | n/a | operation log | date-time | H/M/L (unverified) | yes (unverified) | n/a | no | Phase 6 |
| Things 3 | URL scheme only | — | — | — | — | — | — | — | no | out of scope |
| OmniFocus | Omni Automation (in-app JS) | — | — | — | — | — | — | — | no | out of scope |
| Any.do | MCP server only | — | — | — | — | — | — | — | no | out of scope |

---

## 3. What the code supports today

These facts come from reading the katlendar source, not from vendor docs.

- **`Provider` trait** (`src/sync/mod.rs:85`).
  - It is calendar-shaped: `list_calendars`, `pull`, `pull_tasks` (defaults to an error), `save_native`
    for events, and the CalDAV-shaped `get(href)`, `put(cal, href, ics, guard)`, `delete(cal, href, etag)`.
  - `RemoteCalendar` has `tasks` and `events` flags. `src/syncing/take.rs:341` makes a katlendar
    `TaskList` for each collection that holds only tasks, and for a mixed one only when the user asks.
- **Push path** (`src/sync/mod.rs:276`).
  - `PushJob::Save(event)` first tries `save_native`, then falls back to iCalendar.
  - `PushJob::SaveTask(task)` always calls `save_task`. That function writes a VTODO and makes the href
    `"{cal.id}/{uid}.ics"` (`src/sync/mod.rs:244`). A JSON provider cannot use it.
- **Pull path** (`src/syncing/run.rs:117`).
  - It calls `pull_tasks` once **per list**, with that list's `sync_state`.
  - Todoist's `sync_token` covers the whole account, so per-list cursors do not fit it.
- **Fields that cross the wire.**
  - `FeedTask` carries `uid, title, notes, due: Option<NaiveDate>, priority (1 = highest), done, href, etag`
    (`src/ics.rs:843`).
  - `apply_tasks` copies exactly these and skips dirty tasks (`src/model/data.rs:368`).
  - `parse_todos` drops repeating VTODOs on purpose: "ticking one off means something else on every server".
- **Local `Task` fields with no remote path yet:**
  - `duration_min`, `flexible`, `tags`, `earliest`, `repeat` (`Daily`, `Weekdays`, `Weekly`, `Monthly`,
    `Custom(weekday bits)`), `color`, `archived` (`src/model/task.rs:9`).
- **Accounts.**
  - `Account` has one bool per kind: `google`, `microsoft`, `nextcloud`, `proton` (`src/model/calendar.rs:53`).
    `open_account` branches on them (`src/syncing/mod.rs:59`). That will not scale to ten more kinds.
  - Secrets go to the keyring as `account:{id}` (`src/secrets.rs`).
- **OAuth helper** (`src/sync/oauth.rs`).
  - It always uses PKCE S256 and `state`.
  - The redirect is `http://127.0.0.1:{ephemeral}` or `http://localhost:{port}`, with no path.
  - The token request is form-encoded. The secret travels only as `client_secret` in the form body.
  - Missing pieces, relevant below:
    - a fixed port or a path on the redirect;
    - a JSON-body token request (ClickUp's reference shows JSON);
    - `client_secret_basic` (TickTick, Notion).
- **No tokio** in `Cargo.toml`. Sync runs on `crate::worker::spawn` threads with an mpsc channel.

---

## 4. Per-provider findings

### 4.1 Todoist

**API.**
- One unified **API v1** replaces Sync v9 and REST v2.
- IDs are now opaque strings, so old numeric IDs are rejected. List endpoints use cursor pagination.
  Sync v9's completed/archive endpoints became `/api/v1/tasks/completed/by_completion_date`:
  https://developer.todoist.com/api/v1/migrating-from-v9.md
- Pagination: https://developer.todoist.com/api/v1/pagination.md
- The Sync endpoint is `POST https://api.todoist.com/api/v1/sync`, form-encoded:
  https://developer.todoist.com/api/v1/sync/overview.md
  - `sync_token=*` gives a full sync. The returned token gives the next incremental sync.
  - Writes are `commands`, each with a client `uuid` and, for creates, a `temp_id`.
  - The reply has `sync_status` per uuid and a `temp_id_mapping`.
  - The token covers the **whole account**, not one project.

**Auth.**
- Personal token: copied from Todoist settings, sent as `Authorization: Bearer`:
  https://developer.todoist.com/api/v1/authorization.md
- OAuth (same page):
  - Authorize at `https://app.todoist.com/oauth/authorize`. Scopes are comma-separated, e.g.
    `data:read_write,data:delete`.
  - Token at `https://api.todoist.com/oauth/access_token`.
  - New apps get `expires_in` 3600 and a rotating refresh token, with a 60-second grace period.
- **Dynamic Client Registration** (same page):
  - `POST https://api.todoist.com/oauth/register` with `token_endpoint_auth_method: none` creates a public
    PKCE client.
  - Redirect URIs must be HTTPS, but "localhost is allowed for testing".
  - A Client ID Metadata Document flow is described for desktop public clients.
  - So katlendar could register itself per install, or host one metadata document, and ship **no** secret.
- `oauth.rs` reuse: yes for PKCE plus form token exchange. Check that the loopback form it uses
  (`http://127.0.0.1:{port}`) is accepted, given the "localhost" wording (unverified).

**Mapping onto `Task`.**
- `content` → `title`. `description` (Markdown) → `notes`.
- Due dates come in three kinds: https://developer.todoist.com/api/v1/due-dates.md
  - full-day `YYYY-MM-DD`;
  - floating `YYYY-MM-DDTHH:MM:SS`;
  - fixed UTC with `Z` plus a timezone.
  - The `due` object also has `string`, `lang` and `is_recurring`.
- Lossy fields:
  - katlendar keeps only a date, so the time part of a due is lost unless round-tripped (section 7.3).
  - Recurrence is the natural-language `due.string` ("every weekday"). Setting `date` alone cannot create
    a recurring due (due-dates.md).
    - Map katlendar `Repeat` to English strings on write: `every day`, `every weekday`, `every week`,
      `every month`, `every mon, wed`.
    - On read, recognise only those strings. Show anything else as read-only "repeats (Todoist)".
- `deadline {date}` is date-only and never recurs. It separates "when to work on it" (due) from
  "must be done by" (deadline): https://developer.todoist.com/api/v1/deadlines.md
  - Best mapping when a deadline is present: Todoist `due` → katlendar `earliest`, `deadline` → `due`.
  - Otherwise Todoist `due` → `due`.
- `duration {amount, unit}` → `duration_min`, where unit is minute or day.
- `priority` is 1–4, with 4 meaning "very urgent" (sync/tasks.md). Mapping in section 7.4.
- `labels` (names) → `tags`. `project_id` → list. `section_id` is lost.
- `parent_id`: flatten subtasks and keep the parent id (section 7.2).
- `checked` → `done`.

**Incremental sync.**
- Poll `/api/v1/sync` with `resource_types=["items","projects"]` and the stored `sync_token`.
- Webhooks are HTTPS-only, allow no custom ports, and fire only after the user authorises the app via
  OAuth. They are unusable from a desktop app: https://developer.todoist.com/api/v1/webhooks.md

**Write-back.** https://developer.todoist.com/api/v1/sync/tasks.md
- Create, edit, move and delete: `item_add`, `item_update` (cannot complete), `item_move`, `item_delete`.
- Completion:
  - `item_complete` completes the task and its subtasks and archives them. `item_uncomplete` reverses it.
  - For recurring tasks, use `item_close`, which advances the date, or `item_update_date_complete`.
- There is no ETag. Concurrency is last-writer-wins per command. Send up to 100 commands per request.

**Rate limits.** https://developer.todoist.com/api/v1/request-limits.md
- 1000 partial syncs and 100 full syncs per 15 minutes per user.
- 1 MiB body. 15 s timeout.

**Terms.** https://developer.todoist.com/api/v1/introduction.md
- "Todoist" must not be the app's primary name.
- The app must say it is not created by, affiliated with, or supported by Todoist.
- Follow the brand guidelines and Terms of Service. A settings-page line covers this.

**Rust crate.**
- `todoist-api` 1.0.0-alpha.4 (2026-09-21), MIT, reqwest-based.
- Hand-written ureq is better: one endpoint plus a JSON command list.

**CalDAV/ICS.** No CalDAV route (none mentioned in the v1 docs; unverified beyond that).

### 4.2 ClickUp

**API.** REST **v2**, base `https://api.clickup.com/api`: https://developer.clickup.com/reference/gettasks.md

**Auth.** https://developer.clickup.com/docs/authentication.md
- Personal token:
  - It starts with `pk_` and is sent as `Authorization: {token}`, with no `Bearer`.
  - It does not expire.
- OAuth:
  - Authorize at `https://app.clickup.com/api?client_id&redirect_uri&state`.
  - Token at `POST https://api.clickup.com/api/v2/oauth/token` with `client_id`, `client_secret` and `code`.
  - The reference shows a JSON body: https://developer.clickup.com/reference/getaccesstoken.md
    Whether a form body also works is (unverified).
  - The access token "currently does not expire". There is no refresh token. PKCE is not mentioned.
  - Only Workspace owners or admins can create OAuth apps.
  - "Non-SSL redirect URIs may not be supported in the future."
- Verdict: personal token only. OAuth needs a secret, a JSON token request, and an admin to register.

**Mapping.**
- List tasks: `GET /v2/list/{list_id}/task` (gettasks.md).
  - 100 tasks per page, `page` from 0.
  - Filters: `date_updated_gt` (ms), `include_closed`, `subtasks`, `include_markdown_description`,
    `assignees[]`, `order_by=updated`.
  - Tasks whose home is another list appear only with `include_timl`.
- Fields:
  - `name` → title. `markdown_description` → notes.
  - `due_date` (ms) plus `due_date_time` (bool: has a time) → due. `start_date` plus `start_date_time`
    → earliest. https://developer.clickup.com/reference/updatetask.md
  - `time_estimate` (ms) → `duration_min`. `tags[].name` → tags.
  - `priority {id, priority, color}` → priority. The example uses 3 = normal. The full scale
    (1 urgent, 2 high, 3 normal, 4 low) is (unverified).
  - `status {status, type}` → done when the status type is closed (unverified detail). `parent` → subtask.
- Recurrence does not appear in the create or update schema, so treat it as not available
  (https://developer.clickup.com/reference/createtask.md, by absence).
- Lossy: custom statuses, custom fields, multiple assignees, checklists.

**Incremental sync.**
- Poll `date_updated_gt` per list. For "assigned to me", use `GET /v2/team/{team_id}/task` with
  `assignees[]`: https://developer.clickup.com/reference/getfilteredteamtasks.md
- Deleted tasks are not reported. Run a periodic full listing to find them.
- Webhooks need a public URL: https://developer.clickup.com/docs/webhooks.md

**Write-back.**
- `PUT /v2/task/{task_id}` takes name, description or `markdown_content`, status, priority, due/start
  with their time flags, `time_estimate`, `parent` and `archived` (updatetask.md).
- Tags are not in the update body. Use the separate add/remove tag endpoints.
- `POST /v2/list/{list_id}/task` creates a task and accepts `tags[]` (createtask.md).
- No ETag or If-Match. Last writer wins.

**Rate limits.** Per token: https://developer.clickup.com/docs/rate-limits.md

| Plan | Requests per minute |
|---|---|
| Free Forever, Unlimited, Business | 100 |
| Business Plus | 1,000 |
| Enterprise | 10,000 |

A 429 carries `X-RateLimit-Limit`, `X-RateLimit-Remaining` and `X-RateLimit-Reset`.

**Rust crate.** `clickup` 0.3.0 (2025-11-17), MIT, reqwest plus tokio. Use ureq instead.

**CalDAV/ICS.** None found (unverified).

### 4.3 Jira (Cloud, with Data Center notes)

**API.**
- Cloud REST v3. Base for OAuth apps: `https://api.atlassian.com/ex/jira/{cloudid}`. Base for API tokens:
  the site URL, e.g. `https://example.atlassian.net`.
  https://developer.atlassian.com/cloud/jira/platform/oauth-2-3lo-apps/
- **Deprecation:** the old `/rest/api/3/search` has been removed. Use `/rest/api/3/search/jql`, which pages
  with `nextPageToken`:
  https://confluence.atlassian.com/jirakb/run-jql-search-query-using-jira-cloud-rest-api-1289424308.html
  - Reference: https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-search/
- v3 descriptions are **Atlassian Document Format** (JSON), not text:
  https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/

**Auth.**
- Basic auth with `email:api_token` is for scripts and personal use:
  https://developer.atlassian.com/cloud/jira/platform/basic-auth-for-rest-apis/
  - The page also discourages apps from collecting users' API tokens. The exact wording should be
    re-read before shipping (unverified wording).
  - katlendar asks the user to create and paste their own token. That is the same pattern as an IMAP app
    password, but it is a policy risk to note in settings.
- OAuth 2.0 (3LO) (oauth-2-3lo-apps page):
  - Authorize at `https://auth.atlassian.com/authorize?audience=api.atlassian.com&...`. Token at
    `https://auth.atlassian.com/oauth/token`.
  - Then call `accessible-resources` to find the `cloudid`.
  - A **client secret is required**. PKCE is not mentioned.
  - `offline_access` gives rotating refresh tokens. They expire after 90 days of inactivity.
  - Apps are private until distribution is enabled. The page warns against asking customers to create
    their own 3LO apps.
- `oauth.rs` reuse: possible with a client secret. But a GPL app cannot keep a secret, so 3LO is not
  recommended.

**Mapping.**
- `summary` → title. `description` (ADF) → notes: render ADF to plain text on read, and never write
  description back.
- `duedate` is `YYYY-MM-DD` (examples in
  https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/) → due.
- `priority` is a named value from the site's priority scheme. Map by name (section 7.4).
- `labels` → tags.
- `timetracking.originalEstimate` → `duration_min`. The field name is real, but its write behaviour is
  (unverified).
- `status` + `statusCategory` → done when the category is Done (unverified detail).
- Sub-tasks and parent → flatten.
- Start date is a per-site custom field (unverified). Sprint, story points and components are lost.
- The planner owns `earliest`, `flexible` and `duration_min` locally.

**Incremental sync.**
- JQL such as `assignee = currentUser() AND statusCategory != Done AND updated >= "<last>"`.
  The exact date format and minute precision are (unverified).
- Re-run a full query every few hours to drop issues that were unassigned or deleted.
- Webhooks need a public endpoint (unverified for Cloud apps without Connect/Forge).

**Write-back.**
- Editing an issue does **not** transition it. "To transition an issue, please use Transition issue"
  (api-group-issues).
  - `GET /rest/api/3/issue/{key}/transitions` lists the moves allowed from the current status.
  - `POST` to the same path performs one.
- So "done" in katlendar means "perform the transition the user chose for this account".
- Editing `duedate`, `labels` and `priority` goes through `PUT /rest/api/3/issue/{key}`.
  It sends notifications unless `notifyUsers=false`, which needs admin rights (api-group-issues).
  Default to read-only plus "done via transition".

**Rate limits.** https://developer.atlassian.com/cloud/jira/platform/rate-limiting/
- Points-based quota of 65,000 points per hour, enforced for 3LO apps from 2026-03-02.
- Bursts: 100 GET/s and 50 PUT/s. Writes to one issue: 20 per 2 s.
- "API token based traffic is not affected" by the points quota.

**Data Center / Server.**
- Personal access tokens are sent as `Authorization: Bearer`. They need Jira 8.14 or later and can be
  set to expire:
  https://confluence.atlassian.com/enterprise/using-personal-access-tokens-1026032365.html
- DC uses REST v2 with wiki-markup descriptions and the older `/rest/api/2/search` (unverified).
  Plan a v2 code path behind the same trait.

**Rust crates.**
- `jira` 0.4.0 (2026-10-24), MIT, reqwest plus tokio.
- `autumn-jira` 0.2.0 (2026-11-06), Apache-2.0, tokio.
- Hand-write instead: about four endpoints (search/jql, issue, transitions, myself).

**CalDAV/ICS.** None in core Jira. Some marketplace apps export iCal (unverified).

### 4.4 Linear

**API.**
- GraphQL at `https://api.linear.app/graphql`: https://linear.app/developers/graphql.md
- It is unversioned. Fields are retired through `@deprecated`: https://linear.app/developers/deprecations.md

**Auth.**
- Personal API key, sent as `Authorization: <API_KEY>` with no Bearer. It is created under Security &
  access settings (graphql.md).
- OAuth: https://linear.app/developers/oauth-2-0-authentication.md
  - Authorize at `https://linear.app/oauth/authorize`. Scopes are comma-separated (`read,write`).
    PKCE is `plain` or `S256`.
  - Token at `POST https://api.linear.app/oauth/token`, form-encoded.
  - With PKCE the `client_secret` is optional, including on refresh. So a public client works.
  - Since 2026-04-01 all apps use refresh tokens. Access tokens last 24 h by default.
  - Redirect URLs are registered per app. The example is `http://localhost:3000/oauth/callback`.
    Whether any loopback port is accepted is (unverified). If not, `oauth.rs` needs a fixed port and a path.

**Mapping.** Schema: https://raw.githubusercontent.com/linear/linear/master/packages/sdk/src/schema.graphql
- `Issue.title`. `description` (Markdown) → notes.
- `dueDate: TimelessDate` (`YYYY-MM-DD`) → due.
- `priority`: 0 none, 1 urgent, 2 high, 3 medium, 4 low.
- `labels` → tags (IDs, resolve names).
- `state: WorkflowState` (its `type` is `completed`, `canceled` and so on) → done.
- `parent` → flatten. `team` / `project` → list. `url` → link.
- `estimate: Float` is story points, **not** time. Do not map it to `duration_min`.
- `startedAt` is when work began, not a planned start. Do not map it to `earliest`.

**Incremental sync.**
- Filter issues with comparators (https://linear.app/developers/filtering.md), e.g.
  `assignee: {isMe: {eq: true}}`, plus an `updatedAt` filter.
- `updatedAt: {gt: ...}` is assumed to work like the other date comparators (unverified).
- Archived and trashed issues need `includeArchived` (unverified).

**Write-back.**
- `issueUpdate(id, input)` with `IssueUpdateInput`: `title`, `description`, `dueDate`, `priority`,
  `stateId`, `addedLabelIds`, `removedLabelIds` (schema).
- "Done" means setting `stateId` to the team's first `completed`-type state.

**Rate limits.** https://linear.app/developers/rate-limiting.md

| Auth | Requests per hour | Complexity points per hour |
|---|---|---|
| API key | 2,500 | 3,000,000 |
| OAuth app | 5,000 | 2,000,000 |

- Leaky bucket. Headers are `X-RateLimit-Requests-*`.
- Pages default to 50 items.

**Rust crates.**
- `lineark` 5.0.0 (2026-11-17), reqwest plus tokio.
- `linear-api` 0.1.0 (2026-07-06).
- Hand-write one query and one mutation as JSON strings.

**CalDAV/ICS.** None (unverified).

### 4.5 GitHub Issues and Projects

GitHub has its own in-depth note: [github-integration.md](github-integration.md).
- Here it is one more "assigned to me" source (section 5). Issues have no due date.
- Projects (v2) custom date and iteration fields are the nearest equivalent.
- Use that note for auth, endpoints and limits. Do not duplicate them here.

### 4.6 GitLab

**API.** REST v4 Issues API: https://docs.gitlab.com/api/issues/
- Filters: `scope=assigned_to_me`, `assignee_username`, `updated_after` (ISO 8601), `due_date`,
  `order_by=updated_at`, `issue_type`.
- Fields: `due_date`, `labels`, `state`, `web_url`.
- Close or reopen with `PUT /projects/:id/issues/:issue_iid` and `state_event=close|reopen` (same page).
- Issue updates are limited to 1,000 per minute per user by default (same page).

**Auth.**
- PAT for self-hosted and GitLab.com.
- OAuth authorization code with PKCE does not need the client secret. `expires_in` is 7200 and comes with
  a refresh token: https://docs.gitlab.com/api/oauth2/
- Each self-hosted instance needs its own app registration. Loopback redirect rules are (unverified).

**Rate limits.** GitLab.com: 2,000 authenticated API requests per minute per user:
https://docs.gitlab.com/user/gitlab_com/rate_limits/

**Mapping.**
- `due_date` → due. `labels` → tags. `state` closed → done.
- No priority field: optionally map labels such as `priority::high` (convention, unverified).
- Weight and time estimate are lossy.

**Rust crate.** `gitlab` 0.1905.0, MIT/Apache. It is large and pulls in many types. Hand-write instead.

### 4.7 Microsoft To Do (and Planner)

**API.** Microsoft Graph v1.0:
- `/me/todo/lists`
- `/me/todo/lists/{id}/tasks`
- `/me/todo/lists/{id}/tasks/delta`

https://learn.microsoft.com/en-us/graph/api/todotask-delta?view=graph-rest-1.0

**Auth.**
- Delegated `Tasks.Read` or `Tasks.ReadWrite`, for work or school **and** personal Microsoft accounts
  (delta page, permissions table).
- katlendar's Graph app asks for `Calendars.ReadWrite offline_access` (`src/sync/graph.rs:33`). Add
  `Tasks.ReadWrite`.
  - The user must consent again.
  - The Entra registration can stay as it is, if the app is configured for these delegated permissions
    (unverified).

**Mapping.** https://learn.microsoft.com/en-us/graph/api/resources/todotask?view=graph-rest-1.0
- `title`.
- `body` (`itemBody`, HTML or text) → notes. Request text where possible: the `Prefer` content-type header
  is (unverified) for To Do.
- `dueDateTime` (`dateTimeTimeZone`) → due. Keep the date, round-trip the time.
- `startDateTime` → earliest. `reminderDateTime` is lossy.
- `importance` is low, normal or high (section 7.4).
- `status` is `notStarted`, `inProgress`, `completed`, `waitingOnOthers` or `deferred`.
  `completed` → done. The rest are lossy.
- `categories` (Outlook category names) → tags.
- `recurrence` (`patternedRecurrence`) → `Repeat`. Daily, weekly with days, and absolute monthly all fit.
- `checklistItems` → flatten or put in notes. `linkedResources` is lossy.

**Incremental sync.**
- Delta returns `@odata.nextLink` pages, then an `@odata.deltaLink` whose `$deltatoken` is the next cursor
  (delta page). Store the whole `deltaLink` in `TaskList.sync_state`.
- Deletions arrive as `@removed` entries (Graph convention; unverified for todoTask).

**Write-back.**
- `PATCH /me/todo/lists/{id}/tasks/{taskId}`, `POST` to create, `DELETE` to remove (todoTask resource page).
- `graph.rs` already has `call(method, url, body, if_match)`. Whether todoTask honours `If-Match` with
  `@odata.etag` is (unverified).

**Rate limits.** Graph-wide throttling applies. No To Do-specific figure was found (unverified).

**Planner.**
- Planner tasks are a separate Graph API (`/planner/tasks`). They have due dates, percent complete and
  priority, and need `Tasks.ReadWrite` (unverified; not fetched).
- Treat Planner as "assigned to me" (section 5) after To Do.

**Rust crate.**
- `graph-rs-sdk` 3.0.1 (2025-04-20), async.
- Reuse the hand-written `graph.rs` instead.

**CalDAV/ICS.** No (Exchange Online has no CalDAV; unverified).

### 4.8 Google Tasks

**API.**
- `https://tasks.googleapis.com/tasks/v1/lists/{tasklist}/tasks`:
  https://developers.google.com/workspace/tasks/reference/rest/v1/tasks/list
- `maxResults` is 20 by default, 100 at most.
- Flags: `showCompleted`, `showDeleted` and `showHidden`. Tasks completed in Google's own apps appear
  only with `showHidden=true`.
- `updatedMin` (RFC 3339) is the lower bound on modification time.
- A list holds up to 20,000 non-hidden tasks. A user holds up to 100,000 tasks in total.

**Auth.**
- Scope `https://www.googleapis.com/auth/tasks` (or `.readonly`) (list page).
- katlendar's Google app asks for `https://www.googleapis.com/auth/calendar` (`src/sync/google.rs:20`).
  Add the Tasks scope.
- Whether `auth/tasks` is a "sensitive" scope that needs verification is (unverified). Assume the same
  burden as Calendar (calendar-integrations.md section 4.1).

**Mapping.** https://developers.google.com/workspace/tasks/reference/rest/v1/tasks
- `title` (up to 1024 chars). `notes` (plain text).
- `due` is the day the task should be done. "It doesn't represent the deadline". "Only date information
  is recorded; the time portion of the timestamp is discarded". The time cannot be read or written.
  It maps exactly onto katlendar's `due: NaiveDate`.
- `status` is `needsAction` or `completed` → done.
- `parent`: change it only with the `move` method. `position`. `deleted`. `hidden`.
- `links[]` and `webViewLink`. `assignmentInfo` marks tasks assigned from Docs or Chat.
- Not in the resource: priority, tags, duration, start date, recurrence. They stay local.
  - Recurring tasks made in Google's UI appear as plain tasks (unverified).

**Incremental sync.** `updatedMin=<last pull>&showDeleted=true&showHidden=true&showCompleted=true`.
Then page through.

**Write-back.**
- `PATCH` to update, `insert` to create, `delete`, and `move` (resource method list).
- Each task has an `etag`. Whether `If-Match` is enforced is (unverified).

**Rate limits.** A courtesy limit of 50,000 queries per day per project:
https://developers.google.com/workspace/tasks/limits

**Rust crates.**
- `google-tasks1` 7.0.0+20251216, hyper plus tokio.
- `io-gtasks` 0.2.1 (2026-11-16).
- Hand-write instead. It is about six calls.

**CalDAV/ICS.** Google's CalDAV serves events only. Tasks are not exposed (unverified).

### 4.9 TickTick

**API.**
- Open API v1, base `https://api.ticktick.com`: https://developer.ticktick.com/docs/openapi.md
- Task endpoints:
  - `GET /open/v1/project` lists projects. `GET /open/v1/project/{projectId}/data` returns a project with
    its tasks.
  - `GET /open/v1/project/{projectId}/task/{taskId}` reads one task.
  - `POST /open/v1/task` creates. `POST /open/v1/task/{taskId}` updates.
  - `POST /open/v1/project/{projectId}/task/{taskId}/complete` completes one task.
    `POST /open/v1/task/completeTasks` completes several.
  - `DELETE /open/v1/project/{projectId}/task/{taskId}` deletes. `POST /open/v1/task/move` moves.
- There is also a batch create/update endpoint, a "List Completed Tasks" endpoint (with a body) and a
  "Filter Tasks" endpoint.

**Auth.** Same page:
- Personal API token, from Settings > Account > API Token, sent as `Authorization: Bearer`.
- OAuth:
  - Authorize at `https://ticktick.com/oauth/authorize`. Scopes are space-separated:
    `tasks:read tasks:write`.
  - Token at `POST https://ticktick.com/oauth/token`, form-encoded. The client id and secret go in
    **HTTP Basic** auth. `oauth.rs` cannot do that yet.
  - Apps are registered at `https://developer.ticktick.com/manage`.
- Verdict: personal token.

**Mapping.**
- `title`. `content` and `desc` → notes.
- `startDate` and `dueDate` are `yyyy-MM-dd'T'HH:mm:ssZ`. `timeZone` and `isAllDay` come alongside.
  - `startDate` → earliest. `dueDate` → due.
- `repeatFlag` is an **RRULE** string, e.g. `RRULE:FREQ=DAILY;INTERVAL=1`. It maps cleanly to `Repeat`.
- `priority` values: None `0`, Low `1`, Medium `3`, High `5` (task schema in the same page).
- `tags[]` → tags.
- `items[]` are checklist subtasks with title, status and dates. Flatten them or append them to notes.
- `status` 0 means open. The completed value is (unverified; 2 by memory). `completedTime`.
- No duration field was seen (lossy).

**Incremental sync.**
- No modified-since parameter was found.
- Poll `GET /project/{id}/data` per project; it returns undone tasks (unverified). Use "List Completed
  Tasks" with a date window for completions.
- Diff against local state. Detect deletions by absence.

**Rate limits.** None documented on the page.

**Rust crate.** `ticks` 1.0.1 (2024-07-10), reqwest plus tokio. Stale. Hand-write instead.

**CalDAV/ICS.** TickTick offers a calendar subscription link for its tasks (unverified; read-only at best).

### 4.10 Asana

**API.** REST 1.0, base `https://app.asana.com/api/1.0` (unverified base; endpoints read below).

**Auth.**
- PAT, sent as `Authorization: Bearer`: https://developers.asana.com/docs/personal-access-token.md
- OAuth: https://developers.asana.com/docs/oauth.md
  - Authorize at `https://app.asana.com/-/oauth_authorize`. Token at `https://app.asana.com/-/oauth_token`.
  - PKCE S256 is supported, but `client_secret` is still **required**.
  - Access tokens last 1 h and come with a refresh token.
  - Native apps should use `urn:ietf:wg:oauth:2.0:oob`. Other apps must use HTTPS redirects.
  - `oauth.rs` has no out-of-band mode.

**Mapping.** Read via `opt_fields`: https://developers.asana.com/reference/gettasks.md
- `due_on` (date) or `due_at` (date-time) → due. `start_on` / `start_at` → earliest.
- `notes` (plain) or `html_notes` → notes. `completed` → done. `tags` → tags.
- `parent` and `num_subtasks` → flatten.
- `memberships` (project and section) → list. `assignee`.
- No native priority, duration or recurrence. Those live in custom fields, which need a per-workspace
  mapping, or are absent.

**Incremental sync.**
- `GET /tasks?modified_since=...` (gettasks.md).
- Or the events API: https://developers.asana.com/reference/getevents.md
  - The first call without a `sync` token returns **412** plus a token.
  - Later calls return up to 100 events with `has_more`.
  - A 412 also means the token expired: re-crawl.

**Rate limits.** https://developers.asana.com/docs/rate-limits.md
- 150 requests per minute on free plans, 1,500 on paid. Search: 60 per minute.
- There are concurrency limits too. A 429 carries `Retry-After`.

**Rust crate.** `asana-sdk` 0.1.3 (2020). Abandoned.

### 4.11 Trello

**API.** REST `https://api.trello.com/1`.

**Auth.**
- Legacy key and token: https://developer.atlassian.com/cloud/trello/guides/rest-api/authorization/
  - The API key belongs to a Power-Up, managed at `https://trello.com/apps/admin`
    (https://developer.atlassian.com/cloud/trello/guides/rest-api/api-key-for-trello-rest-api/).
  - The user visits `https://trello.com/1/authorize?expiration=...&scope=read,write&response_type=token&key=...`.
    Use `return_url` with `callback_method=fragment`.
  - Expiration is `1hour`, `1day`, `30days` or `never`.
  - The key may be public. The token must stay secret.
- OAuth 2.0, 3LO with expiring tokens and refresh:
  https://developer.atlassian.com/cloud/trello/guides/rest-api/oauth-2-getting-started/
  - Public clients are meant for front-end Power-Ups using the client library's PKCE.
  - Callback URLs "should be pages under a domain you control":
    https://developer.atlassian.com/cloud/trello/guides/rest-api/oauth-2-client-configuration/
  - Legacy tokens can be migrated with `POST /1/token/exchange`:
    https://developer.atlassian.com/cloud/trello/guides/rest-api/oauth-2-for-existing-apps/
- Verdict: the user pastes key plus token, or katlendar ships its Power-Up's public key.

**Mapping.** https://developer.atlassian.com/cloud/trello/guides/rest-api/object-definitions/
- `name` → title. `desc` (up to 16384 chars) → notes.
- `due` (date-time) → due. `dueComplete` → done. `start` → earliest.
- `idLabels` → tags. `idList` → list (or the board → list). `closed` means archived.
- No priority, duration or recurrence. Checklists are separate objects.

**Incremental sync.** Poll each board's cards and compare `dateLastActivity` (object-definitions).
That is one request per board.

**Rate limits.** https://developer.atlassian.com/cloud/trello/guides/rest-api/rate-limits/
- 300 requests per 10 s per API key. 100 requests per 10 s per token.
- `/1/members/` routes: 100 per 900 s.

**Rust crate.** `trello-rs` 1.23.0 (2020) is **GPL-3.0-only**. It cannot be linked into a GPL-2.0-only
program. katlendar's README says only "GPL-2.0", so check whether "or later" is intended.

### 4.12 Notion

**API.**
- REST `https://api.notion.com/v1`. Every call needs a `Notion-Version` header.
- The latest version is `2026-03-11`. Version `2025-09-03` introduced data sources:
  https://developers.notion.com/reference/versioning.md

**Auth.**
- Personal access tokens cover one workspace and are created in the developer portal. The docs say not
  to use a PAT to authenticate a product used by many Notion users:
  https://developers.notion.com/guides/connections-and-tokens/personal-access-tokens.md
  - Each katlendar user would create their own token, which is arguably not that case. Flag it as a
    policy question.
- Public integrations use OAuth for any workspace. The user picks which pages to share. A Marketplace
  listing is optional and reviewed:
  https://developers.notion.com/guides/connections-and-tokens/public-integrations/overview.md
- Token exchange uses **HTTP Basic** (`CLIENT_ID:CLIENT_SECRET`) and returns an `access_token` and a
  `refresh_token`. The example redirect is HTTPS:
  https://developers.notion.com/guides/authorization/overview.md

**Mapping.**
- Tasks are rows in a user-built database ("data source"). Title, date, status, priority and tags are
  whatever properties that user created.
- katlendar would need a property-mapping dialog: which property is the due date, which is done, and so on.

**Incremental sync.** Query the data source with a `last_edited_time` filter:
https://developers.notion.com/reference/query-a-data-source.md

**Rate limits.** https://developers.notion.com/reference/request-limits.md
- 180 per minute (about 3 per second) on most plans. 600 per minute on Business and Enterprise.
- Data source queries: about 3 per second. Some limits are workspace-wide. A 429 carries `Retry-After`.

**Rust crate.** `notion` 0.6.0 (2024-08-13), reqwest plus tokio. Stale.

### 4.13 monday.com

**API.**
- GraphQL at `https://api.monday.com/v2`, with the personal token in the `Authorization` header and no
  Bearer: https://developer.monday.com/api-reference/docs/authentication.md
- A new version ships every quarter. Each version is stable for at least six months. Select one with the
  `API-Version` header: https://developer.monday.com/api-reference/docs/api-versioning.md

**Auth.**
- Personal tokens have all scopes and mirror the user's UI permissions (authentication.md).
- The legacy OAuth flow at `https://auth.monday.com/oauth2/authorize` issues tokens that "do not expire",
  with no refresh token. A newer OAuth 2.1 flow with expiring tokens exists:
  https://developer.monday.com/apps/docs/oauth.md
  - Whether it supports PKCE public clients is (unverified).

**Mapping.** Items on boards with typed columns (status, date, people), defined per board. It needs a
mapping dialog like Notion's.

**Incremental sync.**
- Activity logs exist and can be queried per board (unverified; the activity-log reference page was not
  read). Their cursor semantics are (unverified).

**Rate limits.** https://developer.monday.com/api-reference/docs/rate-limits.md
- Complexity budgets: personal tokens get 10M points per minute (1M on trial, NGO or free accounts).
  App tokens get 5M for reads and 5M for writes.
- There are also daily-call, concurrency and IP limits.

**Rust crate.** None found.

### 4.14 Basecamp

**API.** Basecamp REST at `https://3.basecampapi.com/{account_id}/...`:
https://raw.githubusercontent.com/basecamp/bc3-api/master/README.md

**Auth.**
- OAuth 2 only. No personal tokens are documented.
- Authorize at `https://launchpad.37signals.com/authorization/new`. Token at
  `https://launchpad.37signals.com/authorization/token`.
- The token request needs `client_secret` and returns a refresh token:
  https://raw.githubusercontent.com/basecamp/bc3-api/master/sections/authentication.md
- **Every request needs a `User-Agent` with the app name and a link or email address** (README).
  katlendar should send its repository URL, never the user's email.

**Mapping.** https://raw.githubusercontent.com/basecamp/bc3-api/master/sections/todos.md
- `content` → title. `description` (HTML rich text) → notes.
- `due_on` → due. `starts_on` → earliest. `completed` → done. `assignees`.
- No priority, tags, duration or recurrence.

**Write-back.**
- **Update needs every existing parameter**: "Omitting a parameter will clear its value".
  Always send the full todo.
- Complete with `POST /todos/{id}/completion.json`. Uncomplete with `DELETE` on the same path.

**Incremental sync.** Store `ETag` and `Last-Modified`. Send `If-None-Match` and `If-Modified-Since`
for cheap 304s (README).

**Rate limits.** 429 with retry advice (README). The numeric limit was not captured.

**Rust crate.** `basecamp-sdk` 0.37.0 (2026-11-19), MIT. It calls itself official and uses tokio.
Whether 37signals publishes it is (unverified).

### 4.15 Vikunja

- Vikunja serves CalDAV at `/dav/` (e.g. `https://vikunja.example.com/api/v1/dav/principals/<user>/`).
  The integration is marked "early alpha": https://vikunja.io/docs/caldav/
- Authentication is the account password, a dedicated CalDAV token, or (2.3.0+) an API token with the
  CalDAV permission group, which starts with `tk_` (same page).
- Supported properties: UID, SUMMARY, DESCRIPTION, PRIORITY, CATEGORIES, COMPLETED, DUE, DURATION, DTSTART,
  RELATED-TO, VALARM and STATUS (only COMPLETED).
  - RRULE, CREATED and LAST-MODIFIED go only from Vikunja to the client.
- **katlendar can sync Vikunja today** through its generic CalDAV account.
  - Once `FeedTask` grows (phase 0), CATEGORIES → tags, DTSTART → earliest and DURATION → duration_min
    come for free.
  - Test with a real instance before claiming support.
- Vikunja also has a REST API. Crate: `tui-do-api` 2.0.0 (2026-11-10), MIT/Apache. Not needed.

### 4.16 Taskwarrior / TaskChampion

- Taskwarrior 3 keeps tasks in a local database. File-sync tools like Syncthing are "not supported".
  Sync goes through a TaskChampion sync server or cloud storage (S3, GCS):
  https://taskwarrior.org/docs/sync/
- The sync model is replicas exchanging operations, with snapshots, an HTTP protocol and encryption:
  https://gothenburgbitfactory.org/taskchampion/sync.html
- Crates:
  - `taskchampion` 3.1.0 (2026-05-30), MIT. It pulls in tokio and reqwest for sync.
  - `taskchampion-sync-server` 0.7.1 (2026-11-17).
- Options:
  - Embed `taskchampion` as a replica. That is heavy: tokio, plus a second task store to reconcile.
  - Shell out to the local `task export` / `task import` JSON (unverified flags). That is cheap and
    local, with no network.
- Recommendation: later, via the CLI, behind a "Taskwarrior on this machine" account kind.

### 4.17 Out of scope

- **Things 3.**
  - Cultured Code documents only a `things:///` URL scheme (`add`, `update`, `show`, `search`, `json`).
    `update` needs a per-user auth token.
  - The token is found "On the Mac" or "On iOS" in Things' settings, so it is Apple-only and local-only:
    https://culturedcode.com/things/support/articles/2803573/
  - There is no web API and no Linux client.
- **OmniFocus.** "A personal task manager by The Omni Group for macOS, iOS, and iPadOS". Its automation
  is in-app JavaScript (Omni Automation), not a web API: https://omni-automation.com/omnifocus/index.html
- **Any.do.**
  - The developer page answers "Does Any.do have a public REST API? Not today".
  - The supported route is an MCP server at `https://mcp.any.do/mcp` with OAuth sign-in:
    https://www.any.do/developers
  - katlendar could in theory be an MCP client. That is not a stable task API, so skip it.

---

## 5. "Issues assigned to me" in a planner

Jira, Linear, GitHub, GitLab, ClickUp (team view) and Planner hold **work items** owned by a team workflow.
katlendar's value is giving them time on the calendar, not managing them.

- **Pull a saved filter, not a project.**
  - Jira: `assignee = currentUser() AND statusCategory != Done`, with an optional user-supplied JQL
    (search/jql endpoint, 4.3).
  - Linear: `issues(filter: {assignee: {isMe: {eq: true}}, state: {type: {nin: ["completed","canceled"]}}})`
    (filtering.md).
  - GitLab: `GET /issues?scope=assigned_to_me&state=opened` (4.6). GitHub: see github-integration.md.
- **One read-only katlendar list per account filter.** Use `TaskList.read_only = true`, which exists.
- **What the user may change locally:**
  - planner-only fields: `earliest`, `duration_min`, `flexible`, `color`, scheduled blocks;
  - none of the remote fields (title, notes, due, priority).
  - Planner-only fields never sync. They must survive a pull, so `apply_tasks` must not reset them (section 7.3).
- **Completion is a workflow transition, not a flag.**
  - Jira: list `/transitions`, let the user pick the "done" transition once per project, then `POST` it (4.3).
  - Linear: set `stateId` to a `completed`-type state of the issue's team (4.4).
  - GitLab: `state_event=close` (4.6).
  - If no transition is configured, ticking the box in katlendar hides the item locally and shows
    "still open in Jira".
- **Leaving the filter.** When an issue is reassigned or closed elsewhere, the next full refresh drops it.
  Keep the scheduled blocks as plain events, or ask. Deciding which is a UX question.
- **Link out.** Store the web URL (Jira `self` plus browse path, Linear `url`, GitLab `web_url`). Show an
  "Open in …" action in `src/views.rs`.
- **Rate cost is small.** One filtered query per account per poll fits easily in every limit above.

---

## 6. Rust ecosystem survey

Versions and dates are from `https://crates.io/api/v1/crates/<name>`, queried 2026-11-20.

| Crate | Latest (date) | Licence | Model | Verdict |
|---|---|---|---|---|
| todoist-api | 1.0.0-alpha.4 (2026-09-21) | MIT | reqwest, async | Skip. Alpha. One endpoint is easy by hand. |
| clickup | 0.3.0 (2025-11-17) | MIT | reqwest + tokio | Skip |
| jira | 0.4.0 (2026-10-24) | MIT | reqwest + tokio | Skip |
| autumn-jira | 0.2.0 (2026-11-06) | Apache-2.0 | tokio | Skip |
| lineark | 5.0.0 (2026-11-17) | see crate | reqwest + tokio | Skip. Copy query shapes only. |
| linear-api | 0.1.0 (2026-07-06) | see crate | async (unverified) | Skip |
| octocrab | 0.54.9 | MIT/Apache-2.0 | async | See github-integration.md |
| gitlab | 0.1905.0 | MIT/Apache-2.0 | sync + async (unverified) | Skip. Large. |
| google-tasks1 | 7.0.0+20251216 | see crate | hyper + tokio | Skip |
| io-gtasks | 0.2.1 (2026-11-16) | see crate | unverified | Skip |
| graph-rs-sdk | 3.0.1 (2025-04-20) | see crate | async | Skip. `graph.rs` exists. |
| ticks | 1.0.1 (2024-07-10) | see crate | reqwest + tokio | Skip. Stale. |
| asana-sdk | 0.1.3 (2020) | see crate | — | Skip. Abandoned. |
| trello-rs | 1.23.0 (2020) | **GPL-3.0-only** | — | Cannot use (licence) |
| notion | 0.6.0 (2024-08-13) | see crate | reqwest + tokio | Skip. Stale. |
| basecamp-sdk | 0.37.0 (2026-11-19) | MIT | tokio | Skip unless Basecamp ships |
| tui-do-api | 2.0.0 (2026-11-10) | MIT/Apache-2.0 | unverified | Not needed (CalDAV) |
| taskchampion | 3.1.0 (2026-05-30) | MIT | tokio + reqwest | Only if a full replica is wanted |
| taskchampion-sync-server | 0.7.1 (2026-11-17) | see crate | server | Not needed |

No crates were found for monday.com, OmniFocus or Any.do.

**Why hand-written ureq.**
- It matches the existing blocking worker model (`ureq = "3"`, no tokio in `Cargo.toml`).
- Each provider needs 4 to 8 calls with small JSON bodies, which `serde_json` handles.
- Vendor crates track the vendor's whole surface. They would pull in a second HTTP stack and an async
  runtime for little gain.
- Keep one shared helper for: Bearer or raw `Authorization`, JSON in and out, 429 with `Retry-After` →
  `SyncError::RateLimited`, 401 → `Auth`, 404/410 → `Gone`.

---

## 7. Architecture sketch

### 7.1 Extend `Provider` or add `TaskProvider`?

Add a separate `TaskProvider` trait. The existing `Provider` stays for CalDAV, Google Calendar, Graph
and Proton.

Reasons, from the code:
- `Provider::put/get/delete` speak hrefs and iCalendar text. JSON task APIs have IDs and typed fields.
- `pull_tasks` is per remote calendar. Todoist's cursor is per account (4.1). To Do's delta is per list.
  TickTick has no cursor.
- `save_task` builds `"{cal.id}/{uid}.ics"` hrefs. That is meaningless for an ID-based API.

Proposed shape (sketch, not final):

```rust
pub trait TaskProvider: Send {
    /// Lists/projects the account can see, with read-only flags.
    fn list_projects(&mut self) -> Result<Vec<RemoteList>, SyncError>;
    /// Changes since `since` (None = full). One call may cover many lists:
    /// the result is keyed by remote list id.
    fn pull_tasks(&mut self, since: Option<&str>, now: NaiveDateTime) -> Result<TaskDelta, SyncError>;
    /// Create or update; returns the new remote id, version and raw JSON.
    fn save_task_native(&mut self, list: &RemoteList, t: &Task) -> PushResult;
    /// Done/undone. For trackers this runs the chosen transition.
    fn complete(&mut self, list: &RemoteList, t: &Task, done: bool) -> PushResult;
    fn delete(&mut self, list: &RemoteList, remote_id: &str, version: Option<&str>) -> Result<(), SyncError>;
}

pub struct TaskDelta {
    pub upserts: Vec<RemoteTask>,   // each carries its remote list id
    pub deletes: Vec<String>,       // remote ids
    pub full: bool,                 // true: anything not listed is gone
    pub state: Option<String>,      // next cursor (account- or list-wide)
}
```

- CalDAV keeps the current `Provider` plus `save_task`, so nothing regresses.
- An adapter `impl TaskProvider for CalDav` can come later, if one push path is wanted.
- `push()` gains `PushJob::SaveTask` → `save_task_native` for `TaskProvider` accounts.
  - Done-only changes go through `complete()`. Todoist, TickTick, Basecamp and Jira all have separate
    completion calls.
- `src/syncing/run.rs` pulls **once per account** for account-cursor providers, then fans results out to
  lists by remote list id.
- `src/syncing/take.rs` creates or renames `TaskList`s from `list_projects()`.

### 7.2 Data model changes

**`Task`** (`src/model/task.rs`):
- `remote_id: Option<String>`: the provider ID. `uid` and `href` stay for CalDAV.
  Alternatively, reuse `uid` for the remote ID and leave `href` empty. That is simpler, but muddier.
- `version: Option<String>`: an ETag, `updated_at` or similar for conflict checks. `etag` can be reused.
- `raw: Option<String>` exists. Store the provider's JSON there for round-tripping (7.3).
- `due_time: Option<NaiveTime>` (optional): for Todoist floating times, To Do, TickTick and Asana `due_at`.
  Without it, the time is kept only in `raw`.
- `parent_remote: Option<String>`: subtasks are flattened, but keep the link for later nesting.
- `url: Option<String>`: "Open in …" for trackers.
- `remote_status: Option<String>`: the tracker's status name, shown as a chip.

**`FeedTask`** (`src/ics.rs:843`) → **`RemoteTask`** with the same fields plus:
- `earliest`, `duration_min`, `tags`, `repeat: Option<Repeat>` (None = unknown or unsupported);
- `list_remote_id`, `parent_remote`, `url`, `raw`.

**`TaskList`**:
- `remote_id` exists.
- Add `done_transition: Option<String>` for trackers.
- `sync_state` stays per list for To Do and Google Tasks.

**`Account`** (`src/model/calendar.rs:53`):
- Replace the bools with `kind: AccountKind { CalDav, Nextcloud, Google, Microsoft, Proton, Todoist,
  Jira, Linear, GitLab, TickTick, ClickUp, ... }`. Use a serde default that maps old files: a migration
  reads the old bools.
- Add `task_state: Option<String>` for account-wide cursors (Todoist).
- Add `filter: Option<String>` for the JQL or GraphQL filter of "assigned to me" accounts.
- Add `capabilities` derived from the kind (has tasks, has events, read-only by default).
- `server` and `username` stay. Jira needs a site URL and email. GitLab needs an instance URL.

**`apply_tasks`** (`src/model/data.rs:368`):
- Copy the new fields.
- **Never touch planner-owned fields**: `flexible`, `color`, `archived`, and `earliest` / `duration_min`
  when the provider does not supply them. Model this as `Option` fields on `RemoteTask`: None means
  "the provider has no such field, keep local".

### 7.3 Round-tripping lossy fields

- Keep the last remote JSON in `Task.raw`.
- Send only the fields katlendar owns and that changed since `raw`:
  - Todoist `item_update`, Linear `issueUpdate`, Graph `PATCH` and Google `PATCH` are all partial updates.
  - This preserves due times, sections, custom fields and checklists that katlendar cannot represent.
- **Due with a time.**
  - If `raw` has a date-time due and the katlendar date did not change, omit `due` from the patch.
  - If the date changed, keep the old time-of-day and timezone from `raw`.
- **Recurrence.**
  - katlendar's own `Repeat` advances the due date locally on completion.
  - For a synced repeating task, do **not** advance locally. Call the provider's close or complete
    (Todoist `item_close`, TickTick complete, To Do `PATCH status=completed`). Then re-pull to get the
    next occurrence.
  - Show non-mappable rules as read-only.
- **Full-replace APIs.** Basecamp requires every parameter on update (4.14). Build its body from `raw`
  plus the changes.

### 7.4 Priority mapping

katlendar uses 0 = none, 1–3 high, 4–5 medium, 6–9 low (`src/model/task.rs:14`), as RFC 5545 does.
Write as below. On read, map each remote value to its row. On write-back, map by band: 1–2 → top,
3 → high, 4–5 → medium, 6–9 → low.

| Provider | Remote → katlendar | Notes |
|---|---|---|
| Todoist | 4 → 1, 3 → 3, 2 → 5, 1 → 0 | API 1 is the default "p4", so treat it as none. Low and none collapse. |
| Linear | 1 → 1, 2 → 3, 3 → 5, 4 → 9, 0 → 0 | exact |
| ClickUp | 1 → 1, 2 → 3, 3 → 5, 4 → 9, null → 0 | scale (unverified) |
| TickTick | 5 → 1, 3 → 5, 1 → 9, 0 → 0 | 3 levels |
| To Do | high → 1, normal → 0, low → 9 | `normal` is the default |
| Jira | Highest → 1, High → 3, Medium → 5, Low → 7, Lowest → 9 | default scheme names (unverified); custom schemes map by order |
| Google, Trello, Basecamp, GitLab, Asana | none | stays local, never sent |

### 7.5 Conflicts and concurrency

- Only Google Tasks (etag) and maybe Graph (`@odata.etag`) offer a version check (both unverified for
  enforcement). Todoist, Linear, ClickUp, TickTick and Jira are last-writer-wins.
- Before a write, compare the remote `updated_at` from the last pull with a fresh GET only when cheap
  (single-task GET exists for all).
  - On mismatch, return `PushResult::TaskConflict`. The existing handling re-pulls (`src/syncing/run.rs`).
- Todoist commands carry a `uuid`, so a retried batch is idempotent (sync/overview.md). Store pending
  command uuids with the outbox.

### 7.6 OAuth helper changes (`src/sync/oauth.rs`)

| Need | Who | Change |
|---|---|---|
| Fixed port and/or path on redirect | Linear (registered URLs), maybe Todoist | `redirect: Option<String>` on `OAuthApp` |
| Client auth via HTTP Basic | TickTick, Notion | `client_auth: Post \| Basic` |
| JSON token body | ClickUp | `token_body: Form \| Json` |
| Comma-separated scopes | Todoist, Linear | already a plain string; nothing to do |
| Dynamic client registration | Todoist | new `register(endpoint, metadata)` call, client id stored on the account (`client_id` exists) |
| Non-expiring tokens, no refresh | ClickUp, monday legacy | treat `expires_in` missing as "long-lived" |

Personal tokens skip all of this. Store them in the keyring under the existing `account:{id}` key
(`src/secrets.rs`).

### 7.7 Polling cadence

- Webhooks are out (4.1, 4.2, 4.3), so polling must be cheap.
- Use cursors where they exist: Todoist `sync_token`, To Do `deltaLink`, Google `updatedMin`, Linear and
  GitLab `updated*`, Asana events.
- Run a full refresh every N hours to catch deletions where the provider does not report them
  (ClickUp, Jira, TickTick, Trello).
- Reuse the existing per-account backoff (up to 4 h) and map 429 to `SyncError::RateLimited`.

---

## 8. Recommended build order

Scores are rough. Value means likely demand from Linux desktop planner users, a judgement call.
Effort counts katlendar work after phase 0. Registration is the burden to ship it to everyone.

| # | Provider | Value | Effort | Registration burden | Why here |
|---|---|---|---|---|---|
| 0 | Phase 0 groundwork | — | M | none | Required by everything below |
| 1 | Microsoft To Do | H | S | none new (existing app + scope) | Delta, recurrence, start date. Sign-in exists. |
| 2 | Google Tasks | H | S | none new, but scope verification (unverified) | Date-only due fits katlendar exactly |
| 3 | Todoist | H | M | none for tokens; DCR later | Richest model, best sync cursor |
| 4 | Jira Cloud, assigned to me | H | M | none (user API token) | High work value. Read-mostly keeps it simple. |
| 5 | Linear, assigned to me | M | S | none (API key); OAuth public client later | Clean GraphQL, small surface |
| 6 | GitHub | M | — | see that doc | Tracked in github-integration.md |
| 7 | TickTick | M | M | none (token) | RRULE maps well. No cursor. |
| 8 | ClickUp | M | M | none (token) | Low rate limits on cheap plans |
| 9 | GitLab | M | S | none (PAT) | Self-hosted users |
| 10 | Vikunja | L–M | XS | none | Test existing CalDAV. Document it. |
| 11 | Taskwarrior (CLI) | L–M | M | none | Linux-native audience |
| 12 | Asana, Trello | L | M | Power-Up key (Trello) | Weak date/priority models |
| 13 | Notion, monday.com | L | L | none for tokens | Needs a mapping UI |
| 14 | Basecamp | L | M | OAuth app + secret, no PAT | Hardest auth for a desktop app |

---

## 9. Risks and open questions

- **Policy on pasted tokens.**
  - Atlassian discourages apps that collect API tokens (4.3, wording to re-check).
  - Notion says PATs are not for multi-user products (4.12).
  - Decide whether "user creates and pastes their own token" is acceptable. Document it in settings.
- **Scope creep in sign-in.** Adding Tasks scopes to existing Google and Microsoft accounts forces
  re-consent. Google Tasks scope verification status is (unverified).
- **Repeat semantics.**
  - katlendar's `Repeat` cannot express "every 2 weeks", "every 3rd", or Todoist "every!" (from completion).
  - Those tasks must stay read-only for the repeat rule, or `Repeat` grows an RRULE variant.
- **Subtasks.** Flattening may flood the planner. Consider hiding subtasks by default.
- **Time-of-day dues.** Without `due_time`, the planner may schedule a task after its real due time
  on the due day.
- **Deletions without tombstones** (ClickUp, Jira, TickTick, Trello) depend on full refreshes. The local
  copy can linger for up to N hours.
- **Licence.** `trello-rs` is GPL-3.0-only. Confirm whether katlendar is GPL-2.0-only or "or later".
- **Branding.** Todoist forbids "Todoist" as the primary name and requires a non-affiliation statement
  (4.1). Other vendors' brand rules were not checked (unverified).

### Claims not verified (check before coding)

- Todoist accepts `http://127.0.0.1:{port}` loopback redirects for public clients.
- ClickUp:
  - whether a form-encoded token request works;
  - the priority scale 1–4;
  - "done" equals a status of closed type;
  - no recurrence in the API.
- Jira:
  - the exact wording about apps collecting API tokens;
  - JQL `updated` precision and format;
  - `statusCategory` handling;
  - `timetracking` writes;
  - the start-date field;
  - Data Center v2 / wiki markup and `/rest/api/2/search`;
  - default priority names.
- Linear:
  - whether arbitrary loopback ports are accepted;
  - `updatedAt: {gt}`;
  - `includeArchived`.
- GitLab:
  - loopback redirect rules;
  - priority-label conventions.
- Microsoft:
  - Graph To Do throttling figures;
  - `@removed` in todoTask delta;
  - `If-Match` enforcement;
  - text body preference;
  - Planner details;
  - no CalDAV for Exchange Online.
- Google Tasks:
  - scope sensitivity;
  - recurring tasks surfacing as plain tasks;
  - `If-Match` enforcement;
  - no CalDAV for tasks.
- TickTick:
  - the completed `status` value;
  - whether project data returns only undone tasks;
  - rate limits;
  - an iCal subscription.
- Asana base URL.
- monday.com:
  - whether OAuth 2.1 supports PKCE;
  - activity-log cursor semantics.
- `basecamp-sdk` being published by 37signals.
- Taskwarrior `task export` / `task import` flags and priority values.
- Licences and async models marked "see crate" or "unverified" in section 6.
