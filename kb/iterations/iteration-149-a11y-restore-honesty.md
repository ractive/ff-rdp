---
branch: iter-149/a11y-restore-honesty
date: 2026-08-12
depends_on:
  - kb/iterations/iteration-143-native-a11y-tree.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://example.com --port 6100
  ff-rdp a11y --native --port 6100 --jq '.meta'
  # → when ff-rdp enabled the platform accessibility service and could not
  #   restore it, meta must say so; the browser is left in a degraded state
  #   and a non-verbose caller currently has no way to learn that
first_call_sites: []
status: completed
title: "Iteration 149: a11y --native must report a failed service restore"
type: iteration
tags:
  - iteration
---

# Iteration 149: `a11y --native` must report a failed service restore

Follow-up to [[iteration-143-native-a11y-tree]]. Found by reading the merged diff of that
iteration (`aeca4d0`) during the 134–146 batch, not in a dogfooding session — evidence inline.

## Why this exists

[[iteration-143-native-a11y-tree]] added the opt-in `--native` flag per [[decision-log]]
DEC-027: it enables Firefox's platform accessibility service, walks the native tree, and
restores the prior state afterwards. The enable path is careful and honest — it refuses to walk
the tree if `bootstrap()` still reports the service disabled, rather than silently falling back.

The **restore** path is not. `crates/ff-rdp-cli/src/commands/a11y.rs`:

```rust
if we_enabled {
    // Best-effort restore: report a failure but don't let it mask the
    // primary result. On Windows an active screen reader can block
    // `disable` (kb/rdp/actors/accessibility.md) — that's an expected
    // limitation, not a bug in ff-rdp.
    if let Err(e) = AccessibilityActor::disable_service(ctx.transport_mut(), &parent_actor) {
        if cli.is_verbose() {
            eprintln!("debug: --native: failed to restore the accessibility service ...");
        }
    }
    // ...
}
```

and at the call site:

```rust
let (tree, _we_enabled) = run_native_opt_in(...)?;
```

So when `disable()` fails, ff-rdp leaves Firefox's accessibility service **enabled
browser-wide for the remaining life of the process** and the default JSON output says nothing.
The caller learns about it only by passing `--verbose` and reading stderr. The function computes
exactly the fact a caller needs (`we_enabled`) and the call site discards it with an underscore.

The comment is right about the cause and wrong about the consequence. A screen reader blocking
`disable()` genuinely is an expected platform limitation and not an ff-rdp bug — but an expected
limitation still has to be *reported*. The user-visible outcome is a browser that is slower for
every other tab, caused by a command that looked read-only, with no trace in its output.

**This contradicts iteration-143's own Theme A.** Theme A exists because "a caller cannot tell
[the native and JS trees] apart except by `--verbose` stderr" was judged unacceptable for tree
provenance. The same iteration then reproduced that exact pattern for a side effect that
outlives the command. DEC-027's argument was that a query command must not silently mutate
browser-global state; leaving it mutated *and* not saying so is the failure mode that decision
was written to prevent.

Same class as iter-125's false-good LCP, iter-139's fabricated CLS, and iter-141's
bare-stderr JS exceptions: the JSON envelope stops telling the truth precisely when something
went wrong.

## Themes

### Theme A — surface the failed restore in the envelope

Add a `meta` field (e.g. `service_left_enabled: true` with `service_restore_error: "<reason>"`)
whenever ff-rdp enabled the service and could not restore it. Follow iter-128's
always-present-nullable-key convention if that reads better here — decide once and state it in
the plan, rather than making the key's presence itself the signal in a way callers must
special-case.

Keep the existing behaviour that a restore failure does not mask the primary result: the tree
was walked successfully and must still be returned. This is an additional fact about the
envelope, not an error.

### Theme B — stop discarding the signal

`let (tree, _we_enabled) = ...` throws away the value Theme A needs. Thread it (or a richer
restore-outcome type) to the envelope construction.

Prefer a small enum over a bare `bool` — `RestoreOutcome::{NotNeeded, Restored, Failed(String)}`
carries the three real cases without the caller having to infer "we didn't enable it" from
`false`.

### Theme C — say it on stderr too, unconditionally

The `--verbose`-gated `eprintln!` should fire regardless of verbosity when the restore *failed*.
A human running the command interactively should not have to opt in to learning that their
browser was left degraded. Keep the success-path notice verbose-gated — that one is noise.

## Acceptance Criteria [4/4]

- [x] live_149_restore_failure_reported_in_meta: when `disable_service` fails after ff-rdp
      enabled the service, the JSON envelope carries the left-enabled signal and the reason,
      and the walked tree is still returned in `results`
- [x] live_149_successful_restore_reports_clean: a normal `--native` run that restores the
      service reports no left-enabled signal, and the service is observably disabled afterwards
- [x] live_149_service_already_on_is_not_touched: when the service was already enabled before
      the command, ff-rdp neither disables it nor claims to have left it enabled
- [x] unit_149_restore_outcome_maps_to_meta: each `RestoreOutcome` variant maps to the intended
      envelope shape, including the not-needed case

## Implementation findings (iter-149, corrections to the plan above)

Per the "do not trust the root cause" rule below, two things stated or implied above turned out
wrong on verification. Both were caught by writing the live tests against real Firefox, not by
reasoning about the code.

1. **`a11y` has no daemon-routed path to test.** `run()` in `crates/ff-rdp-cli/src/commands/a11y.rs`
   calls `connect_direct(cli)` unconditionally — the same one-shot-direct family as `screenshot`,
   `cookies`, `storage`, `sources`, `computed`. There is no "default daemon path" for this command
   to additionally exercise; every live test here already takes the only connection path `a11y`
   has. `live_149_a11y_restore_honesty.rs`'s module doc records this so nobody re-litigates it.
2. **A failed restore does not leave the service enabled "for the remaining life of the process"**
   (the wording used above, and in the original code comment this iteration replaced). Verified
   live: Firefox tears the platform accessibility service back down once the RDP connection that
   enabled it disconnects, independent of whether the explicit `disable()` call succeeded. Because
   `a11y --native` makes one short-lived direct connection per invocation, a failed restore's
   window is really just "until this command's process exits" (milliseconds), not an indefinite
   browser-wide slowdown — unless a caller reuses the connection (e.g. an embedding), in which case
   it genuinely does persist. `live_149_service_already_on_is_not_touched` had to open a *second*,
   independently-held connection (`hold_service_enabled`) to construct a genuine "already enabled"
   precondition, because leaving one CLI invocation's own connection "enabled" does not survive
   into the next invocation. Production wording (error message, `--help`, doc comments) was
   corrected to match; `kb/rdp/actors/accessibility.md` Gotchas updated. The `meta.service_left_enabled`
   /`meta.service_restore_error` fields are still worth reporting — the failure is real and the
   window, however short, is real — just not the hazard originally hypothesized.

Actor-boundary fault injection (used for `live_149_restore_failure_reported_in_meta` and
`live_149_service_already_on_is_not_touched`, per the Notes below): `run_native_opt_in` reads
`FF_RDP_A11Y_FORCE_RESTORE_FAILURE=1` and, only when set, targets the *restore* call at a
deliberately-invalid actor ID instead of the real `parentAccessibilityActor`. `enable_service` is
untouched by the flag, so the service really is enabled; the restore call still goes out over the
wire and Firefox genuinely answers with a `noSuchActor`-style error — a real protocol failure, not
a mocked one. Not documented in `--help`; test-only.

## Notes

- Forcing a real `disable()` failure on macOS/Linux is the hard part of Theme A's live test. If
  it cannot be induced against real Firefox, inject at the actor boundary rather than skipping
  the AC — and say so in the AC text. Do not tick a live AC that was only exercised by a mock.
- Verify on the wire first, per the run guidance that has now corrected four plan hypotheses in
  this series (including two of mine in [[iteration-146-live-suite-reliability]]). Confirm the
  current silent-failure behaviour by observation before changing it.
- Every live test added here must exercise the **default daemon path**. Do not add entries to
  the shrink-only grandfather list in `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs`.

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
