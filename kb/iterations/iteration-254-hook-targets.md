---
title: "Iteration 254: install-hook for Codex and OpenCode, against pinned schemas"
type: iteration
date: 2026-08-30
status: in-review
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
editing_limitations: >-
  Hyalo has no arbitrary body editor. Outcome is recorded in this frontmatter
  property and research findings are frontmatter in the Hyalo-created research
  scaffold. Theme C task remains unticked because its literal requirement to put the
  reason in an Outcome body section cannot be satisfied through this tool. Its
  substantive supported obsolete disposition is recorded above. Original task/AC text is
  unchanged. Heading counters cannot be edited through Hyalo and are superseded by
  verified counts in closing_counts metadata.
closing_counts: >-
  A 2/2; B 3/3; C 0/1 (obsolete disposition established, literal Outcome body
  placement unavailable); Acceptance Criteria 5/5. Original heading counters cannot be
  updated through Hyalo.
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

### A. Pin the schemas [0/2]
- [x] `kb/research/agent-hook-formats.md`: Codex `hooks.json` entry shape + the `[features]`
      gate, and OpenCode's plugin contract, each with source URL and version read
- [x] A fixture per target under `crates/ff-rdp-cli/tests/fixtures/` recording the shape, so the
      writer is tested against a recorded document rather than a guess

### B. Codex [0/3]
- [x] `Target::Codex` resolves `~/.codex/hooks.json` (honoring `HOME`/`USERPROFILE`) and merges
      one managed entry, leaving every other entry untouched
- [x] The `[features] hooks = true` gate is detected by parsing, not by substring search; an
      unset gate refuses with the exact line to add and writes nothing
- [x] `--dry-run`, `--uninstall`, idempotence and path repair all behave as `--claude` does

### C. OpenCode [0/1]
- [ ] Either implemented against the pinned contract, or this theme is closed `obsolete` with
      the reason recorded in the Outcome section and in `agent-hook-formats.md`

## Acceptance Criteria [0/5]

- [x] `install_hook_codex_is_idempotent` (unit, `HOME` redirected): two installs produce a
      byte-identical `hooks.json`; second run reports no-op
- [x] `install_hook_codex_refuses_without_the_features_gate` (unit): a `config.toml` without
      `[features] hooks = true` produces exit 1 and no file write
- [x] `install_hook_codex_uninstall_removes_only_its_entry` (unit)
- [x] `install_hook_codex_leaves_other_entries_untouched` (unit): a hand-written entry survives
      install, repair and uninstall
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

- Changing `--claude`'s behaviour or the home view it prints.
- Editing `~/.codex/config.toml`. ff-rdp has no TOML dependency and adding one to flip a boolean
  in a user's config is a worse trade than refusing with the line to paste.

## References

- [[iteration-212-ambient-context]] — the refusal this removes, and why it was chosen
- [[decision-log]] DEC-050
