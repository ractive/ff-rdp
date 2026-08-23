---
name: iteration-close
description: "Closing procedure for an ff-rdp iteration — the live-Firefox sweep, the xtask gate enumeration, and the carry-over sweep to run before /create-pr on an iter-* branch. Invoke when finishing an iteration, before opening or updating its PR, or whenever you need to run or interpret `cargo run -p xtask -- live-sweep`. CLAUDE.md directs you here explicitly; do not rely on this description alone to trigger it."
---

# Closing an iteration

Run these three before `/create-pr` on an `iter-*` branch, in order. None is automated —
iter-162a and iter-162b deleted the gates that used to pretend to check them. See
`kb/discipline-rationale.md` for why, if you want the history; you do not need it to follow this.

---

## 1. The live sweep

**Standing policy: every iteration touching product source pastes a real sweep into its PR body.**

```sh
FF_RDP_LIVE_TESTS=1 [FF_RDP_LIVE_NETWORK_TESTS=1] cargo run -p xtask -- live-sweep
```

Paste the real `LIVE_SWEEP_SUMMARY` line, the pass/fail counts, **and the env gates you set**.
Do not paraphrase and do not reuse an earlier run's numbers.

This is not ceremony. On iter-160 the sweep caught three failures the targeted tests missed, one
of which would have broken every cross-origin frame click. An isolated `cargo test live_foo` is
not a substitute: iter-153 shipped a broken feature certified by a *truthful* isolated run, which
passes alone and fails under contention.

### Why not `cargo test-live`

`cargo test-live`'s `N passed; 0 failed` **does not mean N tests reached Firefox** (iter-155).
Every live test re-checks its env gate at runtime and `return`s early when it is unset; libtest
scores that `ok`, indistinguishable from a real run. `live-sweep` classifies each test from its
own `#[ignore = "…"]` reason, runs the qualified ones with `--include-ignored`, and runs the rest
*without* it so libtest reports them `ignored` in its own vocabulary.

(iter-158 removed the underlying trap for 152 call sites by making them panic rather than skip,
which is what makes a sweep's `ok` mean "reached Firefox". Tests added since must not reintroduce
a silent early return.)

### Reading the summary

```
LIVE_SWEEP_SUMMARY executed=N skipped=M preexisting=K vanished=V launch_timeout=L total=T
```

- `executed=N` — the tests the sweep **intended** to run (`part.qualified.len()`, a partition
  computed before anything executes). It is *not* a count of results. Quote it only alongside the
  runner's own `P passed / F failed`, and **check that `P + F == N`**: on 2026-08-23 five sweeps
  out of eight reported `executed` nine higher than `passed + failed`, with `skipped=0` — nine
  tests produced no verdict and the summary line did not say so. A mismatch means the record is
  incomplete; do not treat the pass count as evidence until it reconciles.
- `skipped=M` — env gate not set. Usually the `FF_RDP_LIVE_NETWORK_TESTS` set.
- `preexisting=K` — needs a Firefox *somebody else* started on the fixed port 6000 (the
  `ff-rdp-core` live tests never launch one). The sweep probes that port at start and, finding
  nothing, reports them `ignored` rather than folding them into `executed`. Start one with
  **exactly** this — a raw browser, never `ff-rdp launch`:

  ```sh
  firefox -no-remote --start-debugger-server 6000 --headless
  ```

  `ff-rdp launch` creates an ff-rdp-*managed* profile, which is precisely the state
  `live_96_profile_cleanup` asserts is absent — so using it red-lines that test and the failure
  looks like a product defect. Four separate agents made this exact substitution on 2026-08-23
  (iters 174, 175, 177, 186); one of them filed a plan blaming this skill for a conflict that
  did not exist. If `live_96` fails, check how you started the port-6000 browser before
  diagnosing anything else.
- `vanished=V` (iter-173) — the port-6000 browser was there at classification time and gone when
  the tier ran. The sweep re-probes immediately before each tier that needs it (and again after a
  failing phase), and reports those tests `ignored`. **Does not fail the sweep.** A non-zero `V`
  means restart the browser and re-run those targets before treating the numbers as the record.
- `launch_timeout=L` (iter-173, folded in from iter-170) — Firefox never opened its debug port
  within the per-test budget under sweep load. **Still fails the sweep** (it is a real red), but
  it is not a product defect; re-run the named test in isolation, or raise
  `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`.

`vanished` and `launch_timeout` are carved out of `executed`, not added to it — `total=T` is
conserved, so a reclassification can never inflate the number you quote.

**A summary is meaningless without the gates that produced it.** Two sweeps compare only when the
same gates were set. Measured across the 158–161 batch:

| iter | gates | summary | result |
|---|---|---|---|
| 159 | both | `executed=237 skipped=0 preexisting=0 total=237` | 225 passed / 3 failed |
| 160 | `FF_RDP_LIVE_TESTS` only | `executed=209 skipped=32 preexisting=8 total=249` | 209 passed / **0 failed** |

160 looks better and is smaller: the 32 unrun network tests include `live_block_url_pattern`, a
real product defect that only fails when it executes. `0 failed` over a shrunken corpus is the
same false green this whole section exists to prevent, one level up. Cite sweeps as
`FF_RDP_LIVE_TESTS=1 [FF_RDP_LIVE_NETWORK_TESTS=1] → LIVE_SWEEP_SUMMARY …`, and never compare a
`skipped=0` run against a `skipped>0` one without saying so.

---

## 2. The xtask gates

There is **no aggregator subcommand**. iter-162a deleted `check-iteration-ready` because it
hard-coded its own sub-check list, so every gate change cost a count bump and an assertion edit.
Enumerate what xtask actually ships, then run each one:

```bash
cargo run -q -p xtask -- --help          # list the check-* subcommands
cargo run -p xtask -- check-<name> ...   # run each one
```

**Do not invent subcommand names that are not in the help output.** Pass `--plan <path>` or
`--base origin/main` only where that gate's own `--help` documents the flag. Fix every reported
failure before pushing — most gates are local-only, so do not assume CI will catch what you skip.

CI's `discipline` job runs only two of them: `check-live-test-layout` and
`check-source-invariants`. Two more are useful and local-only:

- `check-firefox-refs <plan>` — validates `firefox_refs:` line ranges against the local Firefox
  checkout (`FF_RDP_FIREFOX_PATH`). The only gate checking a claim against ground truth *outside*
  this repository; both of its catches were false Firefox spec citations, stopped before merge.
- `check-actor-kb-sync --since origin/main` — fails if an actor `.rs` changed without a
  corresponding `kb/rdp/actors/*.md` update. A docs-sync reminder, not a defect gate.

---

## 3. The carry-over sweep

**This replaced the AC gate. It is the only thing standing between "not finished" and "silently
forgotten", and nothing enforces it.**

**Build the table mechanically, from the sweep output — not from memory of what you flagged.**
Read back the live-sweep log line by line and give **every non-green line its own row**. Then add
everything else this iteration produced:

- every AC left unticked,
- every `[deferred …]` annotation,
- every out-of-scope finding you or a reviewer flagged, in the plan, the PR body or a milestone.

A finding stated **anywhere** in the PR body or the plan is a carry-over candidate regardless of
which section it appears in — including the prose of the live-sweep write-up. That is exactly how
iter-165 lost one: it diagnosed a leaked Firefox correctly in its sweep prose, gave it no row, and
neither its own sweep nor its review pass noticed.

Rows you must **not** drop:

- failures you diagnose as **environmental**, pre-existing, or load-sensitive. **"Environmental" is
  a diagnosis, not a disposition** — a live suite that kills an unowned process, or a sweep that
  misreports a browser that vanished, is a defect of ours either way;
- items that **passed this run** but failed a previous one. One green run is not evidence a defect
  is fixed, particularly for anything load-sensitive.

For **each** row, record one of exactly four dispositions, now:

1. **fold** it into the next iteration's plan (edit in place — never rename or move a plan file
   that a running loop references),
2. **file** a new plan at `kb/iterations/iteration-NN-slug.md`, validated with
   `cargo run -p xtask -- check-iteration-plan <path>`,
3. **closed in this PR** — the fix landed here; say where,
4. **no plan, with a stated reason** — allowed only when there is nothing measured left to act on,
   and it must name what would change that ("if it fails again it needs its own plan").

List every item and its disposition in the PR body under `## Carry-over`. An unlabelled row is a
visibly incomplete sweep. An item you cannot place is not a reason to skip it — file it as its own
plan.

**If an external process interfered with the sweep** (someone killed a browser, the machine slept,
another sweep overlapped), say so on the affected rows and re-run before treating the numbers as
the record. A contaminated sweep produces confidently-worded plans for defects that were never
there.

Carry-over must be filed **before the current PR merges**. iter-165 was filed late, after its
parent merged, and only because a human went looking.

### Ticking the plan

Tick every scope checkbox whose work actually landed, verified against the real diff. Then update
each section heading's `[N/M]` count.

**Nothing checks tick state.** No gate reads these boxes — the only thing making a tick worth
anything is that you ticked it honestly. If an AC's premise turned out to be wrong, leave the box
empty and say why in the plan. **Never reword an acceptance criterion so it matches what
happened**; that reflex produced 28 commits and is why the gate was deleted rather than repaired.

Recording a measured result alongside a ticked `live_*` AC is still worth doing for the next
reader — as plain prose, no special syntax:

```
- [x] live_screenshot_full_page: PNG height ≥ scrollHeight × DPR [2026-08-12: 2400 px ≥ 1200 × 2]
```

---

## Then

`/create-pr`, then `/review-pr`, then `/merge-pr`. `/merge-pr` sets the plan's `status: done` on
the branch before merging, so the flip rides in with the merge.
