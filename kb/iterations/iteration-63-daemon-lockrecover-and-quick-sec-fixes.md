---
title: "Iteration 63: Daemon lock-recover sweep + file:// gating + table sanitization"
type: iteration
date: 2026-05-24
status: in-review
branch: iter-63/quick-sec-fixes
depends_on:
  - iteration-61w-security-hardening-and-cleanup
first_call_sites: []
dogfood_path: |
  # 1. Daemon stays alive after a poisoned mutex.
  ff-rdp daemon start &
  # Inject a panic into a handler (manual or via test hook), reconnect,
  # confirm the daemon thread is still serving — no zombie state.

  # 2. file:// is rejected without the new flag.
  ff-rdp navigate file:///etc/passwd          # exits non-zero, "url scheme not allowed"
  ff-rdp navigate --allow-file-urls file:///etc/hosts   # works

  # 3. Hostile cookie name with ANSI escapes renders neutered.
  ff-rdp cookies --format table   # any \x1b in a name appears as `?`, never moves the cursor
tags: [iteration, security]
---

# Iteration 63: Daemon lock-recover sweep + file:// gating + table sanitization

Closes three cheap, high-value security gaps surfaced by the post-iter-62
review: the daemon hot path still uses `.lock().unwrap()` (poisoned mutex →
DoS), `file://` is in the default-allowed URL scheme list (info-disclosure
via a redirect), and stdout table/text rendering doesn't sanitize terminal
escapes (only the `eprintln!` paths do). All three are sub-hour fixes; bundle
them so the security debt from iter-61w gets paid off before more surface
lands.

## Themes

- **A — Daemon lock recovery.** Replace remaining `.lock().unwrap()` in the
  daemon dispatcher with `lock_or_recover!`; add an xtask grep guard so the
  next regression is caught in CI.
- **B — `file://` gating.** Move `file://` out of the default-allowed scheme
  set; gate it behind a new `--allow-file-urls` flag.
- **C — Output sanitization.** Wire `sanitize_for_terminal` through the
  `output_pipeline` cell formatter so `--format table` / `--format text`
  cannot be ANSI-injected by attacker-controlled fields (cookie names, page
  titles, console messages).

## Tasks

### A. Daemon lock recovery
- [x] Audit `crates/ff-rdp-cli/src/daemon/server.rs` for every `.lock().unwrap()` (currently ~15 sites in `accept_loop`, `handle_client`, stream/ref/last_activity helpers).
- [x] Replace each with `lock_or_recover!` (the macro defined at `server.rs:30-44`).
- [x] Add `crates/xtask/src/main.rs` subcommand `check-daemon-locks` that fails if `rg '\.lock\(\)\.unwrap\(\)' crates/ff-rdp-cli/src/daemon/` returns any hits. Wire into CI.

### B. `file://` gating
- [x] Remove `"file"` from `ALLOWED_SCHEMES` in `crates/ff-rdp-cli/src/commands/url_validation.rs:3`.
- [x] Add `--allow-file-urls` CLI flag (mirror the pattern of `--allow-unsafe-urls`); thread it through `commands/navigate.rs:394, 517` and any other caller of `validate_url`.
- [x] Update the existing test at `url_validation.rs:41` that asserts `file://` is allowed by default — invert the assertion and add a paired test for the gated path.
- [x] Document the threat model in the URL-validation module docstring (file:// → exfil via subsequent page_text/eval/screenshot).

### C. Output sanitization
- [x] Wrap the string branch of `value_to_cell` (`crates/ff-rdp-cli/src/output_pipeline.rs:295`) with `sanitize_for_terminal`.
- [x] Audit every `println!` in `output_pipeline.rs` for un-sanitized attacker-influenced data; apply the wrapper at the cell-formatting boundary (lines 263, 267, 279, 290).
- [x] Add a unit test asserting `\x1b[2J` in a cookie name renders as `?[2J` (or whatever the sanitizer emits) under `--format table`.

## Acceptance Criteria [5/5]

- [x] `daemon_lock_or_recover_sweep`: `rg '\.lock\(\)\.unwrap\(\)' crates/ff-rdp-cli/src/daemon/` returns zero hits (verified by `cargo run -p xtask -- check-daemon-locks`).
- [x] `xtask_check_daemon_locks_fails_on_regression`: synthetic `.lock().unwrap()` injected via fixture → xtask exits non-zero (test `check_daemon_locks::tests::fails_on_regression`).
- [x] `url_validation_rejects_file_scheme_by_default`: `validate_url("file:///etc/passwd")` returns `Err`; with `--allow-file-urls` returns `Ok` (tests `rejects_file_by_default` + `allows_file_when_opted_in` in `url_validation.rs`).
- [x] `output_table_sanitizes_ansi_escapes`: rendering a cookie named `"foo\x1b[2Jbar"` under `--format table` produces no raw `0x1b` bytes (test `value_to_cell_strips_ansi_escapes_from_strings` in `output_pipeline.rs`).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

`lock_or_recover!` is already the right primitive; this iteration is just
mechanical sweep + a guard. The sweep is safe because every call site is
inside a `?`-returning function or a thread closure that already handles the
fallback path the macro produces.

`file://` is gated, not removed, because dogfooding against local HTML
fixtures is a legitimate workflow (and the test suite uses it). The flag
matches the `--allow-unsafe-urls` ergonomics.

The sanitizer is already battle-tested in iter-61w; the gap was just plumbing.

## Out of scope

- Path-traversal hardening on `--out`/`--output` flags (separate iter, see iter-65).
- Backfilling the iter-61w test debt (separate iter, see iter-66).
- XPI integrity (separate iter, see iter-64).

## References

- [[iteration-61w-security-hardening-and-cleanup]]
- Security review report (2026-05-24), findings F-1, F-2, F-5
