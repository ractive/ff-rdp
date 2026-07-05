---
title: "Iteration 98: media-query truthfulness — responsive stops lying, cascade winner respects media context, doctor scope, single error emission"
type: iteration
date: 2026-07-05
status: planned
branch: iter-98/media-query-truthfulness
depends_on: []
firefox_refs: []
kb_refs: [kb/dogfooding/field-report-responsive-cascade-2026-07-05.md]
first_call_sites:
  - primitive: "responsive truthful viewport emulation + media_query_check self-check field in the JSON envelope"
    site: "crates/ff-rdp-cli/src/commands/responsive.rs"
  - primitive: "cascade winner evaluation filters candidate rules by their enclosing @media condition and cross-checks the winner value against computed"
    site: "crates/ff-rdp-cli/src/commands/cascade.rs"
  - primitive: "doctor binary_staleness repo-identity guard — skipped outside the ff-rdp checkout"
    site: "crates/ff-rdp-cli/src/commands/doctor.rs"
  - primitive: "single JSON error envelope emission (duplicate human eprintln removed)"
    site: "crates/ff-rdp-cli/src/main.rs"
dogfood_script: iteration-98-media-query-truthfulness.dogfood.sh
tags: [iteration, responsive, cascade, doctor, errors, media-queries, truthfulness]
---

# Iteration 98 — media-query truthfulness

An external agent session ([[field-report-responsive-cascade-2026-07-05]])
used ff-rdp for exactly the responsive-debugging workflow it was built for and
hit two defects that both produce **confidently wrong answers** — the worst
failure mode for an agent-facing tool:

1. `responsive` reported a 390px viewport in which `html` measured 390px but
   `(min-width: 1024px)` media queries **stayed active** (shell-main at
   980px) — a physically impossible CSS state. The current implementation
   constrains layout width without changing what media queries evaluate
   against, and nothing in the output admits that.
2. `cascade`'s winner flag marked a `min-width: 0` rule as winning while
   `computed` correctly reported the value from a `(width >= 1024px)` rule —
   the winner algorithm ignores media-query context entirely.

Verdict from the report: *"until responsive and cascade are
media-query-truthful, I'd cross-check any viewport-dependent conclusion with
real viewport emulation. Fix those two and it's the best agent-facing browser
tool I've used."* Two smaller nits ride along: `doctor`'s binary_staleness
compares against the **cwd repo's** git HEAD even in foreign checkouts, and
errors are emitted twice (human line + JSON envelope).

## Hard rule

ff-rdp must never present a viewport state where the reported width and the
page's media-query evaluation disagree **without flagging it in the same JSON
envelope**. Truthful emulation is the goal; an honest warning is the floor.

## Themes

### A. `responsive` becomes media-query-truthful (or refuses to lie)

Two layers, in order of preference:

1. **Truthful emulation.** Research and implement a mechanism that makes
   media queries actually flip at the requested width. Candidates, to be
   settled by a research task whose outcome lands in
   `kb/research/responsive-truthful-viewport.md` (+ `firefox_refs` filled
   here): resizing the real top-level window (`window.resizeTo` is permitted
   for script-opened/headless windows; relaunch-width for launch-mode),
   or an RDM/emulation actor surface over RDP if one exists in the current
   Firefox tree. Any spec-surface use must follow the allow-spec-drift rule.
2. **Self-check floor.** Whatever mechanism is active, after applying the
   viewport `responsive` evaluates a probe in-page
   (`matchMedia("(width: <requested>px)").matches` plus
   `innerWidth`) and writes a `media_query_check` object into the envelope:
   `{requested, inner_width, matches}`. On mismatch it adds a warning and,
   with `--strict`, exits non-zero. This fires even if theme-A-1 concludes
   truthful emulation is impossible in attach-mode.

### B. `cascade` winner respects media-query context

Only rules whose full enclosing conditional chain currently matches may
compete for the winner flag: evaluate each candidate's `@media` condition
text in-page via `matchMedia(...)` (and `@supports` via `CSS.supports(...)`)
and exclude non-matching rules before specificity/order comparison. As a
safety net, cross-check the winner's resolved value against `computed` for
that property and set `winner_verified: false` on disagreement — the command
whose purpose is "explain why this value wins" must never assert a winner
that contradicts the computed value silently.

### C. `doctor` binary_staleness only fires inside the ff-rdp checkout

The check currently compares the running binary against the git HEAD of
whatever repo the cwd happens to be in (observed firing against the user's
`neon` repo). Guard it with a repo-identity test — the cwd repo qualifies
only if it is actually the ff-rdp workspace (e.g. workspace `Cargo.toml`
membership marker such as `crates/ff-rdp-core`); otherwise report
`status: "skipped"`, `reason: "not in an ff-rdp checkout"`.

### D. Errors are emitted exactly once

`main.rs` currently prints a human `error: {message}` line *and* the JSON
error envelope. Per the JSON-only output convention, the envelope is the
single emission; the duplicate human line goes away. Exit codes and `--jq`
behavior are unchanged.

## Pre-fix repro

- `pre_fix_repro_responsive_media_queries_do_not_flip` (live) — fixture page
  with a `#probe` styled narrow by default and 980px inside
  `@media (min-width: 1024px)`; run `responsive` at 390: pre-fix, `html`
  reports 390 while `matchMedia("(min-width: 1024px)").matches` is still
  `true` and `#probe` computes 980px, with no warning in the envelope.
- `pre_fix_repro_cascade_winner_ignores_media_context` (live) — element with
  a base `width` rule and a `(min-width: 1024px)` override; at a ≥1024px
  viewport, pre-fix `cascade` marks the base rule as winner while `computed`
  reports the override's value.
- `pre_fix_repro_doctor_staleness_uses_foreign_repo_head` (e2e) — run
  `doctor` with cwd inside a freshly-initialized temp git repo: pre-fix, the
  binary_staleness check evaluates against that repo's HEAD instead of
  reporting itself skipped.
- `pre_fix_repro_error_emitted_twice` (e2e) — run a failing command and
  capture stderr/stdout: pre-fix, the same error appears both as a human
  `error:` line and inside the JSON envelope.

## Tasks

### Theme A — truthful `responsive` [0/4]

- [ ] Research truthful viewport mechanisms (real window resize in headless
      and launch modes; RDM/emulation actor surface over RDP, if any, in the
      current Firefox tree). Outcome recorded in
      `kb/research/responsive-truthful-viewport.md`; `firefox_refs:` in this
      plan filled with the relevant server files/line ranges.
- [ ] Implement the chosen truthful mechanism in
      `crates/ff-rdp-cli/src/commands/responsive.rs` (spec-surface use, if
      any, annotated per the allow-spec-drift rule).
- [ ] Add the `media_query_check` self-check to the envelope
      (`{requested, inner_width, matches}` + warning on mismatch) and a
      `--strict` flag that turns a mismatch into a non-zero exit.
- [ ] Land `pre_fix_repro_responsive_media_queries_do_not_flip` and
      `live_responsive_self_check_reports_mismatch` (self-check exercised
      with emulation forced into layout-only mode).

### Theme B — media-aware `cascade` winner [0/3]

- [ ] Filter winner candidates by evaluating each rule's enclosing
      conditional chain in-page (`matchMedia` for `@media`, `CSS.supports`
      for `@supports`) before specificity/order comparison in
      `crates/ff-rdp-cli/src/commands/cascade.rs`.
- [ ] Cross-check the winner's value against `computed`; on disagreement set
      `winner_verified: false` on the property (never silently wrong).
- [ ] Land `pre_fix_repro_cascade_winner_ignores_media_context` and
      `unit_cascade_winner_disagreement_flagged` (synthetic disagreement →
      flag set).

### Theme C — scoped binary_staleness [0/2]

- [ ] Add the repo-identity guard to binary_staleness in
      `crates/ff-rdp-cli/src/commands/doctor.rs`; outside the ff-rdp
      checkout the check reports `skipped` with a reason instead of
      comparing against a foreign HEAD.
- [ ] Land `pre_fix_repro_doctor_staleness_uses_foreign_repo_head`
      (temp foreign git repo → `skipped`).

### Theme D — single error emission [0/2]

- [ ] Remove the duplicate human `error:` line in
      `crates/ff-rdp-cli/src/main.rs`; the JSON error envelope is the single
      emission; exit codes unchanged.
- [ ] Land `pre_fix_repro_error_emitted_twice` (failing command emits the
      error exactly once, as the envelope).

## Acceptance Criteria [0/7]

- [ ] `pre_fix_repro_responsive_media_queries_do_not_flip`: post-fix, at a
      requested 390px the fixture's `matchMedia("(min-width: 1024px)").matches`
      is `false` and `#probe` computes its narrow value — or, if layout-only
      mode is in effect, the envelope carries `media_query_check.matches ==
      false` plus a warning.
- [ ] `live_responsive_self_check_reports_mismatch`: with emulation forced
      to layout-only, the envelope contains `media_query_check` with
      `matches == false` and `--strict` exits non-zero.
- [ ] `pre_fix_repro_cascade_winner_ignores_media_context`: post-fix, at a
      ≥1024px viewport the `(min-width: 1024px)` rule is the winner and its
      value equals `computed`'s answer for the property.
- [ ] `unit_cascade_winner_disagreement_flagged`: a winner whose value
      disagrees with computed carries `winner_verified: false`.
- [ ] `pre_fix_repro_doctor_staleness_uses_foreign_repo_head`: post-fix,
      `doctor` in a foreign git repo reports binary_staleness `skipped`
      with reason `not in an ff-rdp checkout`.
- [ ] `pre_fix_repro_error_emitted_twice`: post-fix, a failing command
      emits exactly one error (the JSON envelope) and no duplicate human
      line.
- [ ] `dogfood_script_full_run_iter_98`: `.dogfood.sh` drives `responsive`
      at 390 and 1280 against a live media-query fixture, asserts the
      envelope is truthful at both widths, runs `cascade` on the override
      property asserting winner == computed, and exits 0.

## Out of scope

- DPR / touch / user-agent emulation — this iteration is width/height
  media-query truthfulness only.
- `@container` query awareness in `cascade` — follow-up if/when container
  queries enter the winner algorithm at all.
- Full RDM feature parity (rotation, network throttling, device presets).

## References

- [[field-report-responsive-cascade-2026-07-05]] — the triaged external
  feedback this plan implements.
- [[iteration-88-cascade-fifth-attempt-single-theme]] — prior cascade work;
  the winner algorithm this theme extends.
- `crates/ff-rdp-cli/src/main.rs:180-188` — the double error emission site.
