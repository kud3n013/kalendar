# GitHub integration for katlendar

Research notes, gathered 2026-11-20. Scope: how katlendar (a Rust/egui Linux desktop calendar and task planner,
GPL-2.0, local-first, one JSON data file, no account of its own, blocking `ureq` on worker threads) could
show a person's GitHub work as **tasks** and their GitHub activity as **read-only calendar events**.
Three scenarios drive the design:

1. Someone assigns me an issue or PR, or asks me (or my team) to review a PR.
2. I work on a PR: my open PRs become tasks, and "what I did today" becomes a log on the calendar.
3. I create or take part in an issue: I wrote it, commented on it, or was @mentioned.

General task managers (Todoist, Linear, Jira…) are covered in `task-integrations.md`; this note covers GitHub
only.

Citation conventions used below:

- A bare link after a claim is the **primary source** (docs.github.com, the GitHub changelog/blog,
  cli.github.com) and was read for this note.
- **(secondary)** means the claim comes from a third party, not GitHub.
- **(unverified)** means it comes from memory or a search snippet only, and needs checking before code relies on it.
- Crate versions and dates come from the crates.io API (`https://crates.io/api/v1/crates/<name>`), queried 2026-11-20.

---

## 1. TL;DR

- **Poll; there is no push for a desktop app.** Webhooks are set on a repository, organization, Marketplace,
  Sponsors account or GitHub App, and GitHub POSTs to a URL you own. There is no per-user webhook, so a
  desktop app with no server has to poll.
  https://docs.github.com/en/webhooks/about-webhooks
- **Issue/PR search is the backbone.** One query per view against `GET /search/issues` covers every
  scenario. Examples: `is:open assignee:@me`, `is:open is:pr review-requested:@me`, `is:open is:pr author:@me`,
  `is:open mentions:@me`. Search allows 30 requests/min and returns up to 1,000 results per query.
  https://docs.github.com/en/rest/search/search
- **Notifications are the cheap change trigger.** `GET /notifications` with `If-Modified-Since` costs nothing
  when nothing changed (a 304 does not count against the rate limit). Each thread carries a `reason`
  (`assign`, `review_requested`, `mention`, `author`, `comment`, `state_change`…). The catch: the docs say
  these endpoints work **only with a classic PAT**, or an OAuth token with the `notifications` or `repo` scope.
  Fine-grained PATs and GitHub App tokens do not work.
  https://docs.github.com/en/rest/activity/notifications
- **"What I did today" comes from the Events API.** `GET /users/{me}/events` returns timestamped push, PR,
  review, comment and issue events, including private ones when you are signed in as that user. Limits: at
  most 300 events, nothing older than **30 days**, and 30 s to 6 h latency. Show it as a read-only
  "GitHub activity" calendar. The GraphQL `contributionsCollection` is the fallback for older or summary data.
  https://docs.github.com/en/rest/activity/events
- **Auth: an OAuth App with the device flow.** katlendar can embed a public client ID because the device flow
  needs **no client secret**, and neither does refreshing a device-flow token. Ask for `repo notifications`
  (plus `read:org` for team review requests). Offer two fallbacks: paste a classic PAT, or import
  `gh auth token`. A GitHub App is not the default because notifications do not work with its tokens.
  https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
- **Read-only first.** GitHub owns state such as open/closed/merged and assignees, so katlendar mirrors it.
  If the user ticks a GitHub task off locally, it is dismissed until the item changes on GitHub. Write-back
  (closing an assigned issue, marking a notification read) is an opt-in later phase.
- **No new crates.** octocrab is async and pulls in tokio/hyper/tower. Hand-rolled `ureq` + `serde_json`
  matches `src/sync/graph.rs` and is about 600 lines.
- **Model changes are needed.** `apply_tasks` currently overwrites `due`/`priority`/`done` on every pull, and
  `FeedTask` has no tags. Both need a small change before GitHub tasks can feel right (see §8).

### Phased plan

| Phase | What | katlendar files touched |
|---|---|---|
| 0 | Model prep. `FeedTask` gains `tags` and an optional `url`. `apply_tasks` gets a per-list "server owns these fields" mask, so a missing remote due date does not wipe a local one. Read-only lists remember a local "dismissed at updated_at" mark. | `src/ics.rs`, `src/model/data.rs`, `src/model/task.rs` |
| 1 | Sign-in: device flow plus PAT paste plus `gh auth token` import. `Account.github` flag (or a kind enum). Settings kind "GitHub". Token goes in the keyring. | `src/sync/oauth.rs` (device-flow fn, `bad_refresh_token` → Auth), `src/model/calendar.rs`, `src/settings.rs` (KINDS, help text, sign-in button), `src/syncing/connect.rs` (`connect_github`), `src/syncing/mod.rs` (`SyncDone::GitHubDone`), `src/secrets.rs` (unchanged API) |
| 2 | `GitHub` provider, tasks only. `list_calendars` returns one read-only task list per view (Assigned, Review requests, My PRs, Involved). `pull_tasks` runs the search queries serially, merges by `node_id`, and returns a full `Delta`. `sync_state` holds per-query ETags. | new `src/sync/github.rs`, `src/sync/mod.rs` (mod + export), `src/syncing/mod.rs` (`open_account` branch) |
| 3 | Change trigger: poll `/notifications` with `If-Modified-Since` and obey `X-Poll-Interval`. Re-run the searches only when something changed or on the 15-minute tick. | `src/sync/github.rs`, `src/syncing/run.rs` (honour a provider-supplied next-poll time) |
| 4 | "GitHub activity" read-only calendar from the Events API, with ETag polling. Events are clustered into blocks. | `src/sync/github.rs` (`pull` for the events calendar), `src/model/calendar.rs` (none if the existing `Calendar` fits) |
| 5 | Optional write-back. Tick an assigned issue → close it with `state_reason: completed`. Mark a notification thread read or done. Off by default, per list. | `src/sync/github.rs`, `src/sync/mod.rs` (a native task-write hook next to `save_native`), `src/syncing/edits.rs` (let opted-in lists go dirty) |
| 6 | Optional extras: Projects v2 date/iteration fields as due/earliest, GHES/GHE.com host field, GitLab/Gitea/Forgejo sibling providers. | `src/sync/github.rs`, `src/settings.rs` |

---

## 2. Detection options compared

| Option | What it tells you | Auth that works | Cost and limits | Verdict |
|---|---|---|---|---|
| `GET /search/issues` (REST) | Current set of issues/PRs matching a query, with state, labels, milestone, `updated_at` | any token; private repos need `repo` (classic/OAuth) or a fine-grained PAT with access | 30 req/min; 1,000 results max per query; `per_page` ≤ 100 | **Backbone** |
| GraphQL `search(type: ISSUE)` | Same, plus `reviewDecision`, `reviewRequests`, `isDraft`, Projects fields in one round trip | any token | 5,000 points/h; cost ≈ 1 per 100 nodes | Good later, for Projects fields |
| `GET /issues?filter=assigned\|created\|mentioned\|subscribed` | Issues and PRs across repos the user can see | classic/OAuth; fine-grained is not listed (unverified) | 5,000/h core | Simpler than search for "assigned" and "created", but has no review-request filter |
| `GET /notifications` | What changed for me and why (`reason`) | **classic PAT / OAuth only** | 304 is free; obey `X-Poll-Interval` | **Change trigger** |
| `GET /users/{me}/events` | Timestamped actions by me | any token for public; the user's own token for private; fine-grained needs "Events" read | 300 events, 30 days, 30 s–6 h latency; ETag 304 is free | **Activity log** |
| GraphQL `contributionsCollection` | Per-day contribution summaries; issue/PR/review contributions with `occurredAt` | any token | GraphQL points | Fallback for history |
| Webhooks | Push on change | needs a public HTTPS endpoint | — | Not for a desktop app |

Sources: https://docs.github.com/en/rest/search/search,
https://docs.github.com/en/graphql/reference/search, https://docs.github.com/en/rest/issues/issues,
https://docs.github.com/en/rest/activity/notifications, https://docs.github.com/en/rest/activity/events,
https://docs.github.com/en/graphql/reference/users,
https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens

---

## 3. The three scenarios

### 3.0 Summary table

| Scenario | Detect with | katlendar object | Refresh rule |
|---|---|---|---|
| 1a. Assigned an issue or PR | search `is:open assignee:@me`; trigger: notification `reason=assign` | Task in list **GitHub · Assigned** | Drop or mark done when no longer in the open set. Then one check query `assignee:@me is:closed updated:>=<last sync>` tells closed/merged (→ done) from unassigned (→ removed). |
| 1b. Review requested from me | search `is:open is:pr user-review-requested:@me` | Task in **GitHub · Reviews** | The request disappears from search once I review. Then the task is marked done; on PR close/merge it is removed. A re-request brings it back. |
| 1c. Review requested from my team | search `is:open is:pr team-review-requested-user:@me` (any of my teams, not direct) | Task in **GitHub · Reviews**, tag `gh:team` | Same as 1b. Teams need `read:org` (unverified for search). |
| 2a. My open PRs | search `is:open is:pr author:@me` (optionally `archived:false`) | Task in **GitHub · My PRs** | Merged → done. Closed unmerged → done with a "closed" note, or removed. Draft → tag `gh:draft`. Review decision → tag `gh:approved`/`gh:changes`. |
| 2b. PRs I contribute to but did not open | search `is:open is:pr involves:@me -author:@me` or `review-involves:@me` | Optional; off by default (noisy) | As 2a. |
| 2c. What I did today | `GET /users/{me}/events` (ETag) | Read-only events in calendar **GitHub activity** | Append-only. Events older than 30 days stay as fetched. A new ETag → fetch page 1 until a known event id. |
| 3a. Issues I created | search `is:open is:issue author:@me` | Task in **GitHub · Involved**, tag `gh:author` | Closed → done (or removed, per setting). |
| 3b. Issues I commented on | search `is:open is:issue commenter:@me` | Same list, tag `gh:commented` | Off by default. Can be large; cap at N most recently updated. |
| 3c. Issues/PRs that @mention me | search `is:open mentions:@me`; trigger: notification `reason=mention` | Same list, tag `gh:mention` | As 3a. A mention does not "go away" on GitHub, so local dismiss matters here. |

All search qualifiers: https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests

### 3.1 Scenario 1: assigned, or review requested

**Detection**

- `assignee:@me` finds issues and PRs assigned to me. `@me` works on every qualifier that takes a username.
  https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests
- Review requests have several qualifiers:
  - `review-requested:USER` covers direct requests and team requests. The docs note: "Requested reviewers
    are no longer listed in the search results after they review a pull request."
  - `user-review-requested:@me` covers direct requests only.
  - `team-review-requested:ORG/TEAM` covers one named team.
  - `team-review-requested-user:USER` covers any team the user is on, but not direct requests.
  - `review-involves:USER` keeps the PR listed after the review.
  - Source: same page.
- So "my review inbox" is `is:open is:pr review-requested:@me`. If the UI wants "direct" and "team" apart,
  use `user-review-requested:@me` and `team-review-requested-user:@me`.
- Notification reasons give the same signal faster:
  - `assign`;
  - `review_requested` ("You, or a team you're a member of, were requested to review a pull request");
  - `approval_requested`.
  - https://docs.github.com/en/rest/activity/notifications

**State changes**

- **Closed or merged.** The item leaves an `is:open` query. The provider cannot tell "closed" from
  "unassigned" by absence alone. Run one extra query per sync, `assignee:@me is:closed updated:>=<last sync>`.
  A hit means done; a miss means unassigned, so the task is removed.
- **Unassigned.** The item leaves the query and is removed from the list. If the user had edited the task
  locally (notes, planned time), keep it as a plain local task instead of deleting it. That needs a "detach"
  path in `apply_tasks` (see §8).
- **Review submitted.** It leaves `review-requested:@me` (per the quote above), so the task is marked done.
  `latestReviews`/`viewerLatestReview` in GraphQL, or `GET /repos/{o}/{r}/pulls/{n}/reviews` (chronological,
  with `state` and `submitted_at`), give the review state if the UI wants to show it.
  https://docs.github.com/en/rest/pulls/reviews, https://docs.github.com/en/graphql/reference/pulls
- **Re-request.** A re-request after a review puts the PR back in the result. The task comes back as not done,
  because `done` is server-owned for this list.

### 3.2 Scenario 2: contributing to a PR

**Open PRs as tasks**

- Query: `is:open is:pr author:@me`. Add `archived:false` to skip archived repos.
  https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests
- Useful state from GraphQL `PullRequest`, all from https://docs.github.com/en/graphql/reference/pulls:
  - `isDraft`;
  - `reviewDecision` (`APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`);
  - `state` (`OPEN`, `CLOSED`, `MERGED`);
  - `mergedAt`, `closedAt`.
- With REST search only, the item has `draft` and `pull_request.merged_at`. The review decision needs a
  search with `review:approved`/`review:changes_requested`, or the GraphQL query.
- On state change:
  - merged → done;
  - closed without merge → done, with "closed" in the notes;
  - converted to draft or back → the tag changes.

**"What I did today"**

- `GET /users/{username}/events` lists events by that user. Signed in as that user, it includes private
  events. Limits: "up to 300 events", "Only events created within the past 30 days will be included", and
  latency of 30 s to 6 h. It supports ETag; obey `X-Poll-Interval`. With a fine-grained PAT, private events
  need the "Events" user permission (read).
  https://docs.github.com/en/rest/activity/events
- Event types and payloads: https://docs.github.com/en/rest/using-the-rest-api/github-event-types
  - Every event has `id`, `type`, `actor`, `repo`, `created_at` and `public`.
  - `PullRequestEvent`: `action` is opened/closed/merged/reopened/assigned/…, plus `number` and `pull_request`.
  - `PullRequestReviewEvent`: `review`. `PullRequestReviewCommentEvent`: `comment`.
  - `IssueCommentEvent` and `IssuesEvent`.
  - `CreateEvent`: a branch, tag or repository.
  - `PushEvent` now lists only `repository_id`, `push_id`, `ref`, `head` and `before`. There is **no commit
    list or count** any more.
- GitHub announced the PushEvent payload cut in the changelog on 2025-08-08, with a brownout on 2025-09-08
  and the change live on 2025-10-07 (dates from a search snippet, so **unverified**). A third-party post says
  the `commits` array was dropped (**secondary**).
  https://github.blog/changelog/2025-08-08-upcoming-changes-to-github-events-api-payloads/
- So a push shows as "pushed to `branch` in `repo`". Getting commit messages costs one
  `GET /repos/{o}/{r}/compare/{before}...{head}` per push. Do that only on demand, for example when an event
  is opened. (The compare endpoint was not checked for this note.)
- Mapping to the calendar:
  - Each event is a short read-only `Event` in a "GitHub activity" calendar: start = `created_at`, end =
    start + 15 min.
  - Merge events in the same repo less than 30 min apart into one block, titled e.g. "katlendar: 3 pushes,
    1 review".
  - This is a reconstruction, not a time log, and the UI should say so.
- Fallback for days older than 30, or for daily totals: GraphQL
  `user { contributionsCollection(from, to) { … } }`.
  - It has `issueContributions`, `pullRequestContributions` and `pullRequestReviewContributions`, each with
    `occurredAt`.
  - `commitContributionsByRepository` counts commits per day per repository (`commitCount`), with no time
    of day.
  - `restrictedContributionsCount` counts private work the viewer cannot see.
  - `from` defaults to a year ago.
  - https://docs.github.com/en/graphql/reference/users
- Turning the log into a time log (planned vs actual) is a katlendar feature. GitHub has no time tracking.

### 3.3 Scenario 3: issues I created or take part in

- Queries:
  - `is:open is:issue author:@me` finds issues I wrote.
  - `commenter:@me` finds issues I commented on.
  - `mentions:@me` finds issues that mention me.
  - `involves:@me` is "a logical OR between the author, assignee, mentions, and commenter qualifiers".
  - https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests
- Recommendation: one **Involved** list fed by `is:open is:issue involves:@me -assignee:@me`. Assigned items
  already live in the Assigned list. The view comes from a second cheap check, or from tags set while merging
  results from the narrower queries.
- `commenter:` can match hundreds of old issues. Sort by `updated` and cap the list, for example at the 50
  most recently updated. Search sorts by `updated` with `sort=updated&order=desc`.
  https://docs.github.com/en/rest/search/search
- A notification `reason` can change while a thread lives. The docs give the example of `author` becoming
  `mention`. Do not key the task on the reason; key it on the issue's `node_id`.
  https://docs.github.com/en/rest/activity/notifications
- On state change:
  - closed → done;
  - reopened → not done;
  - locked or transferred → `node_id` stays the same (unverified), so the task survives.

---

## 4. Auth

### 4.1 Options

| Option | Secret needed in app | Reaches private repos | Notifications API | Multi-org | Expiry | Notes |
|---|---|---|---|---|---|---|
| **OAuth App, device flow** | no | yes, with `repo` | yes, with `notifications` (unverified for OAuth tokens; the docs say "classic PAT") | yes, all orgs the user can see, unless the org has OAuth app access restrictions | none by default; 8 h + 6-month refresh if opted in | **Recommended** |
| OAuth App, web flow (loopback + PKCE) | **yes**, `client_secret` is required | yes | as above | as above | as above | The secret cannot be kept in a GPL binary |
| GitHub App, user token (device flow) | no | only repos where the app is installed **and** the user has access | **no** | per installation | 8 h + 6-month refresh | Least privilege, but users must install it |
| Classic PAT (pasted) | — | yes | **yes** | yes | 1 year unused → auto-removed; optional expiry | Simple fallback |
| Fine-grained PAT (pasted) | — | yes, one owner per token | **no** | **no**: one user or org per token | default 30 days, max 366 or none | Works for search and events in one org |
| `gh auth token` import | — | depends on gh's scopes (`repo`, `read:org`, `gist` minimum) | yes | yes | gh refreshes it | Zero-click for gh users |

**OAuth App flows** (https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps)

- The web flow's token exchange lists `client_secret` as **Required**.
- PKCE is supported with S256 only ("plain" is not) and is "strongly recommended". The changelog of
  2025-07-14 announced PKCE support (title and summary only, so unverified in detail).
  https://github.blog/changelog/2025-07-14-pkce-support-for-oauth-and-github-app-authentication/
- Loopback redirects may use any port. The docs recommend `127.0.0.1` or `::1` over `localhost`, and say "the
  redirect_uri does not need to match the port specified in the callback URL". `oauth.rs` already redirects
  to `http://127.0.0.1:{port}`, so the web flow fits **if** a secret is acceptable (BYO client, as for Google).
- **Device flow** (RFC 8628 shape):
  - It must be enabled in the app's settings.
  - Step 1: `POST https://github.com/login/device/code` with `client_id` and `scope`. The reply has
    `device_code`, `user_code` (8 characters with a hyphen), `verification_uri`
    (`https://github.com/login/device`), `expires_in` (900) and `interval` (5).
  - Step 2: poll `POST https://github.com/login/oauth/access_token` with `client_id`, `device_code` and
    `grant_type=urn:ietf:params:oauth:grant-type:device_code`.
  - "The client_secret is not needed for the device flow."
  - Errors:
    - `authorization_pending`;
    - `slow_down` (add 5 s to the interval);
    - `expired_token` (the page also says `token_expired`; **unverified** which one is sent);
    - `access_denied`;
    - `device_flow_disabled`;
    - `incorrect_device_code`.
  - Limit: 50 code submissions per hour per app.
- Token replies are URL-encoded unless you send `Accept: application/json`. `token_request` in `oauth.rs`
  already sends it.
- **Expiring tokens:** access tokens last 8 h; refresh tokens last 6 months "without use".
  - A user opts in per sign-in with the `offline_access` scope, or the app forces it in its settings.
  - Refresh uses `grant_type=refresh_token`. `client_secret` is required unless the token came from the
    device flow.
  - Refreshing rotates both tokens: the old ones stop working.
  - A bad refresh token gives the error `bad_refresh_token`, not `invalid_grant`.
- The changelog of 2026-08-14 adds:
  - opt-in short-lived tokens for OAuth Apps, "enabled by default for all new applications";
  - up to 10 redirect URIs;
  - all of this in GHES 3.23.
  - https://github.blog/changelog/2026-08-14-multiple-redirect-uris-and-token-refresh-for-oauth-apps

**Scopes** (https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps)

- With no scope, a token reads public info only.
- `repo` gives full read/write to public and private repositories. There is **no read-only private scope**.
- `public_repo` limits the token to public repositories.
- `notifications` reads notifications, marks them read, and watches/unwatches.
- `read:org` reads org and team membership.
- `read:project` reads Projects; `project` also writes them.
- Scopes "do not grant any additional permission beyond that which the user already has".

**Org restrictions**

- "When you create a new organization, OAuth app access restrictions are enabled by default." An unapproved
  OAuth App cannot reach the org's private resources. Users can ask an owner to approve the app.
  https://docs.github.com/en/organizations/managing-oauth-access-to-your-organizations-data/about-oauth-app-access-restrictions
- GitHub Apps are not subject to those policies.
  https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps

**GitHub App user tokens**

- The device flow is the recommended route for desktop and CLI apps, and needs no secret.
- Tokens last 8 h (`expires_in` 28800); refresh tokens last 6 months (15897600 s).
- Refresh needs `client_secret` "unless the user access token was generated using the device flow".
- The token has "only permissions that both the user and the app have". Repository access is also the
  intersection of where the app is installed and what the user can reach.
- https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app
- The changelog of 2026-10-13 adds per-sign-in `offline_access` for GitHub Apps.
  https://github.blog/changelog/2026-10-13-github-app-improvements-for-least-privilege-and-management

**Notifications auth**

- "These endpoints only support authentication using a personal access token (classic)", with the
  `notifications` or `repo` scope.
  https://docs.github.com/en/rest/activity/notifications
- OAuth App tokens with those scopes are widely used with this API (e.g. by gh), but the page does not say so.
  **(unverified)**

**PATs**

- Fine-grained PATs: "Each token is limited to access resources owned by a single user or organization".
  They also cannot reach repos where the user is an outside collaborator, nor several orgs at once. Orgs can
  require approval or block them.
  https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens
- Classic PATs need SAML SSO authorization for SSO orgs, and GitHub removes them after a year unused.
  "Outside collaborators can only use personal access tokens (classic)" (same page).
- The fine-grained permission table lists per-repo issue and PR endpoints. It does **not** list
  `GET /search/issues`, `GET /issues`, `GET /notifications`, `GET /user` or `GET /users/{u}/events`. Those
  endpoints probably work without a specific permission, limited to the repos the token can see. **(unverified)**
  https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens

**gh CLI**

- `gh auth token` "outputs the authentication token for an account on a given GitHub host".
  https://cli.github.com/manual/gh_auth_token
  - `--hostname` picks the host.
  - gh refreshes expiring tokens unless given `--no-refresh`.
- gh stores its token in the system credential store.
  https://cli.github.com/manual/gh_auth_login
- katlendar must not reuse gh's own OAuth client ID. That would mean presenting as another app.

### 4.2 Recommendation

1. **Default: "Sign in with GitHub" using the device flow of an OAuth App registered for katlendar.**
   - Ship the client ID in the binary. It is public by design for the device flow, and no secret is needed.
   - Scopes: `repo notifications read:org`. Offer a "public repositories only" checkbox that swaps `repo`
     for `public_repo`.
   - Enable expiring tokens on the app. Store `{refresh_token, access_token, expires_at}` in the keyring
     through `secrets.rs`, the same way `google.rs` packs its secret JSON.
   - The UI shows the `user_code` large, with a "Copy" button, and opens `verification_uri`.
2. **BYO client ID**, matching the Google/Microsoft accounts. Some users need their own app, for example in
   an org that blocks unapproved OAuth Apps.
3. **Paste a token.**
   - Accept a classic PAT, or a fine-grained PAT. With a fine-grained PAT, say that notifications are off and
     only one owner's private repos are reached.
   - Detect the token type by prefix: `ghp_` classic, `github_pat_` fine-grained (**unverified** prefixes,
     from memory).
   - Check the token with `GET /user`.
4. **"Use my gh login"**: run `gh auth token --hostname <host>` once on connect and on each `Auth` error.
   Never store the token; gh owns it.
5. **Not now: a GitHub App.**
   - The notifications API refuses its tokens.
   - Users would also have to install the app on every org/repo to see private issues.
   - Revisit if GitHub adds App support to notifications.

---

## 5. Incremental sync and rate limits

**Limits** (https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)

- Primary limit: 5,000 requests/h for PATs and OAuth user tokens; 60/h unauthenticated.
- Secondary limits:
  - 100 concurrent requests;
  - 900 points/min for REST (GET = 1 point);
  - 80 content-creating requests/min and 500/h;
  - 2,000 OAuth token requests/h.
- The search API has its own limit: 30 requests/min authenticated.
  https://docs.github.com/en/rest/search/search
- GraphQL allows 5,000 points/h. A query costs about one point per 100 requested nodes, with a minimum of 1.
  The secondary limit is 2,000 points/min.
  https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api
- Headers: `x-ratelimit-limit`, `-remaining`, `-used`, `-reset` and `-resource`.
- On 403/429: wait for `retry-after` if present. Otherwise wait for `x-ratelimit-reset` when remaining is 0.
  Otherwise wait at least 1 min and back off exponentially. GitHub may ban integrations that ignore the
  limits. This maps onto `SyncError::RateLimited`.

**Conditional requests** (https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)

- Send `If-None-Match: <etag>` or `If-Modified-Since: <last-modified>`. A 304 "does not count against your
  primary rate limit" when the request is authorized.
- Make requests one at a time, not in parallel. Wait at least 1 s between writes. Keep query parameters
  stable, so the ETag stays valid.
- Notifications: "optimized for polling with the Last-Modified header". The `X-Poll-Interval` header gives
  the allowed poll interval: "Please obey the header."
  https://docs.github.com/en/rest/activity/notifications
- Events: ETag plus `X-Poll-Interval`. https://docs.github.com/en/rest/activity/events
- Whether `/search/issues` returns useful ETags/304s is **unverified**. Assume a search always costs one of
  the 30/min.
- GraphQL ETags are **unverified**. Assume none.

**Recommended loop**

- **Every `X-Poll-Interval` (usually 60 s, unverified) while the window is open:** one `GET /notifications`
  with `If-Modified-Since`.
  - On 304, stop; nothing changed, and it cost nothing.
  - On 200, note the repos/threads that changed and schedule a task refresh now.
- **Every 15 min (`SYNC_EVERY_S = 900` in `src/model/mod.rs`), or on a notification hit, or on window focus
  after 60 s (`SYNC_ON_FOCUS_S`):** run the task searches serially.
  - Assigned, Reviews, My PRs and Involved, plus one "recently closed" check: **5 search calls**.
  - Add a second page only when `total_count` > 100.
  - Merge the results by `node_id` and return `Delta { full: true, … }` per list.
- **Every 15 min:** one `GET /users/{me}/events` with `If-None-Match`. Page further only until a known event id.
- **Budget per hour, window open all hour:**
  - about 60 notification polls, nearly all 304s and free;
  - 4 × 5 = 20 search calls, plus notification-triggered refreshes, at most about 60;
  - 4 events calls;
  - so **under 150 requests/h of the 5,000**, and never more than about 6 search calls in any minute
    (limit 30).
- With a fine-grained PAT (no notifications), only the 15-minute search loop runs. That is about 24
  requests/h.
- Handle `incomplete_results: true` on search (a timeout) as "no change", not as a full set. Otherwise a slow
  search would wipe the list. https://docs.github.com/en/rest/search/search

**API version**

- Send `X-GitHub-Api-Version: 2026-03-10`. Supported versions are 2026-03-10 and 2022-11-28; the latter ends
  on 10 March 2028. With no header, GitHub uses 2022-11-28.
  https://docs.github.com/en/rest/about-the-rest-api/api-versions
- **Advanced search:** REST `advanced_search=true` or GraphQL `ISSUE_ADVANCED` adds AND/OR and parentheses.
  https://github.blog/changelog/2025-03-06-github-issues-projects-api-support-for-issues-advanced-search-and-more/
  - A GHES breaking-changes page says advanced search would become the default in September 2025 (**unverified**
    for github.com).
  - The 2026-04-02 changelog says the default search type is lexical, adds `search_type=semantic|hybrid`
    (10/min), and lets `@` go before assignee handles.
    https://github.blog/changelog/2026-04-02-improved-search-for-github-issues-is-now-generally-available/
  - The queries above use only space-separated qualifiers, which mean AND in both modes. Avoid
    repeated-qualifier ORs to stay safe.

---

## 6. Mapping

| katlendar `Task` field | From GitHub | Notes |
|---|---|---|
| `uid` | `node_id` (GraphQL `id`) | Stable across renames and transfers (unverified for transfer) |
| `title` | `"{owner}/{repo}#{number} {title}"` | Short repo name if the user picks it |
| `notes` | URL on the first line, then `body` (Markdown, truncated to ~2 KB) | katlendar notes are plain text |
| `href` | `html_url` | Opens in the browser; today `href` is a CalDAV path, so a separate `url` field is cleaner |
| `etag` | `updated_at` | Cheap change check per item |
| `done` | `state == closed` or merged; for review tasks, "no longer requested" | Server-owned |
| `due` | `milestone.due_on` (date-time or null) → date; or a Projects v2 date field | https://docs.github.com/en/rest/issues/issues |
| `earliest` | Projects v2 iteration `startDate` | https://docs.github.com/en/graphql/reference/projects |
| `duration_min` | Projects v2 number field named e.g. "Estimate" (user picks) | Optional |
| `priority` | label matching `P0`–`P3` / `priority:high`, or a Projects single-select "Priority" | User-configurable mapping; 0 when none |
| `tags` | labels + view tags (`gh:assigned`, `gh:review`, `gh:author`, `gh:mention`, `gh:draft`, `gh:approved`, `gh:changes`) | Needs `FeedTask.tags` |
| `list` | one synced list per view | Read-only lists |

- **Lossy:** comments, review threads, assignees other than me, CI status, linked PRs and reactions.
  Show them as a link only.
- **Projects v2:**
  - Read field values with `fieldValues` fragments: `ProjectV2ItemFieldDateValue.date`,
    `…SingleSelectValue.name`, `…IterationValue { startDate duration title }`, `…TextValue`.
  - Or call `fieldValueByName(name:)`.
  - This needs `read:project`. Items you cannot see come back as `REDACTED`.
  - https://docs.github.com/en/issues/planning-and-tracking-with-projects/automating-your-project/using-the-api-to-manage-projects
- **Activity events** map to `Event`:
  - `title` = "repo: verb object", for example "katlendar: reviewed #42";
  - `start` = `created_at`, with a 15-min block;
  - `uid` = `gh-event-{id}`;
  - `calendar` = the GitHub activity calendar.
  - Merged blocks keep a list of event ids in `notes`.

---

## 7. Write-back

- **Default: read-only.** Every GitHub list has `read_only: true`. Ticking a task is a local
  "dismissed at `updated_at`" mark. If `updated_at` changes later, for example because someone comments, the
  task comes back.
- **Opt-in "close issues I tick"**, per list, Assigned only:
  - `PATCH /repos/{o}/{r}/issues/{n}` with `{"state":"closed","state_reason":"completed"}`.
  - Un-ticking sends `{"state":"open"}` (`state_reason` becomes `reopened`).
  - "Issue owners and users with push access or Triage role can edit an issue." With a fine-grained PAT this
    needs Issues write (or Pull requests write).
  - https://docs.github.com/en/rest/issues/issues,
    https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens
  - On 403/404, show the error and revert to a local dismiss.
- **Never merge or close PRs from katlendar.** Ticking a PR task dismisses it locally only.
- **Notifications:**
  - When a task is ticked, optionally `PATCH /notifications/threads/{id}` (mark read, 205) or `DELETE` it
    (mark done, 204).
  - This needs the thread id, so keep the mapping from the notification poll.
  - https://docs.github.com/en/rest/activity/notifications
- **Writes cost 5 secondary points each.** Space them at least 1 s apart.
  https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api
- **Code impact.** `push()` in `src/sync/mod.rs` sends `SaveTask` through ICS `get`/`put`. GitHub needs a
  native task hook, for example `fn save_task_native(&mut self, list, &Task) -> Option<Result<PushResult>>`,
  mirroring `save_native` for events. `src/syncing/edits.rs` only marks tasks dirty in `is_synced_writable`
  lists, so an opted-in GitHub list must count as writable, but for `done` only.

---

## 8. Fit with the code

**Provider** (`src/sync/github.rs`, new; modelled on `src/sync/graph.rs`)

- `struct GitHub { api: String, token: String, login: String }`.
  - `api` is `https://api.github.com`, `https://HOST/api/v3` or `https://api.SUB.ghe.com`.
  - `call()` uses a `ureq` Agent like `Graph::call`: a 30 s timeout, `http_status_as_error(false)` and no
    redirects.
  - It sends `Authorization: Bearer`, `Accept: application/vnd.github+json`, `X-GitHub-Api-Version` and
    `User-Agent: katlendar`. GitHub requires a User-Agent (unverified wording).
  - It sends the token only to `api`.
- `list_calendars()` returns:
  - one `RemoteCalendar { tasks: true, events: false, read_only: true }` per enabled view;
  - plus `GitHub activity` with `{ events: true, tasks: false, read_only: true }`.
- `pull_tasks(cal, state, now)`:
  - Decode `state` as JSON: per-query ETag, last sync time and dismissed marks if kept remotely (no; keep
    those local).
  - Run that view's queries and build `FeedTask`s.
  - Return `Delta { full: true, tasks, state: Some(new_state), … }`.
  - On `incomplete_results`, return `full: false` with no deletes.
- `pull(cal, state, now)` for the activity calendar:
  - Events since the last seen id, as `FeedEvent`s.
  - `full: false`, so old events are kept past GitHub's 30-day window.
- Map HTTP errors to `SyncError`:
  - 401 → `Auth`, which triggers a refresh, or a gh re-import;
  - 403/429 with rate headers → `RateLimited`;
  - 404/410 → `Gone`.

**Model and apply changes**

- `src/ics.rs` `FeedTask`: add `tags: Vec<String>` and `url: Option<String>`. CalDAV leaves them empty.
- `src/model/data.rs` `apply_tasks`:
  - Today it overwrites title, notes, due, priority and done. It also removes non-dirty tasks missing from a
    full pull.
  - Add a list-level `owns: FieldMask`. For GitHub, the server owns title/notes/done/tags and owns due only
    when it sent one.
  - A local `done` on a read-only list should survive until `etag` (= `updated_at`) changes.
  - When a task vanishes from a full pull but has local work (planned time on the calendar via
    `Event.task_id`, edited notes), detach it into a plain local task instead of deleting it.
- `src/model/calendar.rs` `Account`:
  - Add `github: bool`. Better: replace the growing bool set (`google`, `microsoft`, `nextcloud`, `proton`)
    with a `kind` enum.
  - `server` holds the API base.
- `src/model/task.rs` `TaskList`: no change if the view name lives in `remote_id`, e.g.
  `"assigned"`/`"reviews"`/`"prs"`/`"involved"`.

**Auth and UI**

- `src/sync/oauth.rs`:
  - Add `device_flow(app, on_code: impl Fn(&str, &str), stop) -> Result<Tokens>`, reusing `token_request`.
  - Map `bad_refresh_token` to `SyncError::Auth`, next to `invalid_grant`.
- `src/syncing/connect.rs`: add `connect_github` to run the device flow on a worker, then `GET /user` for
  `login`.
- `src/syncing/mod.rs`:
  - Add a `github` branch to `open_account`.
  - Add `SyncDone::GitHubDone`, the same shape as `GoogleDone`.
  - The `TokenCache` already keeps access tokens in memory.
  - A rotated refresh token is saved back to the keyring, as for Microsoft.
- `src/settings.rs`:
  - Add `(7, "GitHub")` to KINDS.
  - Add help text.
  - Add a "Sign in with GitHub" button, plus "Paste token" and "Use gh login".
  - Add per-view checkboxes and an optional host field for GHES/GHE.com.
- `src/syncing/run.rs`: let a provider report a sooner next-poll time (from `X-Poll-Interval` and
  notification hits), so notifications can poll every 60 s without the full sync running that often.

**Tests:** follow the `stand_in` local HTTP server pattern in `graph.rs`. Feed it canned search, notification
and events JSON. Check 304 handling, `incomplete_results`, the closed-vs-unassigned check and the refresh
token rotation.

---

## 9. Rust crates

| Crate | Version (date) | Fit |
|---|---|---|
| `ureq` | 3.5.2 (2026-11-12); lock has 3.5.1 | Already used. Blocking, which fits the worker threads. **Use this.** |
| `serde_json` | 1.0.154 (2026-10-11) | Already used |
| `octocrab` | 0.54.9 (2026-10-31) | Async. Default features include `retry` and `timeout`. `hyper` and `tower` are not optional, so it would bring an async runtime (tokio inferred, **unverified**). No. |
| `octorust` | 0.10.0 (2025-03-12) | Generated, async, stale. No. |
| `graphql_client` | 0.16.0 (2026-01-15) | Typed GraphQL codegen. Optional later for Projects v2; plain JSON strings work too. |
| `github-device-flow` | 0.3.0 (2026-11-18) | Depends on `reqwest` 0.12 and `clap`. The flow is about 60 lines with `ureq`, so no. |

All versions are from `https://crates.io/api/v1/crates/<name>`, queried 2026-11-20.

---

## 10. GHES and GHE.com

- **GitHub Enterprise Server:**
  - REST is at `http(s)://HOSTNAME/api/v3`.
    https://docs.github.com/en/enterprise-server@3.18/rest/using-the-rest-api/getting-started-with-the-rest-api
  - GraphQL is at `/api/graphql` (**unverified**).
  - OAuth is at `https://HOSTNAME/login/oauth/…` and `/login/device/code` (**unverified** for the device path).
  - Each server needs its own OAuth App, so the embedded client ID will not work. Use BYO client ID, PAT
    paste or `gh auth token --hostname`.
  - Expiring OAuth App tokens arrive in GHES 3.23.
    https://github.blog/changelog/2026-08-14-multiple-redirect-uris-and-token-refresh-for-oauth-apps
  - Older servers may not support the newer API version header. Fall back to sending none.
- **GHE.com (data residency):**
  - The API is at `https://api.SUBDOMAIN.ghe.com`. The network page lists `auth.ghe.com`, and points to
    `/meta` for the hosts.
    https://docs.github.com/en/enterprise-cloud@latest/admin/data-residency/about-github-enterprise-cloud-with-data-residency
  - The OAuth endpoints are on `SUBDOMAIN.ghe.com` (**unverified**).
  - A katlendar OAuth App on github.com does not exist there, so the user needs a BYO app or a PAT.
- **UI:** one "Host" field, empty for github.com, from which the API base and OAuth base are derived.

---

## 11. GitLab, Gitea and Forgejo

Same shape, later, as sibling providers. None of this was checked for this note, so all of it is **unverified**.

- **GitLab:**
  - `GET /api/v4/issues?scope=assigned_to_me` and `GET /api/v4/merge_requests?reviewer_username=…`.
  - Personal access tokens and OAuth with PKCE (no secret for public apps), plus a device flow on newer
    versions.
  - Todos (`GET /api/v4/todos`) play the role of notifications.
  - `GET /api/v4/events` is the activity log.
- **Gitea and Forgejo:**
  - `GET /api/v1/repos/issues/search?assigned=true&type=issues|pulls`, plus `created`, `mentioned` and
    `review_requested` flags.
  - `GET /api/v1/notifications`; application tokens.
  - Forgejo stays API-compatible with Gitea for these endpoints.
- **Design consequence:** keep the GitHub provider's internals as "fetch view → `Vec<FeedTask>`". A second
  forge is then a new fetch layer, not a new data model.

---

## 12. Risks and open questions

- **Notifications with OAuth App tokens.** The docs say "only … personal access token (classic)". Check with a
  real OAuth App token before phase 3. If it fails, phase 3 works only for pasted classic PATs or gh tokens.
- **Search ETags.** If `/search/issues` never returns a 304, every refresh costs real search calls. The budget
  above still fits, but check.
- **Org OAuth restrictions.** Many orgs block unapproved OAuth Apps by default. Private org issues would then
  be silently missing, because search just returns fewer results. Show a hint in settings: "Missing org
  issues? Ask an owner to approve katlendar, or paste a token."
- **`repo` is broad.** It grants write access to all private code. Document why; offer `public_repo` and
  fine-grained PATs for cautious users.
- **Who registers the katlendar OAuth App?** It needs an owning GitHub account or org, with the device flow
  enabled. This is the same open question as the Google/Microsoft client IDs in `calendar-integrations.md`.
- **Events latency (up to 6 h) and the 30-day window.** "Today" may lag. Keep fetched events locally; never
  delete them on a full pull.
- **PushEvent has no commits.** A push shows as one line. Commit counts need extra compare calls.
- **Noise.** `commenter:` and `involves:` can produce hundreds of tasks. Cap them and make them opt-in.
- **Local edits on read-only tasks** (due, priority, planned time) currently get overwritten by
  `apply_tasks`. Phase 0 is a prerequisite, not polish.
- **Exact device-flow error string:** `expired_token` vs `token_expired`. Handle both.
- **Advanced search default.** If github.com makes advanced search the default, space-separated qualifiers
  still mean AND. Re-check before using any OR syntax.
- **Token prefixes** (`ghp_`, `github_pat_`, `gho_`, `ghu_`, `ghr_`) are used only for UI hints. Do not rely
  on them.
