---
title: "Iteration 242: live-suite ownership: prune --all liveness flake, guard-coverage gaps, sweep-only failures"
type: iteration
date: 2026-08-24
status: planned
branch: iter-242/profile-liveness-flake-in-prune-all
depends_on:
  - iteration-193-dogfood-scripts-pkill-and-path-binary
  - kb/iterations/iteration-151-residual-live-firefox-leak.md
  - iteration-175-failed-launch-leaks-unmarked-profile-dir
first_call_sites: []
dogfood_path: |
  # Reproduce: launch a headless Firefox into an isolated profile root, back-date
  # the live profile past the age threshold, then ask --all to reclaim it. The
  # basename must appear in `removed_live`, because its owner is still running.
  export FF_RDP_HOME="$(mktemp -d)"
  cargo build --quiet -p ff-rdp-cli --bin ff-rdp
  B=./target/debug/ff-rdp
  J=$("$B" launch --headless --port 39111)
  P=$(echo "$J" | jq -r '.results.profile_path')
  PID=$(echo "$J" | jq -r '.results.pid')
  find "$P" -maxdepth 1 -exec touch -t "$(date -v-30d +%Y%m%d%H%M)" {} +
  "$B" profiles prune --older-than 1s | jq -c '.results'   # must NOT remove $P
  kill -0 "$PID" && echo "owner still alive"
  "$B" profiles prune --all | jq -c '.results.removed_live' # observed empty ~1 run in 2
  "$B" --port 39111 daemon stop; rm -rf "$FF_RDP_HOME"

  # Or drive the whole sequence through the iteration-97 gate, which asserts it:
  #   FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script \
  #     kb/iterations/iteration-97-profile-liveness-guard.md
  # --- from iteration 243 ---
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 live_1
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 --skip live_1
  ff-rdp profiles list --jq '.results.count'
  # → after BOTH chunks exit, `count` must be 0, and every profile that does survive must
  #   name its spawning test via the .ff-rdp-owner-test marker (never "unknown test").
  # --- from iteration 244 ---
  # Test-harness defect, two mechanisms. Both were observed in iteration 175's
  # closing sweeps: two full dual-gate runs, each 282 executed / 272 passed /
  # 1 failed, with a DIFFERENT single failure each time, and each failure
  # passing green when re-run in isolation.

  # A. The live_96 precondition versus the sweep's own port-6000 setup.
  #
  # `iteration-close` tells you to start a Firefox on port 6000 so the
  # `preexisting` tier executes. The obvious way to do that is:
  ff-rdp launch --headless --debug-port 6000
  # ...which creates an ff-rdp-MANAGED profile owned by a live PID. That is
  # exactly the precondition live_96 asserts against:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_profiles_prune_removes_all_when_no_firefox_running -- --include-ignored
  # → OBSERVED 2026-08-23: "precondition violated — 1 ff-rdp-managed profile
  #   dir(s) ... still owned by a live process ... (pid 63228, spawned by
  #   unknown test)". The `unknown test` is the operator's own setup launch.
  #
  # The workaround iteration 175 used, and which should be either documented
  # or made unnecessary: start a RAW Firefox on an unmanaged profile with the
  # devtools prefs written by hand.

  # B. live_eval_on_hn depends on news.ycombinator.com responding.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -p ff-rdp-cli --test live live_eval_on_hn -- --include-ignored
  # → OBSERVED 2026-08-23 under sweep load: document.title came back "" instead
  #   of "Hacker News". Green in isolation 3 minutes later.
tags: [iteration, profiles, flake, liveness, live-tests, sweep, carry-over]
---

# Iteration 242: live-suite ownership: prune --all liveness flake, guard-coverage gaps, sweep-only failures

> **Renumbered 204 → 242 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 204.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-243-live-guard-coverage-sweep]] as Part B and [[iteration-244-live-sweep-only-failures]] as Part C. One branch, one PR, one carry-over sweep for all parts.

## Part A: `profiles prune --all` intermittently reports a live-owner profile as not live

Carry-over from [[iteration-193-dogfood-scripts-pkill-and-path-binary]]'s close.

### The defect

Iteration 97's dogfood gate asserts three things in sequence against one launched Firefox:

1. `profiles prune --older-than 1s` must **skip** the live-owner profile (Theme B), and
2. `profiles prune --all` must **remove** it but list its basename in `removed_live` (Theme C).

Both read the same predicate, `profile_is_owned_by_live_process`. On 2026-08-24, running the
migrated iteration-97 gate live, Theme B passed and Theme C failed in the *same run*:

```
PASS: Theme B — live-owner profile survived age-gated prune
FAIL: Theme C — --all did not report ff-rdp-profile-1frdDIlOuyiQy3uI in removed_live
```

An immediate re-run of the identical command passed all themes, and a hand repro of the same
sequence outside the gate passed. Nothing changed in between. So within a few hundred
milliseconds the same profile read as live-owned and then as not-live-owned.

That predicate is what stops a running Firefox having its profile deleted out from under it.
Theme B is the direction that matters — an age-gated prune that reads a live owner as dead
deletes a live profile — and the observed failure was in Theme C only, but both call the same
function, so a flake in one is a flake in the other with the consequences swapped.

### What to find out first

`owner_liveness` (`crates/ff-rdp-cli/src/util/profile_dir.rs`) has four outcomes and three of
them can flip without the owner dying:

- `Dead` from `is_process_alive(pid)` returning false for a process that is alive but, say,
  momentarily a zombie during a Firefox content-process restart;
- `Dead` from `process_start_token(pid)` disagreeing with the recorded token — a *recycled PID*
  verdict, which is a hard "this is a different process";
- `Unverified` from `process_start_token` returning `None` (still counts as live, so this one
  cannot explain the observed failure);
- `Unmarked` if the marker file is unreadable at that instant — note the reproduction back-dates
  every top-level file in the profile with `touch` just before the two prune calls.

Instrument which branch fires before proposing a fix. The failure is intermittent, so a fix
justified by reasoning alone will not be distinguishable from the flake going quiet.

### Themes

- **A — Identify the flipping branch.** Add enough tracing (or a test-only accessor) to
  attribute a `false` from `profile_is_owned_by_live_process` to one of the four outcomes, and
  reproduce until the attribution is recorded.
- **B — Make the verdict stable, or make it honest.** Either the transient reading is wrong and
  should be retried/ignored, or it is a genuine "cannot tell" that must not be reported as a
  confident "not live" on the deletion path — `Unverified` already errs toward keeping the
  directory, and whatever branch fires here may belong with it.

### Tasks

#### A. Attribution [0/2]
- [ ] `profile_is_owned_by_live_process` can report *which* `OwnerLiveness` it derived
- [ ] The intermittent failure is reproduced with the branch recorded

#### B. Stability [0/1]
- [ ] The identified branch either stops firing spuriously or stops being treated as a
      confident negative on the prune path

### Acceptance Criteria [0/2]

- [ ] A test pins the identified transient case: given that condition, an age-gated prune does
      **not** remove the profile
- [ ] The iteration-97 dogfood gate passes ten consecutive runs
      (`FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script kb/iterations/iteration-97-*.md`)

### Out of scope

The dogfood-script harness itself — [[iteration-193-dogfood-scripts-pkill-and-path-binary]] owns
it and has landed. If the investigation shows the assertion rather than the predicate is wrong,
say so and fix the assertion; do not reword it to match whatever the run produced.

### References

- [[iteration-193-dogfood-scripts-pkill-and-path-binary]] — where this was observed, and why the
  iteration-97 gate could be executed live at all
- `crates/ff-rdp-cli/src/util/profile_dir.rs` — `owner_liveness`,
  `profile_is_owned_by_live_process`
- `crates/ff-rdp-cli/src/commands/profiles.rs` — `select_prune_targets`, `removed_live`

### Additional observation — `daemon stop` leaves the profile behind (iter-224 close, 2026-08-31)

A second reclamation path shows the same "unstable across one run" shape, so it belongs to this
plan rather than a new one. `live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json`
failed in **both** live sweeps taken on `iter-224/with-page-daemon-connection-reset`:

```text
profile_removed must be true — got
  {"stopped":true,"pid":18379,"port":57624,"profile_removed":false,"profile_removed_path":null}
```

`stopped: true` means the escalation ladder reported the process gone and the port free, so
`stop_daemon_and_build_result_with` did call `cleanup_profile_dir` — and got no removed path back.
It **passed alone** (`--test-threads=1`, 2 passed in 2.86 s) immediately after sweep 1. Same
predicate family as Theme B/C above (`profile_is_owned_by_live_process` guards
`cleanup_profile_dir`'s refusal), same "true once, false once, one run apart" signature, so whatever
makes the liveness read unstable is the thing to find. Worth checking whether a content process
still holding the profile dir open is enough to make the removal fail silently — the JSON reports
`profile_removed: false` with no reason attached, which is its own honesty gap.

Carried over from [[iteration-224-with-page-daemon-connection-reset]]; nothing in that iteration
touches profile reclamation.

## Part B: close the remaining live-suite guard-coverage gaps (absorbed from iteration 243)

> **Renumbered 152 → 243 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 152.

> **Premise check (2026-09-06):** the chunk-A/chunk-B `--test-threads=1` methodology in `dogfood_path` and the ACs predates the parallel `cargo run -p xtask -- live-sweep` (iterations 188/197). Run the verification through the sweep; do not tick the chunk ACs on a runner the repo no longer uses — leave them unticked and say so.

Carry-over from [[iteration-151-residual-live-firefox-leak]]. Filed from the independent
code review of PR #188 (2026-08-12), which found five real gaps that were out of scope for
151's own fix but are the same bug family.

### Why this exists

151's PR review turned up **two high-severity leaks that 151 itself missed** — the
`launch --replace` class, where the CLI reaps the prior Firefox and starts a *new* one whose
PID no guard owns. Those two were fixed in 151's PR because they *were* the residual leak.

The five items below are the remainder of that review: real, confirmed, but each either
lower severity or a broader refactor than 151's scope allowed. They are filed here rather
than fixed inline so 151 stayed honest to its own title.

**The meta-lesson worth carrying:** 151's original audit searched for *discarded guards*
(`ManuallyDrop`, `mem::forget`). It did not search for *processes nothing ever owned*, which
is why it missed the `--replace` class entirely. Any audit here must enumerate launch sites,
not guard sites.

### Themes

#### Theme A — guard the remaining unowned launches

`live_90_daemon_lifecycle.rs:265` (`pre_fix_repro_daemon_state_sharing_red_then_green`) still
uses the unguarded `launch_on_port`, which returns a bare `u32`. It runs four assertions
before any cleanup, and the relaunch falls back to `new_pid = 0` on a parse miss, which
silently skips its `kill_pid`. This is the same "no RAII guard across an assertion" shape
151 Theme B claims to have eliminated suite-wide — in a file 151 edited.

- Give `launch_on_port` the `common::FirefoxGuard` return shape.
- Remove the `unwrap_or(0)` cleanup gate — a PID that failed to parse must fail the test,
  not silently skip the kill.

#### Theme B — close the spawn→guard window

`live_142_disk_growth.rs`'s `launch_headless` still has an unguarded window: after
`out.status.success()` confirms Firefox is running, the `?` / `.ok()?` on JSON parse and
`results.pid` can return `None` and drop the launched process with nothing to reap it. The
same applies to `live_142_throttle_json_gc`, where two `.expect()`s sit between spawn and
`FirefoxGuard` construction.

- Extract the PID first and construct the guard before any other parsing or assertion.
- On the error path, `kill_pid` before returning `None`.

#### Theme C — owner-marker coverage across raw launch sites

151 Theme A's `FF_RDP_LIVE_TEST_NAME` instrumentation only covers `common::LiveFirefox`.
Four live files carry private `LiveFirefox` clones (`live_oneway.rs`, `live_target_destroyed.rs`,
`live_cross_actor.rs`, `live_61l.rs`) and several sites launch raw
(`live_90_daemon_lifecycle.rs`, `live_110_kill_scoping.rs`, `live_86_perf_field_fixes.rs`,
`live_123_daemon_autostart_and_registry.rs`). None set the env var — so a profile leaked by
any of them still reports "unknown test", which is precisely the traceability 151 was
supposed to deliver.

- Make `current_test_name()` public and add a `common::launch_command()` helper that
  pre-sets `SPAWNING_TEST_ENV`.
- Route every `"launch"` invocation under `tests/live/` through it.

#### Theme D — guard Drop must not signal a known-dead PID

`live_90_daemon_lifecycle.rs:169`: keeping a guard live after `daemon stop` / `--replace` has
already reaped the PID means `Drop` unconditionally signals a PID the test knows is dead.
`kill_pid` does no liveness or ownership check, which reintroduces at test scope the
recycled-PID hazard iter-110 guarded against in production. Low probability — but 151
removed the `ManuallyDrop` that was incidentally preventing it.

- Have `kill_pid` (or the guard's `Drop`) skip when `!pid_alive(pid)`, or add an explicit
  `disarm()` for paths that have already asserted the process is gone.

#### Theme E — de-duplicate the owner-marker helpers

`live_151_residual_leak.rs`'s `live_owned_profile_dirs` is copy-pasted from
`live_96_profile_cleanup.rs`, and the `.ff-rdp-owner-test` literal now appears in four places.
The justification recorded in 151 ("no `[lib]` target for an integration-test binary to import
from") is wrong: both files are modules of the *same* `tests/live` binary, and
`tests/common/mod.rs` exists for exactly this — it already hosts `kill_pid` / `pid_alive` /
`FirefoxGuard`.

- Move the helper and the marker-name constant into `common/mod.rs`.
- The `SPAWNING_TEST_ENV` duplication between `src/` and `tests/` is genuinely unavoidable
  (the product-side constant is private); leave that one and keep its explanatory comment.

### Acceptance Criteria [0/5]

- [ ] live_152_no_unowned_launch_sites: a test (or xtask check) enumerates every `"launch"`
      invocation under `crates/ff-rdp-cli/tests/live/` and asserts each one's PID is bound to
      an RAII guard before the next assertion — enumerating launch sites, not guard sites
- [ ] live_152_marker_names_test_from_raw_launch: a profile leaked by a raw-`Command` launch
      site (not `common::LiveFirefox`) still names its spawning test in `.ff-rdp-owner-test`
- [ ] live_152_guard_drop_skips_dead_pid: a guard whose PID was already reaped does not
      signal it on drop, proven by observing no kill against a recycled/dead PID
- [ ] live_152_chunk_a_leaves_no_orphans: a full chunk-A run leaves zero ff-rdp-spawned
      Firefox processes — the AC 151 could not tick because it never ran the chunk
- [ ] live_152_chunk_b_leaves_no_orphans: the complementary chunk-B run leaves zero
      ff-rdp-spawned Firefox processes, and `live_96_profile_cleanup`'s precondition passes
      without manual cleanup

### Notes

- Do not tick the two chunk ACs without actually running the chunks. 151 ticked its
  equivalents on "implemented and compiled" and they were unticked in review; the chunk runs
  are ~6 minutes each and are the only evidence that counts. Note that
  `check-iteration-ready`'s `ac-fidelity-check` will NOT catch this: it verifies a ticked AC
  *references* a test slug, not that the test ran.
- Environment quirks (long commands killed at ~9–10 min, the two-chunk split, the
  `pgrep -f "firefox.*ff-rdp-profile"` self-match over-report) are documented in
  [[iteration-151-residual-live-firefox-leak]]'s Run guidance section — read it first.
- Verify on the wire before fixing. Across 135–151 the stated root cause diverged from
  reality at least eight times, most recently in 151 itself.

## Part C: live tests that assert on third-party page content, so a slow or changed site reds the sweep (absorbed from iteration 244)

> **Renumbered 190 → 244 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 190.

> **Premise check (2026-09-06):** AC 1 asks for a sweep in which `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` passes, but that test was deleted in iteration 188's review (see `live_151_residual_leak.rs` and iteration 245). Leave the AC unticked with that reason rather than rewording it. Theme B task 3 overlaps iteration 251 (`live_137_consent_accept_via_daemon`); 251 owns that signature.

Carry-over from [[iteration-175-failed-launch-leaks-unmarked-profile-dir]]'s closing sweeps.

### What was observed

Two full dual-gate live sweeps were run back to back on the iteration 175 branch:

```
sweep 1: LIVE_SWEEP_SUMMARY executed=282 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=282
         272 passed / 1 failed — live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running
sweep 2: LIVE_SWEEP_SUMMARY executed=282 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=282
         272 passed / 1 failed — live_61r_eval::live_eval_on_hn
```

Same gates, same corpus, **different** single failure each time, and both green in isolation.
Neither touches iteration 175's code paths — that iteration changed profile-directory lifetime in
`launch`, and its own two live tests passed in both sweeps. But "environmental" is a diagnosis,
not a disposition, so both get an iteration.

### Themes

- **A — `live_96`'s precondition fights the sweep's documented setup.** The `iteration-close`
  skill tells the operator to start a Firefox on port 6000 so the `preexisting` tier executes.
  Doing that with `ff-rdp launch` — the obvious, dogfooding way — creates a managed profile owned
  by a live PID, which is precisely what `live_profiles_prune_removes_all_when_no_firefox_running`
  refuses to run alongside (deliberately, since iter-146 Theme B). The test is *right*; the trap is
  that our own closing procedure walks the operator into violating it, and the failure message
  says `spawned by unknown test`, which points at the live suite rather than at the operator's own
  setup command. Decide between: teaching `live_96` to recognise a profile whose owner is not part
  of this test binary; having the sweep start (and own) the port-6000 browser itself on an
  unmanaged profile; or documenting the raw-Firefox recipe in `iteration-close` and leaving the
  test alone. Do not "fix" it by relaxing the precondition — that precondition exists because
  `prune --all` would rip a profile out from under a live session.
- **B — `live_eval_on_hn` depends on a third-party site under load.** It navigates to
  news.ycombinator.com and asserts `document.title == "Hacker News"`; under sweep load it got `""`,
  i.e. the document was not there yet or the request was throttled. Establish whether the fix is a
  readiness wait (our defect), a local fixture (removes the dependency but also removes the only
  real-world eval coverage), or an accepted retry. Check whether any other live test asserts on a
  third-party page's content the same way — if so, this is a class, not one test.

### Folded in from iteration 176's closing sweep (2026-08-23)

A **third** way into the same `live_96` failure, and the one with a real product defect behind it:
a live test that leaks its own Firefox poisons every later sweep on that machine, permanently, until
someone notices the process by hand.

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
LIVE_SWEEP_SUMMARY executed=275 skipped=0 preexisting=9 vanished=0 launch_timeout=0 total=284
274 passed / 1 failed — live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running

  precondition violated — 1 ff-rdp-managed profile dir(s) ... still owned by a live process ...
  ff-rdp-profile-yktWx82EW87KORBQ (pid 79010, spawned by
  live_160_envelope_honesty::live_160_selector_diagnostics_survive)
```

Note the attribution: **not** `unknown test` (Theme A's operator-launch signature) but a named live
test. `ps -o lstart` put pid 79010 at 04:23, roughly five hours before that sweep started, so it
outlived its own test run by hours. It failed identically when `live_96` was re-run in isolation —
so this is not sweep load, and not iteration 176's diff (which touches only `eval`'s statement
scanner). `kill 79010` followed by a re-run gave `live_96_profile_cleanup`: 3 passed / 0 failed.

Two things follow, and Theme A's chosen fix must address both or say why not:

1. `live_160_selector_diagnostics_survive` (or whatever it delegates its browser lifetime to) can
   leave a Firefox running after the test ends. That is the defect; iter-151 and iter-168 both
   worked this seam.
2. `live_96`'s failure is *correct* but reads as a flake, because nothing in the sweep output tells
   the operator that a five-hour-old orphan is the cause. A sweep that begins by naming any
   ff-rdp-managed profile whose owner PID predates the sweep would have said so in one line.
   Related but distinct from `vanished` (iter-173), which is about the port-6000 browser leaving,
   not about a test browser refusing to.

### Folded in from iteration 181's closing sweep (2026-08-23)

A **second** instance of Theme B's class — a live test asserting on a third-party site under sweep
load — this time on the daemon route.

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
LIVE_SWEEP_SUMMARY executed=275 skipped=0 preexisting=9 vanished=0 launch_timeout=0 total=284
274 passed / 1 failed — live_137_daemon_mode_parity::live_137_consent_accept_via_daemon

  daemon never reported live frame targets — status: {
    "running": true, "uptime_seconds": 16, "connections": 0,
    "buffer_sizes": {"network-event": 438},
    "target_count": 1, "live_target_count": 0
  }
```

It navigates to theguardian.com and waits for the daemon to enumerate live frame targets before
driving the consent banner. `network-event: 438` says the page was very much loading; the daemon
just had no *live* frame target 16 s in. Iteration 181's diff cannot reach this: it changes only
`ff-rdp run`'s `assert_network`, and `consent accept` is not a script step. Not re-run in
isolation, so whether this is load, the site, or a real frame-target enumeration gap on the daemon
is open — that determination belongs to Theme B's audit.

This makes Theme B's third task concrete rather than speculative: there are now **two** named
third-party-content assertions (`live_eval_on_hn` → news.ycombinator.com,
`live_137_consent_accept_via_daemon` → theguardian.com), so it is a class.

### Tasks

#### A. live_96 versus the sweep setup — CLOSED 2026-08-23, scope removed

The last box below was the answer, and it has been done: the raw-Firefox recipe is now a setup
step in `.claude/skills/iteration-close/SKILL.md`, naming `ff-rdp launch` as the wrong form and
citing the four occurrences (iters 174, 175, 177, 186). There was never a test-versus-skill
conflict to adjudicate — `live_96` fails only against an ff-rdp-**managed** profile, which is
exactly what `ff-rdp launch` creates and what the documented raw command does not.

- [x] If the answer is documentation, the raw-Firefox recipe goes into `iteration-close`, not
      into a comment nobody reads
- ~~Reproduce by starting the port-6000 browser with `ff-rdp launch`~~ — reproduced four times
  already, unintentionally; a fifth deliberate reproduction buys nothing
- ~~Pick one of the three shapes and record why the other two were rejected~~ — moot, the
  premise (a genuine conflict) was false
- [ ] **Still open, and unrelated to the above:** establish whether
      `live_160_selector_diagnostics_survive` can actually leak its browser, or whether pid 79010
      (iteration 176) came from an earlier interrupted sweep of that same test. This is an orphan
      question, not a `live_96` question

#### B. live_eval_on_hn
- [ ] Determine whether the empty title is a readiness gap on our side or the site not answering
- [ ] Audit the live suite for other assertions on third-party page content — start from the two
      already named (`live_eval_on_hn`, `live_137_consent_accept_via_daemon`)
- [ ] Re-run `live_137_consent_accept_via_daemon` in isolation and decide whether its
      `live_target_count: 0` is load, the site, or a daemon frame-target enumeration gap
- [ ] Fix or re-scope, with the reasoning recorded

### Acceptance Criteria [0/2]

- [ ] Theme A has a landed decision, and a sweep run with a port-6000 browser present no longer
      fails `live_96` — demonstrated by an actual sweep, not by reasoning
      **Half done 2026-08-23, deliberately left unticked.** The decision landed (documentation:
      the raw-browser command is now a setup step in `iteration-close`), but **no sweep was run**
      to demonstrate it, and this AC explicitly forbids closing on reasoning. Tick it when the
      next dual-gate sweep starts its port-6000 browser the documented way and `live_96` passes
- [ ] Theme B's mechanism is named from evidence (readiness vs. site), not guessed

### Out of scope

- Iteration 175's profile-lifetime change. Both failures reproduce independently of it, and its
  own live tests passed in both sweeps.
- The known-on-purpose `live_62` failure under load — that is iteration 181's.

### References

- [[iteration-175-failed-launch-leaks-unmarked-profile-dir]] — the sweeps these came from
- [[iteration-146-live-suite-reliability]] — where `live_96`'s explicit named-PID precondition came from
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the `vanished` / `launch_timeout` classification this
  sweep reported as zero, so neither failure is one of those

## Closing acceptance criterion (covers all parts) [0/1]

- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep (covers all parts)
