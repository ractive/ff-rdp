---
title: "Iteration 15: Launch & Connection Reliability"
status: completed
date: 2026-04-06
tags:
  - iteration
  - bugfix
  - launch
---

# Iteration 15: Launch & Connection Reliability

From dogfooding session (2026-04-06): launching Firefox and establishing a reliable
connection was the biggest pain point. These are "can't even get started" blockers
that must be fixed before anything else.

## Bugs

- [x] **`launch` doesn't pass `-no-remote`** — when another Firefox instance is already
      running (e.g., user's regular browser), `launch --temp-profile` spawns a process but
      it silently fails to bind the debugger port. The manual workaround was
      `firefox -no-remote -profile /tmp/... --start-debugger-server 6000`. `launch` should
      always pass `-no-remote` to ensure the new instance is fully independent.

- [x] **`launch` doesn't verify port binding** — `launch` returns success JSON even when
      Firefox fails to bind the debugger port (e.g., port conflict with another instance).
      Should probe the port after launch and return an error if unreachable within timeout.

- [x] **`launch --temp-profile` shows Firefox welcome/onboarding screen** — a fresh profile
      triggers the first-run welcome page (`about:welcome`) and possibly a session restore
      page (`about:sessionrestore`), which can block automation. Investigate how to suppress
      this — likely via `user.js` prefs written into the temp profile before launch, e.g.:
      `browser.aboutwelcome.enabled = false`, `browser.startup.homepage_override.mstone = "ignore"`,
      `datareporting.policy.dataSubmissionEnabled = false`, `toolkit.telemetry.reportingpolicy.firstRun = false`.
      Ensure `launch --temp-profile` creates a truly automation-ready profile out of the box.

- [x] **`network` returns empty after page load** — on comparis.ch (43 external scripts,
      heavy network activity), `network` returned `[]`. The WatcherActor likely needs to be
      attached *before* navigation to capture requests. Consider: (a) documenting this
      limitation, (b) adding a `--replay` flag that retrieves already-completed requests
      from the netmonitor cache, or (c) auto-attaching the watcher on connect.

## Acceptance Criteria

- [x] `launch --temp-profile` works reliably when another Firefox is already running
- [x] `launch` returns an error if the debug port is not reachable after startup
- [x] Fresh temp profiles open to `about:blank`, no welcome/onboarding/session-restore
- [x] `network` captures requests for an already-loaded page (or clearly documents the limitation)
- [x] All existing tests still pass
