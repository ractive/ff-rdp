# Handoff: ff-rdp iteration run 233–256

Written 2026-09-07 by the session that drove `/new-ralph-loop 233 256`.
Read this top to bottom before touching the repo. Nothing here is guesswork unless it says so.

**Why this file exists:** the session was cut short. The code-review findings for PR #247 in
section 3 existed ONLY in that session's transcript and would have been lost. They are the most
valuable thing in this document.

---

## 0. Current blocker

The last delegated agent died with:

```
Your organization has disabled Claude subscription access for Claude Code
· Use an Anthropic API key instead, or ask your admin to enable access
```

This is an account/entitlement problem, not a repo problem. Until it is resolved, no subagent
work will run. Resolve it before relaunching anything.

## 1. Repo state (verified, not assumed)

- Working tree **clean**, nothing unpushed.
- Branch checked out: `iter-252/content-process-resources-direct-route`.
- `origin/main` head merge: `d9b913a`.
- Exactly one open PR: **#247** (iteration 252).

Ten of the thirteen pending iterations in the range are merged and each merge was verified against
`origin/main` by hand, not taken from an agent's word:

| iter | PR | merge SHA | what landed |
|---|---|---|---|
| 233 | 239 | `d3a76c8` | plan-linter sweep now blocking in CI's `discipline` job |
| 235 | 240 | `f8d278e` | live_bulk_cap stops shrinking a process-global cap; live_166 cache-busting |
| 237 | 241 | `872f354` | `type --submit` navigation reporting; `click` not-found fast path |
| 239 | 242 | `84fade6` | bare `ff-rdp` opens one RDP connection instead of two |
| 240 | 243 | `9937bf8` | resumable FrameDecoder; per-client writer; write deadlines |
| 242 | 244 | `944c17c` | `prune --all` silent ENOTEMPTY; launch-site ownership; 3rd-party census |
| 245 | 245 | `9e6c3de` | real-root orphan guarantee; live_158 stack hook; Windows paths |
| 246 | 246 | `d9b913a` | launch_timeout classification; load-sensitive assertions carry evidence |

The 11 obsolete plans in the range (234, 236, 238, 241, 243, 244, 247–251) were absorbed into
others by DEC-051 and are correctly skipped by preflight. Do not run them.

## 2. What is left to do, in order

1. **Finish iteration 252** — PR #247. See section 3. It is finished work blocked only on review.
2. **Run iterations 253, 254, 255, 256** — see section 5.
3. **Discharge the outstanding verification** in section 6.
4. **Correct the wrong conclusion** in section 7.

## 3. PR #247 (iteration 252) — the immediate task

**State:** open, `MERGEABLE`, all 10 CI checks green, head was `f3f1ba1` when last checked.
Nothing is uncommitted. The workflow's review agent was force-terminated mid-review, so the
iteration-close checklist is done EXCEPT the binding local review.

Already verified done by the terminated agent (re-verify cheaply, do not redo):
- All 9 xtask `check-*` gates exit 0.
- Plan scope boxes and all 3 ACs ticked accurately against the real diff.
- Carry-over table complete; plans 263 and 264 filed and committed on the branch.
- Coherence pass over plans 253–256 found nothing stale (0 edits needed).

**What the PR does.** `console --follow` was returning nothing on BOTH routes, for two different
reasons, and this fixes both:
- Direct route: the subscription was acked then starved. Fixed with
  `getWatcher{isServerTargetSwitchingEnabled: true}` plus `watchTargets("frame")` **before**
  `watchResources`, in `crates/ff-rdp-cli/src/commands/console.rs`.
- Daemon route: `parse_single_console_resource` in `crates/ff-rdp-core/src/actors/watcher.rs`
  only understood the legacy nested `{message:{…}}` shape, but a `console-message` *resource* is
  flat. Fixed with a flat branch discriminated on `level`, ordered AFTER the nested branches.

Isolated measurement on Firefox 155.0.1: neither fix 0/0; parser fix only daemon 7 / direct 0;
both fixes 8/8. This also explains why iteration 174's attempt to measure this "proved nothing" —
its instrument was broken, so an absence of output looked like an absence of evidence.

### THE THREE REVIEW FINDINGS — preserve these, they are not recorded anywhere else

A high-effort independent review produced these. **Finding 1 governs whether this can merge.**

**Finding 1 (MEDIUM) — `crates/ff-rdp-core/src/actors/watcher.rs:953`. Needs a MEASUREMENT, not
an argument.** The flat branch removes an accidental de-duplication on the daemon route.
`console --follow` in daemon mode is both the RPC-slot owner and a `console-message` stream
subscriber on the same socket, and the daemon feeds that socket from two independent channels:
`dispatch_console_push_event` + `forward_to_rpc_client` for `consoleAPICall` pushes
(`daemon/server.rs:1465-1469`), and `forward_to_rpc_client` for the `resources-available-array`
frames (which fall through `is_watcher_event` because their `from` is
`…watcherN.processM//windowGlobalTargetK`, not the watcher actor — exactly the sample recorded in
`kb/rdp/actors/watcher.md`).

Reproduce it: run `ff-rdp console` once in daemon mode — `prime_console_cache` issues
`startListeners(PageError, ConsoleAPI)` on the daemon's **shared** Firefox connection, which stays
armed for that connection's lifetime — THEN run `ff-rdp console --follow` against a page logging
on a timer. Before this PR the resource copy parsed to `None` and was invisible; now it parses, so
every `console.log` may print TWICE.

The iteration's own harness cannot exclude this: neither the dogfood script nor `live_252_…`
primes `startListeners`, and the new comment at `live_console_no_double_delivery.rs:147` says
outright that that test's watcher is obtained without the flag and so does not cover the new
configuration. **Run the sequence against a real headless Firefox and count lines.** If duplicates
occur, de-duplicate where the two channels converge and add a regression test that includes the
priming step. If they do not, record the exact commands and counts in the PR body and extend the
live test or dogfood script to cover the primed configuration.

**Finding 2 (LOW) — `crates/ff-rdp-cli/src/commands/console.rs:324`.** Neither half of the
direct-route fix has any non-live coverage. The mock helpers `follow_server_with_events` /
`follow_server_with_direct_notification` answer `watchTargets` but never assert that `getWatcher`
carried `{"isServerTargetSwitchingEnabled": true}`, nor that `watchTargets` arrived *before*
`watchResources`. Reverting `Some(true)` → `None`, or moving `watch_targets` below
`watch_resources`, leaves the whole workspace suite green — only the `#[ignore]`d live test
catches it, and this repo's own discipline notes say a live-only guard is the one most likely to be
skipped. `tab.rs` has `get_watcher_with_options_sends_flag` as a model. Add both assertions and
prove they have teeth by making each mutation in turn and confirming failure.

**Finding 3 (LOW) — `crates/ff-rdp-cli/src/commands/console.rs:331`.** The `watchResources`
catch-up burst is silently discarded on the direct route. `watch_resources` goes
`actor_request` → `recv_reply_from(transport, watcher_actor)`, which forwards any packet whose
`from` differs from the watcher to `transport.forward_event()`; `run_follow_direct` installs no
event sink, so those are dropped. Console-message resources always arrive `from` the
content-process `windowGlobalTarget`, never the watcher, so every frame between the
`watchResources` request and its reply — including the cached catch-up batch — is lost before
`follow_loop` starts. Same mechanism swallows the `watchTargets` burst. Arguably acceptable for
`--follow` semantics; if you judge it so, say so in the PR body with reasoning AND make the drop
visible (a `--help` note or a debug log) rather than silent. Otherwise install a buffering sink.

Reviewer also confirmed as **not** problems: `first_call_sites: []` is correct (no new `pub`
items); plans 263/264 filed without number collisions; the `iteration-203` condition-8 row is
marked FIRED with real failure text rather than re-tabled; the `live_111` and
`live_console_no_double_delivery` header corrections change explanations only, not assertions.

### How to finish it

Address the three findings (fix with a test, or answer in the PR body under `## Review` with
evidence), run the gates in the mandated order, then merge. Iteration 252 touches product source,
so a closing live sweep with both env gates is required, and its real `LIVE_SWEEP_SUMMARY` line
must go in the PR body.

## 4. Merge authority — you do not need to ask

`CLAUDE.md` has a section "Autonomous merges — standing authorization from the repo owner". The
owner (github: `ractive`) durably authorizes merging iteration PRs without per-PR approval when
three conditions hold: all CI checks green on the PR head; a local review pass has run and its
findings were addressed; and the merge goes through GitHub via `gh pr merge --merge` so branch
protection is enforced server-side. Quote that section verbatim into any delegated prompt — the
harness refuses unattended review-and-merge agents without it.

Never `--squash`. Never a local `git merge` + push to main.

## 5. Relaunching the loop for 253–256

```
/new-ralph-loop 253 256
```

Preflight re-derives done-ness from merge commits on `origin/main`, so already-merged iterations
are skipped automatically. There is no state file to clean up. If you instead relaunch a wider
range, that is harmless for the same reason.

**Pass these as launch arguments** (the workflow script is at
`~/.claude/skills/new-ralph-loop/scripts/ralph.workflow.js`; pass its contents inline):
- `mergeAuthorization`: the CLAUDE.md section named in section 4, verbatim.
- `extraContext`: the operational rules in section 8. They are binding run-wide instructions and
  the prompts are the ONLY reliable channel to the agents. A PR comment is not a control — that
  was measured and falsified. Never `SendMessage` a running workflow subagent; it forks a
  duplicate into the same working tree.

Models used, which worked well: implement `opus`, review `sonnet`, verify `haiku`. Copilot off.

Cost, measured: about 5.1M subagent tokens for the nine iterations that ran, roughly 570k each,
over about 12 hours of wall clock across two launches.

## 6. Outstanding verification (one item)

**Iteration 242 (`944c17c`) merged with no live sweep at all.** Its agent claimed the environment
could not run one and left six ACs unticked on that basis. That claim was false — iterations 245,
246 and 252 all ran full dual-gate sweeps afterwards on the same machine. Iteration 242 touched
product source (`crates/ff-rdp-cli/src/util/profile_dir.rs` and the prune path), so it owes a
sweep:

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
```

Treat anything it finds as fix-forward work, not as a reason to revisit the merge. Note the six
unticked ACs in plan 242 are the durable record that this is owed.

A second concern here is already discharged: two xtask gates were unrun by iteration 242, but the
terminated 252 agent later ran all nine on a branch containing 242's code and all nine passed.

## 7. A wrong conclusion to correct in the knowledge base

`kb/iterations/iteration-246-sweep-load-misclassification.md` (around line 551) flags
`live_navigate_default_fast::live_navigate_elapsed_matches_wall` as "may be a real honesty
regression (iter-122 Theme B)" because reported `elapsed_ms` came in ~928 ms **smaller** than
measured wall clock (`elapsed_ms 322` vs wall `1250`) in both of iteration 245's sweeps. Its
argument was that load lengthens the wall clock but cannot shorten ff-rdp's own measurement.

**That is inverted, and the measurement disproves it.** This session ran the test in isolation
three times: passed at 14.48 s, 8.10 s, 8.95 s. It also passed inside iteration 246's own sweep.
Four passes against two failures under load. The wall-clock figure spans work outside the region
ff-rdp measures, so contention inflates the wall number while the internal measurement stays
honest and the gap widens — which is exactly the load signature, not a counter-argument to it.

Fix the addendum in place to say so and move that row into the load-shaped set. This is a
kb-only change; per the owner's standing preference, merge it locally without a PR.

## 8. Operational rules learned the hard way — pass these to every agent

1. **Run anything that can be quiet for over ~2 minutes with `run_in_background: true` and poll it
   with short cheap commands.** The harness kills an agent after ~180 s without a transcript
   event. Iteration 237 lost two full implement agents to this before one survived. Cold builds,
   the workspace test suite, the live sweep and `gh pr checks --watch` are all in the kill window.
2. **Commit and push WIP constantly, even broken and half-done.** This is what made every kill in
   this run recoverable: a killed agent's successor found 4, then 12 commits already pushed and
   resumed instead of starting over. Ugly WIP commits vanish into the merge commit.
3. **Never wait on a review subprocess in the foreground, and hand off early rather than dying
   mid-wait.** This killed BOTH launches of this run, at iterations 240 and 252. Bound the wait to
   about 15 minutes, retry once with narrower scope, then return `ok=false` with
   `failure_reason: "review subagent unavailable"` and push everything. A launching session can
   review and merge an intact PR in minutes; it cannot recover a verdict that was never returned.
4. **A green CI is not evidence of correctness.** Both hand-run reviews in this session found real
   defects behind 10 green checks. Iteration 240's was HIGH severity: the daemon released a slow
   client's RPC slot while that client still believed it owned it, so a second client could claim
   the slot and receive the first client's in-flight Firefox reply, which carries no correlation
   ID — silent cross-client data corruption. Iteration 242's own review found a HIGH where a new
   "unreadable marker" grading made a corrupt-marker directory unreclaimable forever.
5. **Watch a review that reports nothing.** "No findings were returned" is a STOP, not a clean
   bill. "0 findings" from a review that demonstrably ran (turn counts, gates it ran itself) is
   fine.
6. **Verify agent claims about counts and capabilities.** Two claims in this run were simply
   wrong: that the environment could not run a live sweep (it could), and one agent reported 7
   xtask gates when 9 exist. Reconcile a sweep's `passed + failed` against `executed` across ALL
   tiers, not the CLI tier alone — the four non-CLI tiers hold about 9 tests and comparing against
   the CLI tier alone fabricates a phantom shortfall.
7. **Never reword an acceptance criterion to fit what happened.** Nothing checks tick state, so an
   honest empty box with a reason is the only signal a later reader gets. Agents in this run did
   this correctly and it is why the record is trustworthy.
8. **Do not work in this repo while a run is live.** The agents operate directly in this working
   tree and will fight you for the cargo target directory, Firefox processes and port 6000.

## 9. Backlog this run created — eight plans, all OUTSIDE the 233–256 range

Nothing will pick these up automatically. They are filed, valid and pending.

| plan | what | priority |
|---|---|---|
| 257 | Firefox 155 changed `drawSnapshot`'s 4th arg to a dictionary — **every `--full-page` screenshot is broken right now** | **highest, user-visible** |
| 258 | advisory poll budget found while fixing iteration 237 Part A | low |
| 259 | RPC-slot handover strands an in-flight reply — the residual iteration 240's fix could not close; needs request-reply attribution the protocol may not offer | medium |
| 260 | live-owner removal race in `daemon stop` | medium |
| 261 | a failed `ProfileCleanup::Skipped` never reaches the JSON envelope | medium |
| 262 | daemon frame-target promotion: latched product race, diagnosed as `target_count=1, live_target_count=0` after 20 s with a healthy dispatcher; the 15 s bound is NOT the problem and was deliberately left alone | medium |
| 263 | live-sweep silently omitted a gated test from its plan: 319 of 320 enumerated, no `ok`, no `FAILED`, no mention | medium |
| 264 | wall-clock bounds crossed under sweep load | low |

Plan 257 is the one to run first once 253–256 are done. It is a shipped regression, not a test
defect.

Also still open, tracked but unresolved: plan 203 holds the live-sweep and toolchain watch
conditions (the 178 → 192 → 203 holder chain). Condition 8 fired during iteration 252 and is
recorded there. Plan 203 is outside the range too.

## 10. Standing repo rules you must not rediscover by breaking them

Read `CLAUDE.md`, `.claude/CLAUDE.md`, `CONTRIBUTING.md` and `kb/discipline-rationale.md` in full.
The ones that bite hardest:

- Gates before any commit or PR, **in this order**: `cargo fmt`; then
  `cargo clippy --workspace --all-targets -- -D warnings`; then `cargo test --workspace -q`.
  Run `rustup update stable` before treating a green clippy as evidence, and read
  `gh pr checks <PR>` rather than substituting your local run for CI.
- **Invoke the `iteration-close` skill before `/create-pr` on any `iter-*` branch.** It carries
  the live sweep, the xtask gate enumeration and the carry-over sweep. None is automated.
- Never quote a `cargo test-live` pass count as evidence; it does not mean the tests reached
  Firefox. Use `cargo run -p xtask -- live-sweep`.
- Every iteration touching product source pastes a real sweep, with the env gates it used, into
  its PR body. It is per-iteration, not per-batch; iteration 160's sweep caught three failures
  before its PR opened.
- Carry-over work is filed as a new iteration plan **before** the current PR merges.
- Use `hyalo`, never Read/Grep/Glob, for markdown in `kb/`. It walks up to find `.hyalo.toml`, so
  never prefix a `cd`.
- Plan `status:` is `planned | in-progress | in-review | done | obsolete`. `done`, never
  `completed`.
- Validate every new plan: `cargo run -p xtask -- check-iteration-plan <plan>`. Plans need
  `dogfood_path`, `first_call_sites` if they add `pub` items, and an unclaimed iteration number.
- No `.unwrap()`/`.expect()` outside tests; `anyhow::Context` with `?`. `thiserror` in core,
  `anyhow` in the CLI. Every new `pub` item needs a non-test consumer in the same PR.
- Docs-only and kb-only changes: merge locally, do not open a PR.
- Dogfood scripts source `dogfood-lib.sh`, call `dogfood_init`, drive the CLI through `ffrdp` (a
  bare `ff-rdp` from PATH certifies the wrong binary), and tear down **only** the browser that run
  launched. Never `pkill` — it kills a sibling agent's Firefox on a shared tree.
- Skill-edit iterations (anything touching `~/.claude/skills/`) cannot run through the loop. Drive
  them by hand.
