---
title: "Iteration 217: install-hook for Codex and OpenCode, against pinned schemas"
type: iteration
date: 2026-08-30
status: planned
branch: iter-217/hook-targets
depends_on: [212]
first_call_sites:
  - primitive: ff_rdp_cli::commands::install_hook::Target::Codex
    site: crates/ff-rdp-cli/src/commands/install_hook.rs (run → settings_path/apply_install)
dogfood_path: |
  ff-rdp install-hook --codex --dry-run
  # prints the ~/.codex/hooks.json entry and the [features] hooks = true precondition
  ff-rdp install-hook --codex && ff-rdp install-hook --codex
  # second call: no-op, exit 0, hooks.json byte-identical
  ff-rdp install-hook --codex --uninstall
  ff-rdp install-hook --opencode --dry-run
tags: [iteration, cli, agent-ergonomics]
---

# Iteration 217: install-hook for Codex and OpenCode

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
- [ ] `kb/research/agent-hook-formats.md`: Codex `hooks.json` entry shape + the `[features]`
      gate, and OpenCode's plugin contract, each with source URL and version read
- [ ] A fixture per target under `crates/ff-rdp-cli/tests/fixtures/` recording the shape, so the
      writer is tested against a recorded document rather than a guess

### B. Codex [0/3]
- [ ] `Target::Codex` resolves `~/.codex/hooks.json` (honoring `HOME`/`USERPROFILE`) and merges
      one managed entry, leaving every other entry untouched
- [ ] The `[features] hooks = true` gate is detected by parsing, not by substring search; an
      unset gate refuses with the exact line to add and writes nothing
- [ ] `--dry-run`, `--uninstall`, idempotence and path repair all behave as `--claude` does

### C. OpenCode [0/1]
- [ ] Either implemented against the pinned contract, or this theme is closed `obsolete` with
      the reason recorded in the Outcome section and in `agent-hook-formats.md`

## Acceptance Criteria [0/5]

- [ ] `install_hook_codex_is_idempotent` (unit, `HOME` redirected): two installs produce a
      byte-identical `hooks.json`; second run reports no-op
- [ ] `install_hook_codex_refuses_without_the_features_gate` (unit): a `config.toml` without
      `[features] hooks = true` produces exit 1 and no file write
- [ ] `install_hook_codex_uninstall_removes_only_its_entry` (unit)
- [ ] `install_hook_codex_leaves_other_entries_untouched` (unit): a hand-written entry survives
      install, repair and uninstall
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

- Changing `--claude`'s behaviour or the home view it prints.
- Editing `~/.codex/config.toml`. ff-rdp has no TOML dependency and adding one to flip a boolean
  in a user's config is a worse trade than refusing with the line to paste.

## References

- [[iteration-212-ambient-context]] — the refusal this removes, and why it was chosen
- [[decision-log]] DEC-050
