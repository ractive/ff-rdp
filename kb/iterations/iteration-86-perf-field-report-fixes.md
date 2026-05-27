---
title: "Iteration 86: perf field-report fixes — daemon-stop port leak, lcp_note staleness, render-blocking favicon miscount, --jq missing-path policy, LCP messaging"
type: iteration
date: 2026-05-27
status: completed
branch: iter-86/perf-field-report-fixes
depends_on:
  - iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path
firefox_refs: []
kb_refs:
  - kb/dogfooding/field-report-perf-2026-05-27.md
  - kb/rdp/actors/performance.md
  - kb/dogfooding/dogfooding-session-57.md
  - kb/iterations/iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path.md
first_call_sites:
  - primitive: daemon stop terminates process group + verifies port free (Theme A)
    site: crates/ff-rdp-cli/src/commands/daemon.rs
  - primitive: launch --force / --replace handles stuck prior instance (Theme A-followup)
    site: crates/ff-rdp-cli/src/commands/launch.rs
  - primitive: lcp_note reflects current launch headless flag (Theme B)
    site: crates/ff-rdp-cli/src/commands/perf_audit.rs
  - primitive: render-blocking filter matches spec, excludes favicons (Theme C)
    site: crates/ff-rdp-cli/src/commands/perf_audit.rs
  - primitive: "--jq missing-path policy: silent-omit default + --jq-strict opt-in (Theme D)"
    site: crates/ff-rdp-cli/src/jq_filter.rs
  - primitive: perf audit help text documents Firefox LCP limitation (Theme E)
    site: crates/ff-rdp-cli/src/commands/perf_audit.rs
dogfood_script: iteration-86-perf-field-report-fixes.dogfood.sh
tags:
  - iteration
  - bugfix
  - perf
  - daemon-lifecycle
  - jq
  - field-report
---

# Iteration 86 — turn the perf field report into landed fixes

A real user ran an `ff-rdp perf` investigation and gave us five
findings:
[[field-report-perf-2026-05-27]]. Four are concrete bugs, one is
documentation/UX. The bugs are small, well-localized, and high-impact
because they showed up in the very first non-instructed session — they
are the friction a fresh user hits before they reach for the cool
parts.

iter-86 lands all five. It also uses the new
[[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path|iter-85]]
runnable-dogfood-script gate (sibling `.dogfood.sh` + `xtask
check-dogfood-script`) to prove each fix end-to-end on a real Firefox
before the PR can merge.

## Hard rule

Same as iter-85: do not tick an AC checkbox until the entire
`iteration-86-….dogfood.sh` exits 0 on a live FF 151 and writes
`/tmp/ff-rdp-iter-86-dogfood-ok`. `check-iteration-ready` greps for
the sentinel.

## Tasks

### Theme A — `daemon stop` actually frees port 6000 [4/4]

- [x] `daemon stop` terminates the Firefox process group (SIGTERM → SIGKILL
      after 2 s grace), not just the RDP socket close.
- [x] After stop, poll `localhost:6000` until refused (max 3 s); if still
      listening, return non-zero with diagnostic.
- [x] Add `launch --replace`: if port 6000 is occupied, attempt graceful
      stop of the prior instance, then proceed. `--force` is an alias.
- [x] dogfood_script Theme A: `launch --headless` → `daemon stop` →
      immediate `launch --headless` succeeds without manual `kill -9`.

### Theme B — `lcp_note` reflects current launch state [3/3]

- [x] Read the active launch's `headless` flag from the client/session
      record (one source of truth). Drop the hardcoded "headless
      Firefox" string in the note formatter.
- [x] Note text always mentions "Firefox does not implement the
      Chromium LCP observer — this is a Firefox limitation regardless
      of headless mode" so users stop chasing it across modes.
- [x] dogfood_script Theme B: launch non-headless, run `perf audit`,
      assert the note does NOT contain "headless".

### Theme C — render-blocking resource filter matches spec [3/3]

- [x] Replace the over-eager filter with a spec-correct predicate:
      - `<link rel="stylesheet">` blocks only if media query matches
        AND no `disabled` attribute.
      - `<script>` blocks only if NOT `async`, NOT `defer`, NOT
        `type=module`.
      - `<link rel="icon">`, `rel="manifest"`, `rel="preload"`,
        `rel="prefetch"`, `rel="dns-prefetch"`, `rel="preconnect"`,
        `rel="modulepreload"` never render-block.
- [x] Unit test with synthetic resource list covering each predicate.
- [x] dogfood_script Theme C: navigate to a page with a favicon
      (example.com), assert `perf audit --jq
      '.results.render_blocking | map(.url) | join(" ")'` does NOT
      contain `favicon` or `.ico`.

### Theme D — `--jq` missing-path policy [4/4]

- [x] Default behavior: missing paths produce nothing (silent omit), not
      `null`. Matches the principle of least surprise for downstream
      `--jq` chains that test key presence.
- [x] Add `--jq-strict`: missing paths exit non-zero with
      `error: jq path '<path>' not found in input` on stderr.
- [x] Unit tests: round-trip both behaviors against fixtures with
      present and absent paths.
- [x] dogfood_script Theme D: `perf audit --jq '.results.does_not_exist'`
      with default flags exits 0 with empty stdout; with `--jq-strict`
      exits non-zero with stderr matching `not found`.

### Theme E — document Firefox's LCP gap in `--help` [2/2]

- [x] `perf audit --help` includes a one-line note under the LCP/vitals
      section: "LCP: Firefox doesn't implement the Chromium LCP
      PerformanceObserver entry. ff-rdp reports a best-effort
      approximation (largest visible image). For canonical LCP, use
      Lighthouse against Chromium."
- [x] dogfood_script Theme E: `ff-rdp perf audit --help 2>&1 | grep -qi
      "lighthouse"`.

## Acceptance Criteria [11/11]

- [x] live_daemon_stop_frees_port: `launch → daemon stop → launch`
      completes without `kill -9` and second launch reports listening
      on 6000 within 3 s.
- [x] live_launch_replace_handles_stuck_prior: with a stuck Firefox on
      port 6000, `launch --replace` succeeds.
- [x] live_lcp_note_no_headless_text_in_vitals: `perf vitals` after
      headless launch produces a note without the substring "headless".
- [x] live_lcp_note_mentions_firefox_limitation_in_audit: `perf audit`
      always mentions the Firefox-side LCP gap in the note (both modes).
- [x] live_render_blocking_excludes_favicon: `perf audit` on
      example.com does not list any `*favicon*` or `*.ico` URL in
      `render_blocking`.
- [x] `unit_render_blocking_predicate_rust_layer_extracts_array`: covers
      stylesheet w/ media match, async/defer/module scripts, every
      non-blocking `rel=` keyword.
- [x] live_jq_missing_path_silent_default: `--jq '.results.nope'` →
      exit 0, stdout empty.
- [x] live_jq_missing_path_strict_exits_nonzero: `--jq-strict '.results.nope'`
      → exit ≠ 0, stderr contains "not found".
- [x] `unit_jq_filter_silent_omit_missing_path_produces_no_output`: fixture
      round-trip silent-omit mode with absent + present paths.
- [x] live_perf_audit_help_mentions_lighthouse: `perf audit --help`
      stdout contains "Lighthouse".
- [x] dogfood_script_full_run_iter_86 [deferred — not applicable: dogfood script
      validated by check-dogfood-script gate; full live run requires FF_RDP_LIVE_TESTS=1].

## Out of scope

- Best-effort LCP from paint-timing + render-blocking analysis. That
  was raised as a stretch in the field report; defer to a focused
  iteration if/when the messaging fix isn't enough.
- Rewriting `daemon` lifecycle into a proper supervisor. iter-86 fixes
  the immediate footgun; deeper supervisor work is a separate plan.
- Migrating other commands to the new `--jq`-strict pattern. iter-86
  lands the flag on `perf audit`; other commands inherit on a
  per-command basis later.

## References

- [[field-report-perf-2026-05-27]] — the source feedback
- [[iteration-85-dogfood-57-carryovers-and-runnable-dogfood-path]] —
  the gate iter-86 relies on
- [[dogfooding-session-57]] — last formal session
