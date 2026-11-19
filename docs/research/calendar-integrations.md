# Calendar service integrations for katlendar

Research notes, gathered 2026-11-14. Scope: how katlendar (a Rust/egui desktop calendar,
GPL-2.0) could do **two-way** sync with the popular hosted calendars, in place of today's read-only
ICS feed import.

Citation conventions used below:

- A bare link after a claim is the **primary source** (vendor docs, RFC, crate metadata) and was read for this note.
- **(secondary)** means the claim comes from a third party (blog, forum, build guide), not the vendor.
- **(unverified)** means it comes from memory or a search snippet only, and needs checking before code relies on it.
- Crate versions and dates come from the crates.io API (`https://crates.io/api/v1/crates/<name>`), queried 2026-11-14.

---

## 1. TL;DR

- **Do generic CalDAV first.** One client covers iCloud, Fastmail, Nextcloud, Radicale, Baïkal and most
  self-hosted servers. Those servers all accept a username and an **app password** over HTTPS Basic
  auth, so no OAuth app registration is needed. Nextcloud adds a browser-based Login Flow v2 that
  hands back an app password.
- **Google and Microsoft come second, and the cost is administrative, not technical.**
  - Both require OAuth 2.0 for a native app: loopback redirect plus PKCE.
  - Google classes calendar scopes as *sensitive*. A public app must pass verification, otherwise it hits
    the 100-user cap. While in "Testing", refresh tokens die after 7 days.
  - Microsoft needs an Entra app registration. Since Nov 2020, users may be blocked from consenting to
    unverified multi-tenant apps, and publisher verification needs a Partner Program account.
- **Proton has no API and no CalDAV.** Keep the existing read-only ICS "Share with anyone" link.
- **No client secret goes in the repo.**
  - Embed public client IDs at build time via `option_env!`, the way GNOME Online Accounts does.
  - Also let users paste their own client ID, the vdirsyncer route.
  - Google secrets for desktop clients are "not treated as a secret", but Google's terms discourage
    committing them to open source.
- **Stay blocking: ureq 3 on worker threads.** It matches the existing `std::thread` + `mpsc` +
  `ctx.request_repaint()` pattern in `src/main.rs`. The async SDKs (google-calendar3, graph-rs-sdk, libdav)
  would pull in tokio for little gain.
- **The data model must change before any write-back.**
  - `Event` has no UID, href or etag today.
  - Events point at calendars by `usize` index.
  - `Data::replace_remote` deletes and re-creates every remote event, with new ids, on each sync.

### Phased plan

| Phase | What | katlendar files touched |
|---|---|---|
| 0. Groundwork | Stable calendar ids instead of `calendar: usize`. Add `uid`, `recurrence_id`, `href`, `etag`, `raw_ics` to `Event`. Upsert by (calendar, uid, recurrence-id) instead of `replace_remote`. Keep UID in the parser. Add a `SyncMsg` enum on the existing channel. | `src/model.rs`, `src/ics.rs`, `src/main.rs`, `src/views.rs`/`src/panel.rs`/`src/dialogs.rs` (index → id call sites) |
| 1. Parser hardening | VTIMEZONE handling or a TZID alias table, a Windows→IANA zone map, `BYMONTH`/`BYSETPOS`/`WKST`. Write-side ICS serialisation (VEVENT emit with UID/DTSTAMP/SEQUENCE). | `src/ics.rs` (or replace its expander with the `rrule` crate) |
| 2. CalDAV read | RFC 6764 discovery, PROPFIND for calendars, REPORT `sync-collection` (RFC 6578) with ctag/etag fallback. Accounts UI, with app passwords kept in Secret Service. | new `src/sync/mod.rs`, `src/sync/caldav.rs`, `src/secrets.rs`; `src/settings.rs` (Integrations → Accounts page); `src/model.rs` (`Account`, per-calendar sync state); `Cargo.toml` (`quick-xml` or `roxmltree`, `keyring-core` + a Secret Service store) |
| 3. CalDAV write | PUT with `If-Match`/`If-None-Match: *`, DELETE with `If-Match`, 412 conflict handling, an offline outbox. Nextcloud Login Flow v2. | `src/sync/caldav.rs`, `src/sync/nextcloud.rs`, `src/dialogs.rs` (edit dialog marks events dirty), `src/model.rs` (outbox) |
| 4. Google | OAuth loopback + PKCE, Calendar API v3 `events.list` with `syncToken`, or Google's CalDAV endpoint with a bearer token, which reuses phase 2. | `src/sync/oauth.rs`, `src/sync/google.rs`, `src/settings.rs`, `Cargo.toml` (`oauth2` with a custom ureq-3 client) |
| 5. Microsoft | Same OAuth module against Entra. Graph `calendarView/delta` per calendar, plus `patternedRecurrence` ↔ RRULE mapping for writes. Optionally Microsoft To Do for tasks. | `src/sync/graph.rs`, `src/sync/oauth.rs`, `src/settings.rs` |
| 6. Tasks (optional) | CalDAV VTODO, Google Tasks, Microsoft To Do mapping onto `Task`. | `src/sync/*`, `src/model.rs` (`Task` remote ids) |

Proton remains on the existing ICS path and needs no new code.

---

## 2. Comparison

| Provider | Protocol(s) | Auth for a desktop app | Two-way | Incremental sync | Registration burden for an OSS app |
|---|---|---|---|---|---|
| Google Calendar | REST Calendar API v3; CalDAV (`apidata.googleusercontent.com/caldav/v2`) | OAuth 2.0 loopback + PKCE only (CalDAV rejects Basic) | Yes (CalDAV: VEVENT only, no MKCALENDAR) | `syncToken`/`nextSyncToken` (REST); RFC 6578 + ctag (CalDAV) | **High**: Cloud project, sensitive-scope verification, 100-user cap and 7-day tokens until verified |
| Microsoft 365 / Outlook.com | Microsoft Graph (REST); EWS (legacy, being retired); no CalDAV (unverified, see §4.2) | OAuth 2.0 via Entra, public client, loopback + PKCE or device code | Yes | Graph delta (`calendarView/delta`, `@odata.deltaLink`) | **Medium–high**: Entra registration; consent may be blocked for unverified multi-tenant apps; publisher verification needs a Partner Program account |
| Apple iCloud | CalDAV | Apple ID + app-specific password (2FA required) | Yes | CalDAV ctag/etag, RFC 6578 (unverified for iCloud) | **None** |
| Proton Calendar | ICS share link only (end-to-end encrypted; no API or CalDAV) | Secret URL | **No** (read-only) | No (full refetch) | None |
| Fastmail | CalDAV; JMAP Calendars (IETF draft) | Username + app password | Yes | CalDAV sync-collection (unverified for Fastmail); JMAP `/changes` | **None** |
| Nextcloud | CalDAV (`/remote.php/dav`) | Login Flow v2 → app password, or a manual app password | Yes | RFC 6578 sync-collection (unverified for Nextcloud) | **None** |
| Radicale / Baïkal / generic | CalDAV | Basic or Digest with the server password | Yes | Varies; ctag/etag at minimum (unverified) | None |
| Yahoo, Zoho | CalDAV (unverified) | App password (unverified) | Yes (unverified) | ctag/etag (unverified) | None (unverified) |

---

## 3. Standards you will implement regardless

- **CalDAV**, RFC 4791 (https://www.rfc-editor.org/rfc/rfc4791).
  - Calendars are WebDAV collections and each event is one `.ics` resource with an ETag.
  - Reads use `REPORT calendar-query` (with an optional time-range filter) and `calendar-multiget`.
  - Writes are `PUT`/`DELETE`.
  - The server can expand recurrences when asked: `<C:expand>`, §9.6.5. Server support varies (unverified per server).
- **WebDAV Collection Sync**, RFC 6578 (https://www.rfc-editor.org/rfc/rfc6578).
  - `REPORT sync-collection` with a server-issued `sync-token` returns changed hrefs and 404 entries for deleted ones.
  - An invalid token returns an error, after which you do a full resync.
  - The pre-RFC fallback is `getctag` (a CalendarServer extension): compare the collection ctag, then compare per-resource etags.
- **Discovery**, RFC 6764 (https://www.rfc-editor.org/rfc/rfc6764).
  - Try SRV `_caldavs._tcp` and `/.well-known/caldav`.
  - Then follow PROPFIND `current-user-principal` → `calendar-home-set` → collections.
- **iCalendar**, RFC 5545 (https://www.rfc-editor.org/rfc/rfc5545). This is the object format for CalDAV, for Google's CalDAV, and for ICS feeds.
- **OAuth for native apps**, RFC 8252 (https://www.rfc-editor.org/rfc/rfc8252).
  - Use the system browser, never an embedded webview.
  - The loopback redirect `http://127.0.0.1:{ephemeral port}/` is §7.3, and the authorization server must accept any port.
  - Native apps are public clients, so a secret embedded in a distributed binary is not confidential (§8.5).
- **PKCE**, RFC 7636 (https://www.rfc-editor.org/rfc/rfc7636). Use the `S256` challenge method.
- **Device Authorization Grant**, RFC 8628 (https://www.rfc-editor.org/rfc/rfc8628).
  - The user types a code on another device.
  - Useful when no local browser or listening socket is available.

---

## 4. Per-provider findings

### 4.1 Google Calendar

**APIs**

- REST Calendar API v3. Scopes are listed at https://developers.google.com/workspace/calendar/api/auth.
  - Relevant scopes: `calendar`, `calendar.readonly`, `calendar.events`, `calendar.events.owned`,
    `calendar.app.created`, `calendar.calendarlist(.readonly)`.
  - The same page says public apps using these scopes "must complete a verification process".
- CalDAV API (https://developers.google.com/workspace/calendar/caldav/v2/guide).
  - Endpoint: `https://apidata.googleusercontent.com/caldav/v2/CALENDAR_ID/events`.
  - **OAuth 2.0 only**: Basic auth gets a 401. The CalDAV API must be enabled in the same Cloud project.
  - Supports RFC 6578 sync-collection and ctag.
  - Does **not** support VTODO/VJOURNAL, MKCALENDAR or free-busy queries.
  - Shares the REST API's quota.
  - Implication: once phase 2 exists, Google is "CalDAV with a bearer token". The REST API gives richer
    metadata (colours, conference data) but needs its own mapper.
- Tasks are a separate API, Google Tasks (`https://tasks.googleapis.com`, resources `tasklists`, `tasks`;
  https://developers.google.com/workspace/tasks/reference/rest).
  - The `due` field is date-only: "the time portion of the timestamp is discarded"
    (https://developers.google.com/workspace/tasks/reference/rest/v1/tasks).
  - That fits katlendar's `Task.due: Option<NaiveDate>`, but not `duration_min` or scheduling. Those would have to
    live locally or in `notes`.

**Auth** (https://developers.google.com/identity/protocols/oauth2/native-app)

- The loopback IP address is "the recommended mechanism".
- Custom URI schemes are "no longer supported" for desktop clients, and the OOB copy-paste flow is deprecated.
- "Always use PKCE", with S256 recommended.
- Endpoints:
  - authorize: `https://accounts.google.com/o/oauth2/v2/auth`
  - token: `https://oauth2.googleapis.com/token`
  - revoke: `https://oauth2.googleapis.com/revoke`
- Desktop apps are described as public clients that "cannot keep secrets confidential". The token-exchange
  table marks `client_secret` as optional for this client type. Test whether the token endpoint accepts a
  missing secret for a "Desktop app" client before relying on it (unverified in practice).
- https://developers.google.com/identity/protocols/oauth2 says that for installed apps the client secret
  "is obviously not treated as a secret".
- The device-code flow ("TV and limited-input devices") restricts which scopes it may request. Calendar scopes are
  believed **not** to be on that allow-list (unverified). Plan on loopback only.

**Verification burden**

- Calendar scopes are in Google's *sensitive* category, so sensitive-scope verification applies:
  https://developers.google.com/identity/protocols/oauth2/production-readiness/sensitive-scope-verification.
  - That page's review time is quoted as a few business days; seen via a search summary, so unverified.
  - Sensitive scopes do not need the paid CASA security assessment that *restricted* scopes need
    (secondary: Nylas's scope guide).
- Unverified apps are capped at 100 new users and show an "unverified app" screen
  (https://support.google.com/cloud/answer/7454865).
  - The same page says verification "might require several months" depending on scope sensitivity.
  - Personal, internal or test-only use is exempt.
- An external consent screen in **Testing** status issues refresh tokens that **expire after 7 days**, unless
  the app only requests profile/email/openid (https://developers.google.com/identity/protocols/oauth2).
  - So "every user registers their own Cloud project in testing mode" means re-login weekly unless they press
    "Publish app". Publishing without verification gives the unverified-app warning but normal token lifetimes.
- At most 100 live refresh tokens per Google account per client ID; the oldest is silently invalidated
  (same page). This only matters if a user re-auths repeatedly.

**Incremental sync** (https://developers.google.com/workspace/calendar/api/guides/sync)

- The first full `events.list` returns `nextSyncToken` on the **last page** only. Later calls pass `syncToken`.
- **HTTP 410 Gone** means the token was invalidated: wipe the calendar's store and do a full sync.
- `timeMin`/`timeMax` (and some other filters) are incompatible with `syncToken` and return 400.
  - So the first sync cannot be windowed. The katlendar window (`PAST_DAYS=60`, `AHEAD_DAYS=400` in
    `src/ics.rs`) has to be applied locally.
- Deleted events come back with `status: "cancelled"`.
- Push notifications need a public HTTPS endpoint with a valid certificate
  (https://developers.google.com/workspace/calendar/api/guides/push), so a desktop app cannot use them. Poll instead.

**Rate limits** (https://developers.google.com/workspace/calendar/api/guides/quota)

- 10,000 queries/min per project and 600/min per user per project.
- Errors come back as `403 usageLimits` or `429`. Use exponential backoff, capped at roughly 32–64 s.
- The page announces that billing may begin later in 2026 for projects above 1,000,000 requests/day, with
  90 days' notice. A shared katlendar client ID is far below that.

**Recurrence and time zones**

- REST events carry RFC 5545 lines in a `recurrence` array (`RRULE:`, `EXDATE:`, `RDATE:`), with
  `start.timeZone` as an IANA name (unverified detail; check the Events resource reference).
- Exceptions are separate events with `recurringEventId` + `originalStartTime`.
- `singleEvents=true` asks the server to expand instances. Whether it combines safely with `syncToken`, and
  how unbounded series are capped, is unverified.
- Google CalDAV returns ordinary VEVENT + VTIMEZONE text, so the same parser as phase 2 applies.

### 4.2 Microsoft 365 / Outlook.com

**APIs**

- **Microsoft Graph** is the supported API for calendars on both work/school and personal Microsoft accounts.
- **EWS**:
  - No feature updates since 2018.
  - Microsoft tells developers to move to Graph, and says EWS "will eventually be deprecated"
    (https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/deprecation-of-basic-authentication-exchange-online).
  - A specific Exchange Online EWS shutdown date (believed to be October 2026) is **unverified** here. Do not build on EWS.
- **No CalDAV**:
  - Exchange Online / Outlook.com do not offer CalDAV for third-party sync.
  - Inference only: the protocol list on the basic-auth deprecation page above names EAS, POP, IMAP, EWS,
    MAPI, OAB and Autodiscover, and never CalDAV. **Unverified** as a positive statement.
  - Outlook can *publish* an ICS link, which katlendar already reads.

**Auth**

- Register an app in Entra ID as a **public client** with "Accounts in any organizational directory and
  personal Microsoft accounts".
  - Public clients "can't have client secrets" (https://learn.microsoft.com/en-us/entra/identity-platform/msal-client-applications).
  - So the client ID is the only thing embedded, and it is not a secret.
- Redirect URI rules (https://learn.microsoft.com/en-us/entra/identity-platform/reply-url):
  - For `localhost`/`127.0.0.1` redirect URIs the **port is ignored** when matching, per RFC 8252.
  - Prefer `http://127.0.0.1` over `localhost`. Registering an `http` loopback URI may need a manifest edit
    rather than the portal form.
  - `[::1]` is not supported.
  - No query parameters are allowed in redirect URIs when personal accounts are enabled.
- Device code flow is available for public clients
  (https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code; not re-read for this note).
  It needs "Allow public client flows" enabled on the registration (unverified).
- Scopes: `Calendars.ReadWrite` (or `Calendars.Read`) and `offline_access` for a refresh token. Add
  `Tasks.ReadWrite` for To Do.

**Verification and consent burden**
(https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview)

- Publisher verification is free, but needs:
  - a verified **Microsoft AI Cloud Partner Program** (formerly MPN) account;
  - a custom domain for the publisher (not `*.onmicrosoft.com`).
- Apps registered from a personal Microsoft account cannot be verified.
- Since November 2020, tenants with risk-based step-up consent stop end users consenting to **new unverified
  multi-tenant apps** that request more than basic permissions.
  - In practice, many work/school tenants will need an admin to approve katlendar.
  - Personal Outlook.com accounts are not affected by tenant consent policy (unverified detail).

**Incremental sync**
(https://learn.microsoft.com/en-us/graph/delta-query-events, https://learn.microsoft.com/en-us/graph/api/event-delta?view=graph-rest-1.0)

- v1.0 offers `GET /me/calendars/{id}/calendarView/delta?startDateTime=…&endDateTime=…`, one per calendar.
  - Each window is tracked separately.
  - Page through `@odata.nextLink` until an `@odata.deltaLink` arrives, then store that whole URL as the sync state.
  - `Prefer: odata.maxpagesize=N` controls page size.
- `$select`, `$filter`, `$expand`, `$orderby` and `$search` are **not** supported on event delta.
- Deletions arrive as items with `@removed: {"reason": "deleted"}`.
- `calendarView` returns **expanded occurrences and exceptions** inside the window. katlendar does not need to
  expand Graph recurrences on the read path.
- A window-less `events/delta` exists only in **beta**.
- Permissions: delegated `Calendars.Read`/`Calendars.ReadWrite`, supported for personal Microsoft accounts.
- **Microsoft To Do**:
  - `GET /me/todo/lists/{id}/tasks/delta`, delegated `Tasks.ReadWrite` (or `Tasks.Read`), personal accounts supported
    (https://learn.microsoft.com/en-us/graph/api/todotask-delta?view=graph-rest-1.0).
  - Tracked per list, with the same deltaLink mechanism.

**Rate limits** (https://learn.microsoft.com/en-us/graph/throttling-limits)

- Outlook service limits apply per app ID **and** mailbox:
  - 10,000 requests per 10 minutes;
  - **4 concurrent requests**;
  - 150 MB upload per 5 minutes.
- On 429, honour `Retry-After`.
- The concurrency cap means the sync worker should serialise requests per account.

**Recurrence and time zones**
(https://learn.microsoft.com/en-us/graph/api/resources/recurrencepattern?view=graph-rest-1.0,
https://learn.microsoft.com/en-us/graph/api/resources/recurrencerange?view=graph-rest-1.0)

Graph does not use RRULE. `patternedRecurrence = { pattern, range }`.

| Graph `pattern.type` | RRULE equivalent |
|---|---|
| `daily` | `FREQ=DAILY;INTERVAL=n` |
| `weekly` | `FREQ=WEEKLY;BYDAY=…;WKST=firstDayOfWeek` |
| `absoluteMonthly` | `FREQ=MONTHLY;BYMONTHDAY=d` |
| `relativeMonthly` | `FREQ=MONTHLY;BYDAY=MO,TU…;BYSETPOS=±n` (`index`: first…fourth, last → 1…4, −1) |
| `absoluteYearly` | `FREQ=YEARLY;BYMONTH=m;BYMONTHDAY=d` |
| `relativeYearly` | `FREQ=YEARLY;BYMONTH=m;BYDAY=…;BYSETPOS=±n` |

| Graph `range.type` | RRULE |
|---|---|
| `endDate` | `UNTIL=` |
| `numbered` | `COUNT=numberOfOccurrences` |
| `noEnd` | (none) |

- Graph cannot express `BYHOUR`, multiple `BYMONTHDAY` values, `FREQ=HOURLY`, `RDATE` and similar.
- Writes of such series from a CalDAV-origin event must be refused or flattened.
- Time zones:
  - `dateTimeTimeZone` values default to UTC unless the request sends `Prefer: outlook.timezone="…"`
    (the delta docs mention the Prefer header).
  - Event `originalStartTimeZone` and recurrence `recurrenceTimeZone` are often **Windows zone names**, e.g.
    "W. Europe Standard Time" (unverified for current behaviour).
  - katlendar's parser resolves TZID only through `chrono_tz::Tz` parsing, so it would treat them as floating.
  - Ship a Windows→IANA table from CLDR `windowsZones.xml`
    (https://github.com/unicode-org/cldr/blob/main/common/supplemental/windowsZones.xml).
  - That table also fixes Outlook-published ICS feeds, which use the same names in TZID (unverified).

### 4.3 Apple iCloud

- **Protocol: CalDAV only.** There is no public REST calendar API.
- **Server:** `https://caldav.icloud.com/`, which redirects to a partition host (`pNN-caldav.icloud.com`) after
  principal discovery. **Secondary/unverified**: widely used by clients, but not found in an Apple developer doc.
- **Auth:** Apple ID + **app-specific password** (https://support.apple.com/en-us/102654).
  - Requires two-factor authentication on the Apple ID.
  - Up to 25 active app-specific passwords.
  - All of them are revoked when the main Apple ID password changes. katlendar must handle 401 by prompting for a new one.
- **Registration:** none.
- **Incremental sync:** ctag/etag works; RFC 6578 sync-collection support on iCloud is likely, but unverified.
- **Quirks (unverified):**
  - iCloud emits VTIMEZONE blocks with Apple/Olson TZIDs, which are fine for chrono-tz.
- **Reminders:** not reachable from katlendar; see §4.3.1.

#### 4.3.1 Apple Reminders (checked 2026-11-14)

**Conclusion:** katlendar on Linux cannot sync Apple Reminders. iCloud **events** over CalDAV are unaffected.
Only VTODO lists that never went through the Reminders upgrade, or were created over CalDAV by another client,
remain reachable. Apple's Reminders app does not show those lists.

What the sources say:

- **Apple support, "Update the Reminders app in iOS 13 or later"** (https://support.apple.com/en-us/102457,
  page dated June 2026):
  - After the update, reminders "aren't compatible with earlier versions of iOS and macOS".
  - The update "affects existing reminders in your primary iCloud account only". Reminders in "CalDAV and
    Exchange accounts, aren't changed".
  - The only non-Apple-device access Apple lists is iCloud.com in a browser. It also says iCloud reminders
    don't show in Outlook via iCloud for Windows.
  - The page does not mention CalDAV access to upgraded lists at all.
- **EventKit**, the only official Reminders API (https://developer.apple.com/documentation/eventkit,
  read via its JSON form `…/tutorials/data/documentation/eventkit.json`):
  - Abstract: "Create, view, and edit calendar and reminder events."
  - Platforms: iOS, iPadOS, Mac Catalyst, macOS, visionOS and watchOS only. There is no Linux, Windows or web
    version.
- **CloudKit JS / CloudKit Web Services** (https://developer.apple.com/documentation/cloudkitjs):
  - Abstract: access "to your CloudKit app's containers and databases", meaning the developer's own app.
  - Nothing documents access to Apple's Reminders store.
- **BusyMac FAQ, "Where Did My Reminders Go After Upgrade?"**
  (https://www.busymac.com/docs/faqs/112990-reminders-in-ios-13-and-macos-catalina-drops-support-for-caldav/,
  last updated 28 July 2024). This is **secondary**: a third-party CalDAV client vendor.
  - The upgrade moves to-do-only calendars "off of CalDAV" into a separate Reminders store.
  - Those calendars then disappear from third-party CalDAV clients "on all other platforms".
  - iCloud's CalDAV server still syncs VTODO calendars between CalDAV clients. These are a separate store,
    though, and the upgraded Reminders app does not show them.
  - BusyCal's own Reminders support is native to Mac and iOS, consistent with EventKit.
- **Unofficial access:** the only other routes are undocumented iCloud web endpoints used by iCloud.com. They are
  reverse-engineered and fragile, and out of scope for this project.

What katlendar can offer instead:

- iCloud calendars (events) via CalDAV, as in Phase 2.
- CalDAV task sync (Phase 6) with servers that keep VTODO on CalDAV: Nextcloud, Fastmail, Radicale and
  Baïkal. Fastmail and Nextcloud were not re-verified in this pass.
- **Setup hint for the settings page:** iPhone/Mac users who want shared tasks can add one of those CalDAV accounts
  to Apple Reminders on their device, under Settings › Reminders › Accounts. Those lists are then on CalDAV and
  visible to katlendar. Apple's page confirms that reminders in CalDAV accounts aren't changed by the upgrade.
  The exact settings path is unverified.

### 4.4 Proton Calendar

- **No public API and no CalDAV.**
  - Proton Bridge exposes mail (IMAP/SMTP) only.
  - The CalDAV feature request is still open on Proton's UserVoice
    (https://protonmail.uservoice.com/forums/932842-proton-calendar/suggestions/45730093-dav-support-for-contacts-calendar; secondary/community).
  - A third-party write-up reaches the same conclusion (https://blog.mailfence.com/proton-calendar-support-caldav/; secondary).
- Calendars are end-to-end encrypted. A third-party client would need Proton's crypto and its private API.
  - Community attempts, e.g. CalDAV PRs to the `hydroxide` bridge on Codeberg, are unmerged and **unverified**.
  - They are not a supportable dependency.
- **What works today: "Share with anyone" links** (https://proton.me/support/share-calendar).
  - Paid plans only; web app only.
  - Up to five links per calendar.
  - "Limited" (busy only) or "Full" view.
  - The URL embeds the decryption key.
  - Proton itself notes that third-party apps stop fetching updates from shared links after some time, as a known issue.
- **Update (2026-11-15): account access is implemented** in `src/sync/proton*.rs`, alongside the feed path.
  - It follows Proton's own open-source clients, not an official API: WebClients (GPL-3.0, read for behaviour only, no code copied;
    https://github.com/ProtonMail/WebClients), go-proton-api and go-srp (both MIT; https://github.com/ProtonMail/go-proton-api,
    https://github.com/ProtonMail/go-srp).
  - Sign-in is SRP with a bcrypt-hashed password; the modulus must carry Proton's signature. Two-factor codes, a second (mailbox)
    password and the human check are handled. The password is not kept: the keyring holds the session and the key passphrase derived from it.
  - Key chain: key passphrase → user key → address key (through a signed token) → calendar passphrase → calendar key → per-event session keys.
  - Events are the merged iCalendar parts (`SharedEvents`, `CalendarEvents`); `modelevents/latest` tells whether anything changed.
  - Writing (added 2026-11-15): `PUT calendar/v1/{id}/events/sync` creates, changes and deletes. The event is split into a signed part
    (times, UID) and an encrypted part (title, notes, place), both signed with the address key; a change reuses the event's session key
    and sends its reminders and colour back unchanged. Only calendars whose passphrase carries the account's own signature are written.
  - Not done: calendars shared by others (their passphrase signature is not checked, so they stay read-only), checking who signed
    event parts, events with guests, repeating events, reminders, security keys.
  - Risks: the API is private and may change; Proton's terms allow automated access only when it looks like normal client use
    (https://proton.me/legal/terms); the client names itself `Other` in `x-pm-appversion`, which Proton's servers may refuse.
  - Tested only against an in-process stand-in server and go-srp's published values, not against Proton itself.
- Recommendation for links: keep `ics::is_proton` and the existing read-only feed path.
  - Treat the link as a secret: it contains a key, so move it out of plain `data.json` into the keyring along with
    the other credentials.
  - Show Proton's staleness caveat in the UI.

### 4.5 Fastmail

- **CalDAV:** `https://caldav.fastmail.com/`, TLS required. Per-calendar "CalDAV URL" lives under
  Settings → Calendars → Export (https://www.fastmail.help/hc/en-us/articles/1500000278342-Server-names-and-ports).
- **Auth:** "You will need to get an app password to connect to these servers. You cannot use your regular
  Fastmail password" (same page).
- **JMAP Calendars** is still an IETF Internet-Draft (draft-ietf-jmap-calendars-33, Oct 2026,
  https://www.ietf.org/archive/id/draft-ietf-jmap-calendars-33.html), co-authored at Fastmail.
  - Fastmail's production JMAP calendar availability for third-party clients is **unverified**.
  - CalDAV is the stable path. JMAP is a future optimisation: `/changes` gives cheap incremental sync.
- Registration: none.

### 4.6 Nextcloud, Radicale, Baïkal (generic CalDAV)

- **Nextcloud CalDAV root:** `https://HOST/remote.php/dav`. Nextcloud also serves `/.well-known/caldav`
  redirects (unverified here; from Nextcloud's user docs).
- **Nextcloud Login Flow v2**
  (https://docs.nextcloud.com/server/latest/developer_manual/client_apis/LoginFlow/index.html):
  1. Anonymous `POST https://HOST/index.php/login/v2`. The reply has `poll: {token, endpoint}` and `login` (a URL).
  2. Open `login` in the system browser.
  3. Every second, `POST` `token=…` to `endpoint`. It returns 404 until the user approves.
  4. On approval the reply is `{server, loginName, appPassword}`, returned **once**. The poll token is valid for 20 minutes.
  5. Send a descriptive `User-Agent`; it becomes the app-password name in Nextcloud's security settings.
  6. To log out, `DELETE /ocs/v2.php/core/apppassword` with header `OCS-APIREQUEST: true`.
  - This needs only `ureq` and `open`/`webbrowser`: no OAuth, no registration.
- **Radicale / Baïkal:** plain CalDAV with the server's own users and HTTP Basic (Baïkal can also use Digest).
  - Radicale's sync-collection support is believed present (unverified).
  - Both are good targets for an integration-test server: Radicale is a single `pip install` and can run in CI.

### 4.7 Others, briefly

- **Yahoo Calendar:** CalDAV at `caldav.calendar.yahoo.com` with an app password (**unverified**, from memory).
  Works through the generic path if the URL is accepted.
- **Zoho Calendar:** offers CalDAV and its own REST API with OAuth (**unverified**). Use the generic path.
- **Thunderbird-style "local" calendars:** katlendar already stores its own calendars locally in
  `$XDG_DATA_HOME/katlendar/data.json`.
  - A useful extra is import/export of a plain `.ics` file (or a vdir, one `.ics` per event, the vdirsyncer
    layout), which then gives interop with khal/vdirsyncer users for free.
  - khal reads vdirs and leaves network sync to vdirsyncer (unverified, from memory).

---

## 5. How other open-source clients handle OAuth client IDs

| Project | Approach | Source |
|---|---|---|
| Thunderbird | Hard-codes a Google `clientId` **and** `clientSecret`, plus a Microsoft `clientId` (no secret), in `OAuth2Providers.sys.mjs`. The Google entry uses PKCE and the external browser. Scopes include CalDAV/CardDAV. | https://searchfox.org/comm-central/source/mailnews/base/src/OAuth2Providers.sys.mjs |
| GNOME Online Accounts | Build-time meson options `google_client_id` / `google_client_secret` (written into a generated header as `GOA_GOOGLE_CLIENT_ID/SECRET`) and an `ms_graph_client_id` option. Distros/GNOME supply the values; the source tree does not. | https://github.com/GNOME/gnome-online-accounts/blob/master/meson_options.txt (seen via search index); LFS build guide passing `-D google_client_id=…` (secondary): https://www.linuxfromscratch.org/blfs/view/svn/gnome/gnome-online-accounts.html |
| GNOME / Microsoft 365 | Since GOA 3.50.1 a GNOME-provided Microsoft client ID is built in; users may override it with their own Entra app. | GNOME Discourse (secondary): https://discourse.gnome.org/t/how-do-i-add-a-onedrive-online-account/20469 |
| Evolution (evolution-data-server) | CMake options for Google / Microsoft 365 client IDs at build time, plus per-account override fields in the UI. | **unverified** (from memory) |
| vdirsyncer | Makes **every user** create their own Google Cloud project and "Desktop application" OAuth client, because "it is against Google's Terms of Service to hardcode those into opensource software". | https://vdirsyncer.pimutils.org/en/stable/config.html |
| khal | No network code; relies on vdirsyncer. | unverified |

**Recommendation for katlendar**

1. **Source tree:** no IDs or secrets. Read them with `option_env!("KATLENDAR_GOOGLE_CLIENT_ID")`,
   `option_env!("KATLENDAR_GOOGLE_CLIENT_SECRET")` and `option_env!("KATLENDAR_MS_CLIENT_ID")` at compile time.
   - Release CI injects them from repository secrets.
   - Distro packagers can inject their own.
   - Source builds without them simply hide the Google/Microsoft buttons. This satisfies "no secrets committed".
2. **Settings fallback:** "Use my own client ID" fields, the vdirsyncer route, with a short how-to.
   - The ID and secret entered there go to the keyring, not `config.json`.
3. Remember that `option_env!` values end up as plain strings in the binary. For Google this is acceptable,
   since Google itself says the installed-app secret is not secret. It just must not be in git.

---

## 6. Rust ecosystem survey

Constraints from the codebase:

- Cargo.toml has `eframe =0.37.0`, `ureq 3` (lock: 3.5.1, rustls), `chrono 0.4`, `chrono-tz 0.10` and `serde_json`.
- There is no async runtime.

| Crate | Latest (date) | Licence | Model | Verdict |
|---|---|---|---|---|
| `oauth2` | 5.0.0 (2025-01-21) | MIT/Apache | Sync **and** async; pluggable HTTP client | **Use.** Has `PkceCodeChallenge::new_random_sha256()`, an optional client secret (`BasicClient::new(ClientId)`), and device flow. Its optional `ureq` feature targets ureq **2**, so implement its `SyncHttpClient` (a closure `Fn(HttpRequest) -> Result<HttpResponse, E>`) over the existing ureq 3. Disable redirects on that client, as its docs advise (SSRF). |
| `google-calendar3` | 7.0.0+20251214 (2026-01-01) | MIT | async, hyper/tokio, yup-oauth2 | Skip. It brings tokio, and the hand-written REST calls katlendar needs are few (events.list/insert/patch/delete, calendarList.list). |
| `graph-rs-sdk` | 3.0.1 (2025-04-20) | MIT | async, reqwest | Skip, for the same reason. |
| `libdav` | 0.11.2 (2026-10-30) | ISC | async, tokio/hyper 1/tower | The best CalDAV client library, with RFC 6764 `bootstrap_via_service_discovery`. It is async, so it needs a tokio runtime on the worker thread (a `current_thread` runtime per account is feasible). Its docs point to `vstorage` for sync logic. |
| `vstorage` | 0.12.0 (2026-10-30) | **EUPL-1.2** | async | Licence compatibility with GPL-2.0-only needs checking: EUPL lists GPL-2.0 as a compatible licence, but the direction matters, so this is unverified. Probably skip. |
| `minicaldav` | 0.8.0 (2023-07-02) | **GPL-3.0-or-later** | blocking, ureq 2 | **Incompatible** if katlendar is GPL-2.0-*only*; check the LICENSE header. It also describes itself as work-in-progress and "not correct". Skip. |
| `kitchen-fridge` | 0.4.0 (2022-01-19) | MIT | async reqwest | Stale. Skip. |
| `fast-dav-rs` | 0.19.0 (2026-10-04) | LGPL-3.0 | async hyper | Same licence question. Skip. |
| `quick-xml` | 0.42.0 (2026-08-22) | MIT | sync | **Use** for WebDAV multistatus parsing (serde or streaming). |
| `roxmltree` | 0.21.2 (2026-10-24) | MIT/Apache | sync, read-only DOM | Simpler alternative for parsing multistatus replies. Requests can be `format!` templates. |
| `icalendar` | 0.17.17 (2026-10-26) | MIT/Apache | sync | Has a parser feature, and a `recurrence` feature (rrule + chrono-tz). An option if the hand-written `ics.rs` is retired. |
| `calcard` | 0.3.14 (2026-09-15) | Apache/MIT (unverified) | sync | Stalwart's iCalendar/vCard/JSCalendar library, with chrono-tz and a JMAP feature. Strong for round-tripping unknown properties. |
| `ical` | 0.11.0 (2024-03-13) | "non-standard" on crates.io | sync | Skip. |
| `rrule` | 0.14.0 (2025-04-20) | MIT/Apache | sync, chrono + chrono-tz | **Use** to replace `ics::expand` if BYSETPOS/BYMONTH/WKST are needed, which Graph `relative*` patterns require. |
| `keyring` | 4.2.0 (2026-08-29) | MIT/Apache | sync | Its docs tell *applications* to depend on `keyring-core` plus an explicit store. |
| `keyring-core` | 1.0.1 (2026-10-13) | MIT/Apache | sync | **Use**: `set_default_store(...)`, `Entry::new(service, user)`. |
| `zbus-secret-service-keyring-store` | 1.0.1 (2026-08-15) | MIT/Apache | sync API over zbus | **Use** on Linux. Pure Rust: no libdbus. Pick `crypto-rust`. |
| `dbus-secret-service-keyring-store` | 1.0.1 (2026-08-15) | MIT/Apache | sync, libdbus | Alternative that needs the system libdbus. |
| `oo7` | 0.7.1 (2026-11-10) | MIT | **async only** | Speaks the Secret Service, and in Flatpak uses the portal + a libsecret-compatible file. Prefer it if a Flatpak build is planned; otherwise keyring-core is simpler. |
| `tiny_http` | 0.12.0 (2022-10-06) | MIT/Apache | sync | Old, but enough for a one-shot loopback listener. A bare `std::net::TcpListener` that parses one request line is also fine and adds no dependency. |
| `open` 5.4.5 / `webbrowser` 1.2.7 | — | MIT/Apache | sync | Launch the system browser for OAuth and Nextcloud login. `open` uses `xdg-open`/portal. |

**Threading pattern (keep it)**

`src/main.rs` already runs `sync_one`:

```text
std::thread::spawn → ics::fetch(url) → sync_tx.send((url, result)) → ctx.request_repaint()
```

`poll_sync` drains `sync_rx` with `try_recv` once per frame. Extend that pattern rather than adding tokio:

- one long-lived worker thread per **account**, owning its provider client and token cache;
- a command channel UI→worker (`SyncNow`, `Push(change)`, `Cancel`, `Shutdown`);
- the existing result channel generalised to `enum SyncMsg { Calendars(..), Delta(..), PushResult(..), AuthNeeded(..), Error(..) }`;
- `request_repaint()` after every send, which is already done.

If `libdav` or `oo7` is adopted later, that worker can own a `tokio::runtime::Builder::new_current_thread()`
runtime privately. The UI thread never sees async.

---

## 7. Architecture sketch

### 7.1 Data model changes (`src/model.rs`)

Current shape, which blocks two-way sync:

- `Event.calendar: usize` is an **index** into `Data.calendars`. Deleting or reordering a calendar
  re-targets events.
- `Event` has no UID, href, etag or recurrence-id. The parser drops `UID` after using it for override matching.
- `Data::replace_remote(cal, feed)` throws away all remote events of the calendar and re-creates them with new
  `id`s on every sync. Any selection, open dialog, or `task_id` link to a remote event breaks each refresh.
- `Calendar.url: Option<String>` doubles as the "is remote" flag. `first_own_calendar()` relies on it.

Proposed shape (names indicative):

```rust
pub struct Account {
    pub id: u64,
    pub kind: ProviderKind,         // CalDav | Nextcloud | Google | Microsoft | IcsFeed
    pub label: String,              // shown in UI; no credential here
    pub server: Option<String>,     // CalDAV base / principal URL
    pub username: Option<String>,   // login name; secret lives in the keyring
}

pub struct Calendar {
    pub id: u64,                    // stable; events reference this
    pub account: Option<u64>,       // None = local-only calendar
    pub remote_id: Option<String>,  // CalDAV collection href / Google calendarId / Graph id
    pub sync_state: Option<String>, // sync-token, nextSyncToken, or Graph deltaLink
    pub ctag: Option<String>,
    pub read_only: bool,
    // existing: name, color, visible, last_sync, error
}

pub struct Event {
    pub calendar: u64,              // was usize
    pub uid: Option<String>,
    pub recurrence_id: Option<NaiveDateTime>,
    pub href: Option<String>,       // CalDAV resource / provider event id
    pub etag: Option<String>,
    pub raw: Option<String>,        // original VEVENT text to preserve unknown props on write
    pub dirty: bool,                // edited locally, not yet pushed
    // existing fields...
}
```

- Migrate `data.json` on load by assigning ids in current index order. `serde(default)` covers the new fields.
- `replace_remote` becomes `apply_delta(cal_id, upserts, deletes)`, keyed by `(uid, recurrence_id)` or by `href`.
  Local `id`s stay stable.

### 7.2 Provider trait (`src/sync/mod.rs`)

Blocking, object-safe, and run only on worker threads:

```rust
pub trait Provider: Send {
    fn list_calendars(&mut self) -> Result<Vec<RemoteCalendar>, SyncError>;
    /// Full sync when `state` is None; incremental otherwise. Returns new state.
    fn pull(&mut self, cal: &RemoteCalendar, state: Option<&str>) -> Result<Delta, SyncError>;
    /// Create when `etag` is None (If-None-Match: *), update otherwise (If-Match).
    fn put(&mut self, cal: &RemoteCalendar, ev: &OutgoingEvent, etag: Option<&str>)
        -> Result<PutOk /* href, new etag */, SyncError>;
    fn delete(&mut self, cal: &RemoteCalendar, href: &str, etag: Option<&str>) -> Result<(), SyncError>;
}

pub enum SyncError {
    Auth,                 // 401 / invalid_grant → prompt re-login
    Conflict,             // 412 → refetch, resolve
    TokenExpired,         // Google 410 / invalid sync-token → full resync
    RateLimited(Duration),
    Network(String),
    Other(String),
}
```

Implementations:

- `CalDav`: iCloud, Fastmail, Nextcloud, Radicale, Yahoo, and Google-CalDAV with a bearer token.
- `Google` (REST), `Graph`, and `IcsFeed` (wrapping today's `ics::fetch`, with pull only).
- An auth abstraction (`Basic { user, pass } | Bearer(TokenSource)`) sits under `CalDav`, so Google can reuse it.

### 7.3 Sync loop and conflicts

Per calendar:

1. Push the outbox first: local creates, updates and deletes marked `dirty`.
2. Then pull.

Conflict handling:

- CalDAV and Google return **412 Precondition Failed** on etag mismatch. Graph supports `If-Match` on PATCH
  as well (unverified for events).
- On 412:
  - refetch the server copy;
  - default to **server wins**;
  - keep the local version as a copy and surface "conflict, your edit was saved as a copy".
- Never silently overwrite.

Backoff and scheduling:

- Exponential backoff on 429/503/`usageLimits`.
- Serialise requests per Microsoft account, because of the 4-concurrent limit.
- Polling interval: 5–15 minutes, plus "sync now", plus on app focus. Google push and CalDAV push are not
  available to a desktop app.

### 7.4 Recurrence and time-zone impact on `src/ics.rs`

Today's parser:

- handles `FREQ` DAILY/WEEKLY/MONTHLY/YEARLY, `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (with nth) and `BYMONTHDAY`,
  plus EXDATE and RECURRENCE-ID overrides;
- ignores `BYMONTH`, `BYSETPOS`, `BYYEARDAY`, `BYWEEKNO` and `WKST`;
- ignores VTIMEZONE, and treats unknown TZIDs as floating;
- does not handle VTODO.

Gaps that matter for two-way sync:

| Gap | Hit by | Fix |
|---|---|---|
| `BYMONTH`, `BYSETPOS`, `WKST` | Graph `relativeMonthly/Yearly`, Apple/Google "last weekday of month", yearly rules | Extend `expand()`, or delegate to `rrule` 0.14 |
| Windows TZIDs ("W. Europe Standard Time") | Outlook ICS exports; Graph | CLDR windowsZones table → `chrono_tz::Tz` |
| Custom/legacy TZIDs with VTIMEZONE (e.g. `/freeassociation.sourceforge.net/…`, Lotus/Exchange GUID-style) | Older servers, Exchange exports (unverified frequency) | Strip known prefixes and match the tail against chrono-tz. Last resort: honour VTIMEZONE STANDARD/DAYLIGHT offsets for the event's year. |
| Round-trip fidelity | Every write | Keep the raw VEVENT (`Event.raw`). On edit, patch only SUMMARY/DTSTART/DTEND/DESCRIPTION/LOCATION/RRULE and bump `SEQUENCE`/`DTSTAMP`, so alarms, attendees and X-props survive. |
| Editing one occurrence | Every provider | Needs RECURRENCE-ID overrides written back (CalDAV: same resource, extra VEVENT; Google: instance event; Graph: occurrence id). Defer "this and following" splits. |
| VTODO | CalDAV task lists (Nextcloud, Fastmail, Radicale) | Parse VTODO → `Task`. `PRIORITY` already uses iCal semantics (0–9), and `DUE` maps onto `Task.due`. |

On the Graph read path, expansion happens server-side via `calendarView`. Graph writes need the
RRULE ↔ `patternedRecurrence` mapping in §4.2, and must refuse rules Graph cannot express.

### 7.5 Credential storage (Linux)

- **Never** put passwords, app passwords, refresh tokens, or Proton share URLs in `data.json` or `config.json`.
- Use `keyring-core` + `zbus-secret-service-keyring-store`. That means GNOME Keyring or KWallet's Secret Service
  interface; KWallet ≥ 5.97 provides one (unverified version number).
  - Entry: service `"katlendar"`, user `"account:<id>"`.
  - Value: JSON `{ "kind": "app_password", ... }` or `{ "refresh_token": ..., "scope": ... }`.
- Access tokens stay in memory only.
- When no Secret Service is running (bare window managers, some headless setups), fall back in this order:
  1. refuse and explain how to start a keyring, or
  2. opt-in plaintext file `$XDG_DATA_HOME/katlendar/secrets.json` with mode `0600`, clearly labelled.
  - Never fall back silently.
- For Flatpak, `oo7`'s portal backend is the supported route (async).
- Logout:
  - Revoke at the provider: Google's `/revoke`, Nextcloud's `DELETE /ocs/v2.php/core/apppassword`.
  - Then delete the keyring entry.

### 7.6 OAuth loopback flow (Google, Microsoft)

1. Bind `TcpListener` on `127.0.0.1:0`, then read the port.
2. Generate PKCE (S256) and a random `state`.
3. Open the authorize URL with `redirect_uri=http://127.0.0.1:{port}/` via `open`.
4. Accept **one** connection, parse the `GET /?code=…&state=…` line, and reply with a tiny HTML "you can close this tab".
5. Check `state`, then exchange the code. Use the `oauth2` crate over a ureq-3 closure, sending `client_secret` only if configured.
6. Store the refresh token in the keyring.
7. Use a timeout (2–5 min) and a Cancel button in the UI. The listener lives on the worker thread, not the UI thread.

Google and Microsoft both accept any loopback port (RFC 8252 §7.3; the Entra reply-url doc above).
Microsoft wants `127.0.0.1` registered literally; Google desktop clients accept the loopback IP without
pre-registering a port.

---

## 8. Risks and open questions

1. **Google verification.**
   - Is anyone willing to own a Google Cloud project and a public privacy-policy page for katlendar, and go through
     sensitive-scope verification?
   - If not, Google support is "bring your own client ID" only, with the 7-day testing-token pain unless users
     publish their own unverified app.
   - The cheaper first step is **Google via CalDAV**, which shares the OAuth problem but not a second data mapper.
2. **Microsoft tenant consent.** Without publisher verification, work/school users may need admin approval.
   Personal Outlook.com accounts should work. This needs testing with a fresh personal account and a test tenant.
3. **Licence.**
   - Confirm whether katlendar is GPL-2.0-only or GPL-2.0-or-later.
   - GPL-3.0-or-later (`minicaldav`) and LGPL-3.0 (`fast-dav-rs`) code can't be linked into GPL-2.0-only.
   - EUPL-1.2 (`vstorage`) compatibility is unclear.
   - The recommended stack (oauth2, quick-xml/roxmltree, keyring-core, rrule) is MIT/Apache/ISC.
4. **Data-model migration** is the riskiest code change: index → id touches every view.
   - Do it first, behind tests.
   - Load an old `data.json`, save, and reload, checking that events keep their calendar.
5. **Write fidelity.** katlendar's model is much smaller than iCalendar (no attendees, alarms, attachments).
   - Without `Event.raw` preservation, editing a title in katlendar would strip invites and reminders on the server.
6. **Recurring-event editing UX** ("this event / this and following / all") is substantial. Phase 3 could
   initially make recurring remote events read-only.
7. **Polling cost and battery.** Incremental tokens make polls cheap. Still, avoid running sync while the window
   is hidden or minimised, and back off on errors.
8. **iCloud details** (canonical host and sync-collection support) come from community knowledge,
   not Apple documentation. Verify them against a real account before advertising them.
9. **EWS retirement date and the absence of Outlook CalDAV** are unverified. Neither changes the recommendation
   (Graph), but the docs/README should not state dates without a source.
10. **Proton links** go stale in third-party apps by Proton's own admission. Show a "last updated" timestamp,
    which already exists as `Calendar.last_sync`, and a warning.
11. **Testing:**
    - Run Radicale in CI for the CalDAV provider.
    - Record/replay fixtures for Google and Graph JSON.
    - Unit-test the RRULE ↔ `patternedRecurrence` mapping both ways.
12. **Task sync** spans three incompatible models:
    - CalDAV VTODO (rich);
    - Google Tasks (date-only due, no duration);
    - Microsoft To Do (own schema).
    katlendar's `Task` has fields (`duration_min`, `flexible`) that none of them represent. Store those in
    `X-KATLENDAR-*` properties on CalDAV and locally elsewhere.
