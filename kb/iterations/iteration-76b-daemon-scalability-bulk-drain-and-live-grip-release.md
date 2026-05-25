---
title: "Iteration 76b: Daemon scalability follow-up — bulk-frame drain, wire up grip release, type-safe Grip dispatch"
type: iteration
date: 2026-05-25
status: planned
branch: iter-76b/daemon-scalability-bulk-drain-and-live-grip-release
depends_on:
  - iteration-75b-pre-create-pr-discipline-gate
  - iteration-76-daemon-scalability-bulk-grips-pipelining
firefox_refs:
  - path: devtools/shared/specs/object.js
    lines: "205-218"
    why: "ObjectActor.release is oneway in spec — confirms the release-method name and no-reply semantics."
  - path: devtools/shared/specs/string.js
    lines: "58-85"
    why: "LongStringActor.release is also oneway with the same method name (`release`); GripKind markers must dispatch to this when the grip is a LongString."
  - path: devtools/shared/transport/packets.js
    lines: "247-431"
    why: "BulkPacket framing — confirms that on type/actor mismatch the client must still drain the announced byte length before reading the next frame."
  - path: devtools/server/actors/watcher.js
    lines: "405-460"
    why: "Watcher event payload shape — where to find grip actor IDs (object-actor / long-string-actor sub-fields) inside `consoleAPICall` / `evaluationResult` resource updates."
kb_refs:
  - kb/rdp/actors/object.md
  - kb/rdp/actors/string.md
  - kb/rdp/actors/watcher.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites:
  - primitive: "ff_rdp_core::transport::drain_bulk_frame"
    site: "crates/ff-rdp-core/src/transport.rs (called inside recv_bulk_with_handler on type/kind mismatch and on the JSON-first-byte fast-path)"
  - primitive: "ff_rdp_core::actors::watcher::extract_grips"
    site: "crates/ff-rdp-cli/src/daemon/server.rs::dispatch_firefox_message (populates ResourceGripGuard)"
  - primitive: "ff_rdp_cli::daemon::server::spawn_grip_release_drainer"
    site: "crates/ff-rdp-cli/src/daemon/server.rs::run_daemon (replaces the dropped `_grip_release_rx` binding)"
dogfood_path: |
  # 1. Stream-corruption regression: after a --bulk screenshot fallback to
  #    JSON, the next command must succeed (was: misaligned reads / panic).
  ff-rdp launch --auto-consent &
  ff-rdp --bulk screenshot https://example.com /tmp/s.png   # falls back to JSON
  ff-rdp eval https://example.com 'document.title'          # was broken; must succeed
  pkill -f 'firefox.*ff-rdp-test-profile'

  # 2. Grip release smoke: daemon mode, fire 100 evals, assert Firefox
  #    object pool stays bounded.
  ff-rdp daemon start --port 8965
  for i in $(seq 1 100); do
    ff-rdp eval --daemon 8965 https://example.com 'window'
  done
  # Inspect the daemon log: must contain release packets for objectActor grips.
  grep '"type":"release"' ~/.cache/ff-rdp/daemon-8965/rdp-trace.log | wc -l
  # Expected: ≥ 100 (one release per grip).

  # 3. iter-76b discipline gate.
  FF_RDP_FIREFOX_PATH=/Users/james/devel/firefox \
    cargo run -p xtask -- check-iteration-ready \
      --plan kb/iterations/iteration-76b-daemon-scalability-bulk-drain-and-live-grip-release.md \
      --base origin/main
tags: [iteration, daemon, transport, grips, followup]
---

Post-merge review of iter-76 (PR #109) surfaced four shipping defects.
Two are stream-corruption bugs in the new bulk-receive path; the other
two mean Theme B of iter-76 — `release` packets on object / long-string
grips, the headline "fix daemon-mode grip leaks" item — is **completely
inert in production**: the release-queue receiver is dropped immediately
in `run_daemon`, and `ResourceGripGuard::add_grip` is never called from
the watcher dispatch path, so no grip is ever queued for release.
iter-76's `live_grip_release_no_leak` test passes only because zero
grips ever enter the system.

iter-76b closes those four real defects and the smaller cleanups the
post-merge review surfaced (typed error on `DemuxReader::run_loop`,
type-safe `add_grip` dispatch on grip variant, late-registration doc).

## Themes

- **A — Bulk-frame drain on mismatch.** `recv_bulk_with_handler` (and
  the screenshot `--bulk` fallback) currently return
  `BulkPacketUnexpected` *mid-frame* — the announced body length stays
  in the socket buffer, poisoning every subsequent `recv_from` call.
  Per Firefox's `packets.js`, a non-matching bulk frame must still be
  fully consumed before the next frame is read.
- **B — Wire up grip release end-to-end.** Spawn a release-drainer
  thread that owns `grip_release_rx` and issues `actor_send("release")`
  for each enqueued grip. Add `extract_grips` to `actors/watcher.rs`
  that surfaces `object-actor` / `long-string-actor` IDs from
  `consoleAPICall` / `evaluationResult` / `inspectorChange` resource
  updates, and call `ResourceGripGuard::add_grip` from
  `dispatch_firefox_message` for each one.
- **C — Type-safe Grip dispatch + small cleanups.** Make `add_grip`
  pattern-match on `Grip::Object` / `Grip::LongString` and construct
  the corresponding `GripHandle<Kind>` so the marker types are
  honoured (today both go through `GripHandle::<ObjectGrip>::new`,
  defeating the type-safety rationale). Replace
  `DemuxReader::run_loop`'s `.expect()` with a typed `ProtocolError`
  return so misuse doesn't panic the daemon. Document the
  "register-before-run_loop" invariant prominently and add a
  debug-assert in `register` that fires after `run_loop_with` has
  consumed the reader.

## Tasks

### A. Bulk-frame drain on mismatch

- [ ] Add `pub(crate) fn drain_bulk_frame(reader, header_first_byte) -> Result<(), ProtocolError>` in `crates/ff-rdp-core/src/transport.rs` that finishes reading the bulk header (`bulk <actor> <kind> <length>:`), then reads-and-discards exactly `length` bytes from the body in 8 KiB chunks. Apply the existing `max_frame_bytes()` cap before the discard loop (carry forward the iter-75 M-1 fix to this path).
- [ ] In `recv_bulk_with_handler_from`, on the `first[0] != b'b'` fast-path: do NOT return immediately. Push the first byte back onto a peek buffer (already supported via `BufReader::seek_relative(-1)` if Buf; otherwise prepend to the read buffer for the next `recv_from`) — the caller's next `recv_from` must see a well-aligned frame. If true push-back isn't feasible, instead consume the rest of the JSON length prefix + `:` + body in-place and surface the parsed JSON via a new `BulkPacketUnexpected { json: Value }` variant so the caller can act on it.
- [ ] In `recv_bulk_with_handler_from`, on actor/kind mismatch: call `drain_bulk_frame` to consume the remaining body before returning `BulkPacketUnexpected { actor, kind }`. The stream must remain aligned afterwards.
- [ ] Restructure `try_two_step_screenshot` in `crates/ff-rdp-cli/src/commands/screenshot.rs`: do NOT call `send_capture_request` then `recv_bulk_with_handler`. Instead, call the full `ScreenshotActor::capture` and inspect the reply: if the reply is a base64 `data:` URL, decode in-process; if Firefox ever returns a bulk hint (future), fall through to the bulk path. Remove the pre-consume race entirely.
- [ ] Tests:
  - `bulk_recv_drains_on_actor_mismatch` (`transport.rs`): a fixture stream of `bulk other-actor screenshot 30:<30 bytes>` followed by `5:hello` — call `recv_bulk_with_handler("actor", "screenshot", ...)`, assert it returns `BulkPacketUnexpected`, then `recv_from` succeeds with `hello`.
  - `bulk_recv_drains_on_json_peek` (`transport.rs`): stream `30:{"from":"x","msg":"y"}…` — call `recv_bulk_with_handler`, assert error variant, then `recv_from` returns the JSON intact.
  - `bulk_recv_caps_drain_length` (`transport.rs`): announced length > `max_frame_bytes()` is rejected without entering the discard loop.

### B. Wire up grip release end-to-end

- [ ] Spawn `grip_release_drainer` thread in `run_daemon` (`crates/ff-rdp-cli/src/daemon/server.rs`): owns `grip_release_rx`, loops `recv()` and for each `(actor_id, kind)` issues `actor_send(transport, &actor_id, "release", None)`. Thread exits when the channel disconnects. Replace the `let (grip_release_tx, _grip_release_rx) = …` line so `_grip_release_rx` is genuinely moved into the drainer.
- [ ] Add `pub fn extract_grips(event: &Value) -> Vec<Grip>` to `crates/ff-rdp-core/src/actors/watcher.rs`. Walks `consoleAPICall.arguments[]`, `consoleAPICall.styles[]`, `evaluationResult.result`, `evaluationResult.exception`, and resource-array variants; for each `{ type: "object", actor: <id> }` returns `Grip::Object(actor)`, and for each `{ type: "longString", actor: <id> }` returns `Grip::LongString(actor)`. Tolerate missing/extra fields.
- [ ] In `dispatch_firefox_message` (server.rs:548-553), call `extract_grips(&msg)` and `ResourceGripGuard::add_grip(grip)` for each returned grip BEFORE the guard drops.
- [ ] Live test `live_grip_release_actually_releases` (`crates/ff-rdp-cli/tests/live_grip_release.rs`): daemon mode, evaluate `window` 100 times, snapshot Firefox's object-actor count via `Memory.getDistinguishedObjects` (or a proxy: count `objectActor` IDs in trace); assert the count after `sleep 1s` is bounded by 10 (release latency tolerance). Gated `FF_RDP_LIVE_TESTS=1`.
- [ ] Unit test `extract_grips_finds_object_and_long_string` (`actors/watcher.rs`): synthetic `consoleAPICall` JSON with one object grip and one longString grip; assert both are returned with correct kind.

### C. Type-safe Grip dispatch + small cleanups

- [ ] `ResourceGripGuard::add_grip(grip: Grip)` matches on `grip`:
  ```
  Grip::Object(a)     => self.handles.push(GripHandle::<ObjectGrip>::new(a, self.tx.clone()).into_dyn()),
  Grip::LongString(a) => self.handles.push(GripHandle::<LongStringGrip>::new(a, self.tx.clone()).into_dyn()),
  ```
  (Or store a homogeneous `Vec<Box<dyn ReleaseHandle>>`.) The point is: a `LongStringGrip` must NOT be wrapped as `ObjectGrip`. Today both methods happen to be `"release"` so there's no observable bug, but the type-level distinction must be honoured.
- [ ] `DemuxReader::run_loop` (`transport.rs:885-890`): replace `self.reader.take().expect(...)` with a `match` that returns `Err(ProtocolError::InvalidState("run_loop called without a reader — use split_demux()".into()))` when `None`. Add `ProtocolError::InvalidState(String)` variant if not present.
- [ ] Add a `tracing::warn!` in `DemuxReader::dispatch` (`transport.rs:846`) when the incoming packet has no `from` field — silent routing to fallback hides protocol errors.
- [ ] Document the "all actors must be registered before `run_loop` is called" invariant in the rustdoc on `DemuxReader::register` AND `DemuxReader::run_loop`. Cross-link them.
- [ ] Unit tests:
  - `demux_run_loop_without_reader_returns_error` (`transport.rs`): `DemuxReader::new()` then `run_loop()` returns `Err(InvalidState)`, no panic.
  - `add_grip_dispatches_on_kind` (`watcher.rs`): mock release queue; add one `Grip::Object` and one `Grip::LongString`; drop guard; assert two distinct release-handle types were enqueued (use a `#[cfg(test)]` accessor on `GripHandle` to expose its kind).

## Acceptance Criteria [0/9]

- [ ] `bulk_recv_drains_on_actor_mismatch`: `crates/ff-rdp-core/src/transport.rs::bulk_recv_drains_on_actor_mismatch` — after a mismatched bulk frame, the next `recv_from` returns the next frame intact.
- [ ] `bulk_recv_drains_on_json_peek`: `crates/ff-rdp-core/src/transport.rs::bulk_recv_drains_on_json_peek` — JSON frame peeked by bulk recv is preserved for the next `recv_from`.
- [ ] `bulk_recv_caps_drain_length`: `crates/ff-rdp-core/src/transport.rs::bulk_recv_caps_drain_length` — over-cap announced length is rejected without entering the discard loop.
- [ ] `live_screenshot_bulk_fallback_then_eval` (live, `FF_RDP_LIVE_TESTS=1`): `crates/ff-rdp-cli/tests/live_screenshot_bulk_fallback.rs::live_screenshot_bulk_fallback_then_eval` — `--bulk` screenshot then `eval` both succeed against the same Firefox instance (regression for the stream-poison bug).
- [ ] `live_grip_release_actually_releases` (live, `FF_RDP_LIVE_TESTS=1`): `crates/ff-rdp-cli/tests/live_grip_release.rs::live_grip_release_actually_releases` — 100 evals in daemon mode produce ≥ 100 `release` packets in the trace; object-actor count stays bounded.
- [ ] `extract_grips_finds_object_and_long_string`: `crates/ff-rdp-core/src/actors/watcher.rs::extract_grips_finds_object_and_long_string` — synthetic event with both grip kinds returns both.
- [ ] `demux_run_loop_without_reader_returns_error`: `crates/ff-rdp-core/src/transport.rs::demux_run_loop_without_reader_returns_error` — typed error, not panic.
- [ ] `add_grip_dispatches_on_kind`: `crates/ff-rdp-core/src/actors/watcher.rs::add_grip_dispatches_on_kind` — `LongStringGrip` enqueued through `add_grip` is NOT wrapped as `ObjectGrip`.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean; `cargo xtask check-iteration-ready --plan kb/iterations/iteration-76b-…md --base origin/main` exits 0.

## Design notes

**Why not just revert iter-76 and redo?** The bulk-recv core,
per-actor channel demux, and `Grip<Kind>` plumbing in iter-76 are
correct in shape — the defects are at the boundaries (frame drain on
mismatch, wiring the drainer thread, populating the guard). A
follow-up iter is much smaller surface than a redo and keeps iter-76's
real wins (no full-buffer base64 for bulk paths, real pipelining).

**Why `extract_grips` lives in `actors/watcher.rs` and not in `daemon/server.rs`?**
The shape of the watcher event payload is a protocol concern, not a
daemon concern. The CLI's single-shot path would also benefit
(currently doesn't track grips at all). Putting it in `actors/watcher.rs`
keeps the daemon dispatch wire-up to one line.

**The `BulkPacketUnexpected { json: Value }` variant** (Theme A,
alternative) is the right shape if peek-pushback isn't feasible. It
lets `try_two_step_screenshot` receive the parsed JSON-fallback
response in one call rather than two. Decide which path during
implementation; both satisfy the acceptance criteria.

**Why a debug-assert on late `register`?** The type system can't catch
"register after run_loop consumed self" because the channel map is
threaded into the spawned reader thread and `register` becomes a no-op
on the now-empty local. A `debug_assert!(self.reader.is_some(), ...)`
in `register` surfaces misuse in tests without breaking release
builds.

## Out of scope

- The "late actor registration via `Arc<Mutex<HashMap>>`" alternative
  noted in the review. iter-76b sticks with the "pre-register all
  actors" invariant; if we ever need dynamic registration after the
  reader spawns, file a follow-up.
- The `kind="screenshot"` placeholder in `try_two_step_screenshot`
  (#7 in the review). It's a no-op today; revisit when Firefox adds
  bulk screenshot support.
- The accessor regression test for iter-75b's
  `Acceptance Criteria [0/8]` → `[8/8]` counter hygiene
  (CodeRabbit nit). Fix inline at the start of this iter; no AC.

## References

- [[iteration-76-daemon-scalability-bulk-grips-pipelining]] — the
  iter that introduced these surfaces.
- [[iteration-75b-pre-create-pr-discipline-gate]] — the gate that
  *should* have caught Theme B's `add_grip`-never-called inertness
  pre-merge but didn't (discipline checks don't know "this `pub` is
  called but does nothing").
- Post-merge review report (2026-05-25) on PR #109 — local +
  CodeRabbit findings #1–9.
- `kb/rdp/from-our-codebase/open-gaps.md:36` —
  `actor-leak-in-daemon`; iter-76 *claimed* to close this, iter-76b
  actually does.
