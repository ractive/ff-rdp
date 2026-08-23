---
title: "Iteration 190: two live tests that only fail under sweep conditions — live_96's precondition versus the sweep's own setup, and live_eval_on_hn's third-party dependency"
type: iteration
date: 2026-08-23
status: planned
branch: iter-190/live-sweep-only-failures
depends_on: [iteration-175-failed-launch-leaks-unmarked-profile-dir]
first_call_sites: []
dogfood_path: |
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
tags: [iteration, live-tests, sweep, carry-over]
---

# Iteration 190: live tests that only fail under sweep conditions

Carry-over from [[iteration-175-failed-launch-leaks-unmarked-profile-dir]]'s closing sweeps.

## What was observed

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

## Themes

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

## Folded in from iteration 176's closing sweep (2026-08-23)

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

## Tasks

### A. live_96 versus the sweep setup
- [ ] Reproduce: start the port-6000 browser with `ff-rdp launch`, run the sweep, confirm the
      precondition failure
- [ ] Reproduce the folded-in variant too: leave a test-spawned Firefox alive, confirm `live_96`
      fails with a *named* test as the owner and passes once that PID exits
- [ ] Establish whether `live_160_selector_diagnostics_survive` really can leak its browser, or
      whether pid 79010 came from an earlier interrupted sweep of that same test
- [ ] Pick one of the three shapes above and record why the other two were rejected
- [ ] If the answer is documentation, the raw-Firefox recipe (profile + devtools prefs written by
      hand) goes into `iteration-close`, not into a comment nobody reads

### B. live_eval_on_hn
- [ ] Determine whether the empty title is a readiness gap on our side or the site not answering
- [ ] Audit the live suite for other assertions on third-party page content
- [ ] Fix or re-scope, with the reasoning recorded

## Acceptance Criteria [0/3]

- [ ] Theme A has a landed decision, and a sweep run with a port-6000 browser present no longer
      fails `live_96` — demonstrated by an actual sweep, not by reasoning
- [ ] Theme B's mechanism is named from evidence (readiness vs. site), not guessed
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Iteration 175's profile-lifetime change. Both failures reproduce independently of it, and its
  own live tests passed in both sweeps.
- The known-on-purpose `live_62` failure under load — that is iteration 181's.

## References

- [[iteration-175-failed-launch-leaks-unmarked-profile-dir]] — the sweeps these came from
- [[iteration-146-live-suite-reliability]] — where `live_96`'s explicit named-PID precondition came from
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the `vanished` / `launch_timeout` classification this
  sweep reported as zero, so neither failure is one of those
