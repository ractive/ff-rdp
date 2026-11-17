---
title: "Iteration 300: launch waits for the startup URL"
type: iteration
date: 2026-11-17
status: done
branch: iter-300/launch-startup-url-wait
depends_on: []
tags: [iteration, launch]
---

# Iteration 300: launch waits for the startup URL

`ff-rdp launch --headless <URL>` returned as soon as a tab was listed. `listTabs` already
reports the startup URL at that point, but the tab's document is still the initial
`about:blank` (`readyState` `uninitialized`), so the next command read the wrong page: an
immediate `eval` saw about:blank in 22 of 30 runs (Firefox 160, local http page). The
`--temporary-addon` path waited for its own `navigateTo`, yet the setup-ff-rdp smoke once read
`undefined` for the content script's mark right after it (run 44127537614, macos-latest).
Agents chain `launch <URL>` with a page command, so `launch` must not return before the page is
there.

## Tasks

- [x] Reproduce both variants 30× on macOS with a local http page and record what the failing
      read saw
- [x] One wait, `launch/startup_url.rs::wait_for_document`: poll the way the next command
      reaches the page (fresh `listTabs`, selected tab, `getTarget`, one evaluation) until the
      document is the URL's (or a redirect's) with `readyState` `complete`; an error page ends it
- [x] Plain path: keep the URL on Firefox's command line (file: and about:support need that
      route), then wait within `--timeout`; a failed wait is a `results.warnings` entry
- [x] `--temporary-addon` path: the same wait after `navigateTo` (replaces the pre-140
      `listTabs` commit poll); when the tab settles back on its old page, open the URL again
      (at most twice; Firefox 140+ only, where `navigateTo` replied at the load's stop)
- [x] Help text, `--url` doc, README; skill_drift snapshot
- [x] Live test `live_300_launch_startup_url` (plain and `--temporary-addon`, immediate read);
      `live_launch_positional_url` reads once instead of polling

## Acceptance Criteria [3/4]

- [x] `ff-rdp launch --headless --temp-profile <local http URL>` followed at once by
      `ff-rdp eval` reads that URL with `readyState` `complete` in 30 of 30 runs (was 8 of 30).
- [x] The same holds with `--temporary-addon <dir>`, and the content script's mark is present,
      in 30 of 30 runs.
- [x] `ff-rdp launch --headless --temp-profile` without a URL takes no longer than before.
- [ ] The setup-ff-rdp smoke's `--temporary-addon` flake (backlog) does not recur — cannot be
      shown here: the smoke runs released binaries, and the local form of it (2 of 290 live
      runs) is too rare for 100 clean runs with re-navigation to prove it gone; see Design
      notes.

## Design notes

- `listTabs` is not evidence of a loaded document: it shows the startup URL while the content
  document is still the initial about:blank. The wait therefore reads `location.href`,
  `document.readyState` and `document.documentURI` through the console of the target a fresh
  `getTarget` hands out — exactly what the next command gets.
- Navigating over RDP after start (the add-on path's approach) for the plain path too was
  rejected: content-process `navigateTo` to file: kills Firefox 158 and about:support is
  refused there (`url_validation::validate_content_navigation_url`); the command-line route
  handles both.
- The add-on flake: Firefox's tab-descriptor `navigateTo(waitForLoad)` resolves on any top-level
  `STATE_STOP` whose `originalURI` is the URL and never checks the status, so a load that was
  aborted replies like one that finished; the tab then keeps its old page (inferred) — a loaded
  about:blank has a `documentElement`, so `dataset.smoke` reads `undefined`, as the smoke saw,
  with no warning. Locally the live test caught it twice in 290 runs (before re-navigation):
  launch warned "not loaded after 9911 ms" of a 10 s budget, i.e. `navigateTo` had replied
  within ~90 ms and the tab never showed the URL loaded; what it showed was not recorded. What aborts the load is not confirmed (likely
  Firefox's own startup load of the first window's page). The wait now opens the URL again
  once the tab has sat on its old, fully loaded page for three polls (at most twice), and a
  timeout names the last states the tab showed.
- Rejected for the add-on case: trusting `navigateTo`'s reply (the cause above) and a content-
  script injection race — Firefox's `Extension:Startup` broadcast is not awaited by
  `Extension.runManifest`, but it travels the same IPC channel ahead of the navigation.

## Out of scope

- The Windows "listed no tab within 30s (`currentWindowGlobal` is null)" flake (backlog): it
  fails inside the first-tab `listTabs` wait, on launches without a URL, before this wait runs.

## References

- [[iteration-297-launch-temporary-addon]]
- [[iteration-299-windows-launcher-process]]
