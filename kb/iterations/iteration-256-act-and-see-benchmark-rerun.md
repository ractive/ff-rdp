---
title: "Iteration 256: re-measure the axi benchmark after act-and-see"
type: iteration
date: 2026-08-29
status: done
branch: iter-256/act-and-see-benchmark-rerun
depends_on: [iteration-210-act-and-see, iteration-211-find-not-guess, iteration-212-ambient-context]
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --jq '.results.page.interactive[] | select(.name | test("Babbage")) | .ref'
  # expected: one ref string — the handle the benchmark's click-through tasks need
  ff-rdp click --ref e<N> --with-page --jq '.results.page.headings[0].text'
  # expected: "Charles Babbage" — the two-command trajectory, measured end to end
tags: [iteration, benchmark, agent-ergonomics, measurement]
takeover_sequencing_2026_09_09: >-
  To resolve the 255/256 cycle while preserving one iteration per PR, Theme A is
  prepared and verified first on the 256 branch, checkpointed only after ordered
  gates, and exported at that exact revision outside the active product tree for 255
  Theme C. Park that branch while 255 is implemented, reviewed and merged; then
  update 256 onto the merged baseline and perform its final measurements before its
  single PR merges. This explicitly supersedes the status notes land-first phrase
  with prepare-and-verify-first. Original acceptance criteria, harness ownership,
  historical model/prompt, and final measurement coverage are preserved.
takeover_reconciliation_252: "2026-09-09, reconciliation after252: historical runner and task sources recovered from original Claude transcripts; runner SHA25611659d64e71fa116744f6b837d0b8b246c8623eb0f3cf796a2342a7567a24a8b matches pinned upstream d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb. Use agent AND judge claude-sonnet-4-6. Actual same-model probes twice failed HTTP401 invalid API key before tokens, unlike the historical organization error. ThemeA implementation/export can proceed; real clean-checkout comparison and remaining measurement ACs cannot be claimed satisfied without successful account capability. Keep baseline prompt unchanged and label the separate ambient payload/hook treatment precisely; preserve all historical completed measurements and unmet targets."
benchmark_auth_resolution_252: "2026-09-09: the two earlier HTTP401 probes are retained as history. A third exact same-model probe succeeded with CAPABILITY_OK, exit0, claude-sonnet-4-6 modelUsage and 900ms duration, using the already cached claude.ai Team session with only the stale ANTHROPIC_API_KEY omitted per process. No login, global configuration or model/prompt change. Use that scoped environment for actual harness runs. This resolves account capability only; actual benchmark acceptance criteria remain unticked until measured. Evidence: .git/ralph-loop/20260909-takeover/harness-prep/auth-resolution.md and capability-cached-session.stdout."
theme_a_preparation_2026_09_13: "Theme A is prepared on the dedicated 256 branch at base f18a876866fd5fa047d4ca7e3a71dd88018b8824. Preserve the existing prepare/verify/export then 255 then final 256 sequence. Historical task wording remains unchanged: actual inventory is 14 task definitions x 3 repetitions = 42 runs, not 126. Final clean-checkout comparison and ambient-delivery ACs remain pending; no acceptance is inferred from harness smoke. Actual agent and judge modelUsage, list cost, Claude CLI 2.1.259 versus historical 2.1.241, both source revisions and binary hash are required provenance."
dogfood_script: iteration-256-act-and-see-benchmark-rerun.dogfood.sh
---

# Iteration 256: re-measure the axi benchmark after act-and-see

> **Renumbered 213 → 256 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 213.

> **Status note (2026-09-06):** was `in-progress` with no branch and no PR; reset to `planned`. Theme E's blocker (iteration 220) merged on 2026-08-30. Theme A (land `tools/axi-bench/` — verified absent today) is autonomous and must land first because 255 Theme C needs the harness. Themes B/D are the paid, hours-long measurement: run once, last, so it covers 237/238/239/253/255 together; if the budget is not there, leave those ACs unticked and file the measurement as carry-over.

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

### A. Reproduce the harness [2/2]
- [x] Land the ff-rdp side of the harness under `tools/axi-bench/` **without checking in the
      upstream TypeScript** (CLAUDE.md: no polyglot tooling): a `README.md` pinning the upstream
      `kunchenguid/axi` commit, `ff-rdp-condition.patch` (the `conditions.yaml` / `types.ts` /
      `lifecycle.ts` diff), `ffrdp-bench.sh` (sources `dogfood-lib.sh`, launches one headless
      Firefox, kills only that PID), and a `run.sh` that clones the pinned commit into a temp
      dir, applies the patch, builds ff-rdp into a temp target dir, prepends it to PATH, refuses
      to start if port 6000 is busy, and runs `pnpm bench matrix` — so a later reader can
      re-run the comparison without recovering a session scratchpad. The current copy is at
      `/private/tmp/claude-501/-Users-james-devel-ff-rdp/6e29cb88-95d5-48ad-b750-bad99c3e3d49/scratchpad/`
      (`axi/`, `bin/ffrdp-bench.sh`); the 2026-08-30 wrappers are in the launching session's
      scratchpad (`run-axi-bench.sh`, `run-mcp-bench.sh`)
- [x] Record the exact system prompt used for both tools, verbatim, in
      [[axi-benchmark-comparison]] — a turn count is not comparable without it

### B. Re-measure [3/3]
- [x] Re-run all 42 tasks `--repeat 3` for ff-rdp with the post-210/post-211 binary; keep
      chrome-devtools-axi's 2026-08-29 numbers as the reference rather than re-running them
- [x] Record the per-task table in the Outcome section of this plan AND in
      [[axi-benchmark-comparison]]
- [x] Classify every extraction trajectory as "used `--query`" or "did not" — the same
      mechanism-vs-discoverability split Theme C applies to `--with-page`

### D. Ambient context (from iter-212) [2/2]
- [x] Decide and record how the hook is delivered under `--setting-sources ""`: either a harness
      change that loads a real settings file, or `--append-system-prompt` with the hook's output —
      and state plainly which of the two the recorded numbers measure
- [x] Classify every ff-rdp trajectory by its **first** tool call: `--help`, a browser command, or
      bare `ff-rdp`

### C. Decide the default [1/1]
- [x] From the measured trajectories, state whether `--with-page` should default on for JSON
      output — and if agents never used it, say so and name which discoverability surface to
      change

### E. Re-measure after iter-219's reader-view fix [2/2]
- [x] Carried over from [[iteration-219-reader-view-page]] (its own AC 6, left unticked there,
      not reworded): re-run `wikipedia_link_follow` / `wikipedia_infobox_hop`, `--repeat 3`, same
      harness/model/prompt, against a binary carrying 219's reader-view fix (content zones +
      excerpt). 219 fixed the two defects this benchmark's Theme B diagnosed — chrome-truncated
      `interactive` and no page text — but could not re-measure: the harness this theme owns
      wasn't available to it, and its own click-through dogfood step times out on Wikipedia
      (a pre-existing iter-210 defect, not a 219 regression — filed as
      [[iteration-220-with-page-after-navigating-click]]). **Blocked on 220**: the trajectory this
      re-measurement needs is exactly the one that times out today.
- [x] Update [[axi-benchmark-comparison]] with the new numbers once the above runs, alongside the
      existing 2026-08-30 table — done 2026-08-31: `link_follow` 7.7, `infobox_hop` 10.3 on
      `5a0071d`; not ≤ 5; causes filed as 224 (daemon reset) and 225 (excerpt lacks infobox)

## Acceptance Criteria [6/6]

- [x] `tools/axi-bench/run.sh` runs the comparison from a clean checkout with no scratchpad recovery
      and no TypeScript committed to this repo
- [x] The three click-through tasks (`wikipedia_infobox_hop`, `wikipedia_link_follow`,
      `wikipedia_search_click`) have a measured post-210 average turn count recorded, whatever it
      is — a number that did NOT improve is a valid, publishable result and must not be re-run
      until it looks better
- [x] Every trajectory is classified as "used `--with-page`" or "did not", so a flat turn count
      can be attributed to the mechanism or to discoverability
- [x] Carried over from [[iteration-211-find-not-guess]] (left unticked there, not reworded): the
      three extraction tasks (`tabular_data_analysis`, `wikipedia_deep_extraction`,
      `github_issue_investigation`) have a measured post-211 average turn count recorded —
      211's own target was ≤ 6 turns for the first two (were 9.3, 10.7) and 3/3 passes for the
      third, but as with the click-through tasks a number that did NOT improve is a valid,
      publishable result and must not be re-run until it looks better
- [x] Carried over from [[iteration-212-ambient-context]] (left unticked there, not reworded):
      every ff-rdp run's first tool call is classified, and the share that is a browser command
      rather than `--help` is recorded. 212's own target was ≥ 80%; as above, a number that did
      NOT improve is a valid, publishable result. The record must also say which delivery
      mechanism Theme D chose, because "≥ 80% with the hook's text pasted into the system prompt"
      and "≥ 80% with the hook installed" are different claims.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome (2026-08-30, measured by hand from the launching session — Theme A not yet done)

Run against ff-rdp `28695d3` (post-211, pre-212), same harness/model/prompt as the baseline;
full tables and trajectory analysis in [[axi-benchmark-comparison]] § "Re-measurement 2026-08-30".

| | axi (ref) | ff-rdp @0a87d1d | ff-rdp @28695d3 |
|---|---|---|---|
| all 42 runs | 6.0 turns / $0.160 / 42 pass | 7.1 / $0.134 / 41 | **6.0 / $0.121 / 42** |
| wikipedia_infobox_hop | 4.0 | 8.0 | 8.3 |
| wikipedia_link_follow | 4.0 | 7.3 | 8.3 |
| wikipedia_search_click | 6.7 | 8.3 | 5.3 |
| tabular_data_analysis | 4.0 | 9.3 | 6.7 |
| wikipedia_deep_extraction | 7.0 | 10.7 | 10.0 |
| github_issue_investigation | 9.0 / 3 pass | 10.3 / 2 pass | 4.3 / 3 pass |

- **210's click-through target (≤ 5 on all three) is not met**: 8.3 / 8.3 / 5.3. Recorded, not
  re-run.
- **211's extraction target** (≤ 6 on `tabular`/`deep_extraction`, 3/3 on `issue_investigation`):
  one of three — 6.7, 10.0, and 3/3. Recorded, not re-run.
- **Adoption**: `--with-page` used in 13/42 runs, `--query` in 18/42. Runs that used `--with-page`
  averaged 5.9 turns vs 6.0 without, at +12% input tokens — no turn benefit where used.
  `link_follow` used it in all three runs and still took 8/7/10: the view lacks body text, so the
  agent re-fetched the page. `infobox_hop` never used it and re-navigated by URL after each click.
- **First tool call**: 42/42 runs start with `ff-rdp --help` (0% browser-command-first). Measured
  **without** any hook — the harness passes `--setting-sources ""` and never runs bare `ff-rdp`,
  so 212's home view was not exercised; Theme D's delivery decision is still open.
- **Theme C decision: do not default `--with-page` on.** Instead (a) add a text excerpt to the
  post-action page view so it answers "what does the page say now", and (b) surface `--query` and
  `--with-page` in top-level `--help`'s Quick start, sourced from the `IDIOMS` table. Both are
  follow-up behaviour changes per "Out of scope", not part of this plan.
- Remaining here: Theme A (land the harness under `tools/axi-bench/`), Theme D's delivery
  mechanism, and the ff-rdp condition's `ffrdp-bench.sh` plus the `conditions.yaml`/`types.ts`/
  `lifecycle.ts` diff, all still only in a session scratchpad.

## Outcome — final frozen-source measurement, 2026-09-13

Exactly one baseline matrix and one separately labeled **real private SessionStart**
matrix ran, each 14 task definitions × 3 repetitions =42 runs. All 84 result rows
and168 agent/judge invocations are retained and uniquely associated by actual
agent session/output and the judge's exact formatted trajectory. No missing,
duplicate, unmatched, timed-out, interrupted or replaced invocation was found.
No axi rerun, smoke, preparation-only call or new capability probe was added.
Earlier 255/smoke/probe records remain separate and are not pooled into these tables.

Both conditions used clean product/harness HEAD
`8c400376aa1d8ee85bc0bcee2a7bb2a898a9e277`, tree
`06b2ea52b06cee1ed28d172ac7be229ab11e5d7a`. The source stayed clean through both
matrices and their cleanup; KB changes began afterward. Each setup built its own
private debug binary: baseline SHA-256
`dd9d838db6bb32bdfb4452515a2da219cde9059db90faec67608564274135de9`, treatment
`6eada0b705cbaaee7a2069f281b1968b0eb17bf12f8612d87a2fcbc3a94bb778`.
These are distinct build artifacts of the same source, not a claim of identical
binary bytes. Both report ff-rdp 0.3.0 /8c400376aa1d /2026-09-13.

Task definitions, conditions, harness hashes, every per-task prompt hash and the
append paragraph are byte-identical across conditions. The append hash remains
`758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b`, including
its newline; it still explicitly says to run `ff-rdp --help`. Both request agent
and judge `claude-sonnet-4-6`, with actual Claude Code **2.1.270**, Firefox 155.0.1,
Node 24.19.0, pnpm launcher 11.24.0 / pinned effective 11.1.1. Historical CLI 2.1.241's
default prompt is **not proven identical**. Cached-account selection was retained;
the stale API key was omitted only for child processes. Costs below are reported
**list/API equivalents, not subscription billing**.

### Delivery and execution evidence

The treatment uses the exact binary's installed `ff-rdp home --hook` command,
wrapped for recording in a private per-agent `--settings` file while retaining
`--setting-sources ""`. All 42 treated agents have hook input, stdout, stderr,
exit 0 and exactly one matching successful SessionStart runtime response. This is
installed-hook execution, not hook text pasted into the append prompt. Baseline
agents and all 84 judges have no treatment settings or hook events. Every original
and delivered argv was checked; only treatment settings and the documented judge
JSON-output instrumentation differ.

All 168 invocations have one terminal success, `is_error:false`, child/recorder/
interruption exits 0; all 84 agent stream exits are 0. Both matrix/cleanup/driver
triples are 0/0/0. This does **not** mean every task or command passed.
Each lifecycle records 42 private Firefox starts and84 matching daemon/browser
terminations; no owned process identity survives, port 6000 is free and desktop
Firefox PID 37270 is preserved. Baseline driver ran20:25:13–20:48:09 UTC;
SessionStart ran20:48:45–21:12:25 UTC. These wall periods include setup and judges;
per-task seconds below are the unchanged upstream agent wall metric.

Evidence root: `.git/ralph-loop/20260912-validation-efficiency/iter256-measurement/`.
`baseline/` and `session-start/` retain 3587 raw files, hashes in
`raw-evidence-sha256.json`; `evidence-verification.json` records coverage,
provenance, argv/prompt equality, hook delivery and cleanup. The two
`*-audit-classified.json` files retain every actual top-level first ID/name/command,
automatic and adjudicated categories, its own result/errors, all raw tool results,
modelUsage and agent/judge associations. `per-run.csv` and `per-run-analysis.md`
cover every run's turns, Bash command count, grade, separate/combined costs,
seconds, errors, adoption and task-fidelity caveats. Upstream `turn_count` equals
the terminal `num_turns` in all 84 runs. Bash tools are counted by unique top-level
IDs; baseline's two additional Read tools explain why186 Bash tools are fewer
than230−42. No nested-agent tool was observed. Reasoning-token zero in upstream
usage is not proof of no thinking; raw modelUsage remains authoritative.

### Aggregate results (42 runs per condition)

| Condition | Passes | Turns / mean | Bash tools | Agent USD total | Judge USD total | Combined USD total | Agent seconds/run | P/Q runs |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Baseline (B) | 41/42 | 230 /5.476 | 186 | 4.0562285 | 2.7425790 | 6.7988075 | 22.636 | 40/38 |
| SessionStart (S) | 41/42 | 293 /6.976 | 251 | 4.3384146 | 2.3922554 | 6.7306700 | 25.585 | 5/38 |

Combined final-matrix list cost: **$13.5294775** (agents 8.3946431,
judges 5.1348344). Average agent costs are B $0.0965769 / S $0.1032956;
average combined costs B $0.1618764 / S $0.1602540. The small combined-cost decrease
comes from judges, while agent turns/cost increased. No statistical or billing
claim follows from this small sequential sample.

| Condition / role | Sonnet list USD | Auxiliary Haiku list USD |
|---|---:|---:|
| B agent | 4.0139295 | 0.0422990 |
| B judge | 2.4909000 | 0.2516790 |
| S agent | 4.2961356 | 0.0422790 |
| S judge | 2.1775854 | 0.2146700 |

Actual modelUsage keys are `claude-sonnet-4-6` and
`claude-haiku-4-5-20251001`; requested Sonnet does not imply exclusive Sonnet use.

### Per-task results

B is unchanged baseline; S is real SessionStart. P/Q counts runs that actually
used `--with-page` / `--query`, not strings printed by help or hooks. Each row has
three runs; no failed grade is removed from its means or denominator.

| Task | Condition | Turns per run | Mean turns | Agent USD/run | Judge USD/run | Combined USD/run | Seconds/run | Passes | P/Q runs |
|---|---|---|---:|---:|---:|---:|---:|---:|---|
| github_issue_investigation | B | 5/4/5 | 4.667 | 0.1238554 | 0.1057307 | 0.2295860 | 45.662 | 2/3 | 3/3 |
| github_issue_investigation | S | 9/6/4 | 6.333 | 0.1069458 | 0.0729174 | 0.1798632 | 25.646 | 3/3 | 0/3 |
| github_navigate_to_file | B | 5/5/5 | 5.000 | 0.0882014 | 0.0646423 | 0.1528437 | 18.578 | 3/3 | 3/3 |
| github_navigate_to_file | S | 4/4/4 | 4.000 | 0.0710173 | 0.0566461 | 0.1276634 | 13.557 | 3/3 | 0/3 |
| github_repo_stars | B | 5/5/5 | 5.000 | 0.0731537 | 0.0476857 | 0.1208394 | 17.097 | 3/3 | 3/3 |
| github_repo_stars | S | 4/7/4 | 5.000 | 0.0801519 | 0.0555887 | 0.1357406 | 15.556 | 3/3 | 0/3 |
| multi_page_comparison | B | 4/4/4 | 4.000 | 0.0848350 | 0.0694580 | 0.1542930 | 16.005 | 3/3 | 3/3 |
| multi_page_comparison | S | 6/6/6 | 6.000 | 0.0868371 | 0.0508027 | 0.1376398 | 20.884 | 3/3 | 0/3 |
| multi_site_research | B | 11/8/9 | 9.333 | 0.1186796 | 0.0702250 | 0.1889046 | 30.855 | 3/3 | 3/3 |
| multi_site_research | S | 10/12/8 | 10.000 | 0.1456267 | 0.0697021 | 0.2153288 | 34.032 | 3/3 | 1/3 |
| navigate_404 | B | 4/6/4 | 4.667 | 0.0904887 | 0.0698680 | 0.1603567 | 25.275 | 3/3 | 2/0 |
| navigate_404 | S | 5/5/5 | 5.000 | 0.0695257 | 0.0410037 | 0.1105294 | 20.781 | 3/3 | 0/0 |
| read_static_page | B | 4/5/5 | 4.667 | 0.0712805 | 0.0493270 | 0.1206075 | 13.406 | 3/3 | 2/2 |
| read_static_page | S | 4/4/4 | 4.000 | 0.0496229 | 0.0303967 | 0.0800196 | 12.071 | 3/3 | 0/3 |
| tabular_data_analysis | B | 7/7/6 | 6.667 | 0.1042654 | 0.0609807 | 0.1652461 | 23.819 | 3/3 | 3/3 |
| tabular_data_analysis | S | 7/4/5 | 5.333 | 0.0830323 | 0.0524929 | 0.1355253 | 21.101 | 3/3 | 0/3 |
| wikipedia_deep_extraction | B | 8/6/9 | 7.667 | 0.1493763 | 0.0968153 | 0.2461916 | 29.779 | 3/3 | 3/3 |
| wikipedia_deep_extraction | S | 11/6/6 | 7.667 | 0.1161492 | 0.0653791 | 0.1815283 | 25.664 | 3/3 | 0/3 |
| wikipedia_fact_lookup | B | 3/3/3 | 3.000 | 0.0492921 | 0.0418760 | 0.0911681 | 11.608 | 3/3 | 3/3 |
| wikipedia_fact_lookup | S | 3/3/3 | 3.000 | 0.0415150 | 0.0328664 | 0.0743814 | 10.131 | 3/3 | 0/3 |
| wikipedia_infobox_hop | B | 7/7/6 | 6.667 | 0.1497300 | 0.0718503 | 0.2215804 | 25.178 | 3/3 | 3/3 |
| wikipedia_infobox_hop | S | 11/9/7 | 9.000 | 0.1271686 | 0.0610274 | 0.1881960 | 26.112 | 3/3 | 1/3 |
| wikipedia_link_follow | B | 4/4/4 | 4.000 | 0.0910577 | 0.0748993 | 0.1659570 | 16.725 | 3/3 | 3/3 |
| wikipedia_link_follow | S | 15/9/7 | 10.333 | 0.1559121 | 0.0718567 | 0.2277689 | 53.474 | 2/3 | 1/3 |
| wikipedia_search_click | B | 7/6/6 | 6.333 | 0.0866817 | 0.0465080 | 0.1331897 | 25.434 | 3/3 | 3/3 |
| wikipedia_search_click | S | 10/12/16 | 12.667 | 0.1808988 | 0.0788957 | 0.2597945 | 45.543 | 3/3 | 2/2 |
| wikipedia_table_read | B | 5/5/5 | 5.000 | 0.0711787 | 0.0443267 | 0.1155054 | 17.478 | 3/3 | 3/3 |
| wikipedia_table_read | S | 9/8/11 | 9.333 | 0.1317348 | 0.0578427 | 0.1895775 | 33.636 | 3/3 | 0/3 |

### First actual tool, adoption and targets

All 84 first **top-level assistant tool_use** blocks were inspected, excluding
hook/system/stream events, judge/lifecycle calls and nested tools. The conservative
recorder automatically returned B:6help/36other; S:2browser/40other. Manual review
resolves quoted URLs, redirection, help pipelines and the exact private binary path
from each original command and its own result; no later success replaces that result.

| First-call classification (full42 denominator) | Baseline | SessionStart |
|---|---:|---:|
| Help | 40 (95.238%) | 0 |
| Recognized browser-command attempts | 2 (4.762%) | 40 (95.238%) |
| Other (nonexistent `nav`) | 0 | 2 (4.762%) |
| Bare ff-rdp / missing / missing own result | 0 /0 /0 | 0 /0 /0 |
| First calls with actual usage errors | 1 | 5 |
| **Successful browser-command-first** | **1 (2.381%)** | **37 (88.095%)** |

B static-page1 begins `navigate … && ff-rdp content`; navigate succeeds, but the
nonexistent second command makes the first tool fail with exit 2. S multi-site1
and search 1/2 attempt `navigate --url` and fail with exit 2; S search 3 and infobox 2
attempt nonexistent `nav` and remain other. All five S errors are visible even
though final invocations succeeded. Thus212's ≥80% target is met by **37/42
successful browser-first calls with an installed hook**, not by counting errors
or merely the40 recognized attempts. It is an adoption result, not evidence of
better task efficiency.

- **210 click-through ≤ 5 on all three is still not met.** Baseline infobox/link/
  search means6.667/4.000/6.333; treatment 9.000/10.333/12.667. Only B link meets it.
- **211 extraction targets remain partly unmet.** B tabular 6.667 and deep 7.667
  miss≤ 6, issue investigation 2/3 misses 3/3. S tabular 5.333 meets≤ 6, deep 7.667
  misses it, issue investigation 3/3 meets its pass target. All 18 extraction
  trajectories across those three tasks and both conditions used --query.
- B uses --with-page in 40/42 runs and S in 5/42; --query is38/42 in each.
  Every run's binary adoption is recorded in the per-run artifacts. Initial
  navigation uses --with-page in 40/42 B runs and **0/42 S runs**. The five S
  adopters discover it later; skipping help also skips its working navigation idiom.
- B infobox 7/7/6 still hunts PSF handles3/3/2 commands and consumes no Developer
  fact ref, although all three eventual clicks and Formation views succeed.
  S infobox 11/9/7 consumes none either; only run 1 clicks successfully, while2/3
  navigate directly and recover page-text's "founded formed" miss via "2001".
  These are additional observations under269, not replacements for255's6 runs.

### Failures, action fidelity and decision

B issue-investigation 2 fails for omitting issue37617's full title; no recovery
was attempted. S link-follow2 fails for substituting direct URL navigation for
the requested click. Both failures stay in the42-run denominators. Raw grade
PASS does not certify all actions: all six Makefile runs skip the repository-root
step; all three S link-follow runs replace unsuccessful clicks with URLs (grades
PASS/FAIL/PASS); S infobox 2/3 do likewise (PASS/PASS); B search 1 types into the
box but uses a Special:Search URL after a hidden-button timeout (PASS).

Observed CLI/tool errors total **B6 in 5 runs / S19 in 14 runs**, versus upstream
error_count totals5/18. Every error has an exact tool ID and raw result in the
audit. B search 1's hidden-button timeout is masked by a pipe; S infobox 3's empty
ref is masked by `||` recovery. Other errors include invented commands, unsupported
options and invalid ref/selector/type syntax. Help text mentioning error fields
is excluded from the observed-error count. German console messages from this
raw unmanaged Firefox do not reproduce147's separate managed-launch locale AC.

**Keep --with-page opt-in; do not enable the current hook by default.** The hook
moves the first decision to browsing, but this sample adds 1.500 mean turns and
loses navigation-with-page adoption. Its actual533-byte welcome-page payload
suggests a11y/page-text/console, with no navigate-with-page or click/type syntax.
The next surface to improve is the compact **home/SessionStart next-step guidance**,
with correct act-and-see and ref/type examples, then a separately authorized
controlled validation. This is a measured design recommendation, not a default-on
experiment: neither matrix tests default-on --with-page, n=3 per task is small,
conditions ran sequentially in different upstream-randomized task orders, and
historical CLI prompt parity is unproved. No product/help/default behavior changed.

## Carry-over — final measurement and closing state

| Observation or unmet requirement | Disposition |
|---|---|
| Infobox≤ 5 miss, unconsumed fact refs, query/handle recovery and URL substitution | Folded into [[iteration-269-infobox-discovery-after-fact-refs]] with both256 conditions; original255 evidence and ACs unchanged |
| Remaining click/extraction targets, incomplete issue title, ambient action fluency, invalid commands/options and strict task-action/error audit | Filed as [[iteration-270-benchmark-ambient-action-and-extraction-gaps]]; no broader backlog execution |
| Default-on efficacy unmeasured; historical default-prompt equivalence unproved; sequential/n=3 limits | No claim or product change;270 requires a controlled design and separate authorization before any new paid comparison |
| Prior harness smokes/probe and255 measurement | Preserved separately; not pooled or erased by the84-run result |
| Prior255 sweep334passed/9failed =343 across all tiers, zero profiles | Separate255 evidence only: seven screenshot failures remain 257, promotion137 remains262, pre-auth240 remains268; this harness/KB-only 256 delta requires no product sweep |
| Independent final documentation review, exact-head CI and GitHub merge | Review `01a09cab-c897-7211-a705-70c8db47c143` completed with zero findings after 15 successful inspection calls; all iteration ACs fulfilled. Supervisor verifies final-head CI and the GitHub merge separately before queue advancement |

### Per-occurrence dispositions

Every row below remains in its original run; line numbers refer to that run's `agent_output.txt`. Full commands/results remain in the classified audit.

| Condition/task/run | Non-green observation | Evidence | Disposition |
|---|---|---|---|
| B/github_issue_investigation/2 | Upstream FAIL: incomplete issue title | grade.json and terminal answer | Filed 270; no replacement |
| B/github_navigate_to_file/1 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/github_navigate_to_file/2 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/github_navigate_to_file/3 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/navigate_404/2 | unrecognized subcommand 'content' | line 10, toolu_011KoPZZdkE6bPxc3chyYGGx | Filed/folded 270; preserve actual error |
| B/read_static_page/1 | unrecognized subcommand 'content' | line 6, toolu_01SyjZRdTdfbL9vkGVwc53U6 | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 10, toolu_01LxuiLD8ZM1iFTpKF8GcD9r | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | selector '#vector-sticky-header > div:nth-child(1) > div:nth-child(1) > button:nth-child(1)' not ready — the 1 matching element is hidden after 10000ms; masked by shell | line 16, toolu_017rLwxYYs6sfCbnCRALjevc | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/1 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| B/wikipedia_search_click/2 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 12, toolu_01UUb2V4WRvAu4x32KSrPhrb | Filed/folded 270; preserve actual error |
| B/wikipedia_search_click/3 | the argument '--ref <REF_ID>' cannot be used with '[SELECTOR_POS]' | line 12, toolu_01HkDRcRpr9Tti6krv8LqWP8 | Filed/folded 270; preserve actual error |
| S/github_issue_investigation/1 | unrecognized subcommand 'js' | line 20, toolu_01QBNpD3bpWWNFBQZrkek1Cq | Filed/folded 270; preserve actual error |
| S/github_navigate_to_file/1 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/github_navigate_to_file/2 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/github_navigate_to_file/3 | Skipped initial repository-root visit; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/multi_site_research/1 | unexpected argument '--url' found | line 10, toolu_01Wsg1yWobgWFb1KL7mRooCj | Filed/folded 270; preserve actual error |
| S/multi_site_research/2 | unrecognized subcommand 'tab-new' | line 19, toolu_01R2m8B2ioyv7KzWwjVAhQDJ | Filed/folded 270; preserve actual error |
| S/multi_site_research/2 | unrecognized subcommand 'tab-new' | line 21, toolu_01AVJBzxZe3h9msctTLViFCu | Filed/folded 270; preserve actual error |
| S/wikipedia_infobox_hop/1 | unrecognized subcommand 'find-refs' | line 21, toolu_01DHsyKDZkKXoGrP3kssHvRM | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/2 | unrecognized subcommand 'nav' | line 8, toolu_01JoZQ9sDCZxiGxr6NT3waQA | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/2 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 269; strict action caveat retained |
| S/wikipedia_infobox_hop/3 | resolve-ref requires a non-empty id field; masked by shell | line 15, toolu_011iTA4jH2nuZfPPxSwdigB8 | Filed/folded 269; preserve actual error |
| S/wikipedia_infobox_hop/3 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 269; strict action caveat retained |
| S/wikipedia_link_follow/1 | unrecognized subcommand 'find-ref' | line 13, toolu_01Vjpti2WS8hhJDkk7927Kqy | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | selector 'Charles Babbage' not ready — 0 elements matched (not found) after 2009ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could not have changed  | line 15, toolu_014H5qPnaCDixZQZDVNUZyGJ | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | selector 'a[href='/wiki/Charles_Babbage']' not ready — 0 elements matched (not found) after 2098ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could n | line 32, toolu_01L7GSPCS9VUdwPBf3fMoXQ6 | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/1 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_link_follow/2 | ref Charles Babbage not found (not registered in this daemon session) | line 18, toolu_01McbxG7ifuZJbAFNBSqeyHx | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/2 | Upstream FAIL: URL substituted for click | grade.json and terminal answer | Filed 270; no replacement |
| S/wikipedia_link_follow/2 | No successful required click/submission; URL recovery substituted; raw grade FAIL | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_link_follow/3 | selector 'e209' not ready — 0 elements matched (not found) after 2009ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could not have changed the answer | line 14, toolu_013KK1KkRL4NYUZ6JLvJyhgm | Filed/folded 270; preserve actual error |
| S/wikipedia_link_follow/3 | No successful required click/submission; URL recovery substituted; raw grade PASS | Full command/result trace | Filed/folded 270; strict action caveat retained |
| S/wikipedia_search_click/1 | unexpected argument '--url' found | line 8, toolu_01W2548LvTNGtygtjZUEyWfx | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/2 | unexpected argument '--url' found | line 8, toolu_01RSCfFj6gPGy4RYDHUucnNK | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | unrecognized subcommand 'nav' | line 8, toolu_01Bxe6meQkjXpvBmcpxX6wkJ | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | unexpected argument '--value' found | line 24, toolu_01JmJnsLi5vLLN2baNxfooqU | Filed/folded 270; preserve actual error |
| S/wikipedia_search_click/3 | selector '#searchform button[type=submit]' not ready — 0 elements matched (not found) after 2132ms — the page is idle (document complete, no network in flight, no DOM mutations), so the rest of the 10000ms auto-wait budget could n | line 30, toolu_01LFgqS3VbBXrUoTd2oDvWyb | Filed/folded 270; preserve actual error |
| S/wikipedia_table_read/1 | unexpected argument '--expression' found | line 20, toolu_01Vspx3rqH7eNk9q2rjdBZus | Filed/folded 270; preserve actual error |
| S/wikipedia_table_read/3 | unexpected argument '--expression' found | line 20, toolu_01Xkg1CXi2YM9Z6GgWN1hQ7S | Filed/folded 270; preserve actual error |

Original acceptance wording and historical tables are unchanged. The record-only
measurement ACs are fulfilled despite numeric target misses; no predecessor's
unticked numeric criterion is silently ticked. Fresh ordered stable update,
fmt, strict workspace clippy and workspace tests passed on 2026-09-13 for the
final measurement documentation: 2466 passed, 0 failed, 415 ignored over 36
summaries; clippy 0.1.98 (48a229ceae 2026-09-01). Evidence is retained in
`iter256-measurement-review/ordered-gates.json` beneath the same run root.
All nine xtask gates and actual dogfood remain applicable to unchanged harness
inputs, with prior reuse proofs retained. Affected documentation/plan checks
are rerun after completion-only status and evidence updates. All upcoming
planned iterations, including outside 252–257, have property/body snapshots,
hashes and dispositions in `upcoming-reconciliation.json`; inspecting them does
not execute them. Earlier preparation records describe their then-pending
state and are superseded by this final measurement record.

## Design notes

### Final measurement preparation, 2026-09-13

255 merged at `01f8c28244cc90dece74a8cfcbf7053662ea60e7`; its six measurements
used the immutable 256 Theme A export at `5786329668c95479a4b91710764c7cee0e782f4a`.
The published 256 branch integrates that new main without rewriting the export or
published history. Its baseline runner, task prompts, append paragraph (including
newline) and requested agent/judge `claude-sonnet-4-6` remain unchanged.

Before final measurements, the supervisor selected **14 tasks × 3 repetitions =
42 baseline runs and the same 42 runs with a separately labeled ambient
treatment**, totaling **84 final 256 runs**, not 126. Neither condition has run in
this preparation phase. Prior 255 runs and all diagnostic smokes/probes remain
separate; the original historical outcomes and acceptance wording above stand.

Theme D's selected mechanism is a **real private SessionStart hook**, not a
payload appended to the prompt. The harness obtains the settings shape and command
from the exact binary's `install-hook --claude --project` in a new private project,
then adds an instrumented per-agent settings file with explicit `--settings` while
keeping default `--setting-sources ""`. The judge remains untreated and the
append paragraph is identical in both conditions. Nothing is installed in the
user's configuration or this checkout. See `tools/axi-bench/README.md` for recorded
identities, the conservative first-actual-tool classifier and failure semantics.

The 2026-09-13 independent review found that subcommand help and invented verbs
were incorrectly counted as browser-first. Repair batch 2 recognizes actual
command-path help before browser matching, restricts positives to CLI names,
and retains ambiguous shell/argument syntax as `other`. Raw-trace adjudication
must resolve those rows and the first command's own errors before final shares;
a later successful terminal result cannot repair an invalid first decision.
Regression/mutation evidence is retained in
`.git/ralph-loop/20260912-validation-efficiency/iter256-repair2/`.
No final measurements or historical outcomes changed in this repair.

One separately labeled, bounded no-tools delivery probe on actual Claude CLI
**2.1.270** succeeded: SessionStart runtime events confirmed the captured hook
stdout, and the requested Sonnet model quoted its first line and recognized the
owned reachable Firefox. Probe/cleanup exits were 0/0; list cost **$0.0397914**
includes Sonnet $0.0388014 and auxiliary Haiku $0.00099. This proves hook delivery
under disabled default sources, not any adoption or task outcome. CLI 2.1.270
differs from historical 2.1.241, so default-system-prompt identity is not claimed.
Raw evidence is under
`.git/ralph-loop/20260912-validation-efficiency/iter256-ambient-implementation/runtime-probe/`.

The final baseline/treatment tables, per-run first-call shares, extraction/flag
adoption and default recommendation are pending the reviewed clean checkpoint
and actual 84-run execution. The original ambient AC and harness landing/clean
comparison boxes stay unticked until their full requirements are satisfied. The
new checked-in dogfood script exercises this checkout's live Wikipedia
Ada → Babbage path and trimmed hook view; it does not claim matrix coverage.

### Theme A preparation, 2026-09-13

The dedicated 256 branch prepares the pinned `tools/axi-bench/` harness for a
supervisor-owned checkpoint/export before 255's six measurements. The exported
harness accepts an explicit clean product source root and full revision; its
private build, binary hash, harness revision, prompts, CLI/runtime versions and
actual agent/judge usage are recorded. This is the equivalent sequencing already
recorded in frontmatter, not a second 256 PR or a transfer of harness ownership.

The original "42 tasks" wording above remains historical: the pinned inventory
has 14 task definitions, and three repeats mean 42 runs. No final matrix or
ambient treatment ran during preparation. Two bounded paid `read_static_page`
smokes exercised the historical prompt/model and both passed their task but
failed harness cleanup identity checks; their total list cost was $0.4337284,
including auxiliary Haiku usage. They are harness diagnostics, not benchmark
acceptance or replacement data for any historical table. Subsequent ownership,
process-status, provenance and zero-cost fixture checks diagnose the harness
repairs separately. Full raw phase evidence is retained under
`.git/ralph-loop/20260912-validation-efficiency/iter256-theme-a/` by the supervisor.

Theme A's landing box, clean-checkout comparison AC, final 42-run coverage and
ambient delivery AC remain unticked/pending until their actual later completion.

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
- [[iteration-219-reader-view-page]] — shipped the reader-view fix Theme C recommended (excerpt +
  `--help` idioms); Task E's re-measurement is carried over from its own unticked AC 6
- [[iteration-220-with-page-after-navigating-click]] — the `click --ref … --with-page` timeout on
  Wikipedia that blocks Task E
