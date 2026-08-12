---
branch: iter-145/error-envelope-completeness
date: 2026-08-11
depends_on:
  - kb/iterations/iteration-141-output-hygiene.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://example.com --port 6100
  ff-rdp click 'body' --port 6100 --jq '.error_type'
  # → any JS failure during click must arrive as a parseable JSON envelope on
  #   stdout with error_type set, never as bare text on stderr
first_call_sites: []
status: done
title: "Iteration 145: error envelope completeness — the paths iter-141's sweep missed"
type: iteration
tags:
  - iteration
---

# Iteration 145: error envelope completeness — the paths iter-141's sweep missed

Follow-up to [[iteration-141-output-hygiene]]. Found during the post-batch live sweep on main
(2026-08-11), not during a dogfooding session — so there is no session report to cite; the
evidence is inline below.

## Why this exists

[[iteration-141-output-hygiene]] Theme E routed JS exceptions through the JSON error envelope
(`error_type: "User"`) instead of bare text on stderr, so a scripted consumer can parse a
failure the same way it parses every other ff-rdp error. That sweep touched 61 call sites and
its own review pass caught one straggler in `eval.rs`. It still did not reach `click.rs`.

Two paths remain that print an exception to stderr and exit non-zero with nothing on stdout:

```rust
// crates/ff-rdp-cli/src/commands/click.rs:399  — top-level attempt
// crates/ff-rdp-cli/src/commands/click.rs:508  — inside the frame scan
eprintln!("error: {}", sanitize_for_terminal(&msg));
return Err(AppError::Exit(1));
```

Both fire when the evaluated click JS throws something that is *not* the element-not-found
marker. `AppError::Exit(1)` marks this as a deliberate "already printed, just exit" pattern, so
it predates iter-141 rather than being introduced by it — the sweep simply didn't reach it.

This is the same class of defect as iter-125's false-good LCP and iter-139's fabricated CLS:
the tool stops speaking its own documented protocol at exactly the moment something went wrong.
CLAUDE.md states the contract plainly — "JSON-only output with `--jq` filter support". An agent
driving ff-rdp and parsing stdout gets an empty string and a bare exit code.

**Do not assume these two sites are the only ones.** The point of this iteration is the sweep,
not the two known hits. Enumerate every `eprintln!` on an error path under
`crates/ff-rdp-cli/src/commands/` and classify each as (a) a genuine error that belongs in the
envelope, (b) a debug/diagnostic line that is legitimately stderr (e.g.
`console.rs:199`'s `debug:` line), or (c) already-enveloped and printing a duplicate.

## Themes

### Theme A — `click` JS exceptions bypass the envelope

Route both `click.rs` sites through the same envelope path `eval.rs` now uses. A thrown
exception during click is a **user** error (`error_type: "User"`), consistent with iter-141's
classification of a thrown `Error` in `eval`.

Preserve the existing behaviour that a genuine JS failure short-circuits the frame scan rather
than paying for a scan that cannot help — that reasoning (in the current comment) is correct
and should survive the change.

### Theme B — audit the remaining stderr error paths

Sweep `crates/ff-rdp-cli/src/commands/` for error-path `eprintln!` calls and fix or explicitly
justify each. Where a line is legitimately stderr (progress, debug, skip notices), leave it and
say why in a comment so the next sweep doesn't re-litigate it.

### Theme C — a regression guard

The reason iter-141 shipped this gap is that the one test covering it
(`live_eval_csp::live_eval_script_error_still_surfaces`) was `FF_RDP_LIVE_TESTS`-gated, so CI
never ran it, and the test asserted the *old* stderr shape anyway — it went red only on the
post-batch live sweep and was fixed in `2c0bd12`. A gate that only fires in a manual sweep is
not a gate. Add a cheap non-live check that fails in CI when a new bare-stderr error path
appears.

## Acceptance Criteria [5/5]

- [x] live_145_click_js_exception_envelope: a `click` whose injected JS throws returns a JSON
      envelope on stdout with `error_type` set to `User`, exits non-zero, and writes nothing
      to stderr
- [x] live_145_click_frame_scan_js_exception_envelope: the same holds for a throw raised inside
      the frame-scan path (`click.rs:508`), not just the top-level attempt
- [x] live_145_click_element_not_found_unchanged: the existing informative frame-aware
      not-found diagnostic is byte-identical to its pre-iteration output — this iteration must
      not perturb the not-found path
- [x] e2e_145_no_unenveloped_error_paths: a repo-level check enumerates error-path `eprintln!`
      calls under `crates/ff-rdp-cli/src/commands/` and fails on any that is neither enveloped
      nor annotated with a justification comment
- [x] unit_145_click_exception_maps_to_user_error_type: a thrown JS exception during click maps
      to `error_type: "User"`, not `Internal`

## Notes

- Themes are independent. If the sweep in Theme B turns up more than a handful of sites, land
  Theme A + C and defer the long tail to a sibling plan rather than ticking unverified ACs.
- Verify on the wire before coding, per the batch-138–142 run guidance: reproduce an actual
  click-time JS throw against real Firefox and capture stdout/stderr/exit separately. Do not
  infer the current behaviour from reading the code alone.
- Every live test added here must exercise the **default daemon path**. Do not add entries to
  the shrink-only grandfather list in
  `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs`.

### Resolution (2026-08-12)

The Theme B sweep enumerated every `eprintln!` under `crates/ff-rdp-cli/src/commands/` (~42
call sites across ~20 files). Findings:

- **Two more genuine bugs**, beyond the two `click.rs` sites named in "Why this exists": none —
  `click.rs:399`/`click.rs:508` were the only click-JS-exception sites. But the same
  print-then-`AppError::Exit(1)`-bypass idiom also existed at `scroll.rs:411` (`scroll until`
  timeout) — fixed alongside Theme A, reclassified to `AppError::Timeout` (matching every other
  timeout in this codebase — `js_helpers.rs`, `navigate.rs`, `click.rs`'s own network-wait path
  — not `AppError::User`, since it genuinely is a deadline, not a thrown exception).
- **The remaining ~40 sites** are legitimately stderr: progress/status lines (`index.rs`'s
  `[index] …` crawl progress), `debug:`-prefixed verbose-gated fallback notices (`a11y.rs`,
  `sources.rs`, `console.rs`), warn-and-continue best-effort cleanup (`navigate.rs`,
  `nav_action.rs`, `launch.rs`, `eval.rs`'s property-name enrichment), and `hint:` suggestions
  (`network.rs`, `network_events.rs`'s Performance-API fallback). None of them print an error
  and then bypass the envelope — they either continue execution or supplement an error that is
  *also* separately enveloped via `?`.
- Per this section's own escape hatch ("more than a handful … defer the long tail"): the ~40
  legitimate sites are **not** individually annotated with `// stderr-ok:` comments in this PR.
  Theme C's regression guard (`check-error-envelope-paths`) is scoped narrowly to the actual
  defect class — an `eprintln!` immediately followed by a bare `AppError::Exit(N)` — so it does
  not require retroactively annotating the legitimate long tail to pass. Annotating that tail
  (so a *future* sweep doesn't have to re-derive the (a)/(b)/(c) classification from scratch) is
  deferred to a sibling plan — [[iteration-148-stderr-path-annotations]] — filed before this PR
  merges, per CLAUDE.md's carry-over rule.
- Theme C shipped as `cargo xtask check-error-envelope-paths`, wired into
  `check-iteration-ready` (sub-check 11/11) and the `discipline` CI job.
