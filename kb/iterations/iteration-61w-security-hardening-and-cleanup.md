---
title: "Iteration 61w: Security hardening (auth, refstore, terminal escapes) + bulk-packet skip + kb refresh"
type: iteration
date: 2026-05-23
status: planned
branch: iter-61w/security-hardening-and-cleanup
depends_on:
  - iteration-61t-wire-the-foundations
tags: [iteration, security, daemon, transport, docs, stability-roadmap]
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
- [ ] `daemon/server.rs:873`: replace `==` with `subtle::ConstantTimeEq::ct_eq`. Add `subtle = "2"` to `ff-rdp-cli` deps.
- [ ] `daemon/server.rs:57-68` `RefStore::register`: bound to `const MAX_REFS: usize = 50_000`; reject entries with `resolver.len() > 4096`.
- [ ] `daemon/buffer.rs:87-98`: truncate `nav_url` to `const MAX_NAV_URL_LEN: usize = 4096` chars on insert.
- [ ] `daemon/server.rs:1118-1123`: subscriber `types.insert(resource_type)` validates against `WATCHED_RESOURCE_TYPES`; unknown types return `{"error": "unknown resourceType"}` and are not stored.
- [ ] Audit additional unbounded inserts in the daemon — write a checklist in the PR description.

### B. Operator-surface
- [ ] Add `core/src/util/terminal.rs::sanitize_for_terminal(&str) -> Cow<str>` that replaces control chars (except `\n`, `\t`) with `?`.
- [ ] Wrap every `eprintln!(...)` that prints attacker-influenced text in `sanitize_for_terminal`. Audit targets: `commands/js_helpers.rs:24-25`, `commands/eval.rs:246-247`, `commands/network_events.rs:339`.
- [ ] At startup, if `FF_RDP_TRACE_RAW=1`, write a one-line warning to stderr.
- [ ] `daemon/server.rs:724` and any other dispatcher-path `lock().unwrap()` → `lock().unwrap_or_else(|p| p.into_inner())`. Log a `tracing::error!` once when a recovery happens.

### C. Bulk packet skip
- [ ] `core/src/transport.rs:476-533`: on a leading `b`, parse `bulk <actor> <kind> <length>:` and discard `length` bytes from the stream.
- [ ] Surface as `RdpError::Transport{cause: BulkPacketUnsupported{actor, kind, length}}` so daemon dispatch logs it once and continues, rather than killing the connection.
- [ ] Unit test against a synthetic bulk frame stream that the JSON parser keeps working after the skip.

### D. kb refresh
- [ ] `kb/rdp/from-our-codebase/lessons-learned.md#async-eval-doesnt-resolve-promises`: rewrite — now misleading. The fix landed in iter-61r: `mapped: {await: true}` is sent on every `evaluateJSAsync`. Cross-link `evaluate-js.md`.
- [ ] `kb/rdp/from-our-codebase/lessons-learned.md#screenshot-headless-chrome-scope`: annotate "closed in iter-61v, see `live_screenshot_full_page_dpr2`".
- [ ] `kb/rdp/from-our-codebase/lessons-learned.md#with-network-watcher-engagement` and `#headers-flips-source`: mark resolved (iter-61n/q).
- [ ] `kb/rdp/from-our-codebase/open-gaps.md`: move resolved entries (csp-eval-fallback, navigate-success-on-bad-dns, locale-pin, headers-source-regression, navigate-race-timeout, full-page-screenshot) to a new `## Closed gaps` section with `closed-in:` annotations.
- [ ] `kb/rdp/ff-rdp-wins.md` §4 (consoleActor staleness): annotate "wired in iter-61t" (or update if 61t deferred).
- [ ] `kb/rdp/actors/watcher.md`: add a "method support matrix" table showing which `get*Actor` methods ff-rdp implements after iter-61u.
- [ ] Add new page `kb/rdp/from-our-codebase/wired-vs-primitive.md` with a current snapshot of which 61p/q/r primitives are actually load-bearing.
- [ ] Update `kb/iterations/stability-roadmap.md` with the 61t..61w map and post-mortem of the 61m..61s deferrals.

## Acceptance Criteria [0/10]

- [ ] `cargo audit` and `cargo deny check` remain clean after `subtle` dep add.
- [ ] Statistical-timing test (1000 iterations) shows median timing for full-token comparison vs first-byte-mismatch comparison within 5% of each other.
- [ ] `test_refstore_capped`: register 100 000 refs in a tight loop; assert HashMap len caps at `MAX_REFS`.
- [ ] `test_nav_boundary_url_truncated`: emit a tabNavigated with a 1 MB URL; stored value is exactly 4096 chars.
- [ ] `test_terminal_escape_sanitized`: eval throws an exception containing `\x1b[2J`; stderr output contains `?` not the raw byte.
- [ ] `FF_RDP_TRACE_RAW=1 ff-rdp ...` prints the warning on first line of stderr.
- [ ] Poisoned-mutex injection test: the daemon dispatcher continues running and emits a `tracing::error!` event.
- [ ] Synthetic bulk-frame test: subsequent JSON frames parse correctly after the bulk skip.
- [ ] All kb pages listed in theme D updated; `hyalo find --property 'closed-in~=iter-' --format text` returns the expected set.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

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
