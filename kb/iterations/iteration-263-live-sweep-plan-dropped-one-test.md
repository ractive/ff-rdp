---
title: "Iteration 263: two live sweeps ran 319 of 320 qualified tests and said nothing about the one they dropped"
type: iteration
date: 2026-09-07
status: planned
branch: iter-263/live-sweep-plan-dropped-one-test
depends_on:
  - iteration-252-content-process-resources-on-the-direct-route
first_call_sites: []
dogfood_path: |
  # Reproduce by running a sweep *while* `cargo test --workspace` is running,
  # which is the only condition under which this was observed:
  #
  #   cargo test --workspace -q &                  # hold the build lock
  #   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
  #     cargo run -q -p xtask -- live-sweep | head -1
  #
  # Compare that header's `N qualified` against the uncontended plan:
  #
  #   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
  #     cargo run -q -p xtask -- live-sweep --dry-run | head -1
  #
  # On 2026-09-07 the contended runs printed 319 and the dry-run printed 320.
tags: [iteration, live-sweep, discipline, false-green, carry-over]
---

# Iteration 263: a sweep that runs 319 of 320 and reports it as a full run

Carry-over from [[iteration-252-content-process-resources-on-the-direct-route]], filed before
that PR merges per CLAUDE.md's carry-over rule.

## What was observed

Iteration 252 added one live test,
`live_252_console_follow_content_resources::live_252_console_follow_sees_content_process_messages_both_routes`,
with the standard gate `#[ignore = "requires a live Firefox instance — set FF_RDP_LIVE_TESTS=1"]`.

Two consecutive `live-sweep` runs on 2026-09-07 printed:

```text
live-sweep: -p ff-rdp-cli --test live: 319 qualified …
test result: FAILED. 304 passed; 15 failed; … 12 filtered out
test result: FAILED. 307 passed; 12 failed; … 12 filtered out
LIVE_SWEEP_SUMMARY executed=328 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=328
```

`grep live_252` over both logs found **nothing** — no `ok`, no `FAILED`, no mention. The test
existed in the tree (mtime 14:30, both sweeps started at 14:39 and 14:44), was registered in
`tests/live/main.rs`, and was present in the compiled binary
(`strings target/debug/deps/live-* | grep live_252` → 7 hits;
`--list` names it). Immediately afterwards, and repeatedly since, the plan is **320**:

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -q -p xtask -- live-sweep --dry-run
live-sweep: -p ff-rdp-cli --test live: 320 qualified …
LIVE_SWEEP_SUMMARY executed=329 … total=329
```

The only difference between the runs that said 319 and the runs that say 320: the two 319 runs
were launched while `cargo test --workspace` was running in the same working tree.

## Why this matters more than one test

`executed=N` is the number `live-sweep` exists to make trustworthy. iteration 155 built the
partition so a skipped test says `ignored` in libtest's own vocabulary; iteration 158 made a
failed Firefox launch panic rather than return early; iteration 197 gave a stalled phase a
`timed_out` number so silence could not read as success. This is the same failure one level up:
a test vanished from the *plan itself*, so no counter had anything to report and `total=328`
looked internally consistent. `executed + skipped + preexisting == total` held — over the wrong
corpus. Nothing in the summary line can distinguish "we ran everything" from "we ran everything
we happened to notice."

That is exactly the shape `kb/discipline-rationale.md` warns about, and it is why iteration 252's
own live test came within one `grep` of being reported green without ever having run.

## Scope

- [ ] reproduce the 319/320 split deterministically, or establish that it cannot be reproduced
      and say what evidence would change that
- [ ] find where a qualified name is dropped between `scan_modules_dir` and the `--exact` list
      the phase is handed — candidates worth checking first: the directory read racing a
      concurrent `cargo` writing into the tree, and any cross-check of the partition against a
      binary listing that a concurrent build can leave stale
- [ ] make the sweep **fail rather than under-report** when the names it was handed do not
      account for the whole gated corpus — the counter iteration 197 added for stalled phases,
      applied to the plan instead of the run
- [ ] a regression test that a dropped name is reported, not silently absent

## Acceptance Criteria [0/3]

- [ ] the mechanism is identified and named in the plan, or the plan is closed with the
      measurements and a written reason the mechanism could not be found
- [ ] a sweep whose `--exact` list does not cover every gated test it discovered exits non-zero
      and names the missing tests
- [ ] `cargo run -p xtask -- live-sweep --dry-run` and a real sweep of the same tree, with the
      same gates, report the same `qualified` count — asserted by a test, not by hand

## Notes

- Do **not** "fix" this by making the sweep re-scan more often; the failure to guard against is a
  plan that is silently smaller than the corpus, whatever produced it.
- Related: [[iteration-203-live-sweep-watch-conditions-third-holder]] (the sweep's other standing
  watch conditions).
