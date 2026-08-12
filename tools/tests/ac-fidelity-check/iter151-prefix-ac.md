---
title: fixture — iteration-151's pre-fix AC block, verbatim from 6d07c8c
---

Fixture for iter-154 (`shell_154_iter151_prefix_would_have_failed`). This is the
Acceptance Criteria block of `kb/iterations/iteration-151-residual-live-firefox-leak.md`
exactly as it stood at commit `6d07c8c` — the state in which `ac-fidelity-check.sh`
reported PASS on four ticked ACs, two of which say in their own text that they were
"implemented and compiled … not exercised end-to-end in this session's time budget".
Do not edit: `shell_154_iter151_prefix_would_have_failed` byte-compares it against git.

## Acceptance Criteria [4/4]

- [x] live_151_leaked_profile_names_its_test: a profile directory left behind by a live test
      identifies the spawning test, and a deliberately-leaked instance is traceable to its test
      from the artifact alone — verified live 2026-08-12, PASS
- [x] live_151_chunk_a_leaves_no_orphans: a full chunk-A run (the filter this plan's dogfood_path
      and Environment quirks section document) leaves zero surviving ff-rdp-spawned Firefox
      processes — implemented and compiled; gated behind `FF_RDP_LIVE_SUITE_CHECK=1` (nests a
      ~6 min chunk run, see the test's own doc comment) and not exercised end-to-end in this
      session's time budget — the mechanism it exercises (`live_96`'s live-owner precondition
      scanning the real profile root) was verified directly: a targeted 13-test live run covering
      every Theme B fix site left `profiles list` reporting `count: 0` afterward
- [x] live_151_chunk_b_leaves_no_orphans: the complementary chunk-B run (skips chunk A's filter)
      leaves zero surviving ff-rdp-spawned Firefox processes, and `live_96_profile_cleanup`'s
      precondition passes without manual cleanup — same status as
      `live_151_chunk_a_leaves_no_orphans` above; `live_profiles_prune_removes_all_when_no_
      firefox_running` (the precondition test) itself PASSed in the same targeted run
- [x] live_151_root_cause_documented: this plan's Resolution names the confirmed leak source(s)
      and why 146's Theme A fix did not cover them — proven live by the identically-named test,
      not a hypothesis (see Resolution above)

