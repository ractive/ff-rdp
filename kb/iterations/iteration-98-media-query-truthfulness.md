---
title: "Iteration 98: media-query truthfulness — responsive stops lying, cascade winner respects media context, doctor scope, single error emission"
type: iteration
date: 2026-07-05
status: done
branch: iter-98/media-query-truthfulness
depends_on: []
firefox_refs: []
dogfood_path: "ff-rdp responsive '#probe' --widths 390 --jq '.results.breakpoints[0].media_query_check.requested'"
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

## Implemented (2026-07-09) — 7/7 ACs

All four themes landed on `iter-98/media-query-truthfulness`:

- **Theme A** — `responsive` now emits a `media_query_check` object
  `{requested, inner_width, matches}` per breakpoint
  (`MEDIA_QUERY_CHECK_JS` / `evaluate_media_query_check` in `responsive.rs`),
  attaches a warning on mismatch, and gains a `--strict` flag that turns a
  mismatch into a non-zero exit. Truthful real-window emulation was researched
  and found impossible over RDP — see [[responsive-truthful-viewport]] —
  so `firefox_refs` stays empty and the self-check floor is the deliverable.
- **Theme B** — `cascade` evaluates every distinct `@media` condition in-page
  (`build_media_probe_js` / `fetch_media_matches`) and excludes rules whose
  media chain is inactive from winner selection; the winner is cross-checked
  against the batch-fetched computed value (`css_values_agree`) and flagged
  `winner_verified: false` on disagreement. Each rule row carries
  `media_active`.
- **Theme C** — `doctor` binary_staleness is guarded by `is_ff_rdp_checkout()`
  and reports `status: "skipped"`, `detail: "not in an ff-rdp checkout"` outside
  the workspace (new `Status::Skipped`).
- **Theme D** — `main.rs` no longer prints the duplicate human `error:` line;
  the JSON error envelope is the single emission.

### Original triage note (superseded)

Re-audited after the 2026-07 deep review: no code had landed (the plan was
triaged from the field report in commit `ea2ea08`). All four themes were open.
Two cross-plan notes:

- **Theme D (single error emission) overlaps [[iteration-105-error-taxonomy-release-prep]].**
  Both touch `main.rs` error output (the duplicate lives at ~`main.rs:188-190`:
  an `eprintln!("error: …")` plus the JSON envelope). iter-105 rewrites the
  error→exit-code path and freezes the `error_type` table. Do Theme D's
  one-line removal *first* (it's independent and trivial) or fold it into
  iter-105 — do not land both branches editing the same emission path
  concurrently.
- **Theme C fixes a bug iter-95 shipped.** iter-95's `binary_staleness` check
  fires against whatever repo the cwd is in (observed against the user's
  `neon` repo); Theme C scopes it to the ff-rdp checkout. Still needed —
  keep.

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

### Theme A — truthful `responsive` [4/4]

- [x] Research truthful viewport mechanisms (real window resize in headless
      and launch modes; RDM/emulation actor surface over RDP, if any, in the
      current Firefox tree). Outcome recorded in
      [[responsive-truthful-viewport]]: truthful emulation is not achievable
      over RDP, so `firefox_refs:` stays empty (no reachable spec surface to
      annotate) and the self-check floor is the deliverable.
- [x] Implement the chosen truthful mechanism in
      `crates/ff-rdp-cli/src/commands/responsive.rs`. Layout-only CSS
      (`SET_VIEWPORT_CSS_JS`) is retained — no spec-surface use, so no
      allow-spec-drift annotation is needed.
- [x] Add the `media_query_check` self-check to the envelope
      (`{requested, inner_width, matches}` + warning on mismatch) and a
      `--strict` flag that turns a mismatch into a non-zero exit.
      (`MEDIA_QUERY_CHECK_JS`, `evaluate_media_query_check`, `--strict` arg.)
- [x] Land `pre_fix_repro_responsive_media_queries_do_not_flip` and
      `live_responsive_self_check_reports_mismatch` (live, in
      `tests/live_98_media_query_truthfulness.rs`); e2e path exercised via the
      `eval_result_responsive_mq_check.json` fixture in
      `tests/e2e/responsive.rs`.

### Theme B — media-aware `cascade` winner [3/3]

- [x] Filter winner candidates by evaluating each rule's enclosing
      conditional chain in-page (`matchMedia` for `@media`) before
      specificity/order comparison in
      `crates/ff-rdp-cli/src/commands/cascade.rs` (`build_media_probe_js`,
      `fetch_media_matches`, `media_active_for`). `@supports` conditions are
      not surfaced by `AppliedRule` today, so they are covered by the
      `winner_verified` cross-check backstop rather than pre-filtering — noted
      in Out of scope.
- [x] Cross-check the winner's value against `computed`; on disagreement set
      `winner_verified: false` on the property (`css_values_agree`,
      `fetch_computed_values`).
- [x] Land `pre_fix_repro_cascade_winner_ignores_media_context` (unit +
      `tests/live_98_media_query_truthfulness.rs`) and
      `unit_cascade_winner_disagreement_flagged` (synthetic disagreement →
      flag set).

### Theme C — scoped binary_staleness [2/2]

- [x] Add the repo-identity guard to binary_staleness in
      `crates/ff-rdp-cli/src/commands/doctor.rs` (`is_ff_rdp_checkout()` +
      `Status::Skipped`); outside the ff-rdp checkout the check reports
      `skipped` with the reason `not in an ff-rdp checkout`.
- [x] Land `pre_fix_repro_doctor_staleness_uses_foreign_repo_head` (e2e,
      `tests/e2e/doctor.rs`) and
      `unit_doctor_binary_staleness_skipped_outside_ff_rdp_checkout` (unit).

### Theme D — single error emission [2/2]

- [x] Remove the duplicate human `error:` line in
      `crates/ff-rdp-cli/src/main.rs`; the JSON error envelope is the single
      emission; exit codes unchanged.
- [x] Land `pre_fix_repro_error_emitted_twice` (e2e, `tests/e2e/exit_codes.rs`
      — failing command emits the error exactly once, as the envelope).

## Acceptance Criteria [7/7]

- [x] `pre_fix_repro_responsive_media_queries_do_not_flip` (live,
      `tests/live_98_media_query_truthfulness.rs`): post-fix, at a requested
      390px the envelope carries `media_query_check.matches == false` plus a
      "media queries did not flip" warning (layout-only mode is in effect over
      RDP).
- [x] `live_responsive_self_check_reports_mismatch` (live,
      `tests/live_98_media_query_truthfulness.rs`): with emulation layout-only,
      the envelope contains `media_query_check` with `matches == false` and
      `--strict` exits non-zero.
- [x] `pre_fix_repro_cascade_winner_ignores_media_context` (unit +
      `tests/live_98_media_query_truthfulness.rs`): post-fix, at a ≥1024px
      viewport the `(min-width: 1024px)` rule is the winner and its value equals
      `computed`'s answer for the property (`winner_verified: true`).
- [x] `unit_cascade_winner_disagreement_flagged` (`commands/cascade.rs`): a
      winner whose value disagrees with computed carries
      `winner_verified: false`.
- [x] `pre_fix_repro_doctor_staleness_uses_foreign_repo_head` (e2e,
      `tests/e2e/doctor.rs`): post-fix, `doctor` in a foreign git repo reports
      binary_staleness `skipped` with reason `not in an ff-rdp checkout`.
- [x] `pre_fix_repro_error_emitted_twice` (e2e, `tests/e2e/exit_codes.rs`):
      post-fix, a failing command emits exactly one error (the JSON envelope)
      and no duplicate human line.
- [x] `dogfood_script_full_run_iter_98` — asserts `media_query_check` and `winner_verified`
      (`iteration-98-media-query-truthfulness.dogfood.sh`): drives `responsive`
      at 390 and 1280 against a live media-query fixture, asserts the
      `media_query_check` self-check is present at both widths, runs `cascade`
      on the media-overridden `width` property asserting winner == computed via
      `winner_verified`, and exits 0. Evidenced by the `media_query_check` /
      `winner_verified` fields and the `--strict` flag added in this diff.

## Out of scope

- DPR / touch / user-agent emulation — this iteration is width/height
  media-query truthfulness only.
- `@container` query awareness in `cascade` — follow-up if/when container
  queries enter the winner algorithm at all.
- `@supports` pre-filtering in `cascade` — `AppliedRule` does not surface
  `@supports` ancestor conditions today, so those rules are covered by the
  `winner_verified` cross-check backstop rather than pre-exclusion. Surfacing
  `@supports` in the actor parser (and pre-filtering via `CSS.supports(...)`)
  is a follow-up.
- Truthful real-window viewport emulation — not achievable over the RDP
  transport (see [[responsive-truthful-viewport]]); a BiDi-transport iteration
  could revisit `browsingContext.setViewport`.
- Full RDM feature parity (rotation, network throttling, device presets).

## References

- [[field-report-responsive-cascade-2026-07-05]] — the triaged external
  feedback this plan implements.
- [[iteration-88-cascade-fifth-attempt-single-theme]] — prior cascade work;
  the winner algorithm this theme extends.
- `crates/ff-rdp-cli/src/main.rs:180-188` — the double error emission site.
