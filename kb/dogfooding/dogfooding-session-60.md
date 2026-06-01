---
title: Dogfooding Session 60 — verify iter-92/93/94 against MDN
type: dogfooding
date: 2026-06-01
status: completed
site: developer.mozilla.org
commands_tested: [doctor, launch, navigate, eval, screenshot, daemon stop, dom stats, perf audit, cascade, computed, network]
tags: [dogfooding, iter-92, iter-93, iter-94, regression-verification]
---

# Dogfooding Session 60

Focused verification of iter-92/93/94 (full-page screenshot, navigate parity, eval CSP bypass, daemon-stop race, render_blocking divergence, cascade ambiguity, network text null rows) against MDN — the same target where session 59 surfaced these issues. **Major meta-finding: the cargo-installed `ff-rdp` binary lagged main by half a day. A first pass tested the stale binary and reported 5 of 7 fixes as "still broken" — all five were already-merged fixes that the user's `~/.cargo/bin/ff-rdp` hadn't been rebuilt for.** After `cargo install --path crates/ff-rdp-cli`, 6 of 7 land green; 1 partial.

## What's New Since Last Session (session 59)
- iter-92 — screenshot `--full-page` (forward `fullPage:true` through `WindowGlobalTarget`) + navigate freshness gate
- iter-93 — eval routes through console sandbox (CSP bypass)
- iter-94 — daemon stop 8s bounded wait + SIGTERM/SIGKILL escalation; shared `render_blocking` classifier; cascade `inherited_or_default` note; network text null-key suppression

## Regression Checks (fresh binary `9ecf105b8050`)

| # | Theme (origin) | Previous Status (session 59) | Current Status | Notes |
|---|---|---|---|---|
| 1 | screenshot `--full-page` on FF 151 (iter-92 A) | ❌ silently no-op | ✅ **FIXED** | `screenshot --full-page` on MDN/JavaScript: PNG 1366×**4187** vs viewport 1366×683; md5 differs (`9660e27c…` vs `5a59634b…`) |
| 2 | `navigate` 2nd-nav `elapsed_ms` (iter-92 B) | ❌ `elapsed_ms: 0` | ✅ **FIXED** | `navigate JavaScript` → `elapsed_ms: 4`; `navigate CSS` (2nd) → `elapsed_ms: 1` (no longer 0) |
| 3 | `eval` on strict-CSP site (iter-93) | ❌ `EvalError: blocked by CSP` | ✅ **FIXED** | `eval 'document.title'` on MDN returns `"JavaScript \| MDN"`; meta reports `eval_path: "page-await"` |
| 4 | `daemon stop` race window (iter-94 A) | ⚠️ "still listening after 3s" then port frees | ⚠️ **PARTIAL** | Bound bumped to 8s ✅, SIGTERM+SIGKILL escalation visible ✅, but on real Firefox on MDN tab, port still held after escalation (different failure mode — see Issues §1) |
| 5 | `dom stats` ↔ `perf audit` render_blocking (iter-94 B) | ❌ 22 vs 17 | ✅ **FIXED** | Both report **17** on MDN/JavaScript — shared classifier active |
| 6 | `cascade` `inherited_or_default` note (iter-94 C) | n/a (new) | ✅ **FIXED** | `cascade body --prop background-color` emits `inherited_or_default: true` + `note: "no author rule declares this property; computed value is inherited or default"` |
| 7 | `network --format text` null-key rows (iter-94 D) | ❌ bare-number `82` row in Cause Type section | ✅ **FIXED** | "Requests by Cause Type" section is suppressed entirely when all keys are null |

## Findings

### What Works Well

- **iter-93 eval-via-console-sandbox is a quiet win.** Bypassing CSP on a site as locked-down as MDN unlocks a *huge* class of agent workflows. The `eval_path: "page-await"` meta field is a nice debugging affordance — surfaces which scope handled the eval without `--verbose`.
- **iter-94 cascade note** turns a previously-cryptic `rules: []` into self-documenting output. Tried `body --prop background-color` and the note immediately explained why no author rule matched.
- **iter-94 network text suppression** — the entire problematic section just disappears when grouping keys are null. No `(unknown)` clutter when there's nothing to say.

### Issues Found

#### 1. **`daemon stop` SIGTERM+SIGKILL escalation visible but port still held** ⚠️ (new — iter-95 candidate)
- Cmd: `ff-rdp daemon stop` against a fresh headless Firefox connected to MDN
- Result (post iter-94): `stopped Firefox (pid 65762) but port 6000 is still listening after 8 s (after SIGTERM+SIGKILL escalation, port still listening)` — total ~12s wall
- Expected: SIGKILL should reliably free the port within 8s
- Hypothesis: Firefox child processes (content/GPU/RDD) on macOS may keep the listening socket via FD inheritance; `kill -9` on the parent pid isn't enough. Likely need `pkill -KILL -P <pid>` or recursive process-group kill
- Workaround today: `kill -9 <pid>` manually, then `lsof -i :6000` to confirm

#### 2. **`cascade --prop` `computed` is null for properties that have computed values** (separate pre-existing bug)
- Cmd: `cascade h1 --prop color` on MDN
- Result: `computed: null, rules: []` — but `computed h1 --prop color` returns `rgb(0, 0, 0)` correctly
- Expected: cascade should fill in `computed` from the same source `computed` uses
- Impact: blocks iter-94 Theme C's `inherited_or_default` note on common cases (the note has `computed != null` as a precondition; if cascade never populates computed for some properties, the note never fires for those properties)
- Worked with: `cascade body --prop background-color` does populate computed (`rgba(0,0,0,0)`) → note fires. So this bug is property/element-specific, not a blanket failure
- iter-95 candidate: align cascade's `computed` population with the standalone `computed` command's logic

#### 3. **Stale-binary trap — `cargo install` ≠ `cargo build`** (process/UX, not a code bug)
- Symptom: After three iterations merged to main, the first dogfooding pass against `ff-rdp` (which resolves to `~/.cargo/bin/ff-rdp`) showed every recent fix as "still broken"
- Root cause: the user's `cargo install --path crates/ff-rdp-cli` was last run ~8h before iter-92/93/94 merged
- The `dogfood` skill / agent docs don't currently include "rebuild + reinstall before dogfooding"
- Mitigations to consider:
  - `ff-rdp doctor` could emit a warning when the installed binary's commit SHA differs from `git rev-parse HEAD`
  - The `dogfood` skill could include a pre-flight `cargo install --path … --quiet` step
  - The dogfood script convention iter-87 introduced (`dogfood_path:` in iteration plans) could be paired with an explicit `dev_binary:` flag that uses `cargo run -p ff-rdp-cli --` instead of the installed binary

### Feature Gaps

- **`ff-rdp doctor --staleness-check`** or similar — compare installed binary's embedded commit SHA against `HEAD`. The version string already reports a SHA (`0.2.0 (9ecf105b8050 2026-06-01)`), so this is a one-liner.
- **`daemon stop --force`** — explicit `pkill -KILL -P <pid>` for the case where Firefox child processes hold the port. Iter-94 added escalation; this would add a "go nuclear" knob for the user when escalation isn't enough.

## Ground covered

- ✅ MDN /Web/JavaScript (primary): all 7 focus checks
- ⏭ `../docs` (Next.js secondary): **skipped** after the stale-binary discovery — the MDN results were strong enough that reproducing on a second site wasn't needed for this session. File for session 61 if CSP coverage on a non-MDN target is wanted.

## Summary

- 11 commands tested, **6 fixes verified green** (full-page screenshot, navigate freshness, eval CSP bypass, render_blocking parity, cascade note, network text), **1 partial** (daemon stop — escalation visible but port still held on MDN's Firefox), **2 new issues** filed for iter-95+ (daemon stop on multi-child Firefox, cascade `computed` population)
- Key takeaway: **iter-92/93/94 mostly hit their targets on the real target — but `cargo install`'d binaries can lag main by hours, and the first half of this session was wasted testing the stale binary. The dogfood workflow needs a staleness check.**

## References

- [[dogfooding-session-59]]
- [[iteration-92-full-page-and-navigate-parity]]
- [[iteration-93-eval-via-debugger-csp-bypass]]
- [[iteration-94-session-59-polish-bundle]]
