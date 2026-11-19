---
title: "Iteration 302: Windows launch fails with listTabs currentWindowGlobal is null"
type: iteration
date: 2026-11-18
status: in-review
branch: iter-302/windows-listtabs-launch-failure
depends_on: []
tags: [iteration, windows, launch]
---

# Iteration 302: Windows launch fails with "listed no tab … currentWindowGlobal is null"

On GitHub's windows-latest runner, `ff-rdp launch` (no URL, headless or headed) sometimes fails
after Firefox opened its debug port:

```text
opened debug port 6123 but listed no tab within 30s (last listTabs error: … TypeError — can't
access property "outerWindowId", browsingContext.currentWindowGlobal is null)
```

It failed 6 attempts of the `windows-launch` workflow across #383, #385 and #387 (backlog line
"windows-launch: a headless `launch` on windows-latest"). On lenny, a German Windows 11 machine
with Firefox 160, it did not reproduce in 80 launches. Machine, lab, baseline, CI run IDs and
scripts: [[windows-lenny-baseline-2026-11]]. **Read that first**, especially the rule about the
owner's desktop Firefox on lenny.

The launch path in question: `crates/ff-rdp-cli/src/commands/launch.rs` (first-tab wait after
the debug port opens) and `crates/ff-rdp-cli/src/commands/launch/` (`launcher_process.rs`, and
`startup.rs`). The server side of the error is Firefox's `listTabs` in
`devtools/server/actors/` (root and tab descriptors): it reads
`browsingContext.currentWindowGlobal.outerWindowId` and throws while the first tab's browsing
context has no window global.

## Tasks

- [ ] Re-measure on lenny with current main (#387 merged): 20 headless + 20 headed launches with
      `baseline.ps1`, to confirm 0 failures and launch time back near v0.5.0's ~2.2 s headless
- [ ] Try to reproduce the CI failure on lenny, one variable at a time, ≥30 runs each:
  - [ ] CPU load (e.g. a parallel `cargo build` or a busy loop pinned to most cores), and a
        4-core affinity for Firefox (`start /affinity F`) to mimic the 4-vCPU runner
  - [ ] the runner's sequence: back-to-back launches with a force-kill of the previous instance
        (by PID only!) and no wait in between
  - [ ] Firefox 159.0.1 (the runner's version), unpacked portable next to the installed 160.
        ff-rdp only looks in `C:\Program Files\Mozilla Firefox`, so this needs an override
        (e.g. `FF_RDP_FIREFOX=<path>`). If you add one, it is a product change: help text, a
        test, and an e2e/unit test of the lookup
- [x] If it reproduces: capture `--log-level trace` for a failing launch (listTabs replies over
      the 30 s) and find what keeps the first tab without a window global
- [x] Fix in ff-rdp, if ff-rdp can do anything about it. For example, treat this listTabs error
      as "not ready yet" and keep polling, or open a tab with `newTab` once the root actor answers
      but lists no usable tab. Either way, launch must not fail on it
- [ ] If it never reproduces off the runner: add diagnostics to the `windows-launch` workflow
      (trace log on failure as an artifact) instead of guessing, and record that in the backlog
      line

### Outcome (2026-11-19)

- Re-measure: done by the owner on Firefox 160.0.1 (non-elevated), but it did **not** confirm
  0 failures — the CI failure reproduced (1/15, 2/60). Launch time: median 2.5 s headless. Left
  unticked: its premise (0 failures) was wrong.
- Reproduce: it reproduced without any of the listed variables (CPU load, kill sequence,
  Firefox 159 were not tried), so those sub-tasks stay unticked.
- Cause: Firefox's sandbox cannot start any tab process (`Failed to launch tab subprocess
  @SB::LA::SpawnTarget (Error:-2147024809)` 17–20 times in raw stderr, captured with
  `--log-level debug` + `FF_RDP_TRACE_RAW=1` rather than trace), so the first tab never gets a
  window global. A harness without ff-rdp reproduces it; raw Firefox from PowerShell did not
  (0/150). Why the sandbox fails: unknown. Lab notes: [[windows-lenny-baseline-2026-11]].
- Fix: `launch` detects this (marker in the raw startup stderr + the listTabs error, 8 s after
  the first marker), stops the instance and relaunches once; a second dead instance fails with
  "could not start its content processes, twice". Polling longer or `newTab` cannot help here.
  Caveat on the tick above: two dead instances in a row still fail the launch (plainly, with
  their own error), and the relaunch path is proven by unit tests only unless a real relaunch
  fired in the runs below.
- Diagnostics: added to `windows-launch` anyway (debug + raw stderr of every call, uploaded on
  failure; relaunch warnings noted in the step summary), though the failure did reproduce.

## Acceptance criteria

- [ ] A Windows `launch` without a URL never fails with "listed no tab … currentWindowGlobal is
      null": proven on lenny under the condition that reproduced it (0 failures in ≥30 runs
      after the fix, with the failing count before), and `windows-launch` green on the PR
      without a re-run
- [ ] The owner's desktop Firefox on lenny is untouched by every test run (the `firefox.exe`
      count before = after, recorded in the PR body)
- [ ] The backlog line is closed with this PR, or updated with what was learned if the cause
      lives outside ff-rdp
