---
title: "Iteration 252: audit every other content-process resource subscription on the direct route — iteration 174 fixed only the two navigation waits"
type: iteration
date: 2026-08-23
status: in-review
branch: iter-252/content-process-resources-direct-route
depends_on:
  - iteration-174-direct-route-reload-never-sees-dom-complete
first_call_sites: []
dogfood_script: iteration-252-content-process-resources-on-the-direct-route.dogfood.sh
dogfood_path: kb/iterations/iteration-252-content-process-resources-on-the-direct-route.dogfood.sh
tags: [iteration, rdp, daemon-parity, carry-over, investigation]
review_resolution: "2026-09-09: all three original review findings repaired and measured. Primed daemon follow went from 12 lines/four numbered timer ticks (three copies each) to 12 lines/12 ticks once each; direct and unprimed daemon also measured 12/12 once. Separate flag and request-order mutations each failed explicit non-live assertions; restored e2e tests passed 5/5. Direct subscription catch-up is buffered and covered by a pre-ACK content-process batch test. Ordered fmt/strict workspace clippy/workspace tests passed after repairing test diagnostics and mutex assertions. Final dual-gate sweep: executed=329 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=329; all expected names reconciled, 320 passed/9 failed across five tiers, leaked=0/unattributed=0. Nine xtask gates and actual live dogfood passed (8 direct/7 daemon messages over eight seconds). Seven screenshot failures are owned by 257; consent action failure reproduced unchanged on origin/main and is folded distinctly into 262; transient post-auth timeouts are folded into 203. Ten of eleven isolated anomaly reruns passed; the remaining consent error also reproduced on baseline main. Original AC wording/ticks unchanged; independent review and supervisor PR/merge actions remain pending. Full substantive closing report is retained under .git/ralph-loop/20260909-takeover/iter252/resume-1 for the PR."
---

# Iteration 252: does anything else on the direct route starve on content-process resources?

> **Renumbered 189 → 252 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 189.

> **Premise check (2026-09-06), resolved:** this plan was filed with an eight-row call-site table in its frontmatter, and that table was stale — `commands/click.rs`'s site had moved and three further `getWatcher` sites omitted the flag (`commands/navigate.rs`, `commands/network_watch.rs`, and `ff-rdp-core`'s own `storage.rs`). The table was re-derived from the tree during implementation and the frontmatter copy replaced by a pointer to the dogfood script; the authoritative, eleven-row version is under [Classification](#classification) below. The plan's stated expectation — "all clean, close obsolete" is a valid outcome — did **not** hold: a defect was found.

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

- [x] a working measurement harness for a streaming subscriber on both routes (174's attempt at
      `console --follow` produced empty stdout on *both* routes and therefore proved nothing) —
      `iteration-252-…dogfood.sh` and `live_252_console_follow_content_resources.rs`. Both drive
      the probe from a **page-side** `setInterval` rather than a second `eval`, because a
      daemon-routed `console --follow` holds the daemon's single RPC-writer slot and a second
      daemon command underneath it times out; both fail loudly when the *daemon* leg is silent,
      so a harness that measures nothing can never read as a clean result.
- [x] each remaining direct-route `getWatcher` call site classified: which resource types it
      subscribes to, and whether each is parent- or content-process per
      `devtools/server/actors/resources/index.js` — table below
- [x] every content-process subscriber measured on both routes — there is exactly one
      (`console --follow`), measured below
- [x] the result — defect or clean — recorded in `kb/rdp/actors/watcher.md` under the iter-174
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

## Findings

**The answer is not "all clean".** One remaining direct-route subscriber watches
content-process resources, and it was starved exactly the way iteration 174's navigation waits
were. A second, independent defect sat on top of it and is why 174's attempt to measure this
proved nothing.

### Classification

Re-derived with `grep -rn get_watcher crates --include='*.rs'` (the plan's premise check was
right — eleven flag-less sites, not eight), classified against
`devtools/server/actors/resources/index.js` at the local checkout's revision `0088392ab4cc`,
which is the revision `kb/rdp/actors/watcher.md` already verifies its iter-159 findings against.

| call site | resource types watched | dictionary (index.js line) | starved? |
|---|---|---|---|
| `commands/console.rs:298` (`run_follow_direct`) | `console-message`, `error-message` | `FrameTargetResources` (`:64`, `:80`) | **YES** |
| `commands/navigate.rs:2796` | `network-event` | `ParentProcessResources` (`:213`) | no |
| `commands/click.rs:90` | `network-event` | `ParentProcessResources` | no |
| `commands/throttle.rs:120` | `network-event` | `ParentProcessResources` | no |
| `commands/network_watch.rs:118` | `network-event` | `ParentProcessResources` | no |
| `commands/nav_action.rs:332` | `network-event` | `ParentProcessResources` | no |
| `commands/network.rs:148` | `network-event` | `ParentProcessResources` | no |
| `commands/network.rs:1024` | `network-event` | `ParentProcessResources` | no |
| `commands/network.rs:1101` | `network-event` | `ParentProcessResources` | no |
| `ff-rdp-core/src/actors/storage.rs:83` (`list_cookies`) | `cookies` | `ParentProcessResources` (`:216`) | no |
| `commands/emulate.rs:122` | *none* — resolves the target-configuration actor only | n/a | no |

`emulate.rs` was one of the two the plan marked "determine": it calls `getWatcher` purely to
reach `getTargetConfigurationActor` and never issues `watchResources` at all, so it has no
resource subscription to starve. `click.rs` was the other: `network-event`, parent-process.

### Measurement — `console --follow`, both routes

Firefox 155.0.1, headless, a page logging `console.log` once per second, 8 s window, direct
(`--no-daemon`) and daemon legs measured with identical timings:

| build | daemon route | direct route |
|---|---|---|
| before either fix | 0 lines | 0 lines |
| parser fix only | **7 lines** | **0 lines** |
| both fixes | 8 lines | 8 lines |

The middle row is the isolated proof of the watcher defect: with the payload parsed correctly
the daemon route streams and the direct route still never receives anything.

Direct-route wire trace before the fix — the 174 signature exactly, an ack followed by silence
while the page kept logging:

```text
→ getWatcher     {}                                    ← ack (watcher11)
→ watchResources ["console-message","error-message"]    ← ack
… 8 s of nothing, while `console` (non-follow) reads 189 matching messages …
```

### Defect 1 — the direct route received nothing (the audit's question)

`console-message` and `error-message` are `FrameTargetResources`, and two server-side
preconditions have to hold before one is emitted. `run_follow_direct` satisfied neither:

* `getWatcher {isServerTargetSwitchingEnabled: true}` — without it
  `shouldNotifyWindowGlobal` rejects the top-level browsing context outright
  (`watcher/browsing-context-helpers.sys.mjs:174-182`), so the watcher never instantiates a
  frame target for the page;
* `watchTargets("frame")` **before** `watchResources` — the content-process half of
  `watchResources` fans the new types out over `watcherDataObject.actors`
  (`js-process-actor/DevToolsProcessChild.sys.mjs:409-414`), the targets `watchTargets`
  created. The descriptor-created top-level target is deliberately not in that list; only
  `sessionContext.type == "webextension"` gets a `TargetActorRegistry` fallback there.

### Defect 2 — neither route could print one (why 174 measured nothing)

`parse_single_console_resource` keyed off `item.message` (the legacy `consoleAPICall` push,
`webconsole.js:1453`) and `item.pageError` (`resources/error-messages.js:180`). A
`console-message` **resource** is flat — `resources/console-messages.js:55` passes
`prepareConsoleMessageForRemote`'s result straight to `onAvailable`:

```json
["console-message",[{"arguments":["iter252tick"],"lineNumber":1,"columnNumber":32,
  "filename":"debugger eval code","level":"log","timeStamp":1788783444398.958,
  "sourceId":null,"innerWindowID":17179869186}]]
```

Every such item parsed to `None`, so the daemon route received all of these frames and printed
nothing. That is precisely the empty-on-both-routes result iteration 174 got and could not
interpret. `level` is now the flat shape's discriminator; the nested branches still win when
present, so the legacy push path is untouched.

Two stale claims fell out and are corrected in place rather than deleted, because both were
written as measured findings: `live_111_daemon_follow_cross_process`'s header ("console.log is
not routed through the watcher `console-message` resource stream") and
`live_console_no_double_delivery`'s closing note. Neither test's assertion changes — only the
explanation for the zero they observed.

Plain `console` (no `--follow`) was never affected: 189 matched messages on both routes in the
same session in which `--follow` showed zero.

## Acceptance Criteria [3/3]

- [x] the classification table above is filled in from Firefox source, with the file and the
      dictionary each type appears in
- [x] every content-process subscriber has a measurement on both routes recorded in this plan,
      or a written reason it could not be measured
- [x] if a defect is found: fixed with a live test that fails without the fix; if none is found:
      this plan is closed `obsolete` with the measurements left in place as the evidence —
      **a defect was found** (two, in fact), so the plan closes `done`, not `obsolete`. Live
      test: `live_252_console_follow_sees_content_process_messages_both_routes`; it fails on the
      direct leg with the `console.rs` change reverted, and on both legs with the parser change
      reverted as well.

## Notes

- Related: [[iteration-174-direct-route-reload-never-sees-dom-complete]] (parent),
  [[iteration-129]] (which first established that the flag gates `target-available-form`),
  [[iteration-159]] (which established that `network-event` is parent-process and therefore
  cannot be affected).
- `kb/rdp/actors/watcher.md`'s iter-174 section carries the parent/content split table this
  audit extends.
