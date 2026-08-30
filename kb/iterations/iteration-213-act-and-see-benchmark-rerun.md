---
title: "Iteration 213: re-measure the axi benchmark after act-and-see"
type: iteration
date: 2026-08-29
status: planned
branch: iter-213/act-and-see-benchmark-rerun
depends_on: [iteration-210-act-and-see, iteration-211-find-not-guess, iteration-212-ambient-context]
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --jq '.results.page.interactive[] | select(.name | test("Babbage")) | .ref'
  # expected: one ref string — the handle the benchmark's click-through tasks need
  ff-rdp click --ref e<N> --with-page --jq '.results.page.headings[0].text'
  # expected: "Charles Babbage" — the two-command trajectory, measured end to end
tags: [iteration, benchmark, agent-ergonomics, measurement]
---

# Iteration 213: re-measure the axi benchmark after act-and-see

[[iteration-210-act-and-see]] shipped `--with-page`, refs from `a11y summary`/`snapshot`,
`type --submit`, and an idempotent `launch` — every mechanism the
[[axi-benchmark-comparison]] trajectories said was missing. Its last acceptance criterion was to
re-run the benchmark and show the turn count actually fell:

> Benchmark: re-run [[axi-benchmark-comparison]] `--repeat 3`; average turns on
> `wikipedia_infobox_hop`, `wikipedia_link_follow`, `wikipedia_search_click` ≤ 5 (were 8.0, 7.3,
> 8.3) with the same one-paragraph system prompt.

That criterion was left **unticked** in 210 and moved here rather than reworded, because it is not
a code change and cannot honestly be signed off from inside the implementation PR: the harness
drives real Claude Code agents against live Wikipedia over hours and costs real money per run, and
210's own agent had neither the budget nor a way to spawn the sub-agents the harness needs. The
mechanisms are tested (six live tests in `tests/live/live_210_act_and_see.rs`); what is unmeasured
is whether an agent given the same one-paragraph prompt actually *reaches for them*.

That last question is the whole point. `--with-page` is opt-in by design (210's "Default-on?"
note deliberately deferred the decision), so a benchmark where agents never discover the flag
would measure the same 8 turns and would be the correct answer, not a broken run.

## Themes

- **A — Reproduce the baseline harness.** The 2026-08-29 harness lived in a session scratchpad,
  not in the repo, so the comparison is currently unreproducible by anyone else. Land the runner
  and the task list under `tools/` before measuring anything with it.
- **B — Re-measure.** Same tasks, same `--repeat 3`, same one-paragraph system prompt, new ff-rdp
  binary. Record turns, cost, and pass rate per task. Since iter-211 this covers **two** shipped
  changes, not one: the click-through mechanisms from 210 and the extraction mechanisms
  (`--query`, the `page-text` cap, full accessible names) from
  [[iteration-211-find-not-guess]] — folded in here rather than re-run twice, because it is the
  same harness, the same money, and the same 42 tasks.
- **D — Did the ambient context change turn 1?** Folded in from
  [[iteration-212-ambient-context]], whose own benchmark criterion was left unticked rather than
  reworded. 212 made bare `ff-rdp` a live-state view and added an opt-in `SessionStart` hook that
  prints it; the question is whether an agent's *first* tool call stops being `--help`. The
  harness passes `--setting-sources ""`, so an installed hook is **not loaded** — supplying the
  hook's output through `--append-system-prompt` measures the payload, not the hook, and deciding
  whether that answers the question is part of this theme rather than an assumption it starts
  from.
- **C — Decide `--with-page`'s default.** If agents do not reach for the flag from `--help`, the
  finding is about discoverability, and the options are a default-on JSON page view, an error-path
  hint, or [[iteration-212-ambient-context]] — not a louder `--help`.

## Tasks

### A. Reproduce the harness [0/2]
- [ ] Move the benchmark runner and task definitions into `tools/axi-bench/` so a later reader can
      re-run the comparison without recovering a deleted scratchpad
- [ ] Record the exact system prompt used for both tools, verbatim, in
      [[axi-benchmark-comparison]] — a turn count is not comparable without it

### B. Re-measure [0/3]
- [ ] Re-run all 42 tasks `--repeat 3` for ff-rdp with the post-210/post-211 binary; keep
      chrome-devtools-axi's 2026-08-29 numbers as the reference rather than re-running them
- [ ] Record the per-task table in the Outcome section of this plan AND in
      [[axi-benchmark-comparison]]
- [ ] Classify every extraction trajectory as "used `--query`" or "did not" — the same
      mechanism-vs-discoverability split Theme C applies to `--with-page`

### D. Ambient context (from iter-212) [0/2]
- [ ] Decide and record how the hook is delivered under `--setting-sources ""`: either a harness
      change that loads a real settings file, or `--append-system-prompt` with the hook's output —
      and state plainly which of the two the recorded numbers measure
- [ ] Classify every ff-rdp trajectory by its **first** tool call: `--help`, a browser command, or
      bare `ff-rdp`

### C. Decide the default [0/1]
- [ ] From the measured trajectories, state whether `--with-page` should default on for JSON
      output — and if agents never used it, say so and name which discoverability surface to
      change

## Acceptance Criteria [0/6]

- [ ] `tools/axi-bench/` runs the comparison from a clean checkout with no scratchpad recovery
- [ ] The three click-through tasks (`wikipedia_infobox_hop`, `wikipedia_link_follow`,
      `wikipedia_search_click`) have a measured post-210 average turn count recorded, whatever it
      is — a number that did NOT improve is a valid, publishable result and must not be re-run
      until it looks better
- [ ] Every trajectory is classified as "used `--with-page`" or "did not", so a flat turn count
      can be attributed to the mechanism or to discoverability
- [ ] Carried over from [[iteration-211-find-not-guess]] (left unticked there, not reworded): the
      three extraction tasks (`tabular_data_analysis`, `wikipedia_deep_extraction`,
      `github_issue_investigation`) have a measured post-211 average turn count recorded —
      211's own target was ≤ 6 turns for the first two (were 9.3, 10.7) and 3/3 passes for the
      third, but as with the click-through tasks a number that did NOT improve is a valid,
      publishable result and must not be re-run until it looks better
- [ ] Carried over from [[iteration-212-ambient-context]] (left unticked there, not reworded):
      every ff-rdp run's first tool call is classified, and the share that is a browser command
      rather than `--help` is recorded. 212's own target was ≥ 80%; as above, a number that did
      NOT improve is a valid, publishable result. The record must also say which delivery
      mechanism Theme D chose, because "≥ 80% with the hook's text pasted into the system prompt"
      and "≥ 80% with the hook installed" are different claims.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Do not re-run chrome-devtools-axi.** Its behaviour did not change; re-running it costs the
  same money and adds variance to the reference side of the comparison.
- **Three repeats is the floor, not the target.** The 2026-08-29 run found the turn gap consistent
  across all three repeats, which is what made it worth acting on. A single post-210 run that
  looks good proves nothing.

## Out of scope

- Changing `--with-page`'s default. Theme C produces the recommendation; the change itself is a
  follow-up plan, so that a measurement iteration cannot quietly become a behaviour-change one.

## References

- [[iteration-210-act-and-see]] — the click-through change being measured
- [[iteration-211-find-not-guess]] — the extraction change being measured, whose own benchmark
  AC was folded in here
- [[axi-benchmark-comparison]] — the baseline, and where the new table goes
- [[iteration-212-ambient-context]] — the other candidate answer if discoverability is the problem
