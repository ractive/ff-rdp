---
title: "Iteration 77: Spec drift cleanup + Windows reparse-point safe_io"
type: iteration
date: 2026-05-24
status: planned
branch: iter-77/spec-drift-and-windows-reparse-points
depends_on:
  - iteration-73-spec-fidelity-gates
  - iteration-74-protocol-correctness-oneway-events-lifecycle
  - iteration-75-security-hardening-defense-in-depth
firefox_refs:
  - path: devtools/shared/specs/screenshot.js
    lines: "13-35"
    why: "screenshot.args dict declares fullpage/file/clipboard/selector/dpr/delay only — ff-rdp sends browsingContextID/snapshotScale/rect which are NOT in the dict (S1)."
  - path: devtools/shared/specs/webconsole.js
    lines: "149-164"
    why: "evaluateJSAsync request Options: text, frameActor, url, selectedNodeActor, selectedObjectActor, innerWindowID, mapped, eager, disableBreaks. ff-rdp currently sends only text — S3 wires the rest."
  - path: devtools/server/actors/webconsole.js
    lines: "761-870"
    why: "Server-side evaluateJSAsync uses frameActor/selectedNodeActor/innerWindowID to scope the eval — proves they are not optional decoration."
  - path: devtools/shared/specs/watcher.js
    lines: "20-32"
    why: "unwatchTargets accepts (targetType, options) — ff-rdp omits options (W3). targetType is required (no default to 'frame', W4)."
  - path: devtools/server/actors/webconsole.js
    lines: "1100-1175"
    why: "formatStackTrace / formatted-message construction; reference for porting %s/%d/%c printf substitution (S6) — server formats per-arg, ff-rdp's joiner drops the format string."
  - path: devtools/shared/specs/walker.js
    lines: "125-133"
    why: "walker.releaseNode signature — kept as response-less actor_request (not oneway); flagged here for the spec-reviewer agent to confirm."
kb_refs:
  - kb/rdp/actors/screenshot.md
  - kb/rdp/actors/webconsole.md
  - kb/rdp/actors/watcher.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: "ff_rdp_core::actors::screenshot::ScreenshotArgsExt"
    site: "crates/ff-rdp-cli/src/commands/screenshot.rs (wraps the extra fields ff-rdp adds beyond the spec dict)"
  - primitive: "ff_rdp_core::actors::console::EvaluateScope"
    site: "crates/ff-rdp-cli/src/commands/eval.rs (used by --frame/--node/--inner-window flags)"
  - primitive: "ff_rdp_cli::util::safe_io::reparse_tag_of"
    site: "crates/ff-rdp-cli/src/util/safe_io.rs (consumed by safe_open_windows pre-flight check)"
dogfood_path: |
  # 1. Screenshot still works after spec-dict reconciliation.
  ff-rdp screenshot --selector body -o /tmp/s.png https://example.com
  ff-rdp --log-rdp-trace screenshot --fullpage -o /tmp/f.png https://example.com
  grep 'snapshotScale' ~/.cache/ff-rdp/rdp-trace.log    # via the typed shim

  # 2. eval in a specific frame.
  ff-rdp eval --frame "$FRAME_ACTOR" 'location.href' https://example.com

  # 3. console printf substitution preserved.
  ff-rdp --log-rdp-trace eval --subscribe console \
    'console.log("hello %s, you are %d", "world", 42)' https://example.com
  grep 'hello world, you are 42' ~/.cache/ff-rdp/rdp-trace.log

  # 4. unwatchTargets requires a targetType.
  ff-rdp watcher unwatch --target-type frame     # OK
  ff-rdp watcher unwatch                          # exits 2 with usage error

  # 5. (Windows) safe_write refuses a mount-point swap.
  # In an admin PowerShell:
  #   mklink /D C:\tmp\snap C:\Windows
  # then:
  #   ff-rdp memory heap-snapshot https://example.com C:\tmp\snap\stolen.bin
  # expect ReparsePointRejected error, no write under C:\Windows.
tags: [iteration, protocol, windows, spec-drift]
---

The first iter executed under the iter-73 spec-fidelity gates. It
bundles eight smaller correctness items the review flagged (S1, S3,
S6, W3, W4, L2, L3, M-4) — none of them individually justify a PR but
together they prove the new gates work end-to-end. Items split across
spec drift (S/W) and the long-standing Windows reparse-point gap
(M-4) which has been carried in `safe_io.rs` comments since iter-44.

## Themes

- **A — Screenshot spec dict (S1).** ff-rdp sends `browsingContextID`,
  `snapshotScale`, `rect` — none declared in `screenshot.args`. Wrap
  in a typed local shim and file an upstream-spec issue.
- **B — evaluateJSAsync scope (S3).** Wire frameActor /
  selectedNodeActor / innerWindowID.
- **C — Console printf substitution (S6).** Port `%s/%d/%c` handling
  so `parse_console_resources` stops dropping the format string.
- **D — Watcher unwatchTargets (W3, W4).** Send `options`; reject
  missing `targetType` instead of silently defaulting to `"frame"`.
- **E — ActorId hygiene (L2, L3).** Reject empty-string IDs; preserve
  `alive=false` on re-register (or debug-panic).
- **F — Windows reparse-point pre-flight (M-4).** Replace `is_symlink`
  + `open` with `CreateFileW(FILE_FLAG_OPEN_REPARSE_POINT)` + tag
  inspection.

## Tasks

### A. Screenshot spec dict (S1)
- [ ] Add `pub struct ScreenshotArgsExt` in `crates/ff-rdp-core/src/actors/screenshot.rs` that serializes to a JSON object containing the spec-declared `screenshot.args` fields PLUS the locally-required `browsingContextID`, `snapshotScale`, `rect`. Document at the top that the extra fields are read by the server (per `devtools/server/actors/screenshot.js`) but are not in the published spec dict (`devtools/shared/specs/screenshot.js:13-35`).
- [ ] Replace the current ad-hoc `json!({...})` construction site with `ScreenshotArgsExt`.
- [ ] Open a Mozilla Bugzilla issue tracking the spec-dict gap; record the bug number in a `// allow-spec-drift: bug NNNN` comment on the struct. (`allow-spec-drift` is a new convention; document it in `CLAUDE.md` alongside `allow-claim-miss`.)

### B. evaluateJSAsync scope (S3)
- [ ] Add `pub struct EvaluateScope { pub frame_actor: Option<ActorId>, pub selected_node_actor: Option<ActorId>, pub inner_window_id: Option<u64> }` in `crates/ff-rdp-core/src/actors/console.rs`.
- [ ] Extend `ConsoleFront::evaluate_js_async` to accept `Option<EvaluateScope>` and serialise the provided fields into the request body (per `devtools/shared/specs/webconsole.js:149-164`).
- [ ] CLI: add `--frame <actor>`, `--node <actor>`, `--inner-window <u64>` to `crates/ff-rdp-cli/src/commands/eval.rs`.

### C. Console printf substitution (S6)
- [ ] In `crates/ff-rdp-core/src/actors/watcher.rs` `parse_console_resources`, when the first arg is a string and contains `%s`/`%d`/`%i`/`%f`/`%o`/`%O`/`%c`, run a port of Firefox's formatter (`devtools/server/actors/webconsole.js:1100-1175` is the reference; see Design notes for what subset we port). Remaining args pass through unchanged.
- [ ] Unit tests: `console_printf_string_substitution`, `console_printf_digit`, `console_printf_styled_dropped` (CSS-style `%c` consumes its arg but produces no text in our text-mode output).

### D. Watcher unwatchTargets (W3, W4)
- [ ] In `crates/ff-rdp-core/src/actors/watcher.rs`, extend `unwatch_targets` to accept an `options: Option<Value>` arg and serialise per `devtools/shared/specs/watcher.js:23-32`.
- [ ] Remove the silent default to `"frame"` when `targetType` is missing — make it a required parameter at both the Rust API and CLI level. Logged via `tracing::error!` + returned as `RdpError::Spec { reason: "targetType required" }`.

### E. ActorId hygiene (L2, L3)
- [ ] In `crates/ff-rdp-core/src/actors/mod.rs` `ActorId::from`, reject empty strings (return `Result` or `Option` — choose based on existing call-site count, document the choice).
- [ ] In `crates/ff-rdp-core/src/registry.rs`, re-registering an actor whose previous state was `alive=false` either keeps `alive=false` (preferred, with a tracing warn) or panics in debug builds (`debug_assert!`). Pick one based on whether any current production path legitimately re-registers killed actors; document.

### F. Windows reparse-point pre-flight (M-4)
- [ ] Add `pub fn reparse_tag_of(path: &Path) -> windows::Result<Option<u32>>` in `crates/ff-rdp-cli/src/util/safe_io.rs` using `windows-sys` `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`, then `DeviceIoControl(FSCTL_GET_REPARSE_POINT)` to read the tag. Return `None` for non-reparse files.
- [ ] In `safe_write` / `safe_create` (`crates/ff-rdp-cli/src/util/safe_io.rs:25-28, 222-248`), pre-check the parent directory: if `reparse_tag_of` returns `Some(IO_REPARSE_TAG_SYMLINK | IO_REPARSE_TAG_MOUNT_POINT)`, refuse with `RdpError::ReparsePointRejected { path, tag }`. Application-layer (NTFS) reparse points (AppExecutionAlias etc.) are allowed.
- [ ] Remove the existing "follow-up work" comment block; the gap is now closed.

## Acceptance Criteria [0/11]

- [ ] `screenshot_args_ext_serializes_full_set`: `crates/ff-rdp-core/src/actors/screenshot.rs::screenshot_args_ext_serializes_full_set` — round-trip serialises the spec fields plus browsingContextID/snapshotScale/rect; the `allow-spec-drift: bug` comment is present (doctest greps).
- [ ] `live_screenshot_unchanged_after_shim`: `crates/ff-rdp-cli/tests/live_screenshot_shim.rs::live_screenshot_unchanged_after_shim` — `--fullpage` PNG hash matches the pre-iter baseline. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `evaluate_scope_serializes_fields`: `crates/ff-rdp-core/src/actors/console.rs::evaluate_scope_serializes_fields` — unit test, each EvaluateScope field appears in the request body.
- [ ] `live_eval_in_frame`: `crates/ff-rdp-cli/tests/live_eval_scope.rs::live_eval_in_frame` — create an iframe, eval in it via `--frame`, assert the returned location matches the iframe's URL. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `console_printf_string_substitution`: `crates/ff-rdp-core/src/actors/watcher.rs::console_printf_string_substitution` — `"hello %s"` + `"world"` parses to `"hello world"`.
- [ ] `live_console_printf_e2e`: `crates/ff-rdp-cli/tests/live_console_printf.rs::live_console_printf_e2e` — page emits `console.log("hello %s, you are %d", "world", 42)`, ff-rdp delivers the formatted text. Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] `unwatch_targets_options_serialized`: `crates/ff-rdp-core/src/actors/watcher.rs::unwatch_targets_options_serialized` — options arg appears in the outbound packet.
- [ ] `unwatch_targets_rejects_missing_type`: same module — `targetType=None` returns `RdpError::Spec`, no packet sent.
- [ ] `actor_id_rejects_empty`: `crates/ff-rdp-core/src/actors/mod.rs::actor_id_rejects_empty` — empty input fails.
- [ ] `registry_re_register_preserves_dead`: `crates/ff-rdp-core/src/registry.rs::registry_re_register_preserves_dead` — re-registering a dead actor keeps `alive=false` and emits a warn (or `debug_assert!` fires under `cfg(debug_assertions)`, depending on chosen policy).
- [ ] `safe_io_rejects_mount_point_windows`: `#[cfg(windows)]` test `crates/ff-rdp-cli/src/util/safe_io.rs::safe_io_rejects_mount_point_windows` — create `mklink /D testdir target`, attempt `safe_write` under `testdir`, assert `ReparsePointRejected`; non-reparse path succeeds. Gated `FF_RDP_LIVE_TESTS=1` (needs admin or DevMode for mklink).
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean on all three platforms (Windows CI included).

## Design notes

This iter is the first to be reviewed by the `rdp-spec-reviewer`
agent (iter-73 theme C). Each spec-touching task above cites the
exact line range the agent will re-read; the new `allow-spec-drift`
escape hatch is for cases like S1 where the server reads fields the
spec doesn't declare and we cannot fix that upstream in the same
iteration. Every `allow-spec-drift` annotation MUST link to a
filed Bugzilla bug — the gate enforces the link, not the fix.

For S6, "port Firefox's formatter" means: handle the format
specifiers Firefox handles, in the same order, with the same
arg-consumption rules. We do NOT port the styled-output rendering
of `%c` (no CSS in our text-mode output) but we do consume the
arg so subsequent specifiers stay aligned with their args. If a
spec edge case is missed, the live test catches it.

For E (L3), the choice between "keep dead" vs `debug_assert!` is a
judgment call: if iter-74's registry invalidation lands first
(planned, since iter-74 < iter-77), the population of "re-registered
dead" should be empty in practice, and `debug_assert!` is the
honest choice. We confirm with a one-off audit before picking.

For F, application-layer reparse tags (Windows Store
AppExecutionAlias) are *not* a TOCTOU vector and are widely present
in user-writable paths; rejecting them would break legitimate
workflows. We only reject `IO_REPARSE_TAG_SYMLINK` and
`IO_REPARSE_TAG_MOUNT_POINT`.

## Out of scope

- Filing the Bugzilla bug for S1 itself — done as part of the iter,
  but no upstream fix is expected in this PR.
- Async/streaming `console.log` formatting for the daemon HTTP API
  (separate consumer surface; the resource-bus delivery is what this
  iter fixes).
- Generalising `ReparsePointRejected` to a cross-platform
  "follow-redirect-prevention" abstraction. Unix already handles
  symlinks correctly via `O_NOFOLLOW`.

## References

- [[iteration-73-spec-fidelity-gates]] (gates this iter is the first
  to run under)
- [[iteration-74-protocol-correctness-oneway-events-lifecycle]]
  (registry invalidation precondition for §E policy choice)
- [[iteration-75-security-hardening-defense-in-depth]]
- Protocol review report (2026-05-24): S1, S3, S6, W3, W4, L2, L3, M-4
- `kb/rdp/from-our-codebase/open-gaps.md` (windows-reparse-point entry)
