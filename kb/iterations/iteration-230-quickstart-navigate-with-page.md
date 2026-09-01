---
title: "Iteration 230: put `navigate --with-page --query` in the Quick start — 5 of 6 benchmark runs never see it"
type: iteration
date: 2026-09-01
status: in-progress
branch: iter-230/quickstart-navigate-with-page
depends_on:
  - 228
dogfood_path: |
  # The whole finding is about which lines an agent reads, so read them the way an agent does.
  ff-rdp --help | head -50 | grep -n -- '--with-page'
  # TODAY: one hit, line 16, `ff-rdp click --ref e3 --with-page` — actionable only once you
  # already hold a ref, which an agent that has not navigated yet does not.
  ff-rdp --help | grep -n 'navigate <URL> --with-page'
  # TODAY: line ~333 of 441 — the idiom that produces the 4-turn trajectory, below the fold.
  # AFTER: a `ff-rdp navigate <URL> --with-page --query "<text>"` line inside the first 16,
  # and `xtask check-help-idioms` failing if it leaves.
  ff-rdp launch --headless
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Ada_Lovelace' --with-page --query 'Charles Babbage' \
    --jq '.results.page | {matches, query_source, first_ref: .interactive[0].ref}'
  ff-rdp daemon stop
tags:
  - iteration
  - agent-ergonomics
  - help
  - discoverability
  - carry-over
---

# Iteration 230: the Quick start's `navigate` line is the one that decides the trajectory

## Why

[[iteration-228-two-task-benchmark-after-facts]] measured `wikipedia_link_follow` and
`wikipedia_infobox_hop` at `--repeat 3` on ff-rdp `cf99c84` and got 9.0 and 11.3 turns against
axi's 4.0 / 4.0. Reading the six trajectories gives one cause, not several:

- **`navigate --with-page` was used in 1 run of 6.** That run took **4 turns and 3 commands**:
  `--help`, `navigate … --with-page --query "Charles Babbage"`, `click --ref e1 --with-page
  --query "born"`. Target met, axi matched, zero `page-text` calls.
- The other five ran bare `navigate` → `page-text --query` (answers, returns no ref) → a ref hunt
  over `a11y summary | grep`, `dom <guessed selector>` and 1–3 `eval` scripts → `click --ref` →
  `page-text --query` again. 4–9 extra turns, every time.
- **Five of six runs read `ff-rdp --help | head -50` and nothing else.** In that window the
  `navigate` line is bare and `--with-page` appears only on `click --ref e3` — which an agent
  holding no ref cannot use yet. `ff-rdp navigate <URL> --with-page --query "…"` is at line 333
  of a 441-line `--help`. **The one run that read the whole file is the one run that used it.**

So [[iteration-225-reader-excerpt-infobox]]'s `results.page.facts` is not the bottleneck — it is
correct and it produces the four-turn trajectory when reached. The bottleneck is that the act-and-see
entry point is below the fold of the only text these agents read.

## Themes

- **A — Add the navigate idiom to the Quick start.** One line inside the first ~16, sourced from
  the same `IDIOMS` table `SKILL.md` and the home view render from, so `check-skill-drift` and
  `check-help-idioms` both keep it there. The Quick start is 7 lines today; this is an 8th, or a
  rewrite of line 12 — decide from what reads best at `| head -50`, not from line count.
- **B — Make `check-help-idioms` assert the new line.** The gate exists precisely because `--help`
  is the surface benchmark agents read; a Quick-start line nothing checks is one refactor from
  gone.
- **C — Re-measure the same two tasks, same shape.** `--repeat 3`, exclusive browser, and count
  `navigate --with-page` adoption per run as well as turns. The number to beat is 9.0 / 11.3 and
  the claim to test is that adoption, not payload, was the gap.

## Tasks

### A. The help line [3/3]
- [x] Add the `navigate <URL> --with-page --query "<text>"` idiom to the `IDIOMS` table
      [landed as `skill_doc::NAVIGATE_IDIOM`, a named `(command, why)` pair inside
      `QUICK_START_LINES` rather than a new row: the Quick start replaced its bare
      `navigate <URL>` line instead of growing an 8th, which reads better at `| head -50`]
- [x] Confirm it renders inside `ff-rdp --help | head -50`, in `SKILL.md`, and in the home view
      [2026-09-01: `--help` line 12 of 441; `SKILL.md` line 38; `home.rs` prints the same pair as
      its "no page loaded" hint, and `hints.rs` as the `launch` follow-up]
- [x] `cargo run -p xtask -- gen-skill` and commit the regenerated block
      [`check-skill-drift` reports SKILL.md current]

### B. The gate [1/1]
- [x] `check-help-idioms` fails when the navigate idiom is missing from the first 50 lines
      (add the assertion and prove it fails by deleting the line locally)
      [`check_help_idioms::navigate_idiom_failures`; the deletion is proven by four unit tests
      covering five shapes, rather than by editing the tree — bare `navigate <URL>`, `--with-page` without `--query`,
      the row deleted outright, the two flags split across neighbouring lines, and the right line
      pushed past line 50. Gate output now ends "and the act-and-see navigate line within 50 lines".
      Also asserted end-to-end against the real binary in `tests/cli_help_idioms.rs`]

### C. Re-measure [2/2]
- [x] `matrix --condition ff-rdp --task wikipedia_link_follow,wikipedia_infobox_hop --repeat 3`
      on a browser this run owns
      [2026-09-01, release build of `30f7e7a`, port 6000 verified free, browser started and torn
      down by `ffrdp-bench.sh` alone. 6/6 pass. `link_follow` 9.0 → **4.7** (5, 5, 4);
      `infobox_hop` 11.3 → **8.0** (6, 9, 9); both tasks 10.2 → 6.3 turns, $0.196 → $0.154]
- [x] Record per-run turns **and** per-run `navigate --with-page` adoption in
      [[axi-benchmark-comparison]]
      [second 2026-09-01 section. Adoption **1 of 6 → 6 of 6**; `page-text` calls 2.2 → 1.5 per run]

## Acceptance Criteria [3/4]

- [x] `ff-rdp --help | head -50` contains a `navigate` line carrying `--with-page` and `--query`
      [2026-09-01: line 12 — `ff-rdp navigate <URL> --with-page --query "<text>"`]
- [x] `xtask check-help-idioms` fails if that line is removed
      [`navigate_idiom_failures`; five deletion and near-miss shapes covered by unit tests, plus
      an e2e assertion over the real `--help`]
- [x] Both tasks re-measured at `--repeat 3`, per-run adoption recorded whatever it is
      [6 of 6 runs used `navigate --with-page`, against 1 of 6 on `cf99c84`]
- [ ] Target ≤ 5 turns average on both tasks — or the measured number recorded and this criterion
      left unticked, never reworded

      **Not met, and left unticked.** `wikipedia_link_follow` reached **4.7** turns (5, 5, 4) —
      at target and level with axi's 4.0. `wikipedia_infobox_hop` reached **8.0** (6, 9, 9),
      against a target of 5. The criterion says *both* tasks, so it does not tick.

      The premise it rested on — that adoption, not payload, was the whole gap — held for
      `link_follow` and only partly for `infobox_hop`. Adoption is now 6/6 on both tasks, so the
      residual 3 turns are not a discoverability cost: `infobox_hop` runs 2 and 3 spend three
      turns recovering a ref for an infobox link whose text the `facts` payload already returned,
      and two more because `--query "founded formed"` does not match the infobox key `Formation`.
      Neither is reachable from a `--help` line. Filed as
      [[iteration-231-infobox-facts-refs-and-query-matching]].

## Out of scope

- Changing `--with-page`'s default. That decision ([[iteration-213-act-and-see-benchmark-rerun]]
  Theme C) stays gated on a measurement where the flag is actually adopted; this iteration is what
  makes such a measurement possible.
- Any further change to the page-view payload. 228 showed the payload is not the gap.
- `click`'s not-found poll: a missed selector waits the full `--timeout` (10 s) before reporting
  `0 elements matched`. Real wall-clock cost, no turn cost, and it is an agent-side guess rather
  than an ff-rdp defect — noted in 228's section, deliberately not fixed here.

## References

- [[iteration-228-two-task-benchmark-after-facts]] — the measurement this comes out of
- [[axi-benchmark-comparison]] — the 2026-09-01 section with the six trajectories
- [[iteration-225-reader-excerpt-infobox]] — the payload that works when it is reached
- [[iteration-219-reader-view-page]] — Theme E, which first put the idioms in `--help`
