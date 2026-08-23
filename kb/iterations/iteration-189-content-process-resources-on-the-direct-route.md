---
title: "Iteration 189: audit every other content-process resource subscription on the direct route — iteration 174 fixed only the two navigation waits"
type: iteration
date: 2026-08-23
status: planned
branch: iter-189/content-process-resources-direct-route
depends_on:
  - iteration-174-direct-route-reload-never-sees-dom-complete
first_call_sites: []
dogfood_path: |
  # THIS PLAN CLAIMS NO DEFECT. Iteration 174 measured one instance of a
  # class; whether the class has other members is UNMEASURED. The first task
  # is to find out, and "all four are fine, close obsolete" is a correct
  # outcome — do not manufacture a fix to justify the iteration.
  #
  # What 174 established, on the wire (FF154, static localhost page):
  #   `getWatcher` WITHOUT `isServerTargetSwitchingEnabled: true` →
  #   `watchTargets("frame")` and `watchResources` are both acked, PARENT-process
  #   resources (`will-navigate`, `network-event`) arrive normally, and
  #   CONTENT-process resources (`dom-loading`/`dom-interactive`/`dom-complete`)
  #   never arrive at all. The daemon route was unaffected because
  #   `establish_watcher` has always passed the flag.
  #
  # The five direct-route `TabActor::get_watcher` call sites that remain
  # (i.e. still omit the flag), and the resource each subscribes to:
  #
  #   commands/console.rs:298    console-message   ← content-process
  #   commands/click.rs:85       ?                 ← determine
  #   commands/network.rs:148    network-event     ← parent-process, expected fine
  #   commands/network.rs:1011   network-event     ← parent-process, expected fine
  #   commands/network.rs:1088   network-event     ← parent-process, expected fine
  #   commands/throttle.rs:120   network-event     ← parent-process, expected fine
  #   commands/emulate.rs:122    ?                 ← determine
  #   commands/nav_action.rs:305 network-event     ← parent-process, expected fine
  #
  # 1. Establish the ground truth per resource type from Firefox source, not
  #    from guesswork: `devtools/server/actors/resources/index.js` splits
  #    `ParentProcessResources` from `FrameTargetResources`. Only the latter
  #    can be starved by the missing flag.
  #
  # 2. Measure each content-process subscriber on BOTH routes. `console
  #    --follow` is the obvious candidate and the one 174 tried and failed to
  #    measure cleanly (its stdout stayed empty on both routes inside an
  #    8 s window, so the attempt proved nothing either way — a working
  #    measurement harness is task A, not an afterthought):
  #
  ff-rdp launch --headless --debug-port 7402
  ff-rdp --port 7402 navigate http://127.0.0.1:8099/
  #    then, for route in "--no-daemon" and "" (daemon):
  #      start `ff-rdp --port 7402 $route console --follow` so its stdout is
  #      readable while it runs, emit `ff-rdp --port 7402 $route eval
  #      'console.log("probe")'` from a second process, and record whether the
  #      line appears. A route that shows nothing where the other shows the
  #      line is the 174 defect in a second place.
  #
  # 3. Note that plain `console` (no --follow) is NOT affected and needs no
  #    change: it primes via `startListeners` on the legacy target actor, and
  #    it was measured working on the direct route during 174:
  ff-rdp --port 7402 --no-daemon eval 'console.log("iter174-direct-probe")'
  ff-rdp --port 7402 --no-daemon console --pattern iter174 --jq '.summary'
  #    → 2026-08-22: {"total":1,"matched":1,"shown":1,"by_level":{"log":1}}
tags: [iteration, rdp, daemon-parity, carry-over, investigation]
---

# Iteration 189: does anything else on the direct route starve on content-process resources?

Carry-over from [[iteration-174-direct-route-reload-never-sees-dom-complete]], filed before that
PR merges per CLAUDE.md's carry-over rule.

## What 174 fixed, and what it deliberately did not

174 fixed the two navigation waits in `navigate.rs` (`wait_for_navigation_commit` and `run_core`)
by routing their `getWatcher` through a new `get_navigation_watcher` helper that passes
`isServerTargetSwitchingEnabled: true`. It did **not** flip the flag globally — the flag also
moves top-level target delivery onto the watcher, so any caller holding a target actor across a
navigation must re-resolve it, and `TabActor::get_watcher_with_options`' own doc comment carries
that caution. The two navigation waits already re-resolved (`refresh_console_actor`); the other
call sites were not audited.

So the open question is narrow and answerable: **of the direct-route `getWatcher` call sites that
still omit the flag, does any of them subscribe to a resource that only the content process
emits?** If none does, this closes `obsolete` and the audit result gets written into
`kb/rdp/actors/watcher.md` so nobody has to ask again.

## Why this is worth an iteration rather than a hunch

The failure mode is silence, not an error. In 174 the client got acks for every request it made,
kept receiving parent-process resources on the same subscription, and produced a
*correct-looking* envelope from a fallback path — 70x slower, with `status_reason: "not_observed"`
as the only visible tell. Four iterations passed over it. A second instance of the same shape
would be equally quiet.

## Scope

- [ ] a working measurement harness for a streaming subscriber on both routes (174's attempt at
      `console --follow` produced empty stdout on *both* routes and therefore proved nothing)
- [ ] each remaining direct-route `getWatcher` call site classified: which resource types it
      subscribes to, and whether each is parent- or content-process per
      `devtools/server/actors/resources/index.js`
- [ ] every content-process subscriber measured on both routes
- [ ] the result — defect or clean — recorded in `kb/rdp/actors/watcher.md` under the iter-174
      section, replacing its "were not audited" sentence
- ~~**folded in from iteration 174's carry-over:** the `iteration-close` skill tells you to start
      a Firefox on port 6000 ... Following the documented procedure guarantees one red test.~~
      **WITHDRAWN 2026-08-23 — the diagnosis was wrong.** There is no conflict between the test
      and the skill. `live_96` fails only when the port-6000 browser was started with
      `ff-rdp launch`, which creates an ff-rdp-*managed* profile — precisely the state the test
      asserts is absent. Started the documented way (`firefox -no-remote
      --start-debugger-server 6000 --headless`) the test passes. Iteration 175 reproduced this
      and self-corrected; iterations 177 and 186 then made the same substitution, four occurrences
      in total. The real defect was that the skill buried the raw-browser command inside a bullet
      explaining a counter instead of stating it as a setup step — **fixed in
      `.claude/skills/iteration-close/SKILL.md`, so nothing is carried forward here.**
      Recorded rather than deleted because this entry is a worked example of the failure mode
      `kb/discipline-rationale.md` warns about: a contaminated sweep producing a confidently
      worded plan for a defect that never existed.

## Acceptance Criteria [0/3]

- [ ] the classification table above is filled in from Firefox source, with the file and the
      dictionary each type appears in
- [ ] every content-process subscriber has a measurement on both routes recorded in this plan,
      or a written reason it could not be measured
- [ ] if a defect is found: fixed with a live test that fails without the fix; if none is found:
      this plan is closed `obsolete` with the measurements left in place as the evidence

## Notes

- Related: [[iteration-174-direct-route-reload-never-sees-dom-complete]] (parent),
  [[iteration-129]] (which first established that the flag gates `target-available-form`),
  [[iteration-159]] (which established that `network-event` is parent-process and therefore
  cannot be affected).
- `kb/rdp/actors/watcher.md`'s iter-174 section carries the parent/content split table this
  audit extends.
