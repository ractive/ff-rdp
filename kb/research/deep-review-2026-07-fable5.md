---
title: "Deep review 2026-07 (Fable 5): protocol usage, daemon, RDP surface, Rust, docs"
type: research
date: 2026-07-09
status: open
tags: [research, review, daemon, rdp, rust, gaps]
---

# Deep review — 2026-07-09 (Fable 5, five parallel review agents)

Five specialized agents reviewed the codebase against: Firefox's own devtools client (`../firefox/devtools`), the full RDP spec surface (`devtools/shared/specs`), daemon-vs-one-shot parity, 2026 Rust best practices, and the online docs referenced by this kb. A sixth (Sonnet) Rust pass ran as cross-check. Findings below are deduplicated and exclude everything already tracked in [[open-gaps]] / [[lessons-learned]] / [[actors-we-use]].

## A. Correctness bugs (fix first)

1. **longString grips silently dropped in DOM/storage/computed paths** — `nodeValue`, DOM attribute values, cookie/storage values, and computed CSS values use bare `.as_str()`; a value > ~10 KB arrives as a `longString` grip and is reported as empty. Evidence: `actors/dom_walker.rs:310-324`, `actors/storage.rs:229-231`, `actors/page_style.rs:216`; the typed `LongString` decoder (`specs/types.rs:37-99`) is only used by console/object/watcher/network_event. Firefox declares these slots `longstring` (`specs/node.js:15,78`, `specs/storage.js`, `style/style-types.js:46`). Update [[lessons-learned]]#longstring-grips-everywhere — it lists network bodies as fixed but not these.
2. **Daemon never re-watches resources / re-attaches on cross-process target switch** — on `target-available-form` the daemon only bumps a counter and registers the actor; `watchResources`/`watchTargets` are issued once at startup and never re-issued; `is_top_level` is parsed but never used. Firefox's `target-command.js:230-281` + `resource-command.js` re-attach and re-watch on every switch. Daemon `--follow` across a cross-origin nav can silently drop console/network resources.
3. **Concurrent CLI invocations through the daemon can cross-deliver responses** — single "RPC writer" slot, each new client *replaces* it (`daemon/server.rs:1126-1145`); RDP has no request ids, so client B can receive client A's reply. Known limitation in code, but the daemon auto-starts by default so parallel `ff-rdp` calls hit it silently.
4. **Zombie daemon: unsupervised worker threads + self-masking idle timeout** — `firefox-reader`/`event-dispatcher`/`grip-release-drainer` are spawned with dropped JoinHandles, no `catch_unwind`, and none sets `state.shutdown` on exit (`daemon/server.rs:369-399`); a panic leaves a daemon that accepts clients that hang forever. Worse, `last_activity` is bumped on every accept including failed ones (`server.rs:1017,1026`), so the idle timeout never fires for a zombie.
5. **`TargetFront::reload(force=true)` bypasses reply matching** — last production caller of blind `transport.request()` (`fronts/target.rs:53`); a push event arriving before the reply desyncs the actor's reply stream. Route through `actor_request` and demote `request()` to test-only.
6. **`Registry::register` check-then-insert race can revive dead actors** — `get()` → `insert()` is not atomic on DashMap (`registry.rs:124-136`); concurrent `invalidate_target` is silently overwritten. Use the `entry` API.
7. **Daemon `ResourceBuffer` is one global VecDeque across all resource types** — a network burst evicts buffered console/error events (`daemon/buffer.rs:6,92-95`). Per-type caps or type-aware eviction needed. `MAX_EVENTS` behavior untested.
8. **`network --since` silently does nothing one-shot** — nav-window scoping implemented only on the daemon drain path (`commands/network.rs:43`); the direct path ignores the flag with no error.
9. Smaller daemon lifecycle races (medium): TOCTOU double-spawn in `resolve_connection_target` (`daemon/client.rs:263-320`); `handle_client` cleanup skippable via early `?` return (`server.rs:1174` vs `:1196-1207`); PID re-verified nowhere between port-owner lookup and kill (`client.rs:999-1009`); subscriber identity keyed on recyclable fd (`server.rs:1102,1196-1207`).

## B. Protocol robustness (vs how Firefox's client does it)

- **No traits/`hasActor` feature detection** — greeting traits are discarded (`transport.rs:309-316`); features gate on a parsed UA version range 120–150 (`connection.rs:14-15,128-142`), and if `ua` is absent all version gates silently no-op. Firefox policy: gate on traits, never version (backward-compatibility docs). Also adopt a `// @backward-compat { version XX }`-style marker for client-side shims.
- **Reply-vs-event heuristic is inverted vs Firefox** — "reply = no `type` field" (`transport.rs:1082-1113`) vs Firefox's per-actor spec-declared event set. Known/accepted (iter-54, [[lessons-learned]]#reply-vs-event) but fragile for every newly wired actor; no guard.
- **Close doesn't reject in-flight work** — Firefox purges all pending requests on transport close; daemon `DemuxReader` EOF just returns (`transport.rs:1037`). `forwardingCancelled` unhandled. No `{to:"root",type:"connect",frontendVersion}` handshake (Fx 133+).
- **`call_with_refresh` self-healing primitive is dead code** — only test callers (`registry.rs:323-341`); doc comment claims eval/dom/computed/snapshot/a11y use it (stale). `eval`'s hand-rolled retry misses `ActorDestroyed` and `EvalNavigatedDuringEval`.
- Late resource subscribers get no replay (no existing-vs-live concept, `resources/command.rs`); orphan `resources-updated-array` entries buffered for unknown ids (`daemon/buffer.rs:75-79`). Contained today; bites with multi-consumer daemon streams.
- **DemuxReader has no bulk-packet branch** — a bulk frame kills the daemon reader loop as a framing error (`transport.rs:1025-1040`). Only reachable if heap/perf-file features land.

## C. RDP functionality worth exposing (new gaps, prioritized)

Cheapest wins first (corrections: `getEventTimings` and `getApplied`/cascade are already done — earlier notes claiming otherwise were wrong):

1. **`target-configuration.updateConfiguration` CLI** — Front + live test already exist (`fronts/target_configuration.rs`, `live_61u.rs`), zero CLI consumers. Unlocks UA override, color-scheme simulation, DPPX override, print simulation, touch events, JS-disable, offline, cache-disable. Effort S.
2. **`network-event.getSecurityInfo`** — TLS version/cipher/cert/HSTS/weakness per request; same actor ids we already hold. Effort S.
3. **`manifest.fetchCanonicalManifest`** — parsed Web App Manifest + conformance errors (PWA audit; debug-skill E3 does this by raw fetch today). Effort S.
4. **`accessibilitywalker.startAudit`** — Firefox's native KEYBOARD/TEXT_LABEL/CONTRAST audit with node ancestries; we only hand-roll a JS contrast check. Async: consume `audit-event` pushes. Effort M.
5. **`network-parent.setNetworkThrottling` / `setBlockedUrls` / `blockRequest`** — Slow-3G perf audits, resilience testing. Note: no-`response`-block-but-not-oneway subtlety (like `walker.releaseNode`). Effort M.
6. **`node.getEventListenerInfo`** (S), **`node.getUniqueSelector`/`getCssPath`/`getXPath`** (S), **`walker.getMutations` + `new-mutations`** (M, needs node retention), **`page-style.getLayout` + `layout.getGrids`/`getCurrentFlexbox`** (S–M), **`compatibility.getNodeCssIssues`** (M), **reflow CLI** (S — actor exists in core, no command), **memory actor** (L — heap snapshot/GC/RSS).

From the Mozilla `firefox-devtools-mcp` comparison (it speaks **WebDriver BiDi**, not RDP — its GitHub tagline is stale): dialogs accept/dismiss, profiler start/stop, WebExtension install/list, privileged-context eval + pref get/set, hover/drag/upload primitives, restart_firefox, logpoints, Android. ff-rdp-only strengths: CSP-bypassing eval, a11y/CSS cascade/storage/geometry/scroll/run/record/index, single binary.

## D. Rust (2026)

- **Finish the error-taxonomy migration** — `RdpError`/`ProtocolError` coexist with a lossy `From` bridge: fabricated `after_ms: 0`, `ActorErrorKind` dropped, source chains severed (`error.rs:57-138`). Prefer `#[error(transparent)] Protocol(#[from] ProtocolError)` or complete the migration.
- **`#[non_exhaustive]` missing** on `RdpError`/`ProtocolError`/`ActorErrorKind`/`NavCause` — semver trap for a published 0.2.0 library.
- **Split-brain exit codes** — `AppError::exit_code()` returns 1 for variants `main.rs:208-223` maps to 4/5/6; the documented contract lies. Fold the mapping into `exit_code()`.
- **`error_type` JSON discriminants mix PascalCase and snake_case** — freeze + standardize.
- **`_demux` decoy defeats `check-dead-primitives`** — `daemon/server.rs:331-335` constructs an unused `DemuxReader` explicitly to pass the gate; `split_demux` docs claim production use falsely. Wire it (iter-77 promise) or remove; also avoid the per-packet `value.clone()` by recovering from `TrySendError`.
- **`setup_signal_handler` is a no-op with a doc claiming the opposite** (`server.rs:468-486`); stale rationale — libc/windows-sys already in-tree. SIGTERM leaves stale registry (incl. auth token) behind.
- **Process-global transport knobs** (`MAX_FRAME_BYTES_CELL`, `REDACT_THRESHOLD`, `TRACE_RAW_CACHE`) — test-lock machinery exists purely to fight them; move to a `TransportLimits` field.
- **No MSRV** (`rust-version`) + no toolchain pin — already bitten by clippy drift. **`serde_yaml` archived/deprecated** (migrate to serde_norway/serde_yaml_ng). Three `getrandom` versions. No `[workspace.lints.rust]` (use `unsafe_code="forbid"` instead of the hand-rolled scan test; CLI crate has no forbid at all).
- Residual `.expect()` outside tests: `transport.rs:480`, `screenshot.rs:97` (pub API), `screenshot_content.rs:53`.
- `FrontState.kind/target_root/alive` are `pub` — external code can bypass `invalidate_target`'s cascade (`registry.rs:52-63`).
- Bulk-header parser duplicated (`transport.rs:787-835` vs `697-736`) — attacker-facing, extract shared helper.
- Healthy (verified): framing parser + fuzz targets, bounded channels, poison-recovery, constant-time auth compare, atomic registry writes, no deny_unknown_fields/untagged, `ActorId` design, cargo-deny.

## E. Test-coverage gaps

- Daemon error-shape/exit-code parity: all error tests run `--no-daemon`; `daemon_parity.rs` asserts success shapes only.
- Idle-timeout shutdown and buffer-eviction caps: no unit/mock tests. Grip-release drainer only live-gated (regressed silently once before, pre-iter-76b).
- Windows daemon lifecycle effectively unverified in CI.

## F. Docs / kb deltas (from re-fetching the online docs)

- RDP is **not** deprecated; only CDP was (deprecated Fx129, removed Fx141). Mozilla's strategic investment is WebDriver BiDi; its own MCP is BiDi-based. Add a decision-log entry: RDP vs BiDi trade-off + migration target if standardization ever matters.
- kb fixes: `watchTarget` → `watchTargets` in [[connection-lifecycle]] and [[actor-model]]; note that client-api.html still documents the legacy attach flow (our getWatcher flow is sourced from actor-hierarchy/watcher-architecture); record the `@backward-compat` annotation convention; add pointers to `actor-best-practices.html`, `actor-registration.html`, `debugger-api.html`, `/remote/index.html`.
- Add a `firefox-devtools-mcp` comparison doc next to [[chrome-mcp-comparison]]; label it a **BiDi** competitor.

## Suggested iteration seeds

1. iter: daemon hardening — findings A2, A3, A4, A6, A9 + parity tests (E).
2. iter: longString sweep — A1 (+ regression fixtures with >10 KB values).
3. iter: `target-configuration` CLI (C1) — smallest new-feature win.
4. iter: error taxonomy completion (D items 1–4) before next release cut.
5. iter: security/PWA audit pack — C2 + C3 (+ optional C5 throttling).
