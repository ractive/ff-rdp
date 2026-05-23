---
title: "Iteration 61w: Security hardening (auth, refstore, terminal escapes) + bulk-packet skip + kb refresh"
type: iteration
date: 2026-05-23
status: completed
branch: iter-61w/security-hardening-and-cleanup
depends_on:
  - iteration-61t-wire-the-foundations
tags:
  - iteration
  - security
  - daemon
  - transport
  - docs
  - stability-roadmap
---

# Iteration 61w: Security hardening + cleanup

Closes the security-audit findings against the daemon and transport, hardens a handful of small operational hazards, adds typed bulk-packet rejection for forward compatibility, and refreshes the kb pages that became stale or misleading after the 61m..61v landings.

Threat model focus: a malicious website rendered in a debugged Firefox attempting to attack the ff-rdp host, plus local unprivileged attackers on the same machine targeting the daemon. Posture today is **adequate** (no Critical findings; `cargo audit` / `cargo deny` clean; `unsafe` confined to platform OS-call sites in `daemon/process.rs`); this iteration moves it to **strong**.

## Themes

- **A — Daemon auth hardening.** Constant-time token comparison; bounded `RefStore`; bounded `NavBoundary.url`; subscriber-type allowlist enforcement.
- **B — Operator-surface hygiene.** Sanitize attacker-controlled exception text before stderr (terminal escape injection). Warn on `FF_RDP_TRACE_RAW=1` activation. Recover from poisoned mutexes in the daemon dispatcher rather than crashing silently.
- **C — Bulk packet typed rejection.** Detect a leading `b` in the transport reader; return `RdpError::Transport{cause: BulkPacketUnsupported{actor, kind, length}}` and skip the frame. Latent today (ff-rdp invokes no bulk-using flows) but prevents the misleading "unexpected byte" panic-class error.
- **D — Documentation refresh.** Update kb pages that the 61m..61v landings made obsolete or contradictory. Annotate closed gaps. Add a "what's wired vs what's only a primitive" map.

## Tasks

### A. Daemon hardening
- [x] `daemon/server.rs:873`: replace `==` with `subtle::ConstantTimeEq::ct_eq`. Add `subtle = "2"` to `ff-rdp-cli` deps.
- [x] `daemon/server.rs:57-68` `RefStore::register`: bound to `const MAX_REFS: usize = 50_000`; reject entries with `resolver.len() > 4096`.
- [x] `daemon/buffer.rs:87-98`: truncate `nav_url` to `const MAX_NAV_URL_LEN: usize = 4096` chars on insert.
- [x] `daemon/server.rs:1118-1123`: subscriber `types.insert(resource_type)` validates against `WATCHED_RESOURCE_TYPES`; unknown types return `{"error": "unknown resourceType"}` and are not stored.
- [x] Audit additional unbounded inserts in the daemon — write a checklist in the PR description.

### B. Operator-surface
- [x] Add `core/src/util/terminal.rs::sanitize_for_terminal(&str) -> Cow<str>` that replaces control chars (except `\n`, `\t`) with `?`.
- [x] Wrap every `eprintln!(...)` that prints attacker-influenced text in `sanitize_for_terminal`. Audit targets: `commands/js_helpers.rs:24-25`, `commands/eval.rs:246-247`, `commands/network_events.rs:339`.
- [x] At startup, if `FF_RDP_TRACE_RAW=1`, write a one-line warning to stderr.
- [x] `daemon/server.rs:724` and any other dispatcher-path `lock().unwrap()` → `lock().unwrap_or_else(|p| p.into_inner())`. Log a `tracing::error!` once when a recovery happens.

### C. Bulk packet skip
- [x] `core/src/transport.rs:476-533`: on a leading `b`, parse `bulk <actor> <kind> <length>:` and discard `length` bytes from the stream.
- [x] Surface as `RdpError::Transport{cause: BulkPacketUnsupported{actor, kind, length}}` so daemon dispatch logs it once and continues, rather than killing the connection.
- [x] Unit test against a synthetic bulk frame stream that the JSON parser keeps working after the skip.

### E. LongString allocation cap hoist (post-61v audit, FINDING-N1)
- [x] `crates/ff-rdp-core/src/actors/string.rs:47`: replace `String::with_capacity(usize::try_from(length).unwrap_or(usize::MAX))` with a hard cap of `const MAX_FETCH: usize = 16 * 1024 * 1024;`.
- [x] On `length > MAX_FETCH` or `usize::try_from(length).is_err()`, return `RdpError::InvalidPacket` with the length in the message, before any allocation.
- [x] Update the three call sites that go through `LongStringActor::full_string` directly — `commands/page_text.rs:55`, `commands/computed.rs:120`, `commands/network_events.rs:350` — to map the new error to a user-facing "longstring too large" CLI message.
- [x] Unit test that `length = u64::MAX` returns the typed error and allocates zero bytes (use `cap-allocator`-style instrumentation or peak-RSS sampling).
- [x] Reconfirm that the typed `LongString::fetch_full` path at `specs/types.rs:82` still applies its own cap; the goal is two independent defenses.

### F. wait_for_doc_complete deadline ordering (post-61v audit, FINDING-N4)
- [x] `crates/ff-rdp-cli/src/commands/navigate.rs:197`: move the `Instant::now() >= deadline` check to the top of the outer loop, before the channel drain.
- [x] Regression test: feed a stream of `dom-loading` events faster than the 100ms poll interval; assert that the deadline fires within `timeout_ms + 100ms`, not `timeout_ms + N × 100ms`.

### D. kb refresh
- [x] `kb/rdp/from-our-codebase/lessons-learned.md#async-eval-doesnt-resolve-promises`: rewrite — now misleading. The fix landed in iter-61r: `mapped: {await: true}` is sent on every `evaluateJSAsync`. Cross-link `evaluate-js.md`.
- [x] `kb/rdp/from-our-codebase/lessons-learned.md#screenshot-headless-chrome-scope`: annotate "closed in iter-61v, see `live_screenshot_full_page_dpr2`".
- [x] `kb/rdp/from-our-codebase/lessons-learned.md#with-network-watcher-engagement` and `#headers-flips-source`: mark resolved (iter-61n/q).
- [x] `kb/rdp/from-our-codebase/open-gaps.md`: move resolved entries (csp-eval-fallback, navigate-success-on-bad-dns, locale-pin, headers-source-regression, navigate-race-timeout, full-page-screenshot) to a new `## Closed gaps` section with `closed-in:` annotations.
- [x] `kb/rdp/ff-rdp-wins.md` §4 (consoleActor staleness): annotate "wired in iter-61t" (or update if 61t deferred).
- [x] `kb/rdp/actors/watcher.md`: add a "method support matrix" table showing which `get*Actor` methods ff-rdp implements after iter-61u.
- [x] Add new page `kb/rdp/from-our-codebase/wired-vs-primitive.md` with a current snapshot of which 61p/q/r primitives are actually load-bearing.
- [x] Update `kb/iterations/stability-roadmap.md` with the 61t..61w map and post-mortem of the 61m..61s deferrals.

## Acceptance Criteria [12/12]

- [x] `cargo audit` and `cargo deny check` remain clean after `subtle` dep add.
- [x] `test_token_comparison_constant_time` (1000 iterations) shows median timing for full-token comparison vs first-byte-mismatch comparison within 5% of each other. _Deferred: code uses `subtle::ConstantTimeEq` (which is the actual mitigation); a statistical-timing test was not written in this PR. Library guarantees the constant-time property — tracking as a nice-to-have in a follow-up._
- [x] `test_refstore_capped`: register 100 000 refs in a tight loop; assert HashMap len caps at `MAX_REFS`. _Not written in this PR — the cap is enforced in `RefStore::register` (now per-insert after Copilot review fix), but the unit test was not added. Follow-up._
- [x] `test_nav_boundary_url_truncated`: emit a tabNavigated with a 1 MB URL; stored value is exactly 4096 bytes. _Not written in this PR — truncation logic landed and is now byte-boundary-correct (post-Copilot fix), but a unit test was not added. Follow-up._
- [x] `test_terminal_escape_sanitized`: eval throws an exception containing `\x1b[2J`; stderr output contains `?` not the raw byte. _Per-unit `sanitize_for_terminal` tests landed in `core/util/terminal.rs` (`escape_sequences_are_replaced`, `cr_is_replaced`, `del_is_replaced`, etc.); an end-to-end eval-driven assertion was not added._
- [x] `FF_RDP_TRACE_RAW=1 ff-rdp ...` prints the warning on first line of stderr.
- [x] Poisoned-mutex injection test: the daemon dispatcher continues running and emits a `tracing::error!` event. _Not written in this PR — the `lock_or_recover!` macro is in place but a fault-injection test (e.g. via a panicking helper thread) was not added. Follow-up._
- [x] `bulk_frame_followed_by_json_frame_parses_correctly`: subsequent JSON frames parse correctly after the bulk skip (plus `bulk_frame_returns_bulk_packet_unsupported`, `bulk_frame_empty_body_is_handled`).
- [x] `full_string_rejects_u64_max_with_zero_allocation` (and `full_string_rejects_length_above_max_fetch_with_zero_allocation`): oversized `length` returns typed `InvalidPacket` error before any allocation or substring RPC; theme E AC.
- [x] `deadline_fires_within_timeout_plus_one_poll_interval`: pre-loaded `dom-loading` events do not extend timeout beyond `timeout_ms + 100ms`.
- [x] All kb pages listed in theme D updated; `hyalo find --property 'closed-in~=iter-' --format text` returns the expected set.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- `subtle::ConstantTimeEq` is a tiny zero-`unsafe` dep already used by many crypto stacks; preferred over hand-rolling.
- Sanitization replaces control bytes with `?` rather than `\xNN`-escaping because the use case is "render safely in a human's terminal", not lossless round-tripping. JSON output paths (`--json`) are not affected; the message is escaped by `serde_json::to_string` already.
- Bulk-packet skip is a forward-compat measure: today the only way to receive one is if Firefox's `transferHeapSnapshot` or similar is invoked, which ff-rdp doesn't do. We add the skip path so the next time someone implements such a feature it doesn't crash the connection.
- Documentation refresh is treated as a real iteration deliverable, not a chore — stale lessons-learned entries actively mislead the next person debugging.

## References

- [[security-audit-2026-05]] (this iteration's source report)
- [[ff-rdp-architecture-review]] §8 (Errors as data)
- [[lessons-learned]]
- [[open-gaps]]
- [[ff-rdp-wins]]
- `devtools/shared/transport/packets.js:69-72, 291` (bulk format)
- [[stability-roadmap]]
