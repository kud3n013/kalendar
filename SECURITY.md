# Security Policy

## Reporting a vulnerability

Please report vulnerabilities privately via GitHub's
["Report a vulnerability"](../../security/advisories/new) form on this
repository. Do not open a public issue for security problems.

You can expect an acknowledgement within a week.

## Scope notes

- Kalendar is a local-only application: it keeps all data in a single JSON
  file on your machine and makes network requests only to calendar feed
  URLs you subscribe to.
- Subscription URLs are stored in plain text in the data file. For
  providers that embed access keys in the link (for example Proton
  Calendar), anyone who can read `data.json` can read that calendar.
  Reports that improve this are welcome.
