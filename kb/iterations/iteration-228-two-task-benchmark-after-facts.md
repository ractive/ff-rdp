---
title: "Iteration 228: re-measure the two click-through tasks now that --with-page carries the facts"
type: iteration
date: 2026-08-31
status: in-review
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

### A. Preconditions [2/2]
- [x] Port 6000 free and the release binary rebuilt from the merge commit that carries 225
      (`lsof -ti :6000` empty; `ff-rdp 0.3.0 (cf99c84401be)` installed to `~/.cargo/bin`)
- [x] Hand-verify `query_source: "facts"` on the Python article before spending harness time —
      `--query 'Filename extensions'` and `--query 'Typing discipline'` both answer
      `{"matches": 2, "query_source": "facts"}`, and `facts[0]` is
      `{"Stable release": "3.14.7[3] / 5 August 2026…"}`. Note the plan predicted `query_source:
      "facts"` for `'Stable release'` itself and the run returns `"readability"` with
      `matches: 4`: the article prose also says "the stable release is expected to launch in
      October 2026", and the chain checks reader text first. Cheapest-first working as designed,
      not a broken facts pass — the fact is in `facts` either way.

### B. Measure [2/2]
- [x] `matrix --condition ff-rdp --task wikipedia_link_follow,wikipedia_infobox_hop --repeat 3`
      (6 runs, 6/6 pass, $1.18)
- [x] Record the per-run turn counts, not only the averages — `link_follow` 13 / 10 / 4 (avg
      **9.0**), `infobox_hop` 14 / 8 / 12 (avg **11.3**)

### C. Report [2/2]
- [x] New dated section in [[axi-benchmark-comparison]] with the trajectory reading
- [x] Tick or explicitly leave unticked iteration 225's AC 3, in 225's own file, with the number —
      left **unticked**, annotated with 9.0 / 11.3

## Acceptance Criteria [2/3]

- [x] Both tasks measured at `--repeat 3` on a browser this run owns, numbers recorded whatever
      they are — 9.0 and 11.3; port 6000 verified free before the run, browser started and torn
      down by `ffrdp-bench.sh` (own recorded PID), no `pkill`
- [x] `page-text` calls per run counted and compared against the 2–6 the 2026-08-31 run showed —
      `link_follow` 2 / 2 / 0, `infobox_hop` 5 / 2 / 2; mean 2.2, range 0–5. Essentially unmoved
- [ ] Target ≤ 5 turns average on both tasks — or the measured number recorded and this criterion
      left unticked, never reworded
      [Measured 9.0 and 11.3. Not met. Left unticked.]

## Out of scope

- Any further change to the page view. If the number is still above 5, that is a finding to file,
  not a licence to keep editing the collector inside a measurement iteration.
- Default-on for `--with-page` — [[iteration-213-act-and-see-benchmark-rerun]] Theme C, which was
  always gated on this measurement.

## References

- [[iteration-225-reader-excerpt-infobox]] — the change being measured, and its unticked Theme C
- [[axi-benchmark-comparison]] — the 2026-08-31 baseline (7.7 / 10.3) and the harness invocation

## Outcome

### The numbers

ff-rdp `cf99c84` (`main` at the [[iteration-225-reader-excerpt-infobox]] merge), axi
`bench-browser` harness, `matrix --condition ff-rdp --task
wikipedia_link_follow,wikipedia_infobox_hop --repeat 3`, `claude-sonnet-4-6` for both agent and
judge, same one-paragraph system prompt as every earlier run.

| Task | axi | @5a0071d (08-31) | **@cf99c84 (09-01)** | per run | `page-text` per run |
|---|---|---|---|---|---|
| wikipedia_link_follow | 4.0 | 7.7 | **9.0** / $0.164 / 3 | 13, 10, **4** | 2, 2, 0 |
| wikipedia_infobox_hop | 4.0 | 10.3 | **11.3** / $0.229 / 3 | 14, 8, 12 | 5, 2, 2 |

6/6 pass. +1.3 and +1.0 against 08-31 — at n = 3 that is noise, so the finding is **unchanged**,
not "worse". `query_source` appears in 1 of 6 runs. Target (≤ 5) not met, AC 3 unticked here and
in 225.

### What the trajectories say, which is the point of the iteration

`navigate --with-page` was used in **1 run of 6**. That run is the whole answer:

```
ff-rdp --help
ff-rdp navigate "https://en.wikipedia.org/wiki/Ada_Lovelace" --with-page --query "Charles Babbage"
ff-rdp click --ref e1 --with-page --query "born"
```

4 turns, 3 commands, 0 `page-text` calls — target met and axi matched, on the payload 225 shipped.
The other five ran bare `navigate` → `page-text --query` (which answers but returns no ref) → a
ref hunt across `a11y summary | grep`, `dom <guessed selector>` and 1–3 `eval` scripts → `click
--ref` → `page-text --query` on the destination. 4–9 extra turns.

The split is exactly the help text. **Five of six runs read `ff-rdp --help | head -50` and nothing
else**; in that window `--with-page` appears only on `click --ref e3`, which an agent that has not
navigated cannot use, while the `navigate` line is bare. `ff-rdp navigate <URL> --with-page
--query "…"` is at line **333 of 441**. The one run that read the whole file is the one run that
used it.

225's mechanism is therefore correct and 08-31's "discoverability is solved" was measured too
coarsely: it counted `--with-page` anywhere in a run. Solved for `click`; not for `navigate`, and
`navigate` is the turn that decides whether the ref hunt happens.

### Also observed, deliberately not acted on

- A guessed selector that misses costs the full `click --timeout` (10 s) before reporting
  `0 elements matched (not found)`. `link_follow` runs 1 and 2 both guessed
  `a[href="/wiki/Charles_Babbage"]`; hand-checked in the page's own JS, this Wikipedia render
  writes that attribute absolute, so zero elements really do match. Agent-side guess, not an
  ff-rdp defect — but it is where run 1's 70 s went. No turn cost.
- The `recv failed: Connection reset by peer` of 08-31
  ([[iteration-224-with-page-daemon-connection-reset]]) did not recur in these 6 runs. An absence
  at n = 6 against a hand-reproduced 1-in-5, not evidence of a fix.

### Scope held

No product change was made. The plan's "Out of scope" said an above-5 number is a finding to file,
not a licence to edit the collector, and nothing under `crates/` is touched by this branch.

### Carry-over

- [[iteration-230-quickstart-navigate-with-page]] — put the `navigate <URL> --with-page --query`
  idiom inside the first 50 lines of `--help`, make `check-help-idioms` hold it there, and
  re-measure these two tasks with per-run `navigate --with-page` adoption recorded next to turns.
- [[iteration-213-act-and-see-benchmark-rerun]] Theme C (default-on `--with-page`) stays gated:
  deciding a default from a run where the flag is adopted once in six would be deciding it blind.

## References

- [[iteration-230-quickstart-navigate-with-page]] — the carry-over this measurement produced
