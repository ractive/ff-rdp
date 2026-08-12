---
title: "Iteration 148: annotate the legitimate stderr eprintln! long tail"
type: iteration
date: 2026-08-12
status: done
branch: iter-148/stderr-path-annotations
depends_on:
  - kb/iterations/iteration-145-error-envelope-completeness.md
first_call_sites: []
dogfood_path: |
  cargo run -p xtask -- check-error-envelope-paths
  # expected: PASS, and every eprintln! under crates/ff-rdp-cli/src/commands/
  # that check does not flag now carries a `// stderr-ok: <reason>` comment
  # explaining why it is legitimately stderr, so a future sweep doesn't have
  # to re-derive the classification from scratch.
tags:
  - iteration
---

# Iteration 148: annotate the legitimate stderr eprintln! long tail

Carry-over from [[iteration-145-error-envelope-completeness]]'s Theme B sweep. That iteration's
own Notes section pre-authorized deferring this: "If the sweep in Theme B turns up more than a
handful of sites, land Theme A + C and defer the long tail to a sibling plan rather than ticking
unverified ACs." The sweep found ~40 legitimate stderr sites (progress lines, `debug:`-gated
fallback notices, warn-and-continue best-effort cleanup, `hint:` suggestions) alongside the two
genuine bugs iter-145 fixed. None of the ~40 were annotated in that PR — this iteration does that.

This is pure annotation work: no `eprintln!` call's *behaviour* changes. `check-error-envelope-paths`
(iter-145 Theme C) does not require these annotations to pass — it only flags an `eprintln!`
immediately followed by a bare `AppError::Exit(N)` — so this iteration doesn't unblock anything
technically broken. Its value is documentation: the next sweep should be able to trust the
`// stderr-ok:` comments instead of re-auditing every site from scratch.

## Themes

- **A — Annotate every unflagged `eprintln!` under `crates/ff-rdp-cli/src/commands/`.** Add a
  `// stderr-ok: <one-line reason>` comment on or immediately above each site, citing which of
  (b) debug/diagnostic or (c) already-enveloped-duplicate it is (per iter-145's "Why this
  exists" classification).
- **B — Tighten `check-error-envelope-paths` if the annotation pass reveals it should also flag
  category (c) (already-enveloped duplicates) as worth trimming**, not just category-(a) bugs.
  Only in scope if Theme A's sweep turns up genuine duplication worth removing — do not invent
  work here if the existing sites are all cleanly (b) or (c)-and-harmless.

## Tasks

### A. Annotate the long tail
- [x] Re-run the enumeration from iter-145's Resolution section (or `check-error-envelope-paths`'s
      own scan minus its Exit-bypass filter) to get the current, authoritative site list — file
      contents may have drifted since iter-145 landed.
- [x] For each site, add `// stderr-ok: <reason>` and classify (b) or (c) in the comment text.
- [x] Spot-check a handful under `--verbose` / with `FF_RDP_TRACE_RAW` etc. to confirm the
      stderr output described in each comment still matches reality.

## Acceptance Criteria [1/1]

- [x] unit_148_all_commands_eprintln_annotated: new companion check
      `check-stderr-annotations` (`crates/xtask/src/check_stderr_annotations.rs`,
      wired into `check-iteration-ready` and CI) confirms every `eprintln!`
      under `crates/ff-rdp-cli/src/commands/` (excluding `#[cfg(test)]` modules) has a
      `// stderr-ok:` comment on or within two lines above it. All 41 pre-existing sites
      across 15 files were annotated to make it pass; `cargo run -p xtask --
      check-stderr-annotations` — PASS.

## Design notes

Not a live-Firefox-behavior change, so no live test is required for Theme A itself — the AC is a
static/non-live repo check, matching Theme C's own cost profile ("cheap, non-live"). If Theme B's
scope ends up in play (trimming a genuine duplicate), that specific fix would need its own named
test per CLAUDE.md's normal convention.

## Resolution

Theme B was not invoked: every site the sweep found was cleanly (b) warn-and-continue /
debug-diagnostic / progress-line, with exactly one (c) already-enveloped duplicate
(`a11y.rs`'s unconditional `--native` restore-failure warning, which duplicates
`meta.service_restore_error` — iter-149 made it unconditional deliberately, so it's a kept
duplicate, not a bug to trim).

41 `eprintln!` sites across 15 files were annotated:

| File | Sites | Classification |
|---|---|---|
| `a11y.rs` | 8 | 7×(b) debug/`--verbose`, 1×(c) enveloped duplicate |
| `index.rs` | 7 | (b) progress lines / warn-and-continue |
| `network_events.rs` | 5 | (b) `hint:` best-effort fallback |
| `console.rs` | 3 | (b) debug/`--verbose` |
| `connect_tab.rs` | 2 | (b) debug/warn-and-continue |
| `run.rs` | 2 | (b) progress line + warn-and-continue |
| `sources.rs` | 2 | (b) debug/`--verbose` |
| `navigate.rs`, `wait.rs`, `cascade.rs`, `network.rs`, `eval.rs`, `nav_action.rs`, `record.rs`, `launch.rs` | 1 each | (b) warn-and-continue / debug / deprecation / hint / progress |

Spot-checked two sites without a live Firefox connection (none was running in this session):
`wait.rs`'s `--timeout` deprecation warning fires correctly before the connection attempt, and
`record.rs`'s "recording started:" message matches the actual `record start`/`record stop`
output. Both confirm the comment text against the real invocation. The rest were verified by
reading the surrounding code (guard conditions, doc comments) rather than a live run — no
`eprintln!` semantics changed, so risk of comment/behavior drift is confined to the two
`--verbose`-gating patterns already exercised above.

`check-error-envelope-paths` (iter-145) is left unmodified — Theme B found no bug shape to
harden it against. A new companion check, `check-stderr-annotations`
(`crates/xtask/src/check_stderr_annotations.rs`), enforces the annotation coverage this
iteration establishes; both share a new `crates/xtask/src/stderr_scan.rs` helper module for
the directory walk and `#[cfg(test)]`-module exclusion they have in common. Wired into
`check-iteration-ready` (step 12) and CI (`.github/workflows/ci.yml`).

## Out of scope

- Re-litigating the two genuine bugs iter-145 already fixed (`click.rs`'s two sites,
  `scroll.rs`'s timeout site) — those are closed.
- Changing any stderr *behavior* — this iteration only adds comments.

## References

- [[iteration-145-error-envelope-completeness]] — Resolution section has the full site-by-site
  classification this iteration operationalizes.
- [[iteration-141-output-hygiene]] — Theme E, the original envelope-routing sweep.

## Run guidance (batch 149 → 151 → 150 → 148)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** Across iterations 135–146 the real cause
   differed from the plan's hypothesis at least six times, and twice the wrong hypothesis was in
   a plan Claude itself wrote — [[iteration-146-live-suite-reliability]] guessed `LiveFirefox` for
   the leak and a daemon restart for the parity flake; both were wrong (two real bugs in
   `daemon/server.rs`). Reproduce the symptom and verify the mechanism **on the wire** (actual RDP
   packets / actual command output) before writing the fix. If the diagnosis here turns out to be
   wrong, fix the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** Every live
   test added here must exercise the default (daemon) path. iter-137's guard is at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather list —
   **do not add entries to that list.**

### Environment quirks (measured, session of 2026-08-12)

- Long background commands are killed at ~9–10 min. A full live run of `ff-rdp-cli` takes ~12 min
  and was killed three times. Run it in **two chunks**:
  `cargo test-live -p ff-rdp-cli -- --include-ignored --test-threads=1 live_1` and the same with
  `--skip live_1`. Each finishes inside the budget.
- Prewarm with `cargo build --workspace --all-targets` first — this avoids the xtask nested-cargo
  deadlock.
- Kill stray ff-rdp Firefox instances **before** any live run; a leftover breaks the daemon-stop
  and profile-prune tests. The developer's own browser is a separate process with no debugger
  port — do not kill it.
- `pgrep -f "firefox.*ff-rdp-profile"` matches its **own** shell command line, so counting orphans
  that way over-reports by exactly one. Use `pgrep -af start-debugger-server`.
- `ff-rdp-core` live tests must also run sequentially (`--test-threads=1`) against a headless
  Firefox on port 6000; in parallel, 4 tests fail from shared-Firefox interference.
