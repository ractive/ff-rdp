---
title: "Iteration 193: checked-in dogfood scripts pkill other agents' Firefox and drive a bare `ff-rdp` from PATH"
type: iteration
date: 2026-08-23
status: planned
branch: iter-193/dogfood-scripts-pkill-and-path-binary
depends_on: [iteration-184-dogfood-sentinel-is-a-shared-tmp-path]
first_call_sites: []
dogfood_path: |
  # Carry-over from iteration 184's close. 184 could not run any checked-in
  # dogfood script live, and the reason is the defect this plan owns.

  # 1. Every checked-in script opens by killing every Firefox on the machine
  #    whose command line matches a pattern that is not scoped to this run:
  grep -n "pkill -f" kb/iterations/*.dogfood.sh
  #      kb/iterations/iteration-98-media-query-truthfulness.dogfood.sh:
  #        pkill -f 'firefox.*ff-rdp-profile' || true
  #    On a machine where several agents share one working tree — the normal
  #    case in this project's loop — that kills browsers the running script does
  #    not own, which is the exact orphaning the repo's own run-wide constraints
  #    forbid. It is also the self-matching pattern shape those constraints call
  #    out: built from `ff-rdp-profile`, it matches the checker itself; the
  #    documented safe form is `MacOS/firefox.*ff-rdp-profile`.

  # 2. Every checked-in script drives a bare `ff-rdp` off PATH:
  grep -c "^ff-rdp \|(ff-rdp " kb/iterations/*.dogfood.sh
  #    CLAUDE.md: "Dogfood steps must run via `cargo run -p ff-rdp-cli --` or a
  #    freshly installed binary, never a bare `ff-rdp` from PATH." A stale PATH
  #    binary makes the gate certify a build that is not the one under test —
  #    the same false-PASS shape 184 fixed one layer down.

  # 3. Neither is linted, so nothing stops the next script from repeating both:
  bash tools/lint-dogfood-script.sh kb/iterations/iteration-98-*.dogfood.sh
  #    → exits 0 today
tags: [iteration, tooling, dogfood, discipline-gates]
---

# Iteration 193: make a dogfood script safe to run on a shared machine

Carry-over from [[iteration-184-dogfood-sentinel-is-a-shared-tmp-path]]'s close. 184 moved the
sentinel to a per-run path so two gate runs stop colliding; it did **not** touch what the scripts
themselves do, and what they do makes them unrunnable when anything else is using the machine.

## The defect

Two independent hazards, each present in 14 of the 16 checked-in `kb/iterations/*.dogfood.sh`
(counted 2026-08-23: `grep -l "pkill -f"` → 14, `grep -lE '(^|[^-/[:alnum:]])ff-rdp '` → 14):

1. **`pkill -f 'firefox.*ff-rdp-profile'`** at the top (and often the bottom) of each script. The
   pattern is not scoped to the run, so it terminates every ff-rdp-profile Firefox on the host,
   including ones another agent or the user started. It is also built the way the repo's own
   guidance says not to build it — it matches the checker's own command line, so the paired
   `pgrep` reports phantoms.
2. **bare `ff-rdp` from PATH.** CLAUDE.md requires `cargo run -p ff-rdp-cli --` or a freshly
   installed binary. A script that resolves `ff-rdp` from PATH can certify a months-old build.

The consequence is concrete and already paid: iteration 184 changed the dogfood contract and could
not execute a single migrated script to prove the migration works end-to-end, because doing so
would have killed four sibling agents' browsers. The gate is only as good as the last time someone
dared run it.

## Themes

- **A — Own only your own browser.** Replace the blanket `pkill` with teardown scoped to the
  Firefox this script launched (its pid, or `ff-rdp daemon stop` on the port it opened), and use
  the `MacOS/firefox.*ff-rdp-profile` form wherever a pattern match is genuinely needed.
- **B — Run the binary under test.** Drive the CLI through `cargo run -p ff-rdp-cli --` (or a
  binary this script installed), not PATH.
- **C — Lint both.** `tools/lint-dogfood-script.sh` gains rules for the unscoped `pkill` and the
  bare `ff-rdp`, with good/bad fixtures, so a new script cannot reintroduce either.

## Tasks

### A. Scoped teardown [0/2]
- [ ] Each script tears down only the browser it started
- [ ] No remaining unscoped `pkill -f` in `kb/iterations/*.dogfood.sh`

### B. Binary under test [0/1]
- [ ] No checked-in dogfood script resolves `ff-rdp` from PATH

### C. Lint rules [0/2]
- [ ] `unscoped-pkill` rule with a bad fixture and a passing good fixture
- [ ] `path-binary` rule with a bad fixture and a passing good fixture

## Acceptance Criteria [0/4]

- [ ] `unit_lint_dogfood_script_flags_unscoped_pkill`: a fixture containing
      `pkill -f 'firefox.*ff-rdp-profile'` fails the linter, naming the rule
- [ ] `unit_lint_dogfood_script_flags_path_binary`: a fixture invoking a bare `ff-rdp` fails the
      linter, naming the rule
- [ ] One migrated script is executed for real via
      `FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script <plan>` while a second
      unrelated Firefox is running, and that second browser is still alive afterwards
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

The sentinel contract itself — [[iteration-184-dogfood-sentinel-is-a-shared-tmp-path]] owns it and
has landed. The `~/.ff-rdp` file growth is owned by iteration 186.

## References

- [[iteration-184-dogfood-sentinel-is-a-shared-tmp-path]] — where this surfaced, and why its own
  live verification stopped at the gate rather than the scripts
