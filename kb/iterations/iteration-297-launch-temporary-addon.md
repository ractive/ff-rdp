---
branch: iter-297/launch-temporary-addon
date: 2026-11-16
depends_on: []
status: done
type: iteration
title: "Iteration 297: launch --temporary-addon loads an unsigned WebExtension over RDP"
tags:
  - iteration
  - launch
  - addons
  - webextension
---

# Iteration 297: `launch --temporary-addon`

Another project is testing a Firefox build of its extension: a `document_start`
content script defines `document.modelContext` on each page, and the harness
checks it with `ff-rdp eval` on real sites. ff-rdp could not load it:
`--auto-consent` drops a signed `.xpi` into `<profile>/extensions/`, and release
Firefox refuses unsigned ones there. A temporary install over RDP (the
about:debugging / `web-ext run` path) needs no signing and is gone when Firefox
exits.

```
ff-rdp launch --headless --temporary-addon ./test/firefox-spike/extension https://example.test/
ff-rdp eval "document.modelContext?.constructor?.name"
```

Research: [[research/temporary-addon-install]]. Protocol: [[rdp/actors/addons]].

## Tasks [6/6]

- [x] Protocol research against mozilla-central: request/reply/error shapes,
      `uninstallAddon`, versions, prefs, MV3 host-permission granting.
- [x] Core: `AddonsActor` (`crates/ff-rdp-core/src/actors/addons.rs`) with a typed spec
      (`specs/addons.rs`); replies recorded from Firefox via `live_record_fixtures.rs`.
- [x] CLI: `launch --temporary-addon <path>` (repeatable); paths validated before Firefox
      starts; URL withheld from Firefox's command line and opened after the installs.
- [x] Results in `results.temporary_addons`; a rejected install fails the launch and
      stops the launched Firefox.
- [x] MV3 fixture extension and live tests (directory, `.xpi` built at test time, bad
      manifest); e2e and unit tests for path validation, ordering and teardown.
- [x] `--help`, skill drift, kb actor docs, backlog lines for `addon install/list` on a
      running Firefox and for extension debugging.

## Acceptance criteria [7/7]

- [x] `ff-rdp launch --headless --temporary-addon <dir> <http URL>` exits 0 and
      `results.temporary_addons[0]` names the absolute path and the add-on id.
- [x] The first page command run right after that launch (no retry) reaches the new
      document.
- [x] The first page of that launch, with no reload, carries the attribute the
      fixture's `document_start` content script sets (MV3, no user action).
- [x] The same works with the fixture packed as an `.xpi`.
- [x] A `--temporary-addon` path that does not exist (or a directory without
      `manifest.json`) fails with a `User` error before Firefox starts.
- [x] A manifest Firefox rejects fails the launch with Firefox's message, and the
      Firefox the launch started is gone (port closed, managed profile removed).
- [x] Without `--temporary-addon`, `launch` passes the URL to Firefox at startup as
      before.
