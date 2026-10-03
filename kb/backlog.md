---
title: Backlog
type: backlog
status: in-progress
date: 2026-10-03
tags: [backlog]
---

# Backlog

One line per item, with where it came from. Tick it in the same commit as the fix. This file
replaces GitHub issues and per-item iteration plans for follow-ups; a plan file is only for real
product work that needs design. Keep items to one line — if an item needs more, it is a plan.

## Product

- [x] `navigate --with-network --headers` (and `click --wait-for-network`): fetch response headers before the connection closes; `network --headers` after a finished navigate returns nothing (#287)
- [ ] Skill playbooks A2, D2, E1, E3 (and their `evals/fixtures/*/bug.json`) still read headers with an after-the-fact `network --detail --headers`, which returns nothing on a new connection; rewrite them to `navigate <page> --with-network --headers --all` filtered by `--jq` (A1 done) (#287 follow-up)
- [x] Per-command emulation flags replacing the removed `emulate`: `screenshot --color-scheme dark|light`, `screenshot --media print`, `navigate --user-agent <ua>` — settings reset with the connection (#287)
- [ ] `navigate --user-agent` is refused with `--with-network`: the capture path (`navigate/network_capture.rs`) opens its own watcher and does not apply the override yet (per-command emulation PR)
- [ ] `snapshot` / `a11y` stamp every interactive element with `data-ffrdp-ref` before the output cap applies; stamp only what is emitted (#287)
- [x] Auto-wait's rect-stability probe has a fixed sub-budget that fails under machine load ("auto-wait stopped during rect stability probe did not answer"); first nightly 2026-10-03 hit it in live_160/210/224/237 at `--jobs 4` on a 4-core runner. Make the probe share the command's `--timeout-ms` instead of a separate small budget (nightly run 37071459324)
- [ ] `consent accept` on theguardian.com (Sourcepoint frame detected, not accepted) and bbc.com (no CMP reported) failed on the ubuntu CI runner in the sites job on 2026-10-03 while passing locally the same day — geo/CMP variant difference; check from a non-CH IP before touching the selectors (run 37071459324)
- [ ] `navigate/readiness.rs` still releases a LongString header grip (`GripHandle::without_queue(..).release`); likely removable like eval's (#291)

## Tests and tooling

- [ ] Remaining wall-clock bounds in live tests measure the laptop, not the product: live_113, 129, 145, 158, 174, 237, 272, `live_bulk_cap`, `live_navigate_default_fast` — delete the bounds, keep the functional assertions (#290)
- [ ] Live-sweep post-phase leak check was deleted with the real-root scan; leaked test Firefoxes are now only caught on the watchdog path — run `managed_firefox_pids()` after every phase (#291)
- [ ] `clippy::assert_is_empty` is allowed in the lint tables rather than fixed at ~30 test sites (#286)
- [ ] `kb/iterations/dogfood-lib.sh` and the historical `*.dogfood.sh` scripts remain only because `tools/axi-bench` sources the lib; move the lib under `tools/axi-bench/` and drop the scripts (#286)
- [ ] `count_tabs` in `launch` does not reuse `connect_tab`'s list helper because that helper takes `&Cli` (#291)
- [ ] `kb/dogfooding/*` session logs still mention `--wait-timeout`; historical, leave or annotate (#291)

## Global skills (edit by hand in `~/.claude/skills/`, shared with other repos)

- [x] `create-pr`: drop `check-dogfood-script`; run the xtask `check-*` enumeration and the fmt/clippy/test gates only when `git rev-parse HEAD^{tree}` differs from the last green run (see [[reset-2026-10-execution]] "Global skill follow-ups")
- [x] `review-pr`: one local review pass per PR; no re-review for style-only fixes
- [x] `merge-pr`: no separate `status: done` commit — the flip goes in the same commit as the code

## Done

- [x] Global skills `create-pr`/`review-pr`/`merge-pr` edited 2026-10-03: gates run once per tree hash, one review pass per PR, no status-flip commit (`ralph-loop` skills left untouched — retired)
- [x] CI skips docs-only PR pushes (#292, 2026-10-03)
- [x] #288 / #289 moved here from GitHub issues (2026-10-03)
