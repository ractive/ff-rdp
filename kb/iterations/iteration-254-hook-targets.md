---
title: "Iteration 254: install-hook for Codex and OpenCode, against pinned schemas"
type: iteration
date: 2026-08-30
status: done
branch: iter-254/hook-targets
depends_on: [212]
first_call_sites: []
dogfood_path: |
  ff-rdp install-hook --codex --dry-run
  # prints the ~/.codex/hooks.json entry and the [features] hooks = true precondition
  ff-rdp install-hook --codex && ff-rdp install-hook --codex
  # second call: no-op, exit 0, hooks.json byte-identical
  ff-rdp install-hook --codex --uninstall
  ff-rdp install-hook --opencode --dry-run
tags: [iteration, cli, agent-ergonomics]
takeover_reconciliation_252: "2026-09-09: Codex0.153.4 published hooks default ON, while new/changed commands require trust via /hooks. Preserve the original explicit [features] hooks=true refusal AC as ff-rdp opt-in policy and describe it accurately; do not claim it is Codex's current default. hooks.SessionStart[].hooks[] command shape confirmed; matcher group permits ownership metadata in published schema, runtime verification still owed. OpenCode1.3.13 pinned b5b5f7e0190cdd5272b6d2aeb3d4589a822675a6 only supplies a JS/TS plugin contract, permitting ThemeC obsolete disposition. Evidence: https://learn.chatgpt.com/docs/hooks and https://opencode.ai/docs/plugins/; saved full source/schema in takeover hooks254-prep. Original tasks/AC text and ticks unchanged."
dogfood_script: iteration-254-hook-targets.dogfood.sh
outcome: "2026-09-12: Codex target implemented with a recorded input schema and actual Codex0.153.4 loader acceptance. Uses parsed explicit features.hooks=true installer opt-in, atomic managed-group merge, byte/mtime-preserving no-op, duplicate/shape/path repair, dry-run and uninstall. Malformed Codex containers refuse; Claude behavior stays unchanged. Shell-specific command encoding has argv/sentinel positive controls and mutation proof. Research: [[agent-hook-formats]]. Theme C is obsolete: pinned OpenCode1.3.13 only provides a JS/TS module contract; refusal remains and no installed OpenCode runtime was available."
closing_counts: >-
  A 2/2; B 3/3; C 1/1 (obsolete under the allowed pinned JS/TS-only contract,
  with required Outcome body section); Acceptance Criteria 5/5. Original task and AC
  wording preserved.
validation_notes: "Targeted positive controls:14 unit plus14 e2e tests pass. Two deliberate mutations (gate missing-case acceptance and stripped path quoting) each failed the named regression, restored source hash recorded. Codex loader: one untrusted user SessionStart handler, no warnings/errors, isolated HOME/USERPROFILE/CODEX_HOME, config and installed entry unchanged. Native Unix argv controls exercised installed /bin/sh,/bin/bash,/bin/zsh. Windows code and platform-gated cmd.exe/PowerShell test present; no native Windows result is claimed from macOS. First strict clippy attempt caught two single-element loops in the retained OpenCode refusal tests; corrected before restarting ordered gates."
carry_over: |-
  Carry-over (2026-09-12)
  | Item | Disposition | Evidence in iter254 directory |
  |---|---|---|
  | live_135_screenshot_ff153::live_135_screenshot_full_page_taller | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_135_screenshot_ff153__live_135_screenshot_full_page_taller.log |
  | live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_144_session_hygiene_followup__live_144_full_page_no_duplicate_header.log |
  | live_61l::live_screenshot_full_page | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_61l__live_screenshot_full_page.log |
  | live_61r_screenshot::live_screenshot_full_page | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_61r_screenshot__live_screenshot_full_page.log |
  | live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_92_screenshot_full_page__live_screenshot_full_page_md5_differs_from_viewport.log |
  | live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_92_screenshot_full_page__pre_fix_repro_screenshot_full_page_taller_than_viewport.log |
  | live_screenshot_shim::live_screenshot_unchanged_after_shim | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical fourth-argument dictionary TypeError in sweep and exact isolation. | sweep.log and isolated-live_screenshot_shim__live_screenshot_unchanged_after_shim.log |
  | Dogfood first run exited124 before sentinel; failing command output was discarded | Fold into [[iteration-203-live-sweep-watch-conditions-third-holder]] iteration_254_dogfood_watch. Output retention added; rerun passed. No cause or product fix claimed. | check-dogfood-script-attempt1.log; check-dogfood-script.log |
  | ThemeC task: literal Outcome body-section placement unavailable | No new plan: supported obsolete disposition is established in outcome metadata and research. Hyalo lacks an arbitrary body editor; original task remains unticked until a supported operation or explicit exception permits the required placement. | outcome and editing_limitations metadata |
  | Windows shell test not run natively on macOS | Platform-gated cmd.exe/PowerShell argv and sentinel regression included for Windows CI. No local Windows execution claimed. | commands/install_hook/codex.rs |
  | Initial strict clippy rejected two single-element OpenCode test loops | Closed in this revision: loops simplified; all ordered gates then passed. | gates-attempt1/clippy.log; gates.json |
live_sweep: |-
  2026-09-12 closing sweep; FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1
  LIVE_SWEEP_SUMMARY executed=338 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=338
  LIVE_SWEEP_PROFILES leaked=0 unattributed=0 root=/Users/james/Library/Application Support/ff-rdp/profiles
  331 passed + 7 failed = 338 executed. CLI322pass+7fail; core tiers1+3+3+2 all passed. Exact expected/observed names reconcile in all five tiers with no duplicates. All seven failures also fail exact isolation with the same Firefox155 drawSnapshot fourth-argument dictionary TypeError. RawFirefox20572 /tmp/ff-rdp-254-raw.9fs09E stopped and reaped (wait143); desktopFirefox37270 untouched. Successful current tests do not close existing203 styles/missing-selector watches,262 promotion observations,267 failed-occurrence diagnostics, or prior distinct Sourcepoint action evidence.
supervisor_closing: "2026-09-12: Supervisor verified complete worker tree308256d025bb5fe58eaa8d0fb0bd81905cfd1eaf against file hashes and an alternate index. Ordered gates pass:2458 passed,0 failed,410 ignored across36 summaries. After final338 sweep and seven exact isolations, supervisor reran all9 xtask checks in prescribed order; all passed, including actual browser-owning dogfood20.46s. Preliminary pre-sweep xtask evidence is retained separately. Product source remains unchanged. Pending documentation-only exception for literal Outcome-section placement and heading counters; original AC wording preserved."
review_repair_1_2026_09_12: "Independent findings repaired: Codex uses nonempty CODEX_HOME before HOME/USERPROFILE default .codex, consistently for gate/install/uninstall; empty CODEX_HOME falls back. PowerShell executable paths are separately UTF8-base64 data inside the UTF16LE-base64 command, preventing typographic-quote parsing/injection. Stop-on-error makes missing executables fail while real child exits0/23 survive. Legal Windows fixture names now have non-space suffixes and probes compile at safe paths before copying. This separately addresses original Windows CI34707468524 fixture failure before shell execution; repaired native Windows CI remains required. Three deliberate mutations (path-data removal, custom-home bypass, launch-error guard removal) each failed the actual regression; restored source passed final ordered rustup/fmt/strictclippy/workspace tests:2461passed,0failed,410ignored across36summaries. Initial repair clippy implicit-clone failure was corrected by moving the control path, then gates restarted. Actual isolated Codex0.153.4 hooks/list accepted final installer output in custom CODEX_HOME distinct from HOME: one enabled/untrusted user command, zero warnings/errors; all four config/entry hashes unchanged. No hook execution, trust approval or model call requested. Local PowerShellCore7.5.4 semantics and Unix sh/bash/zsh controls are not native Windows evidence. All9xtask gates ran after final sweep/isolation and passed, including actual browser-owning custom-home dogfood28.41s. No original task/AC text or tick changes; in-review remains. No Outcome body/counter exception has been authorized."
review_repair_1_sweep: "FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep. LIVE_SWEEP_SUMMARY executed=338 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=338. Exact338 qualified/observed names reconcile all5tiers: CLI321pass+8fail, core1+3+3+2pass, total330pass+8fail. All5 per-tier profile scans clean; final LIVE_SWEEP_PROFILES leaked=0 unattributed=0. Seven fullpage155 failures each reproduced in exact isolation and remain257. Additional live_161_build_script_matrix_evaluates failed autostart eval1 with post-auth Timeout/exit124 on port51734 before matrix execution; exact isolation passed8.07s. This is a new named267 observation, not proof of cause; attributable failed-occurrence auth/request/dispatcher timing remains mandatory/unmet. RawFirefox93454 stopped/reaped143, port6000free, desktop37270 preserved. Source frozen77c1383598aff39c6346bdd7f04ab11cdf39bb87 before final gates; full evidence .git/ralph-loop/20260912-validation-efficiency/iter254/review-repair-1. Earlier331pass+7fail sweeps and dogfood124 anomaly remain historical evidence, not erased."
review_repair_1_probe_portability: "2026-09-12 test-only follow-up within repair1: Windows PowerShell5.1 powershell.exe requires -Version <version>, unlike pwsh version display. Replaced only the cfg(test) availability probe with separate argv -NoLogo -NoProfile -NonInteractive -Command exit 0, supported by both. Source: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_powershell_exe?view=powershell-5.1 . Actual local PowerShell regression passed, preserving mandatory Windows execution, optional missing-pwsh Unix distinction, Unicode/injection and native exit0/23/missing-command controls. Ordered stable/fmt/strictclippy/workspace gates rerun and pass. Product prefix and all other previous manifest files are byte-identical to frozen5d7c0aa0cc85737465bd8311af172a3317f9e7cb; scope-proof.json/test-only.patch retain exact proof. Per supervisor,338 final-product sweep330pass+8fail and actual28.41s browser dogfood are REUSED, not rerun or replaced. Eight applicable static xtask gates pass. Dogfood static lint passed, but its complete gate REFUSED exit1 because iter-* requires FF_RDP_LIVE_TESTS=1; this invocation is not a green dogfood gate. Per the test-only reuse instruction, prior actual successful28.41s gate is retained with unchanged product/script proof; no new browser run. This explicit refusal remains in xtask-results.json. Native exact-head Windows CI is still required; no repaired Windows execution/failure is claimed. Prior267 missing failed-occurrence timing and all203/262 observations unchanged. No original tasks/ACs/status or pending Outcome/counter exception changed."
supervisor_repair1_closing: >-
  2026-09-12 supervisor verified worker
  tree095cc4b3a6868f7742c3e1a1819385062a31680b and unchanged HEAD/index. Final ordered gates2461passed/0failed/410ignored
  across36summaries. Bare-Version correction is entirely cfg(test); production
  hash/diff proof preserves repaired-source338sweep330pass+8fail,
  all5tiers/exactnames/profile checks. The follow-up invocation with live disabled was a genuine gate
  refusal; supervisor reran check-dogfood-script with BOTH live env flags and actual
  browser payload passed exit0. Thus no failed closing gate remains; original refusal
  is retained as corrected invocation evidence. Prior actual dogfood28.41s remains
  historical and current rerun duration was not separately measured. All upcoming
  pending bodies remain unchanged; new161post-auth Timeout remains267 with
  failing-occurrence attribution unmet. Native Windows exact-head CI and fresh independent
  review are still pending. No254/246body exception received or applied.
finalization_2026_09_13: "Owner clarified: use Hyalo for supported operations; direct KB body edits allowed. Required Outcome section and heading counts now written, Theme C ticked via Hyalo. Original AC wording unchanged. All ten native CI checks34710505693 passed exact product/test headf0385a556d157f2d15c845ab58280d81582acca8, including actual Windows PowerShell controls and custom-home e2e; fresh independent review01a096d3-1053-7770-9245-d41fb3a56f4b exit0 explicit[] (9read-onlyexec). This finalization changes documentation only; final updated-head checks and review required before merge. Historical metadata below records earlier attempts, including resolved tooling and Windows blockers; it is not current status. Final product sweep is review_repair_1_sweep:338=330pass+8fail, all five tiers/names/profilezero. Seven screenshots257 and distinct161post-authTimeout267 remain with causes/failed-occurrence timing unmet. All9xtask gates passed on unchanged production; ordered gates rerun for this docs checkpoint."
review_repair_2: "2026-09-13 formal repair2 of2: P1 bare powershell.exe cwd hijack repaired with GetSystemDirectoryW plus existing absolute WindowsPowerShell/v1.0/powershell.exe; no SystemRoot/WINDIR/PATH discovery or hardcoded drive. Narrow audited FFI preserves unsafe policy. Quoted system path rejects percent/exclamation/quote/control expansion, API failure, invalid Unicode, missing/nonabsolute executable with visible no-write error; ff-rdp path encoding/child status semantics unchanged. Codex uninstall now skips command resolution and returns command:null, so removal remains available when shell resolution fails. Refetched pinned Codex0.153.4 source confirms COMSPEC/cmd /C raw-quoted command and session cwd. Added native Windows actual decoy + deliberate bare-shell mutation control, child0/23 and missing1 controls; Windows execution is mandatory and pending exact-head CI, not established by macOS. Local deterministic rendering and buffer/error tests pass; deliberate bare-shell and uninstall-resolution mutations each failed the named regression and were restored byte-for-byte. Earlier first ordered gate sequence passed before uninstall adjustment and is retained in gates-before-uninstall; final ordered gates and fresh sweep pending. No original tasks or AC wording changed."
review_repair_2_sweep: "2026-09-13 final repair 2 product-source sweep after freeze a085f5d161ed77762717879de0aef907961b4b42, FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep: LIVE_SWEEP_SUMMARY executed=338 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=338. CLI 320 passed + 9 failed and core 1+3+3+2 passed = 329 passed + 9 failed. All 338 expected/observed qualified names reconcile, no missing/extra/duplicates; all five per-tier profile checks and final LIVE_SWEEP_PROFILES leaked=0 unattributed=0. Seven Firefox 155 full-page failures reproduced identical dictionary TypeError in exact isolation and remain in plan 257. Consent promotion and separately unattributed 140 zero-frame error passed exact isolation, remain in plan 262 with distinct evidence and no claimed common cause; prior ready-target Sourcepoint action failures remain unresolved. Prior 161 post-auth Timeout in plan 267 and 203 styles / first dogfood exit 124 observations remain preserved/unmet even though current sweep passed 161. Raw Firefox PID 65299 /tmp/ff-rdp-254-raw.sA6SzK stopped/reaped with status 143; port 6000 free; desktop PID 37270 untouched. All nine actual xtask checks passed after sweep/isolations, including live owned-browser dogfood 38.88 s. Ordered local gates: 2463 passed / 0 failed / 410 ignored before sweep; final docs checkpoint gates to follow. Native Windows decoy/mutation and final exact-head CI plus independent review remain supervisor-owned prerequisites, no macOS Windows result claimed. Full evidence review-repair-2 directory; all earlier failed attempts retained."
completion_2026_09_13: "2026-09-13 completion: all ten CI checks run34767242765 pass exact product head718035f8aff748b481d0044459f4dd8aad34ae82; native Windows codex_windows_hook_ignores_cwd_powershell_decoy and uninstall regression executed/passed. Independent read-only review01a09b7f-4dc6-7310-a882-826befcbf6cd exit0,10exec,explicit zero findings. Both formal repair rounds complete. Final ordered gates2463pass/0fail/410ignored,36summaries; nine actual post-sweep xtask checks including dogfood38.88s. Exact338=329pass+9fail final product sweep and all5profilezero preserved with unchanged-source proof; all9failures isolated/dispositioned, unresolved257/262/203/267 requirements remain unchanged. Current body/status supersedes historical pending-validation metadata. This metadata-only finalization reruns required ordered gates and all nine xtask checks; its exact-head CI and independent review remain prerequisites to merge, not claimed in advance."
---

# Iteration 254: install-hook for Codex and OpenCode

> **Renumbered 217 → 254 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 217.

Carry-over from [[iteration-212-ambient-context]] Theme B, task 3. Iteration 212 shipped
`install-hook --claude` and made `--codex` / `--opencode` exit 1 naming their file locations,
because neither entry schema could be verified from that build — and an entry with the wrong
shape parses, never fires, and looks installed forever ([[decision-log]] DEC-050).

This iteration removes the refusal by removing its cause: pin each target's schema against its
published documentation first, then write against the pinned shape.

## Themes

- **A — Pin the schemas.** Record, in `kb/research/agent-hook-formats.md`, the exact
  `~/.codex/hooks.json` entry shape and the `[features] hooks = true` gate, and OpenCode's
  plugin module contract, each with the doc URL and the version it was read at.
- **B — Codex.** `install-hook --codex` writes the pinned entry, with the same
  managed-key ownership, idempotence, path repair, `--dry-run` and `--uninstall` surface
  `--claude` has. The `[features]` gate is *checked*, never silently edited: an unset gate is a
  refusal naming the one line to add, not a hook written inert.
- **C — OpenCode.** Only if Theme A finds a way to express a session-start command without
  shipping a JavaScript module (CLAUDE.md: all code stays in Rust). If it does not, this theme
  closes `obsolete` with that finding written down, and `--opencode` keeps its refusal.

## Tasks

### A. Pin the schemas [2/2]
- [x] `kb/research/agent-hook-formats.md`: Codex `hooks.json` entry shape + the `[features]`
      gate, and OpenCode's plugin contract, each with source URL and version read
- [x] A fixture per target under `crates/ff-rdp-cli/tests/fixtures/` recording the shape, so the
      writer is tested against a recorded document rather than a guess

### B. Codex [3/3]
- [x] `Target::Codex` resolves `~/.codex/hooks.json` (honoring `HOME`/`USERPROFILE`) and merges
      one managed entry, leaving every other entry untouched
- [x] The `[features] hooks = true` gate is detected by parsing, not by substring search; an
      unset gate refuses with the exact line to add and writes nothing
- [x] `--dry-run`, `--uninstall`, idempotence and path repair all behave as `--claude` does

### C. OpenCode [1/1]
- [x] Either implemented against the pinned contract, or this theme is closed `obsolete` with
      the reason recorded in the Outcome section and in `agent-hook-formats.md`

## Acceptance Criteria [5/5]

- [x] `install_hook_codex_is_idempotent` (unit, `HOME` redirected): two installs produce a
      byte-identical `hooks.json`; second run reports no-op
- [x] `install_hook_codex_refuses_without_the_features_gate` (unit): a `config.toml` without
      `[features] hooks = true` produces exit 1 and no file write
- [x] `install_hook_codex_uninstall_removes_only_its_entry` (unit)
- [x] `install_hook_codex_leaves_other_entries_untouched` (unit): a hand-written entry survives
      install, repair and uninstall
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome

Codex installation, repair, dry-run, idempotence and uninstall are implemented. The actual Codex 0.153.4 loader accepted the installed entry. The explicit `features.hooks = true` check is ff-rdp installer opt-in policy; Codex itself enables hooks by default. New or changed commands still require trust through `/hooks`.

Theme C is **obsolete**, as permitted by this plan: the pinned OpenCode 1.3.13 contract requires a JavaScript/TypeScript plugin module. `--opencode` retains its refusal. The pinned documentation, fixture and source evidence are recorded in [[agent-hook-formats]].

## Review repair 2 (2026-09-13)

The Windows command now selects an absolute system PowerShell path through
`GetSystemDirectoryW`, with explicit discovery/quoting errors and no cwd or PATH
lookup. Codex uninstall skips that resolution. Literal ff-rdp path data and child
exit status handling are preserved. See [[agent-hook-formats]] for the pinned
Codex shell adapter, Microsoft API contract, and trust assumptions.

The fresh dual-gate sweep executed 338 tests: 329 passed and 9 failed. All five
tiers and 338 exact names reconcile; all five profile scans are clean. Seven
screenshot failures reproduce in isolation; consent promotion and the separate
zero-frame report pass isolation without establishing a fix. All nine xtask
checks ran after isolation, including actual live dogfood. Native Windows
executed and passed the decoy/mutation regression on product head
`718035f8aff748b481d0044459f4dd8aad34ae82` in CI run `34767242765`; all ten
checks passed. Fresh independent read-only review of that head completed with
zero findings (session `01a09b7f-4dc6-7310-a882-826befcbf6cd`, ten inspection
commands, exit 0). This closes the required P1 verification. The final
metadata commit must retain green exact-head CI and independent review before
merge. Original tasks and acceptance criteria are unchanged.

## Carry-over

Earlier failed attempts and their full evidence remain in the historical
frontmatter. These are the current dispositions; no acceptance criterion was
reworded, and later passes do not close unresolved failing occurrences.

| Item | Disposition |
|---|---|
| live_135_screenshot_ff153::live_135_screenshot_full_page_taller | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_137_daemon_mode_parity::live_137_consent_accept_via_daemon | Fold into [[iteration-262-daemon-live-target-never-promoted]]: measured target_count=1 / live_target_count=0 promotion failure after 15042 ms / 47 polls; exact isolation promoted in 35 ms / 1 poll and accepted consent. This pass does not close promotion or the separate historical ready-target Sourcepoint failure. |
| live_140_element_targeting::live_140_frame_error_bounded | Fold into [[iteration-262-daemon-live-target-never-promoted]] with the existing unattributed zero-frame family: missing-selector error reported 0 of 0 / of 0 total; exact isolation passed. No failing-occurrence counters or common cause captured; original promotion ACs remain unmet. |
| live_144_session_hygiene_followup::live_144_full_page_no_duplicate_header | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_61l::live_screenshot_full_page | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_61r_screenshot::live_screenshot_full_page | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_92_screenshot_full_page::live_screenshot_full_page_md5_differs_from_viewport | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_92_screenshot_full_page::pre_fix_repro_screenshot_full_page_taller_than_viewport | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| live_screenshot_shim::live_screenshot_unchanged_after_shim | Fold into [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]]: identical Firefox 155 drawSnapshot fourth-argument dictionary TypeError in sweep and exact isolation. |
| Historical live_161_build_script_matrix_evaluates post-auth Timeout from repair1 | Fold remains [[iteration-267-daemon-post-auth-timeout-recurrence]]: failing-occurrence auth/request/dispatcher timing is still mandatory and unmet. Current sweep pass does not establish cause or resolution. |
| Historical styles/missing-selector sweep watches | Fold remains [[iteration-203-live-sweep-watch-conditions-third-holder]]; retain original observations and firing conditions. |
| Historical first 254 dogfood exit 124 before sentinel | Fold remains [[iteration-203-live-sweep-watch-conditions-third-holder]]: output was lost; later passes do not explain that occurrence. |
| Required native Windows P1 verification | Closed in this PR: `codex_windows_hook_ignores_cwd_powershell_decoy` passed natively in Windows CI34767242765 on718035f, including deliberate bare-shell decoy/97, trusted child0/23 and missing1. Fresh independent review returned zero findings. Final metadata-head CI/review remain mandatory before merge. |
| Uninstall when PowerShell resolution is unavailable | Closed in this repair: Codex uninstall skips executable resolution and emits command:null; the e2e regression and deliberate resolution mutation have opposite verdicts. |
| Intentional bare-shell mutation and initial wrong --lib test invocation | Closed validation controls: bare-shell mutation failed its regression and source was restored; ff-rdp-cli has only a binary target, and the corrected --bin invocation passed. Neither is omitted from evidence. |
| Theme C Outcome placement and heading counts | Closed by the owner-authorized documentation finalization before repair2; OpenCode remains obsolete under the original allowed JS/TS-only contract. Original task/AC wording is unchanged. |

## Out of scope

- Changing `--claude`'s behaviour or the home view it prints.
- Editing `~/.codex/config.toml`. ff-rdp has no TOML dependency and adding one to flip a boolean
  in a user's config is a worse trade than refusing with the line to paste.

## References

- [[iteration-212-ambient-context]] — the refusal this removes, and why it was chosen
- [[decision-log]] DEC-050
