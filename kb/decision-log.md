---
title: Decision Log
type: reference
date: 2026-04-06
tags: [decisions, architecture]
status: active
---

# Decision Log

## DEC-001: Use Firefox RDP directly over TCP, not WebDriver BiDi

**Decision**: Communicate with Firefox using the native Remote Debugging Protocol over raw TCP, not through WebDriver BiDi or Selenium.

**Why**: Mozilla's own firefox-devtools-mcp uses WebDriver BiDi via Selenium, adding Node.js + Selenium as intermediaries. Raw RDP over TCP eliminates all middleware for minimum latency. The protocol is simple (length-prefixed JSON) and well-suited for a Rust implementation.

**Trade-off**: No formal JSON schema for RDP (unlike Chrome's CDP). Must reverse-engineer actor capabilities from geckordp source and Firefox DevTools source code.

## DEC-002: Stateless CLI (connect-per-invocation)

**Decision**: Each CLI invocation opens a TCP connection, performs one operation, and disconnects. No persistent daemon or connection pooling.

**Why**: Simplicity. Firefox maintains all state (tabs, page content, console history). A stateless CLI is trivially composable with shell pipelines and Claude Code's Bash tool. Connection overhead is ~5ms on localhost, negligible compared to the value of simplicity.

**Trade-off**: Cannot stream real-time events (e.g., live console tailing). Can add a `--follow` mode later if needed.

## DEC-003: JSON-only output initially

**Decision**: Only JSON output format. No text/table format in v1.

**Why**: The primary consumer is Claude Code (an LLM), which parses JSON natively. The built-in `--jq` flag handles all formatting needs for human readers. Adding a text format doubles the output code for minimal benefit.

**Revisit**: If human CLI usage grows, add `--format text` in a later iteration.

## DEC-004: Tab targeting by index, URL pattern, or actor ID

**Decision**: `--tab <value>` accepts an integer (1-based index), a string (URL substring match), or `--tab-id <actor>` for precise targeting. Default is the active/selected tab.

**Why**: Inspired by cmux's `--surface <id|ref|index>` pattern. Index is fastest for interactive use, URL pattern is most intuitive, actor ID is for precise scripting. Active tab default means zero flags for the common case. 1-based indexing chosen because it's more natural for humans (`--tab 1` = first tab) and matches the convention in iteration plan examples.

## DEC-005: Crate split — ff-rdp-core + ff-rdp-cli

**Decision**: Two crates following hyalo's pattern. Core is the protocol library (no CLI deps), CLI is the user interface.

**Why**: Core can be reused as a library (e.g., in an MCP server, test framework, or other tool). Clean dependency boundaries: core uses thiserror, CLI uses anyhow. Core is async (tokio), CLI wraps it in a single-threaded runtime.

## DEC-006: thiserror in core, anyhow in CLI

**Decision**: Core library uses typed errors via thiserror. CLI wraps them with anyhow for context chaining.

**Why**: Following hyalo's established pattern. Library consumers need typed errors for matching; CLI just needs human-readable messages with context.

## DEC-007: JSON envelope with meta field

**Decision**: Every command outputs `{"results": ..., "total": N, "meta": {"tab": {...}, "duration_ms": N}}`.

**Why**: Adapted from hyalo's envelope (`results` + `total` + `hints`). Added `meta` instead of `hints` because: (a) we don't need drill-down hints for a debugging tool, (b) knowing which tab was targeted and how long the operation took is valuable debugging context. The envelope is consistent across all commands, enabling `--jq` to operate on a predictable shape.

## DEC-008: Use eval as the implementation for most interaction commands

**Decision**: Commands like `click`, `type`, `dom`, `page-text`, `cookies`, `storage` all use `evaluateJSAsync` internally rather than native protocol actors.

**Why**: The eval approach is simpler (one actor needed), more reliable (JavaScript execution is the best-tested path), and covers 95% of use cases. Native actors (Inspector, Walker, Node) provide structured DOM trees but require complex multi-step actor initialization. Eval-based implementations can be swapped for native ones later without changing the CLI interface.

**Trade-off**: Cannot access HttpOnly cookies, cannot inspect shadow DOM internals, cannot get computed styles directly. These are edge cases deferrable to later iterations.

## DEC-009: Blocking std I/O instead of async/tokio

**Context**: ff-rdp is a stateless CLI tool — each invocation opens one TCP connection, sends one request, reads one response, and exits. There is no concurrent I/O or multiplexing.

**Decision**: Use blocking `std::net::TcpStream` with `set_read_timeout`/`set_write_timeout` instead of tokio async runtime.

**Rationale**:
- Simpler code: no async/await coloring, no tokio runtime boilerplate
- Smaller binary: removes the tokio dependency from both core library and CLI
- Faster compilation: tokio is a heavy dependency tree
- Easier testing: plain `#[test]` instead of `#[tokio::test]`, mock server uses `std::thread::spawn`
- Timeouts are handled natively by socket-level `set_read_timeout`/`set_write_timeout`
- Even iteration 4 (console/network monitoring) is just a blocking read loop — no concurrency needed
- The core library can be embedded in any context without requiring a tokio runtime

**Alternatives considered**:
- Keep tokio: rejected because the async complexity is not justified for sequential request/response over a single TCP connection

## DEC-010: Filter unsolicited events in actor_request

**Decision**: `actor_request` loops on `transport.recv()` until it receives a packet whose `from` field matches the target actor, silently discarding interleaved events.

**Why**: Firefox can emit unsolicited events (tabNavigated, tabListChanged, etc.) at any time on the same TCP connection. The previous single-recv approach would misinterpret an event as the response, causing spurious errors. Filtering by `from` field is the simplest correct approach.

**Trade-off**: Discarded events are lost. This is acceptable for a stateless CLI that connects, does one thing, and exits. A future REPL or streaming mode would need an event buffer or callback mechanism.

## DEC-011: Async eval pattern with resultID correlation

**Decision**: `evaluateJSAsync` sends a request, captures the `resultID` from the immediate ack, then loops on `recv()` until an `evaluationResult` message with a matching `resultID` arrives.

**Why**: Firefox's `evaluateJSAsync` is inherently two-phase: an immediate response confirming the request (with a `resultID`), followed by a separate event containing the actual result. The resultID correlation ensures we match the correct result even if other events are interleaved.

## DEC-012: WatcherActor resource subscription for network events

**Decision**: Network monitoring uses the WatcherActor's `watchResources`/`unwatchResources` pattern rather than individual NetworkEventActor requests. Subscribe to `"network-event"` resources, then collect `resources-available-array` and `resources-updated-array` events in a timeout-bounded recv loop.

**Why**: Firefox's Watcher pattern is the modern approach (replacing the older NetworkMonitor). It delivers events in nested array format `[["network-event", [resources]]]` with resource-available for initial data (method, URL, actor) and resource-updated for completion data (status, timing, size). Merging by `resourceId` gives a complete picture. This matches how Firefox DevTools itself works.

**Trade-off**: The recv loop must drain events with a timeout, making the command slightly slower than a single request-response. The `--timeout` flag controls this. No streaming/follow mode yet — the loop exits when the timeout fires.

## DEC-013: Watcher subscriptions are connection-scoped — no cross-invocation reuse

**Decision**: Accept that each CLI invocation gets its own watcher subscriptions that die with the connection. For capturing traffic during navigation, use `navigate --with-network` (same connection, subscribe → navigate → drain). For retrospective queries, use `network --cached` (Performance Resource Timing API via eval).

**Why**: Firefox RDP actor IDs encode the connection (`conn0`, `conn1`). When a TCP connection drops, all actors and subscriptions are invalidated server-side. There is no session token, cookie, or persistence mechanism. Verified empirically: `watchResources` does NOT replay buffered network events — it is purely real-time.

**Trade-off**: Cannot share watcher state across CLI invocations without a persistent proxy process. An SSH ControlMaster-style approach is documented in `research/connection-persistence.md` for future consideration.

## DEC-014: Performance API as separate concern from RDP network watcher

**Decision**: `network --cached` uses the W3C Performance Resource Timing API (`performance.getEntriesByType("resource")`) via eval. This is a temporary home — it will be extracted into a dedicated `perf` command (iteration 8) that covers the full Performance API family (navigation waterfall, paint milestones, LCP, CLS, long tasks).

**Why**: The Performance API is eval-based browser introspection (like `page-text`), not an RDP protocol feature. Mixing it into the `network` command conflates two unrelated data sources. The `--cached` flag stays for now since the implementation works, but the architectural intent is separation.

## DEC-015: LongStringActor for fetching truncated eval results

**Decision**: Added `LongStringActor::full_string()` in ff-rdp-core to fetch complete content when Firefox returns a `longString` grip (strings > ~1000 chars). Used by `network --cached` when pages have many resources.

**Why**: Firefox truncates long string results in eval responses, returning a `longString` grip with an actor ID, initial prefix, and total length. The `substring` RDP method on the StringActor fetches the full content. This is a protocol-level concern (correctly in ff-rdp-core), needed by any consumer that evaluates JS producing large output.

## DEC-016: Connection daemon with virtual actor protocol

**Decision**: Introduced an SSH ControlMaster-style background daemon that holds a persistent Firefox RDP connection, subscribes to watcher resources, and buffers events. CLI invocations connect to the daemon via TCP loopback instead of directly to Firefox. The daemon exposes a `"daemon"` virtual actor on the same wire format (length-prefixed JSON) for draining buffered events and status queries.

**Why**: Each CLI invocation previously opened a fresh TCP connection (~50-100ms overhead). For AI agent workflows running 5-10 commands in sequence, this adds up. More critically, watcher subscriptions are connection-scoped (see [[#DEC-013]]) — `navigate` and `network` on separate connections can't share events. The daemon solves both: connection reuse eliminates overhead, and persistent subscriptions enable cross-command workflows like `navigate` then `network`.

**Trade-off**: Added complexity (daemon process management, registry file, signal handling). Mitigated by: auto-start/auto-stop lifecycle, TCP loopback (cross-platform), virtual actor (same wire format as RDP — no new framing), serialized one-client-at-a-time access (avoids multiplexing complexity). The `--no-daemon` flag preserves the original direct behavior. See [[research/gradle-daemon-architecture]] and [[research/connection-persistence]] for the analysis that informed this design.

## 2026-04-07: Output Size Control Principles

**Context**: As an LLM-focused CLI tool, ff-rdp output must be bounded by default to avoid flooding agent context windows.

**Decision**: All list-returning commands follow these principles:

1. **Bounded by default**: Every list command has a sensible default `--limit` (typically 20 for resource lists, 50 for console messages). Use `--all` to override.

2. **Summary over detail**: Commands with many entries (network, perf) default to a summary view. Use `--detail` for individual entries.

3. **Transparent truncation**: When results are limited, the envelope includes `"truncated": true`, `"total": N` (actual total), and a `"hint"` string so agents know data was omitted.

4. **Output controls are global flags**: `--limit N`, `--all`, `--sort <field>`, `--asc`/`--desc`, `--fields <f1,f2>`, and `--detail` are available on ALL commands.

5. **`--jq` implies detail mode**: When a jq filter is provided, the command skips summary mode and returns individual entries (since the user wants to process raw data).

6. **Document order preserved**: DOM-related commands maintain document order by default. Use `--sort` to override.

7. **Tree output**: Tree-producing commands (snapshot, a11y, dom tree) use `--depth N` and `--max-chars N` for size control, with consistent truncation markers. (Design only — implemented in iterations 22-24.)

**Applies to**: All current and future list/tree-returning commands.

## 2026-04-16: WohnungsDirekt Fixture as Built-in Eval

**Context**: Iteration 42 introduced the `/site-audit` skill and needed a reproducible test target.

**Decision**: Include a deliberately broken apartment listing page (`tests/fixtures/wohnungsdirekt/`) in the repository with 33 planted issues as a built-in eval fixture.

**Why**: A controlled fixture with known ground truth (issues.json) enables deterministic evaluation of the skill's detection capabilities. Real websites change unpredictably, making them unsuitable as regression baselines. The fixture also serves as a demo for the audit-fix-verify loop — the skill's killer workflow. The 33 issues span 6 categories (perf, a11y, SEO, security, structure, UX) at 3 difficulty levels.

**Trade-off**: Maintaining a ~600-line HTML fixture adds repository weight. Acceptable because the fixture is small, static, and doubles as documentation of what the audit can detect.
