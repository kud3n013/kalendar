# Connecting Google and Microsoft calendars

kalendar has no sign-in registration of its own with Google or Microsoft.
You make one, once, in your own account, and paste its ID into kalendar.
It takes about five minutes per service. Your calendar data still goes
only between your computer and Google or Microsoft: the registration just
tells their sign-in page which program is asking.

Each section below lists what to click, then what to paste, then the
errors people usually hit.

- [Google Calendar](#google-calendar)
- [Microsoft 365 and Outlook.com](#microsoft-365-and-outlookcom)
- [No registration: read-only links](#no-registration-read-only-links)

---

## Google Calendar

You need a Google account and about five minutes. Nothing here costs
money and no billing account is needed.

### 1. Make a project

1. Open the [Google Cloud console](https://console.cloud.google.com/) and
   sign in with the Google account whose calendars you want.
2. Use the project picker at the top → **New project**. Any name works,
   for example `kalendar`. Make sure the new project is selected before
   going on.

### 2. Switch on the CalDAV API

kalendar talks to Google over CalDAV, so this is the one API it needs.

1. Open the [CalDAV API page](https://console.cloud.google.com/apis/library/caldav.googleapis.com).
2. Click **Enable**.

You do *not* need the "Google Calendar API".

### 3. Set up the sign-in screen

1. Open [Google Auth Platform → Branding](https://console.cloud.google.com/auth/branding)
   and click **Get started** if asked.
2. App name: `kalendar` (anything works). Support email: your own.
3. Audience: **External**. Contact email: your own. Agree and **Create**.

### 4. Stop the weekly sign-out (recommended)

A project left in **Testing** hands out sign-ins that expire after
7 days, so kalendar would ask you to sign in again every week. To avoid
that:

1. Open [Audience](https://console.cloud.google.com/auth/audience).
2. Click **Publish app** → **Confirm**.

You do not need Google to verify the app for your own use. The only
difference is that the sign-in page warns that the app is unverified;
see step 6.

If you'd rather stay in Testing, add your own address under
**Test users** on the same page instead, and expect to sign in again
weekly.

### 5. Make the client ID

1. Open [Clients](https://console.cloud.google.com/auth/clients) →
   **Create client**.
2. Application type: **Desktop app**. This matters: other types fail with
   `redirect_uri_mismatch`. Name: anything.
3. Click **Create**. Copy the **Client ID** and the **Client secret**, or
   click **Download JSON**.

### 6. Sign in from kalendar

1. In kalendar: Settings → Calendars → **Google**.
2. Fill in your Google address. Paste the client ID and client secret,
   or paste the whole downloaded JSON file into the client ID box; kalendar
   takes both values out of it.
3. Click **Sign in with Google…**. Your browser opens Google's page.
4. If you published the app, Google says *"Google hasn't verified this
   app"*. Click **Advanced** → **Go to kalendar (unsafe)**. This only
   means Google hasn't reviewed a project you made yourself.
5. Allow access to your calendars. The browser says you're signed in and
   kalendar lists your calendars.

The sign-in is kept in your desktop's keyring. Removing the account in
kalendar also tells Google to forget it.

### Google: when it goes wrong

| What you see | Why | Fix |
| --- | --- | --- |
| `redirect_uri_mismatch` | The client isn't a Desktop app | Make a new client of type **Desktop app** (step 5) |
| `access_denied` or "has not completed the Google verification process" | The app is in Testing and you aren't a test user | Publish the app (step 4) or add yourself as a test user |
| "CalDAV API has not been used in project…" | The API is off, or it was switched on in another project | Enable it in the same project as the client (step 2) |
| kalendar asks you to sign in again after a week | The app is still in Testing | Publish it (step 4), then sign in once more |
| `invalid_client` | The ID or secret was cut off when copying | Paste the downloaded JSON instead |

---

## Microsoft 365 and Outlook.com

Works for work or school accounts (Microsoft 365) and personal ones
(Outlook.com, Hotmail, Live). There is no client secret.

### 1. Open the app registrations

Open [Microsoft Entra admin center → App registrations](https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)
and sign in.

- **Work or school account:** you can usually register apps yourself. If
  you see "you do not have access", ask your IT admin, or use the steps
  below with a personal account instead.
- **Personal account only:** registering an app needs a Microsoft Entra
  directory, which you get by signing up for an
  [Azure free account](https://azure.microsoft.com/free/). No paid service
  is needed. Then come back to the link above.

If Azure says *"You're not eligible for an Azure free account"* or *"Your
current account type is not supported"*:

- The free offer is only for people new to Azure. A card, phone number or
  address used on Azure before, even under another Microsoft account, is
  turned down. Try a card and number never used there.
- A Microsoft account made from a Gmail or other non-Microsoft address
  can count as the wrong kind of account. Sign up with an Outlook.com
  address instead.
- Pick **Pay-As-You-Go** instead of the free offer. It asks for a card,
  but registering an app costs nothing; you are billed only for Azure
  services you start, and this needs none.
- If none of that works, use a [read-only link](#no-registration-read-only-links)
  instead.

### 2. Register the app

1. **New registration**.
2. Name: `kalendar`.
3. Supported account types: **Accounts in any organizational directory
   and personal Microsoft accounts**. Pick this even if you only have one
   kind of account; the narrower choices fail with `AADSTS700016` or
   `AADSTS50020` for the other kind.
4. Redirect URI: platform **Public client/native (mobile & desktop)**,
   address `http://localhost` (exactly that: `http`, no port, no slash).
5. **Register**.

### 3. Add the calendar permission

1. In the new app: **API permissions** → **Add a permission** →
   **Microsoft Graph** → **Delegated permissions**.
2. Tick **Calendars.ReadWrite** → **Add permissions**.

`User.Read` is there already; leave it. `offline_access` is asked for at
sign-in, so you don't have to add it.

### 4. Copy the ID

On the app's **Overview** page, copy **Application (client) ID**. It
looks like `1a2b3c4d-…`. Don't copy the Directory (tenant) ID next to it.

### 5. Sign in from kalendar

1. In kalendar: Settings → Calendars → **Microsoft**.
2. Fill in your Microsoft address and paste the Application (client) ID.
3. Click **Sign in with Microsoft…**, sign in in the browser and accept.

### Microsoft: when it goes wrong

| What you see | Why | Fix |
| --- | --- | --- |
| `AADSTS50011` (redirect URI mismatch) | The redirect isn't `http://localhost` under the mobile & desktop platform | Authentication → add platform **Mobile and desktop applications** → `http://localhost` |
| `AADSTS7000218` (client secret required) | The redirect was added under the **Web** platform | Remove the Web one; add it under **Mobile and desktop applications** |
| `AADSTS700016` (app not found) | You pasted the tenant ID, or the account types are too narrow | Paste the Application (client) ID; set account types as in step 2.3 (Authentication → Supported account types) |
| `AADSTS50020` (account not in tenant) | Account types set to this directory only | Same as above |
| "Need admin approval" | Your organisation doesn't let users approve apps | Ask an admin to grant consent on **API permissions** → **Grant admin consent** |
| Signed in, but no calendars | `Calendars.ReadWrite` wasn't added | Add it (step 3), then remove and add the account again in kalendar |

---

## No registration: read-only links

If you can't make a registration, both services give you a private link
to your calendar that kalendar can subscribe to. You see your events and
they refresh on their own, but changes made in kalendar don't go back.

**Google:** open [Google Calendar settings](https://calendar.google.com/calendar/r/settings),
pick the calendar on the left, and copy **Secret address in iCal format**.

**Outlook.com and Microsoft 365:** open
[Outlook calendar settings → Shared calendars](https://outlook.live.com/calendar/0/options/calendar/SharedCalendars)
(for a work account, the same page in Outlook on the web), under
**Publish a calendar** pick the calendar and **Can view all details**,
click **Publish**, and copy the **ICS** link. Some organisations switch
publishing off.

In kalendar: Settings → Calendars → **Calendar feed**, and paste
it. Anyone with the link can read the calendar, so keep it private; both
pages let you reset it.

---

## Notes

- **Why bring your own?** A shared registration built into the app would
  need Google's paid security review and could be switched off for
  everyone at once. One of your own only answers to you.
- **What's stored:** the client ID in kalendar's data file; the sign-in
  (refresh token, and for Google the client secret) in your desktop's
  keyring.
- **One registration, many accounts:** the same Google client or Microsoft
  app can sign in several of your accounts. For Google, publish the app
  or add each address as a test user.
