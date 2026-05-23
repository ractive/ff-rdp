---
title: Dogfooding Session 50 — iter-61i verification + remaining-bug catalog
type: dogfooding
date: 2026-05-23
status: completed
site: https://en.wikipedia.org + https://example.com
commands_tested: [doctor, launch, navigate, dom, screenshot, scroll, eval, computed]
tags:
  - dogfooding
  - iter-61i
  - regression-verification
  - short-session
---

# Dogfooding Session 50

Short verification pass run immediately after [[iterations/iteration-16-command-fixes|kb/iterations/iteration-61i-dogfood-49-fixes.md]] merged at `a57f06f`. Three iter-61i fixes confirmed working live, two known-deferred bugs confirmed still present (not regressions), one accidental partial-fix discovered.

Previous: [[dogfooding-session-49]].

## TL;DR

- ✅ **Same-URL re-navigate** (iter-61i A) — second `navigate <currentUrl>` returns in ~90 ms (was timing out at 5 s).
- ✅ **`dom` always returns an array** (iter-61i B) — `results | type == "array"` for 0-match, 1-match, and N-match cases on Wikipedia.
- ✅ **`--first` escape hatch** — `dom 'h1' --first --text` returns the scalar `"Firefox"` directly (no array wrapping).
- ✅ **`eval --stringify --format text` no hint suffix** (iter-61i D) — output ends after the value with no trailing `-> ff-rdp …` line.
- 🟡 **`computed --prop=--bg-color` accidentally works** — clap's `=` form bypasses flag-parsing naturally; the deferred C2 task is half-done by accident (multi-prop / `--prop --bg-color` without `=` still need the iter-61j fix).
- ❌ **`screenshot --full-page` still viewport-only** — known deferred bug (chrome-scope fallback ignores `fullpage`).
- ❌ **`computed --prop A --prop B` still rejected** — known deferred to iter-61j.

## Regression Checks

| Verified | Status | Evidence |
|---|---|---|
| iter-61i A: same-URL navigate | ✅ FIXED | `time ff-rdp navigate <currentUrl>` ~0.09 s real |
| iter-61i B: `dom` array shape | ✅ FIXED | `results \| type` == `"array"` for all match counts |
| iter-61i B: `--first` escape hatch | ✅ WORKING | scalar return preserved |
| iter-61i D: `--stringify` no hint | ✅ FIXED | `"{...}"` end of stdout |
| Known: `--full-page` chrome-scope | ❌ STILL BROKEN | both PNGs 1366×683 |
| Known: `computed --prop` repeatable | ❌ STILL BROKEN | "cannot be used multiple times" |
| Newly noticed: `--prop=--<name>` form | 🟡 PARTIAL | `--prop=--bg-color` works via clap's `=`-form quirk |

## Summary

Cycle converged: iter-61i shipped clean, no NEW bugs surfaced, all remaining known bugs are explicitly tracked in iter-61i's deferred list + queued for iter-61j. The goal-loop ([[`/goal`]]) has reached a stable point — dogfood → fix → PR → merge → dogfood again with no new findings.

## What Worked (post-iter-61h baseline)

Quality-of-life wins from recent merges that this session enjoyed silently:
- Zero per-command Firefox-version warnings (iter-61h)
- `doctor` reports Firefox 151 as PASS (not WARN)
- Recorder + runner still round-tripping cleanly
- iter-61i's same-URL fix means `ff-rdp navigate` can be called freely without
  pre-checking the current URL — agents save a `tabs` call per redirect-home pattern.

## References

- [[iteration-61i-dogfood-49-fixes]] — the fixes verified here
- [[dogfooding-session-49]] — predecessor; surfaced the bugs iter-61i closed
- Pending: an iter-61j plan for the deferred C/E themes plus the four
  long-standing known bugs (`--full-page`, ref resolution, CSP eval feature
  gap, computed multi-`--prop`).
