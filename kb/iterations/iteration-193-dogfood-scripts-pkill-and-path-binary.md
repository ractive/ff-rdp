---
title: "Iteration 193: checked-in dogfood scripts pkill other agents' Firefox and drive a bare `ff-rdp` from PATH"
type: iteration
date: 2026-08-23
status: in-review
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

### A. Scoped teardown [2/2]
- [x] Each script tears down only the browser it started — `dogfood_launch` records the port and
      pid, `dogfood_teardown` (EXIT trap installed by `dogfood_init`) stops exactly those
- [x] No remaining unscoped `pkill -f` in `kb/iterations/*.dogfood.sh` (14 → 0)

### B. Binary under test [1/1]
- [x] No checked-in dogfood script resolves `ff-rdp` from PATH (14 → 0); every call goes through
      `ffrdp`, which runs the binary `dogfood_init` built from this tree

### C. Lint rules [2/2]
- [x] `unscoped-pkill` rule with a bad fixture (`unscoped-pkill-bad.sh`) and a passing good
      fixture (`unscoped-pkill-good.sh`)
- [x] `path-binary` rule with a bad fixture (`path-binary-bad.sh`) and a passing good fixture
      (`path-binary-good.sh`)

## Acceptance Criteria [4/4]

- [x] `unit_lint_dogfood_script_flags_unscoped_pkill`: a fixture containing
      `pkill -f 'firefox.*ff-rdp-profile'` fails the linter, naming the rule
- [x] `unit_lint_dogfood_script_flags_path_binary`: a fixture invoking a bare `ff-rdp` fails the
      linter, naming the rule
- [x] One migrated script is executed for real via
      `FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script <plan>` while a second
      unrelated Firefox is running, and that second browser is still alive afterwards.
      Done on 2026-08-24 with a control Firefox launched on port 6000 into the *shared*
      user-level profile root (`ff-rdp-profile-*`, i.e. the exact process the old
      `pkill -f 'firefox.*ff-rdp-profile'` matched — confirmed with `pgrep` before the run), plus
      the user's own Firefox. Six migrated scripts were executed: 90, 92, 96, 97, 98, 103. All
      reported `check-dogfood-script: OK`; the control browser and the user's browser were both
      still alive afterwards, `~/Library/Application Support/ff-rdp/profiles` was byte-for-byte
      unchanged across the two `profiles prune --all` scripts (96, 97), and no private per-run
      `$FF_RDP_HOME` or Firefox was left behind.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome

Sixteen scripts now share `kb/iterations/dogfood-lib.sh`. `dogfood_init` does three things,
each of which removes one way a run could reach outside itself:

1. `$FF_RDP_HOME` is pointed at a private per-run directory. Profile root, daemon registry and
   connection records all follow that override. This was not in the plan and turned out to be
   load-bearing: iterations 96 and 97 run `profiles prune --all`, which before this change swept
   the shared user-level root and deleted profiles belonging to whatever Firefox another agent
   had running — a wider blast radius than the `pkill` this plan was written about.
2. `dogfood_free_port` replaces the hardcoded 6000/6001/6003. A run that adopts a port it did
   not open cannot tell its own browser from a sibling's, however careful its teardown is.
3. `trap dogfood_teardown EXIT` stops the recorded ports and then the recorded pids, and removes
   the private home. `dogfood_on_exit` exists so a script needing extra cleanup does not install
   a second `trap … EXIT` and silently replace the teardown — 93 needed exactly that.

Also folded in, same family, not in the plan: 87 and 91 (the two scripts that had neither defect)
wrote to fixed `/tmp/iter87-*.out` / `/tmp/iter91-run*.out` paths, which are shared between
concurrent runs in the same way the pre-184 sentinel was. Both now use a per-run `mktemp -d`.

Rationale is recorded as `[[decision-log]]` DEC-046.

### Observed during the live sweep, not fixed here

Iteration 97's Theme C (`profiles prune --all` must list a live-owner profile in `removed_live`)
failed on its first live run and passed on the second, with no code change in between —
`removed_live` came back empty, which means `profile_is_owned_by_live_process` read the launched
Firefox as not-live at that moment. A hand repro of the same sequence outside the gate passed.
This is a pre-existing flake in that script's Theme C assertion, not a regression from this
iteration's migration (nothing here touches the liveness markers), and it is left unfixed and
unreworded rather than softened into an assertion that would always pass.

## Out of scope

The sentinel contract itself — [[iteration-184-dogfood-sentinel-is-a-shared-tmp-path]] owns it and
has landed. The `~/.ff-rdp` file growth is owned by iteration 186.

## References

- [[iteration-184-dogfood-sentinel-is-a-shared-tmp-path]] — where this surfaced, and why its own
  live verification stopped at the gate rather than the scripts
- [[iteration-203-live-sweep-watch-conditions-third-holder]] condition 14 — three hand-started
  port-6000 Firefoxes from earlier iterations were found still alive at the start of iteration 192,
  left behind by the `iteration-close` skill's "start one, never told to stop it" instruction. Not
  the same defect as this plan's (that one is a checked-in script's blanket `pkill`; this is a
  skill instruction with no matching teardown step), but the same family — a process this run
  didn't start, still running, because nothing scoped its lifetime to the run that started it.
  Worth a shared glance when doing Theme A here. iteration 192's carry-over sweep (2026-08-24)
  reviewed this plan and placed no other items here — the rest of 192's carry-over is domain-
  specific to the live-sweep watch conditions and landed entirely in 203
