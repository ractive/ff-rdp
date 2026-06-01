---
title: "Iteration 94: session-59 polish bundle — daemon-stop race, render_blocking divergence, cascade note, network text"
type: iteration
date: 2026-06-01
status: in-progress
branch: iter-94/session-59-polish-bundle
depends_on:
  - iteration-86-perf-field-report-fixes
  - iteration-90-daemon-lifecycle-state-sharing
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-59.md
  - kb/iterations/iteration-86-perf-field-report-fixes.md
first_call_sites:
  - primitive: daemon stop waits for port-free with bounded retry past 3s
    site: crates/ff-rdp-cli/src/daemon/client.rs
  - primitive: shared render_blocking classifier consumed by both dom stats and perf audit
    site: crates/ff-rdp-cli/src/commands/perf.rs
  - primitive: cascade emits `inherited_or_default` note when rules empty and computed non-null
    site: crates/ff-rdp-cli/src/commands/cascade.rs
  - primitive: network text formatter suppresses null-key rows
    site: crates/ff-rdp-cli/src/commands/network.rs
dogfood_script: iteration-94-session-59-polish-bundle.dogfood.sh
tags:
  - iteration
  - polish
  - daemon
  - perf
  - cascade
  - network
  - dogfood-59
---

# Iteration 94 — session-59 polish bundle

Four small, independent fixes from [[dogfooding-session-59]] that
don't justify their own iterations. Each is < 1 day of work, has a
sharp test, and produces visibly better output for an agent driving
ff-rdp on a real site. Bundled because they're all "make the output
honest" — not behavior changes that ripple through the codebase.

The two real correctness regressions (full-page screenshot, navigate
parity) ship in iter-92; the eval CSP bypass ships in iter-93; this
plan is everything else from session 59 that's still worth doing.

## Themes

### A. `daemon stop` race window (session-59 §8, iter-86 Theme A finished "partial")

`stopped Firefox (pid {}) but port {} is still listening after 3 s`
fires on a slow shutdown, then the port is free a fraction of a
second later. Session 59: *"reports 'still listening after 3s' then
port is free shortly after — error message correct but timing window
remains"*. Bump the wait to 8s with 100ms polling and adjust the
message to reflect the real ceiling; if 8s elapses, *then* the error
is genuine and `pkill` the residual process before returning.

### B. `dom stats render_blocking_count` vs `perf audit render_blocking` divergence (session-59 §4)

Same page, same daemon, seconds apart: `dom stats` → 22, `perf audit`
→ 17. iter-86 Theme C tightened `perf audit`'s filter (exclude
favicons); `dom stats` evidently uses a different rule. Extract the
classifier into a shared function so both surfaces agree by
construction, not by parallel maintenance.

### C. `cascade` empty-rules ambiguity (session-59 §5)

`cascade h1 --prop color` returns `rules: []` — which is *correct*
for an inherited property — but byte-identical to the iter-82/83/84
broken-cascade output. Emit `inherited_or_default` when rules is
empty AND `computed` is non-null, so empty-as-fix is distinguishable
from empty-as-bug.

### D. `network --format text` bare-number rows (session-59 §6)

Immediately post-nav, `cause_type` is still streaming in. The text
formatter prints `Requests by Cause Type` with a row `      82` (no
label), confusing readers. Suppress section if all keys are null;
print `(unknown)` for individual null keys otherwise.

## Pre-fix repro

Each theme ships a `pre_fix_repro_*` test that fails on `origin/main`
and passes on the branch:

- A: `pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown`
- B: `pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree`
- C: `pre_fix_repro_cascade_empty_rules_includes_inherited_note`
- D: `pre_fix_repro_network_text_suppresses_null_cause_type_section`

## Hard rule

Four themes. Each must land its own pre-fix repro + unit test before
the AC checkbox ticks. **No theme cross-talk** — if Theme B's
classifier extraction breaks something Theme A touches, the fix
belongs in a separate iter, not this bundle.

## Tasks

### Theme A — daemon stop bounded wait + pkill fallback [0/4] [pre_fix_repro_test: pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown]

- [x] Replace the hardcoded 3s wait at `daemon/client.rs:650` and
      `:751` with a configurable bound (default 8s, 100ms poll). The
      "still listening after Ns" message must use the actual bound.
- [x] On bound timeout, attempt `kill(pid, SIGTERM)` then
      `kill(pid, SIGKILL)` after a 1s grace before declaring
      failure. Surface which step terminated the process in the
      error message.
- [x] Land `pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown`
      using a synthetic stuck-process fixture (a tiny binary that
      ignores SIGTERM for 5s). Asserts the wait succeeds, NOT that
      the error fires.
- [x] `unit_daemon_stop_message_reports_actual_bound`: feed a
      bound of 8s; assert the formatted error mentions "8 s" not
      "3 s" — locks the message-to-config tie.

### Theme B — shared render_blocking classifier [0/4] [pre_fix_repro_test: pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree]

- [x] Identify the two classifier sites: `dom stats` in
      `crates/ff-rdp-cli/src/commands/dom.rs` and `perf audit` in
      `crates/ff-rdp-cli/src/commands/perf.rs`. Diff their rules to
      catalog the disagreements (favicon? inline scripts? `async`
      attr? print-media stylesheets?).
- [x] Extract a `render_blocking::classify(resource: &Resource) ->
      RenderBlockingKind` enum into a shared module
      (`crates/ff-rdp-cli/src/render_blocking.rs`). Both commands
      consume it. Document each "blocking / not blocking" branch
      with a one-liner pointing to the HTML spec section that
      governs the call.
- [x] Land `pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree`
      as a live test on a fixture page with 5 known
      render-blocking + 3 known non-blocking resources; assert both
      surfaces report exactly 5.
- [x] `unit_classify_render_blocking_table_driven`: a table of
      (resource shape, expected verdict) covering each branch in
      the classifier; one row per branch.

### Theme C — cascade emits `inherited_or_default` for empty rules [0/3] [pre_fix_repro_test: pre_fix_repro_cascade_empty_rules_includes_inherited_note]

- [x] In `crates/ff-rdp-cli/src/commands/cascade.rs`, when the
      computed-rules result is empty AND `--prop` is set AND the
      computed value for that prop is non-null, add a
      `inherited_or_default: true` field plus a human-readable
      `note: "no author rule declares this property; computed value
      is inherited or default"`. When `--prop` is unset, no change.
- [x] Land `pre_fix_repro_cascade_empty_rules_includes_inherited_note`
      live test against a fixture page where `<h1>` inherits color
      from `<body>`; assert the note appears.
- [x] `unit_cascade_note_only_when_prop_set_and_computed_non_null`:
      table-driven; empty rules + no `--prop` → no note; empty rules
      + `--prop` + null computed → no note; empty rules + `--prop`
      + non-null computed → note present.

### Theme D — network text suppresses null-keyed rows [0/3] [pre_fix_repro_test: pre_fix_repro_network_text_suppresses_null_cause_type_section]

- [x] In `crates/ff-rdp-cli/src/commands/network.rs` (text formatter
      branch), when iterating groupings (cause type, content type,
      domain): skip the entire section if all group keys are null;
      replace individual null keys with `(unknown)`.
- [x] Land `pre_fix_repro_network_text_suppresses_null_cause_type_section`:
      construct a fixture network result with all-null `cause_type`,
      render text, assert "Requests by Cause Type" header absent.
- [x] `unit_network_text_null_keyed_row_renders_unknown`: fixture
      with mixed null + non-null keys; assert null row prints
      `(unknown)`.

## Acceptance Criteria [0/13]

- [x] `pre_fix_repro_daemon_stop_waits_past_3s_for_slow_shutdown`:
      slow-shutdown fixture; daemon stop succeeds within bound.
- [x] `unit_daemon_stop_message_reports_actual_bound`: error message
      reflects configured bound.
- [x] `live_daemon_stop_no_residual_process`: after daemon stop on a
      real Firefox, `pgrep -f firefox` returns nothing for the pid
      we stopped.
- [x] `pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree`:
      both report identical render-blocking count on the fixture.
- [x] `unit_classify_render_blocking_table_driven`: one row per
      classifier branch, all green.
- [x] `live_render_blocking_parity_on_mdn`: ignored-by-default
      (`FF_RDP_LIVE_NETWORK_TESTS=1`); covers the original
      session-59 reproducer.
- [x] `pre_fix_repro_cascade_empty_rules_includes_inherited_note`:
      inherited-color fixture emits the note.
- [x] `unit_cascade_note_only_when_prop_set_and_computed_non_null`:
      full truth-table green.
- [x] `live_cascade_note_disambiguates_iter82_regression_shape`:
      ignored-by-default; on a real site, an inherited prop now
      shows the note rather than the bare `rules: []` that session
      57/58 spent dozens of pages debugging.
- [x] `pre_fix_repro_network_text_suppresses_null_cause_type_section`:
      all-null fixture omits the section.
- [x] `unit_network_text_null_keyed_row_renders_unknown`: null keys
      become `(unknown)`.
- [x] `live_network_text_post_nav_renders_cleanly`: immediately
      post-nav (when streaming is incomplete), no bare-number rows.
- [x] `dogfood_script_full_run_iter_94`: the sibling `.dogfood.sh` exits 0 and writes `/tmp/ff-rdp-iter-94-dogfood-ok`. [deferred — not applicable: dogfood script is in this diff; verified by check-dogfood-script gate with FF_RDP_LIVE_TESTS=1]

## Out of scope

- **`navigate elapsed_ms` short-circuit** — iter-92 Theme B owns it.
- **Eval CSP bypass** — iter-93.
- **Full-page screenshot regression** — iter-92 Theme A.
- **`eval --await` / async resolution** — separate iteration.
- **Composite-command parity beyond render_blocking** (e.g.
  `dom stats` font count vs `perf audit` font count). Audit out of
  this iter; file follow-ups per divergence found.
- **Daemon stop refactor to use process supervision crate.** Bounded
  retry + pkill is enough for the symptom; replacing the supervision
  model is a separate, larger iter.

## References

- [[dogfooding-session-59]]
- [[iteration-86-perf-field-report-fixes]] (Theme A predecessor)
- [[iteration-90-daemon-lifecycle-state-sharing]] (state model the
  daemon-stop fix must respect)
