---
title: "Iteration 228: re-measure the two click-through tasks now that --with-page carries the facts"
type: iteration
date: 2026-08-31
status: in-progress
branch: iter-228/two-task-benchmark-after-facts
depends_on:
  - 225
dogfood_path: |
  # The measurement itself is the dogfood path — one clean browser, nothing else on port 6000.
  ff-rdp launch --headless
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page \
    --query 'Stable release' --jq '.results.page | {matches, query_source}'
  # AFTER iter-225: {"matches": 1, "query_source": "facts"} — verify by hand before spending
  # $1+ of harness time, because a broken facts pass makes the turn counts meaningless.
  ff-rdp daemon stop
tags:
  - iteration
  - benchmark
  - act-and-see
  - measurement
  - carry-over
---

# Iteration 228: re-measure the two click-through tasks after the facts pass

## Why

[[iteration-225-reader-excerpt-infobox]] shipped Themes A and B — `results.page.facts` and the
`--query` fallback chain — but **not Theme C**, its own measurement. The harness
(`axi/bench-browser`, `matrix --condition ff-rdp --task wikipedia_link_follow,wikipedia_infobox_hop
--repeat 3`) drives its agent through a browser on **port 6000**, and when 225 reached that step
the port was held by a headless Firefox that had been running for four hours and that this run had
not launched. `CLAUDE.md`'s teardown rule is explicit — tear down only the browser this run
launched, never `pkill` — so the only honest options were to contend with another agent's browser,
which makes every turn count meaningless, or to leave the number unmeasured and say so. 225 did the
second: its Task C and its third acceptance criterion are unticked, not reworded.

So the question 225 exists to answer — *does putting the infobox in the view remove the `page-text`
round trips?* — is still open. The mechanism is verified (unit tests, five live tests in
`live_225_reader_facts.rs`); the trajectory cost is not.

## Themes

- **A — Run it on an exclusive browser.** Confirm nothing holds port 6000 (`ff-rdp doctor`,
  `lsof -ti :6000`) before starting, and abort rather than share. `ffrdp-bench.sh` already tears
  down only its own recorded PID; the failure mode here was a *pre-existing* browser, not the
  harness.
- **B — Same two tasks, same shape as 2026-08-31.** `wikipedia_link_follow` and
  `wikipedia_infobox_hop`, `--repeat 3`, same model and prompt, so the number is comparable to the
  7.7 / 10.3 that motivated 225 and to axi's 4.0 / 4.0.
- **C — Read the trajectories, not just the average.** The specific claim to check is that
  `page-text --query` calls per run drop toward zero and that `query_source` appears in the runs
  that used `--query`. An unchanged average with the `page-text` calls gone would mean the turns
  went somewhere else, which is a different finding and worth naming.

## Tasks

### A. Preconditions [0/2]
- [ ] Port 6000 free and the release binary rebuilt from the merge commit that carries 225
- [ ] Hand-verify `query_source: "facts"` on the Python article before spending harness time

### B. Measure [0/2]
- [ ] `matrix --condition ff-rdp --task wikipedia_link_follow,wikipedia_infobox_hop --repeat 3`
- [ ] Record the per-run turn counts, not only the averages

### C. Report [0/2]
- [ ] New dated section in [[axi-benchmark-comparison]] with the trajectory reading
- [ ] Tick or explicitly leave unticked iteration 225's AC 3, in 225's own file, with the number

## Acceptance Criteria [0/3]

- [ ] Both tasks measured at `--repeat 3` on a browser this run owns, numbers recorded whatever
      they are
- [ ] `page-text` calls per run counted and compared against the 2–6 the 2026-08-31 run showed
- [ ] Target ≤ 5 turns average on both tasks — or the measured number recorded and this criterion
      left unticked, never reworded

## Out of scope

- Any further change to the page view. If the number is still above 5, that is a finding to file,
  not a licence to keep editing the collector inside a measurement iteration.
- Default-on for `--with-page` — [[iteration-213-act-and-see-benchmark-rerun]] Theme C, which was
  always gated on this measurement.

## References

- [[iteration-225-reader-excerpt-infobox]] — the change being measured, and its unticked Theme C
- [[axi-benchmark-comparison]] — the 2026-08-31 baseline (7.7 / 10.3) and the harness invocation
