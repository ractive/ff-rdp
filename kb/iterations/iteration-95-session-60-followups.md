---
title: "Iteration 95: session-60 follow-ups — daemon stop process-group kill, cascade computed-field population, doctor staleness check"
type: iteration
date: 2026-06-01
status: completed
branch: iter-95/session-60-followups
depends_on:
  - iteration-94-session-59-polish-bundle
firefox_refs: []
kb_refs:
  - kb/dogfooding/dogfooding-session-60.md
  - kb/iterations/iteration-94-session-59-polish-bundle.md
first_call_sites:
  - primitive: daemon stop escalates to process-group SIGKILL on child-process port retention
    site: crates/ff-rdp-cli/src/daemon/client.rs
  - primitive: cascade `--prop` populates `computed` from the same source `ff-rdp computed` uses
    site: crates/ff-rdp-cli/src/commands/cascade.rs
  - primitive: doctor check that the running binary's embedded SHA matches origin/main HEAD
    site: crates/ff-rdp-cli/src/commands/doctor.rs
dogfood_script: iteration-95-session-60-followups.dogfood.sh
tags:
  - iteration
  - polish
  - daemon
  - cascade
  - doctor
  - dogfood-60
---

# Iteration 95 — session-60 follow-ups

Three small, independent fixes surfaced by [[dogfooding-session-60]]
that were each "ah, that's the next bug" while verifying iter-92/93/94.
Bundled because all are < 1 day of work with sharp tests, all are
polish-class (not behavior changes that ripple), and none depend on
each other.

The headline session-59 issues are already closed by iter-92/93/94.
This plan cleans up what session 60 *learned while verifying* those.

## Themes

### A. `daemon stop` port still held after SIGKILL on multi-child Firefox (session-60 §1, partial close of iter-94 A)

iter-94 Theme A bumped the wait to 8s and added SIGTERM→SIGKILL
escalation on the parent Firefox pid. Session 60 found that on a real
MDN-connected Firefox, the escalation message fires *and* the port
remains held — Firefox's content/GPU/RDD child processes inherit the
listening socket via FD inheritance, and killing only the parent
doesn't release it. The escalation is correct as far as it goes; it
just doesn't go far enough.

The fix: target the **process group**, not the single pid. On Unix,
`kill(-pgid, SIGKILL)` reaps the parent and every descendant in one
syscall.

### B. `cascade --prop` `computed` field is null for properties that *do* have computed values (session-60 §2)

`cascade h1 --prop color` on MDN returns `computed: null, rules: []`
even though `computed h1 --prop color` returns `rgb(0, 0, 0)`. The
two surfaces use different code paths to populate the computed value;
the `cascade` path bails too early for some property/element shapes.

Side effect: iter-94 Theme C's `inherited_or_default` note only fires
when `computed != null`, so this bug *also* blocks the note from
surfacing for a large class of properties. Fixing this here closes
both gaps.

### C. `ff-rdp doctor` staleness check (session-60 §3)

The version string already embeds the build's git SHA
(`ff-rdp 0.2.0 (9ecf105b8050 2026-06-01)`). When `doctor` is run from
within a clone of the repo and the installed binary's SHA differs
from `git rev-parse HEAD`, emit a clear warning so dogfooding sessions
don't waste the first hour against a stale binary. Session-60 lost ~30
min to this exact failure mode.

## Pre-fix repro

Each theme ships a `pre_fix_repro_*` test:

- A: `pre_fix_repro_daemon_stop_kills_process_group_on_port_retention`
  — fixture: a tiny binary that forks 3 children, all bound to the
  test port (e.g. via FD inheritance). Assert `daemon stop` reaps the
  whole group within 8s and the port is free.
- B: `pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does`
  — table-driven: for `h1 color`, `body background-color`,
  `p font-size` on a fixture page, assert `cascade … --prop X` and
  `computed … --prop X` return identical computed values.
- C: `pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head`
  — fixture: copy the binary to a tmp dir, then `cd` into a git repo
  at a different SHA; assert `doctor` JSON includes a check named
  `binary_staleness` with status `warn` and a hint mentioning
  `cargo install`.

## Hard rule

Three themes. Each must land its pre-fix repro + unit test before the
AC checkbox ticks. **No theme cross-talk** — Theme B touches `cascade`
output shape; Theme A touches process management; Theme C touches
`doctor` output. If any theme breaks the others, the fix belongs in a
separate iter.

## Tasks

### Theme A — daemon stop process-group kill [4/4] [pre_fix_repro_test: pre_fix_repro_daemon_stop_kills_process_group_on_port_retention]

- [x] In `crates/ff-rdp-cli/src/daemon/client.rs`, after SIGKILL on
      the parent pid fails to free the port within the remaining
      budget, escalate to process-group kill via `kill_process_tree`
      (which uses `libc::getpgid` captured before the escalation
      ladder). Uses `libc::getpgid` directly (not `nix`) per crate constraints.
- [x] Surface the escalation step in the error message: `"after
      SIGTERM+SIGKILL on pid + SIGKILL on pgid, port still listening"`
      so a future failure is debuggable. Implemented in
      `port_still_listening_after_escalation_msg(pid, port, pgid_killed=true)`.
- [x] Windows fallback: `taskkill /F /T /PID <pid>` added in
      `kill_process_tree` in `process.rs`. Documented at the call site
      that Unix (killpg) and Windows (taskkill /T) target the same
      conceptual "kill the whole tree".
- [x] dogfood_script Theme A: navigate to a fetch-triggering page,
      `daemon stop`, assert port freed within 3 s and call completes
      within 15 s. Script at `iteration-95-session-60-followups.dogfood.sh`.

### Theme B — cascade `--prop` populates `computed` field [3/3] [pre_fix_repro_test: pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does]

- [x] In `crates/ff-rdp-cli/src/commands/cascade.rs`, find the
      branch that emits the `computed: null` result and trace why it
      diverges from `commands/computed.rs`. Hypothesis: cascade
      currently extracts `computed` from the matched-rules query
      result (which is empty for inherited properties), rather than
      from a separate `getComputedStyle` query. Wire the same
      `getComputedStyle` call the standalone `computed` command uses
      and populate `computed` from its output.
- [x] Land `pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does`
      as a live test parameterized over three property/element shapes
      (inherited prop, default-valued prop, author-rule-set prop).
- [x] `unit_cascade_computed_field_matches_computed_command_table_driven`:
      table fixture with the JSON responses both surfaces should
      produce; assert byte-for-byte equality of the `computed` field.

### Theme C — doctor binary-staleness check [3/3] [pre_fix_repro_test: pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head]

- [x] In `crates/ff-rdp-cli/src/commands/doctor.rs`, add a new check
      named `binary_staleness`. Read the embedded build SHA from the
      version string (or a `const` if it isn't already split out).
      Spawn `git rev-parse HEAD` in CWD; if both succeed and differ,
      emit `status: warn` with hint `cargo install --path
      crates/ff-rdp-cli`. If `git` isn't available, or CWD isn't a
      repo, or the SHAs match, emit `status: ok`. Never `fail` — this
      check is informational, not a gate.
- [x] Land `pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head`
      using a synthetic fixture (build a no-op binary with a known
      embedded SHA, `cd` into a tmp repo at a different SHA, invoke).
- [x] dogfood_script Theme C: in the repo, `cargo install --path
      crates/ff-rdp-cli` once, then `git checkout HEAD~1 -- .` (or
      similar SHA-changing op without modifying the binary), then
      `ff-rdp doctor`; assert the JSON includes
      `{"name":"binary_staleness","status":"warn"}`.

## Acceptance Criteria [9/10]

- [x] `pre_fix_repro_daemon_stop_kills_process_group_on_port_retention`:
      multi-child fixture; daemon stop frees the port within 10s. [test: pre_fix_repro_daemon_stop_kills_process_group_on_port_retention in client.rs — spawn sleep+setsid, capture pgid via get_process_group_id, kill_process_tree frees port within 5s; #[ignore] #[cfg(unix)]]
- [x] `unit_daemon_stop_uses_killpg_when_kill_pid_fails`: mocked
      escalation ladder; assert `killpg` is invoked when the post-
      SIGKILL port check still finds the port held. [test: unit_daemon_stop_uses_killpg_when_kill_pid_fails in client.rs — EscalationHooks with real TcpListener, TREE_KILL_CALLED AtomicBool; asserts kill_process_tree hook called and msg contains "pgid"]
- [x] `live_daemon_stop_on_mdn_headless`: ignored-by-default
      (`FF_RDP_LIVE_NETWORK_TESTS=1`); covers the session-60 §1
      reproducer (launch → navigate MDN → daemon stop → port free). [test: live_daemon_stop_on_mdn_headless in live_daemon_stop_mdn.rs — LiveFirefox headless, navigate MDN, daemon stop < 15s, port free after]
- [x] `pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does`:
      live test on three property/element shapes; `cascade --prop X`
      and `computed --prop X` agree. [test: pre_fix_repro_cascade_prop_populates_computed_when_standalone_computed_does in live_95_cascade_computed_agreement.rs — three (selector,prop) pairs; cascade computed agrees byte-for-byte with standalone computed]
- [x] `unit_cascade_computed_field_matches_computed_command_table_driven`:
      response fixture; byte-equal computed field. [test: unit_cascade_computed_field_matches_computed_command_table_driven in cascade.rs — build_external_computed_js contains JSON_SENTINEL + getComputedStyle for h1/color, body/background-color, p/font-size]
- [x] `live_cascade_inherited_or_default_note_fires_on_h1_color`:
      with the cascade-computed fix, iter-94 Theme C's note now fires
      for the property where session-60 §2 showed it didn't. [test: live_cascade_inherited_or_default_note_fires_on_h1_color in live_95_cascade_computed_agreement.rs — cascade computed non-empty AND equals standalone computed]
- [x] `pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head`:
      synthetic fixture; JSON check shape verified. [test: pre_fix_repro_doctor_warns_when_installed_sha_differs_from_head in doctor.rs — asserts status==Warn, both SHAs in detail, hint contains `cargo install`]
- [x] `unit_doctor_binary_staleness_check_short_circuits_outside_repo`:
      run from `/tmp`; `binary_staleness` check returns `ok` (or
      `skipped`), not `warn`, not `fail`. [test: unit_doctor_binary_staleness_check_short_circuits_outside_repo in doctor.rs — Err(()) head_sha_result yields Pass]
- [x] `unit_doctor_binary_staleness_check_short_circuits_without_git`:
      mock `git` returning non-zero exit; check returns `ok`/`skipped`. [test: unit_doctor_binary_staleness_check_short_circuits_without_git in doctor.rs — asserts Pass + detail mentions "skipped" or "git"]
- [ ] `dogfood_script_full_run_iter_95`: the sibling `.dogfood.sh`
      exits 0 and writes `/tmp/ff-rdp-iter-95-dogfood-ok`.

## Out of scope

- **Pre-flight `cargo install` in the dogfood skill.** Session-60 §3
  named this as a mitigation. The `doctor` warning closes the loop
  with less invasiveness; if it doesn't, a follow-up can add an
  install gate. Skill changes can't ride ralph-loop anyway (per
  CLAUDE.md), so coupling them to a Rust iteration is awkward.
- **`daemon stop --force` flag** (escalate without waiting). The
  process-group kill is already the "go nuclear" path; an opt-in flag
  would just gate something that should always happen.
- **`computed` command refactor** to share its actor query path with
  cascade in a shared module. Theme B can do the minimal "have
  cascade call the same RDP path" without extracting a shared module.
- **Multi-frame cascade** — today both `cascade` and `computed`
  implicitly hit the top-level frame; cross-frame is its own iter.

## References

- [[dogfooding-session-60]]
- [[iteration-94-session-59-polish-bundle]] (Theme C precondition this
  unblocks)
