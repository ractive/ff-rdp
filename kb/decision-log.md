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

## 2026-05-10: Daemon TCP Auth — Local Multi-User Threat Model

**Context**: Iteration 55 introduced a 32-byte random auth token on the daemon's TCP listener. This documents the threat model and the before/after security posture.

### Before (iter-54 and earlier)

The daemon listened on a loopback TCP port without any authentication. Any local process that could connect to `127.0.0.1:<proxy_port>` could send arbitrary RDP commands — reading cookies, evaluating JavaScript, capturing screenshots — as long as it knew or guessed the port number. The port was written to `~/.ff-rdp/daemon.json` (mode 0o600), but the daemon itself didn't verify caller identity.

**Affected threat actors**:
- DNS-rebinding from a malicious page in any browser tab (browser can connect to `127.0.0.1:<port>` using `fetch()`)
- A compromised npm postinstall script running as the same UID
- A CI runner with shared UID that reuses a port already occupied by a previous job
- A devcontainer or sidecar container that shares the host network namespace

**Not in scope**: processes running as a different UID — they cannot read `~/.ff-rdp/daemon.json` (0o600).

### After (iter-55)

On daemon start, `daemon/registry.rs` generates a 32-byte token using `getrandom::getrandom()` (OS CSPRNG) and encodes it as 64 hex characters. The token is written into `~/.ff-rdp/daemon.json` (0o600).

The first frame every new daemon client must send is `{"auth": "<hex-token>"}`. A wrong token or missing auth frame → socket closed immediately. No RDP frames are forwarded.

The ff-rdp client reads the token from the registry and sends it before any other request.

**Remaining gap**: A process that can read `~/.ff-rdp/daemon.json` can also steal the token. This is equivalent to full home-dir access and is out of scope for this mechanism. The goal is to defeat processes that can `connect()` to loopback but cannot read `$HOME` (DNS-rebinding, sandboxed apps, CI sidecars).

**Why not Unix-domain sockets instead?** UDS provide the same protection (filesystem permissions) with no token overhead, but require platform branching and break the "same wire format everywhere" property. The token approach is ~50 LOC vs ~200 LOC for UDS/named pipes. Deferred to a later iteration if multi-user deployments become a real customer ask.

**Applies to**: `daemon/server.rs` (listener), `daemon/client.rs` (auth handshake), `daemon/registry.rs` (token generation + storage).

## 2026-05-15: Pointer-only as default dispatch mode (iter-59)

**Context**: Iteration 59 added a `--dispatch` flag with three modes: `pointer`, `legacy`, `click-only`. We had to choose a default.

**Decision**: Default to `pointer` (full PointerEvent sequence: pointerover → pointerenter → pointerdown → pointerup → click, plus matching MouseEvents).

**Why**: Radix UI, Headless UI, Floating UI, and most modern React component libraries listen for `pointerdown` to trigger open/close. The old `.click()` fallback missed these handlers in session 44, causing the logout button to silently fail. `pointer` is a strict superset of `legacy` and `click-only` — it fires every event the others fire, plus the pointer events. Cost is a handful of extra JS `dispatchEvent` calls per click, negligible compared to the round-trip to Firefox.

**Trade-off**: Older pages that detect event type as `'pointer'` and treat it differently than `'mouse'` could behave unexpectedly. The `--dispatch legacy` and `--dispatch click-only` flags exist as escape hatches for those cases. In practice this is rare — most event handlers ignore `event.type` once dispatched.

**Applies to**: `commands/click.rs`, `commands/js_helpers.rs` (`build_click_js`).

## 2026-05-24: Iteration discipline tooling (iter-61y)

**Context**: The iter-61m..61s postmortem identified eight process root causes for recurring claim/code gaps: primitives introduced but not wired, TODOs without issue links, iteration plans without dogfood paths or first-call-site declarations. The same failure modes recurred in iter-61t..61v. Humans are bad at noticing what's *missing* — tools are good at it.

**Decision**: Encode the three most frequent failures as mechanical CI gates, not prose rules.

### Dead-primitive check (`cargo xtask check-dead-primitives`)
Every new `pub fn/struct/enum/trait/mod` introduced in the diff since `origin/main` must have at least one non-test consumer in the workspace. Zero matches → fail CI.

**Motivation**: iter-61s shipped `Registry::new` and `ScopedGrip::new` as dead code. The claim "adds X" appeared in commit messages while X had no callers. No tool caught this because the existing tests still passed — dead code doesn't break tests.

### TODO annotation rule (`cargo xtask check-todo-annotations`)
New `TODO`/`FIXME`/`XXX` comments must include a GitHub issue link, Jira-style ticket, or `// allow-todo: <reason>`. Bare annotations are rejected.

**Motivation**: The iter-61s `dispatch_event` TODO knew about `resources-destroyed-array` but was never tracked, and the context was lost across iterations. Annotated TODOs create a paper trail that survives context windows.

### Iteration plan lint (`cargo xtask check-iteration-plan`)
Every iteration plan that introduces new pub items must declare `first_call_sites` in frontmatter, and all plans must declare `dogfood_path`. This makes the claim–code relationship explicit before implementation begins.

**Implementation**: All three checks live in `crates/xtask` (a `publish = false` workspace crate). A pre-commit hook in `.githooks/pre-commit` runs the TODO check locally. The same checks run in the CI `discipline` job on every PR. See `CONTRIBUTING.md` for developer instructions.

**Applies to**: `crates/xtask/`, `.githooks/pre-commit`, `.github/workflows/ci.yml`, `CLAUDE.md`, `kb/iterations/_template.md`.

## 2026-05-24: Discipline skill integration (iter-61z)

**Context**: iter-61y landed the in-repo discipline tooling (dead-primitive, todo-annotation, iteration-plan-lint) but deferred the two checks that have to live inside the ralph-loop skill itself — Phase 2 "Claims vs code" PR-description diff and the AC-fidelity merge gate. The cmux child workspace cannot write to `~/.claude/skills/`, so those couldn't be done from the same loop. The postmortem ([[iter-61m-61s-postmortem-loose-ends]]) lists them as mitigations #4 and #5.

**Decision**: Close the deferral by editing the skill directly (outside the cmux loop) and add a regression check that pins the behaviour.

### `claims-vs-code.sh`
Extracts code-shaped tokens from `git log main..<branch>` commit messages — verb+symbol pairs (`adds Foo`, `implements Bar`), `::`-qualified paths, `SCREAMING_SNAKE_CASE`, and known kebab-event prefixes (`dom-*`, `chrome-*`, `target-*`). For each, greps the branch diff (excluding `kb/` and `*.md` to avoid the plan file producing false ✅s). Emits a `## Claims vs code` markdown section with ✅/❌ rows. Exit 1 if any ❌ remains. Phase 2 appends the report to the PR body so reviewers see the gap surface.

### `ac-fidelity-check.sh`
Parses the iteration plan's `## Acceptance Criteria` block. For each ticked checkbox, looks for evidence in the diff: a test-function slug, a backtick-quoted symbol, a `::`-qualified ident, or a `[deferred — new plan: <path>]` annotation pointing to an existing follow-up plan. Build-process ACs (`cargo fmt && cargo clippy ...`) are whitelisted. Exit 1 if any ticked AC lacks evidence.

### `run-iteration.sh --replay <iter-id>`
New replay mode that re-runs both checks against an already-merged branch. Finds the merge commit on main, derives the branch-side diff via `<merge>^1..<merge>^2`, and writes a structured report to `$RALPH_CACHE_DIR/replay-<iter-id>.txt`. Used as the regression baseline: `--replay 61v` fails (the dom-interactive / chrome-context tokens didn't actually land in iter-61v's code; iter-61x cleaned them up); `--replay 61t` passes.

### `cargo xtask check-discipline-regression`
Diffs `tools/ralph-loop/scripts/` against `~/.claude/skills/ralph-loop/scripts/` and bails on drift, then runs both replay baselines. Wired into the CI `discipline` job — on CI runners the skill dir is absent so the mirror-sync half is skipped, but the replay half still runs against the checked-in mirror. The two combined ensure the heuristics can't silently regress and the mirror can't go stale.

**Motivation**: iter-61t..v had ticked ACs and confident commit messages for code that hadn't actually landed. The pattern was only caught retroactively in iter-61x's "honest commits" cleanup. Mechanical PR-time + merge-time gates make the gap visible while the implementing agent is still in context to fix it.

**Trade-off**: The regex-based extraction is conservative and biased toward false ✅ (per the iter-61z plan's design notes — a false fail blocks all merges). It catches the obvious cases (a `RdpError::Navigation` token claimed but absent from the code diff) without being a fully precise lint. The replay baselines defend against future drift.

**Applies to**: `~/.claude/skills/ralph-loop/scripts/{claims-vs-code,ac-fidelity-check,run-iteration}.sh`, `tools/ralph-loop/`, `crates/xtask/src/check_discipline_regression.rs`, `.github/workflows/ci.yml`, `CLAUDE.md`.

## DEC-017: Vendor Consent-O-Matic XPI rather than downloading at install time

**Decision**: Embed the Consent-O-Matic 1.1.5 XPI at compile time via `include_bytes!` (`crates/ff-rdp-cli/assets/extensions/consent-o-matic-1.1.5.xpi`) and write the bytes into the launched profile's `extensions/` directory. Drop the AMO download path from `commands/auto_consent.rs`.

**Why**: The pre-iter-64 implementation downloaded the XPI from `addons.mozilla.org` on every fresh launch with `--auto-consent`, with TLS as the only integrity guarantee — no SHA-256 pin, no signature verification, no download size cap. The 2026-05-24 security review (finding F-3) flagged this as the highest-impact issue: a hostile network, compromised CA, or AMO compromise could substitute a malicious WebExtension that runs in the user's Firefox with full WebExtension permissions (RCE). Vendoring removes the network call entirely, so the only way to swap the XPI is to ship a malicious release of ff-rdp itself — at which point the user has bigger problems.

**Trade-off**: The crate gains a ~96 KB binary blob and we take on a manual re-vendor step when upstream releases a new Consent-O-Matic version. Consent-O-Matic is slow-moving (1.1.5 has been current since 2024), so the maintenance overhead is low. A pinned-SHA download was the alternative; vendoring is strictly safer because it also defends against AMO outages and against an attacker who can flip just the hash file.

**Applies to**: `crates/ff-rdp-cli/src/commands/auto_consent.rs`, `crates/ff-rdp-cli/assets/extensions/{consent-o-matic-1.1.5.xpi,LICENSE-consent-o-matic.txt}`, iter-64.

## DEC-018: Transparent `RdpError::Protocol(ProtocolError)` bridge, not a full migration

**Decision** (iter-105, Theme A): Replace the flattening `From<ProtocolError> for RdpError` impl with a single `#[error(transparent)] Protocol(#[from] ProtocolError)` variant that passes `ProtocolError` through **losslessly**. Do NOT collapse the two error types into one (the "full migration" alternative the plan left open).

**Why**: The former impl destroyed information — it fabricated `Timeout { after_ms: 0 }`, dropped the `ActorErrorKind` discriminant (so `noSuchActor` became indistinguishable from `wrongState`), and stringified everything else into `Shape { got: … }`, severing `io::Error` source chains. The transparent variant keeps `ProtocolError`'s carefully-typed surface intact and reaches the CLI's existing `From<ProtocolError> for AppError` mapping, which already distinguishes every `ActorErrorKind`. It is strictly additive: the only in-tree consumer of the old `From` impl (`WatcherActor::unwatch_targets`, which does `actor_send(...)?` into an `RdpResult`) keeps compiling via the `#[from]`.

**Why not the full migration**: A full merge of `RdpError` and `ProtocolError` would be a large, cross-cutting API change touching every actor's return type — it would balloon this release-prep PR and, per the carry-over rule, would need its own plan. `is_transient()`'s in-crate exhaustive match over `ActorErrorKind` (the value of keeping `ProtocolError` typed) is unaffected by the change.

**Trade-off**: `RdpError` now has two "protocol-ish" surfaces during the migration window (`Protocol(ProtocolError)` plus the still-present `Shape`/`Timeout`/`Transport` variants). That is acceptable: `#[non_exhaustive]` (Theme B) means a future full migration can retire the redundant variants without a breaking change.

**Applies to**: `crates/ff-rdp-core/src/error.rs`, `crates/ff-rdp-cli/src/error.rs`, iter-105.

## DEC-019: `FrontKind` keeps `Other(String)` catch-all + gains `#[non_exhaustive]`

**Decision** (iter-105, Theme B look-and-decide pass): Add `#[non_exhaustive]` to the four *error* enums (`RdpError`, `ProtocolError`, `ActorErrorKind`, `NavCause`) as planned. For the adjacent `registry::FrontKind` enum — which the plan flagged as gaining a variant nearly every iteration — add `#[non_exhaustive]` **as well**, since it is the same mechanical fix in the same PR and closes the same breaking-change risk for the `FrontKind::Manifest`/`TargetConfiguration`-style additions.

**Why**: `FrontKind` already carries an `Other(String)` catch-all, which softens the risk for *construction* but does not help downstream `match`es: a non-defining-crate exhaustive match still breaks when a named variant is added. `#[non_exhaustive]` forces those matches to carry a wildcard, making variant additions non-breaking. The `Other(String)` catch-all is retained (it serves a different purpose — round-tripping unrecognised front kinds by name).

**Applies to**: `crates/ff-rdp-core/src/registry.rs`, iter-105.

## DEC-020: `eval` CSP "chrome bypass" stays deleted — assert the CSP-safe page-await path, not a `meta.eval_path == "chrome"`

**Decision** (iter-106, Theme A): The un-masked `live_eval_chrome_csp_bypass` test (from iter-61x) failed because it asserted `meta.eval_path == "chrome"`, but that value no longer exists. **Keep the iter-93 design** (the chrome-context bypass was deliberately removed) and rewrite the test to assert the still-load-bearing guarantee: `eval` succeeds and returns the correct value on a `script-src 'none'` page, via the CSP-safe `page-await` path (`meta.eval_path == "page-await"`).

**Why**: Firefox's `evaluateJSAsync` routes through `Debugger.evalInGlobal` (`devtools/server/actors/webconsole/eval-with-debugger.js:119-247`), which operates at the Debugger-API level, *outside* the page's scripting environment, and is therefore not subject to page CSP. iter-93 (`commit 4b18939`) proved the extra `getProcess(0)` → parent-process console-actor hop was unnecessary and removed it; `eval_path` is now hard-set to `"page-await"`. Re-introducing a `"chrome"` path to satisfy an obsolete assertion would restore dead weight and re-open the very CSP-eval failure iter-93 fixed. Verified live: `eval "1+1"` on a CSP-blocked page returns `2`.

**Applies to**: `crates/ff-rdp-cli/src/commands/eval.rs`, `crates/ff-rdp-cli/tests/live/live_61l.rs`, iter-106.

## DEC-021: Cross-invocation daemon network buffer — atomic nav boundary + lossless update serialization

**Decision** (iter-106, Theme D): Fix the "second `ff-rdp network` invocation sees zero entries after a first `navigate --with-network`" bug with **two** changes, not one: (1) `store-events` now carries `navUrl` and the daemon records the nav boundary **atomically** with the event inserts (`ResourceBuffer::record_boundary_and_insert`); (2) `serialize_network_resources_for_buffer` emits the real `resources-updated-array` wire shape (top-level `resourceId` + object-valued `resourceUpdates`) instead of the array-nested shape the drain-side parser could not read.

**Why**: The failure had two compounding causes, confirmed against live Firefox. First, a **thread race**: the daemon's reader loop records the `tabNavigated` boundary asynchronously; it could land *after* the CLI's `store-events` inserts, pushing the boundary's `store_start` past every stored event so the default `--since -1` scope resolved to empty. Recording the boundary in the same critical section as the inserts removes the race (a duplicate reader-loop boundary for the same URL is harmless — it is older). Second, a **serialization mismatch**: `serialize_network_resources_for_buffer` wrote updates as `{"resourceUpdates": [{"resourceId": …}]}`, but `parse_network_resource_updates` reads a top-level `resourceId` and treats `resourceUpdates` as an object — so on the second-invocation drain every update was silently dropped, leaving `status`/`transfer_size` null even once the boundary was fixed. This is a **distinct** root cause from iter-101 Theme B (RPC-writer cross-delivery), exactly as iter-106's plan predicted: the symptom was "empty/partial results", not "wrong client got the reply".

**Applies to**: `crates/ff-rdp-cli/src/daemon/{buffer,server,client}.rs`, `crates/ff-rdp-cli/src/commands/{navigate,network_events}.rs`, iter-106.

## DEC-022: `cargo test-live` runs the suite serially (`--test-threads=1`) — mitigation, not parallel-safety

**Decision** (iter-114, sweep methodology): The `test-live` alias in `.cargo/config.toml` now appends `--test-threads=1`, matching the CI live lane and the iter-110 sweep baseline. The live suite is declared **serial-only**: green means "green under `--test-threads=1`", and parallel runs are explicitly unsupported.

**Why**: Each live test launches its own headless Firefox on a random port, but several tests perform machine- or process-global operations — the daemon-stop suites kill daemons, `live_profiles_prune_removes_all_when_no_firefox_running` prunes profiles assuming nothing else runs, kill-scoping tests signal processes. Under parallelism they destroy each other's Firefox instances. Evidence from the iter-114 sweep: 14 live-binary reds in a parallel run, 13 of them outside the iteration's inventory, and all 13 pass in a single serial invocation. The interference was newly *visible* (not new): before iter-114, ~20 tests failed in milliseconds on port-6000 connection-refused and never held a Firefox instance, so effective concurrency was too low to collide. The CI live lane (`.github/workflows/live.yml`) and the iter-110 recorded sweeps already used `--test-threads=1`; the alias was the outlier, so the documented sweep command ran the suite in a configuration it had never been green under.

**Related but distinct**: process-global state *inside* the test binary bites even serially, depending on run order — `live_bulk_cap` leaked a 1 KiB `set_max_frame_bytes` cap that failed `live_console_no_double_delivery` with `FrameTooLarge`. Fixed in iter-114 with a panic-safe RAII restore guard; that class of bug is not addressed by serialization and must be fixed per-test.

**Trade-off**: Local full sweeps now take ~20 minutes instead of ~3. CI is unaffected (already serial; no `timeout-minutes` override, so well under GitHub's default 6 h cap).

**Revisit condition**: If sweep wall-clock becomes a real cost, the cure is making the global-operation tests parallel-safe — scope daemon kills and profile prunes to the test's own instances (PID- or profile-tagged) — after which the `--test-threads=1` can come back out. File that as its own iteration; do not simply remove the flag.

**Applies to**: `.cargo/config.toml`, `.github/workflows/live.yml` (unchanged, already conformant), `crates/ff-rdp-cli/tests/live/live_bulk_cap.rs`, iter-114.

## DEC-023: `navigate --auto-consent` is a new, separate flag — `launch --auto-consent`'s Consent-O-Matic install is unchanged

**Decision** (iter-129, Theme C): Native CMP detection/acceptance is wired up as a **new** `--auto-consent` flag on `navigate` (mutually exclusive with `--with-network`) plus the explicit `ff-rdp consent accept` command. `launch --auto-consent` keeps its pre-129 behaviour verbatim — it still only installs the Consent-O-Matic extension into the profile before Firefox starts.

**Why**: The plan text ("wired into `--auto-consent` post-navigate") is compatible with two readings: extend the existing `launch`-only flag so it retroactively changes `navigate`'s behaviour with no new flag on `navigate` itself, or add the same flag name to `navigate` where the actual post-navigate action happens. The first reading requires either persisting state across separate process invocations (no such mechanism exists in ff-rdp's stateless-CLI model outside the daemon) or making the daemon carry a launch-time flag — a materially bigger, cross-process design not scoped by this plan's themes. The second reading is additive, keeps `launch --auto-consent` meaningful on its own (Consent-O-Matic still helps on non-headless / non-Sourcepoint sites), and directly matches how every other post-navigate opt-in in ff-rdp already works (`--with-network`, `--wait-text`, `--wait-for`). Chosen for scope discipline, per the plan's own "cut ballooning work into a deferred plan" guidance rather than force-fitting a cross-process flag design into this PR.

**Consequence**: The plan's `dogfood_path` frontmatter (`ff-rdp launch --headless --auto-consent` then bare `ff-rdp navigate ...`) does not exercise native consent handling as originally sketched — the working dogfood command is `ff-rdp navigate https://www.theguardian.com --auto-consent`. Updated in the iteration-129 plan file directly rather than left silently stale.

**Applies to**: `crates/ff-rdp-cli/src/cli/args.rs` (`NavigateArgs::auto_consent`), `crates/ff-rdp-cli/src/commands/{navigate,consent}.rs`, `kb/iterations/iteration-129-consent-and-cross-origin-frames.md`, iter-129.

## DEC-024: `click --frame` / frame-scan zero-match error names URLs, not node counts

**Decision** (iter-129, Theme B): When a selector matches nowhere — neither the top document nor any scanned frame — `click` fails with `"selector '<sel>' matched in 0 of N frames (top + N-1 subframes: <url1>, <url2>, …)"` rather than the pre-129 bare "element not found" / 10s timeout. The frame-scan only triggers when the top-level eval's thrown error contains the literal marker `"Element not found:"` (`ELEMENT_NOT_FOUND_MARKER` in `click.rs`) — any other JS exception (a genuine runtime error) propagates immediately and does not pay for a scan that cannot help.

**Why**: The frame-targets research (`kb/research/frame-targets.md`) confirmed the failure mode this replaces: on theguardian.com, `click "button:has-text('Accept all')"`-equivalent selectors timed out after 10s with no indication the target lived inside a cross-origin Sourcepoint iframe invisible to the top-level `querySelector`. Naming the frame URLs tried turns an opaque timeout into an actionable diagnostic (`click --frame sourcepoint ...` is the documented next step). Matching on the error-message marker rather than a distinct error type keeps the fast top-level path unchanged (`build_click_js` already throws that exact string) — no new eval-side error protocol was needed.

**Applies to**: `crates/ff-rdp-cli/src/commands/click.rs`, iter-129.

## DEC-025: `snapshotScale` is always sent — "omit at the server default" was never true

**Decision** (iter-135): ff-rdp always serialises `snapshotScale` in the `screenshotActor.capture` args, even when `windowDpr * windowZoom == 1.0`. The `Option<f64>` on `ScreenshotArgsExt` became a plain `f64`; `ScreenshotFront::capture` always passes `Some(scale)`.

**Why**: iter-77 dropped the field at 1.0 to keep outbound bytes at the pre-shim baseline, on the assumption that Firefox defaults it. It does not. `devtools/server/actors/utils/capture-screenshot.js` reads `const ratio = args.snapshotScale;` with no fallback and passes it straight into `browsingContext.currentWindowGlobal.drawSnapshot(rect, ratio, …)`; `snapshot.width / undefined` is `NaN`, `canvas.toDataURL` throws, the catch-all returns `null`, and the retry guard `!data && ratio > 1.0` is `false` for `undefined`. Firefox then replies `{"value":{"data":null,…,"messages":[{"level":"error","text":"<screenshotRenderingError>"}]}}`.

**Why it took until Firefox 153 to bite**: on 149–152 `screenshotActor.capture` failed earlier, at actor-module load, and ff-rdp fell back to the parent-process `drawSnapshot` path. Firefox 153 fixed that load (Bug 2043900, `414cbad5bf8b`), the request reached the renderer for the first time, and every capture started failing — misdiagnosed in the iter-135 plan as reply-shape drift and in iteration-110 as environmental "known reds".

**Consequence**: the win is not just correctness — the "minimise outbound bytes" instinct is what created a two-release latent break that was invisible because a *different* bug masked it. Fields the server reads without a default are not optional; treat "the server defaults this" as a claim requiring a source citation, not an inference from the value being the common case.

**Related**: `parse_capture_response()` now folds the reply's `messages` into the error instead of reporting "missing 'data' field" — the server was explaining the failure all along and ff-rdp was discarding the explanation. See DEC-026.

**Applies to**: `crates/ff-rdp-core/src/actors/screenshot.rs`, `crates/ff-rdp-core/src/fronts/screenshot.rs`, `crates/ff-rdp-core/src/specs/screenshot.rs`, `kb/rdp/actors/screenshot.md`, iter-135.

## DEC-026: screenshot failures report what Firefox said, not a guess about headless mode

**Decision** (iter-135, Theme C): the `screenshot` command no longer appends "screenshots require headless mode; relaunch with: `ff-rdp launch --headless`" to capture failures, and no longer reuses `version_mismatch_message()` ("screenshot actor not found in Firefox N root form") on the `drawSnapshot`-fallback path. Failures quote the server's own `messages` entries; the fallback path uses a new `capture_failure_message()` that says Firefox rendered no image and suggests dropping `--full-page` or running `ff-rdp doctor`.

**Why**: both claims were unconditional guesses attached to *any* capture failure. The headless hint fired at users who were already headless — the normal ff-rdp setup — and was the top-line advice in the iter-135 bug report, sending the first diagnosis in exactly the wrong direction. The "actor not found" text is reached only *after* an actor was found and called, so it is false by construction on that path.

**Enforcement**: `screenshot_errors_carry_no_headless_relaunch_hint` greps the pre-`#[cfg(test)]` portion of `commands/screenshot.rs` for both literals, so a new error site cannot reintroduce them; `live_135_screenshot_error_not_misleading` forces a real capture failure against headless Firefox (a 200 000 px page defeats the renderer) and asserts neither string appears in stdout or stderr.

**Applies to**: `crates/ff-rdp-cli/src/commands/screenshot.rs`, `crates/ff-rdp-cli/tests/live/live_135_screenshot_ff153.rs`, iter-135.

## DEC-027: the native accessibility tree is opt-in, never the default

**Decision** (iter-143, Theme B): the flag that enables Firefox's platform accessibility service and walks the native tree is **opt-in**. `ff-rdp a11y` with no flag continues to use the JS-derived fallback when the service is off, and never calls `parentAccessibilityActor.enable()` on the user's behalf. This settles the open question [[iteration-143-native-a11y-tree]] raised ("should `--native` also become the default once it is proven?").

**Why**: `enable()` is browser-global and process-wide, not scoped to the tab or the connection. Its performance cost persists until the browser shuts down, and on Windows an active screen reader can block the matching `disable()`, so ff-rdp cannot reliably restore the prior state it changed. A read-only-looking query command must not mutate whole-browser state as a side effect of being run — an agent calling `a11y` to inspect a page has not consented to degrading every other tab for the life of the process. iter-136 already reached the same conclusion for the same reasons and deliberately did not enable the service; this makes that stance explicit rather than incidental.

**Reversibility**: this is a UX default, not an architectural constraint. If the native path proves cheap in practice, flipping the default is a small follow-up — the reverse (retracting an on-by-default global mutation once users depend on it) is not. Choose the reversible direction first.

**Consequence for Theme A**: because both sources remain reachable, `meta.source` (`"native"` | `"js-fallback"`, plus the fallback reason) is not cosmetic — it is the only way a caller can tell which tree it scored, and it must be present on every `a11y` response regardless of which path ran.

**Applies to**: `ff-rdp a11y`, `a11y audit`, iter-143.

## DEC-028: `launch`'s consent field is renamed, not fixed in place — and the freeze-for-capture pattern generalises DEC-027's "restore only what you changed"

**Decision** (iter-144, Theme C): `launch --auto-consent`'s JSON field is renamed
`auto_consent_extension_installed` (was `auto_consent`). It is still set
unconditionally from the CLI flag — that is now honest, because the new name
only claims "the extension was installed into the profile", which `launch`
can actually know before any page loads. It no longer claims a dismiss
happened, which `launch` never could know (iteration-142's dogfooding
finding: `auto_consent: true` reported while a banner still covered the
page). A real dismiss attestation already existed and is unchanged:
`navigate --auto-consent` / `consent accept` report `results.consent =
{"cmp": ..., "action": ...}` (DEC-023), which run after a page has loaded
and can check the DOM.

**Why rename instead of leaving the name and just documenting the caveat**:
a field named `auto_consent` reads as "consent was automatically handled" to
any caller who has not read the source — the iteration-142 finding is
exactly that misreading happening to a real dogfooding session. A false
name with an accurate docstring is still a false name to `--jq
'.results.auto_consent'`. Grepped for consumers before renaming (none in
fixtures, README, or other command source — `navigate`'s unrelated
`auto_consent` CLI flag and its own `merge_auto_consent` helper are a
different field on a different command and were not touched).

**Also landed in this iteration**: a `NATIVE_CMP_TABLE` in `consent.rs` for
same-origin (non-iframe) CMPs — BBC's own `#bbccookies-continue-button` sits
in the top document, not behind Sourcepoint's iframe, and is tried before
`CMP_TABLE` since Sourcepoint's overlay can sit in front of it on first
paint (verified live: the element exists with a zero-size rect until
Sourcepoint is dismissed, so `native_accept_js` requires a non-zero
bounding rect, not just DOM presence, before it will click). `tabs` filters
the `Consent-O-Matic Options` tab `--auto-consent` leaves open by matching
`moz-extension://` scheme plus a table of the five titles the vendored XPI
ships (locale-defensive, since the launch-time locale pin under
investigation in [[iteration-147-console-locale-repro]] cannot yet be
trusted to hold).

**Screenshot freeze pattern reuses DEC-027's shape** (Theme D): before a
`--full-page` capture, every `position: fixed`/`sticky` element is pinned to
`position: absolute` with its current on-screen `top`/`left` (so the
compositor never treats it as viewport-relative during the
taller-than-viewport `drawSnapshot` call), and always restored afterward —
success or capture failure — exactly the "only touch what you changed, and
always undo it" discipline DEC-027 established for the accessibility
service. Unlike DEC-027's underlying bug, the specific duplicate-header
symptom this was meant to fix (dogfooding session 63, BBC News) could not
be reproduced in the implementation environment despite a deliberate
before/after attempt across page heights 2 000–20 000 px (spanning common
GPU texture-tile boundaries) with both `fixed` and `sticky` headers, and
against the real BBC page directly — see
`live_144_full_page_no_duplicate_header`'s module doc. The freeze/restore
code is landed anyway as a defensive, unit- and live-regression-tested
mitigation matching the plan's own suggested approach, but the AC is
satisfied by a forward-looking invariant check on a deterministic local
fixture, not a reproduced-then-fixed defect.

**Applies to**: `crates/ff-rdp-cli/src/commands/{launch,consent,tabs,screenshot}.rs`,
`crates/ff-rdp-cli/tests/live/live_144_session_hygiene_followup.rs`, iter-144.

## DEC-029: cap-mutating unit tests take a `write` guard, cap-dependent unit tests take a `read` guard — the plain `ff-rdp-core` suite runs parallel, unlike the live suite

**Decision** (iter-150): `transport::FRAME_CAP_LOCK`, previously a private
`Mutex<()>` inside `transport::tests` used only by the five tests that mutate
`MAX_FRAME_BYTES_CELL`, is now a crate-visible (`pub(crate)`)
`RwLock<()>`. The five cap-mutating tests now take `.write()` (unchanged
behaviour — a writer still excludes everyone). Any test elsewhere in the
crate whose correctness depends on `recv_from` seeing the *default* cap
during a real oversized round-trip must now take `.read()` for the duration
of that round-trip;
`specs::types::tests::resolve_slot_longstring_grip_fetches_full_value` is the
first adopter.

**Why**: `resolve_slot_longstring_grip_fetches_full_value` failed
intermittently (`FrameTooLarge`), reproduced in review passes twice
(iter-141, iter-146) before iter-150 root-caused it: it reads
`transport::max_frame_bytes()` via a real 20 KB `recv_from` round-trip
without any guard, so it could observe a transiently-shrunk 1024-byte cap set
by e.g. `max_frame_mb_knob_works` running concurrently. Confirmed by forcing
the interleave deterministically (a throwaway harness that hammered the cap
between two values on one thread while looping the longstring round-trip on
another reliably produced `FrameTooLarge` on the first or second iteration
against the pre-fix code, and zero times in 20 full-suite runs post-fix).
This is a **different flavor** of process-global-state bug than DEC-022's
`live_bulk_cap` leak: that was a *sequential* leak (a restore that didn't
run), fixed with an RAII guard and addressed by the live suite's
`--test-threads=1` serialization. The plain `cargo test -p ff-rdp-core`
suite is **not** serialized — it is the default parallel unit-test binary
that runs on every CI job on every PR — so an RAII restore alone is
insufficient: the cap can be *correctly* restored afterward and a concurrent
reader can still observe it shrunk *during* the guarded window. `RwLock`
over `Mutex` so unrelated reader tests don't serialize against each other,
only against the rare writer.

**Applies to**: `crates/ff-rdp-core/src/transport.rs`,
`crates/ff-rdp-core/src/specs/types.rs`, iter-150.

## DEC-030: a ticked `live_*` AC must carry `[verified: <YYYY-MM-DD>, <measured result>]`; the fidelity gate states its own blindness

**Decision**: `ac-fidelity-check.sh` gains two negative checks and one honest
success message (iter-154):

1. A **ticked** AC whose full text (bullet line plus its continuation lines,
   whitespace collapsed) matches one of six literal phrases as whole words —
   `not exercised`, `not run`, `never run`, `not executed`, `implemented and
   compiled`, `not verified` — fails case-insensitively, regardless of whether
   its slug resolves in the diff. A `[deferred — …]` annotation short-circuits
   first, so the sanctioned way to say "this did not happen" still passes.

   Word boundaries and the shorter list came out of PR #193 review, which
   demonstrated false positives on ordinary AC wording: the phrases were plain
   substrings, so `not run` fired inside "not run*ning*" and "can*not run*", and
   `time budget` fired on any latency AC ("completes within the 200 ms time
   budget"). `time budget` is dropped outright — it earns nothing, since the
   iteration-151 AC that motivated the list is caught by `not exercised` in the
   same sentence. `could not run` is subsumed by `not run`.

   The residual false-positive class — an AC that legitimately *describes*
   behaviour ("`--dry-run` does not run the command") — gets an explicit escape
   hatch, `[allow-ac-wording: <reason ≥10 chars>]`, mirroring the repo's
   `// allow-spec-drift:` / `// allow-todo:` pattern. Without it the only remedy
   for a false positive is to reword the AC, which is precisely the behaviour
   this gate exists to stop; a check that punishes honest wording teaches
   agents to launder wording. The review found this PR had already reworded its
   own AC 3 to get past its own gate.
2. A **ticked** AC naming a `live_*` test must carry a
   `[verified: <YYYY-MM-DD>, <measured result>]` annotation. The gate requires
   an ISO date and at least one further digit inside the bracket; it does not
   and cannot validate the number. Non-live tests are exempt — they run in CI,
   so `cargo test --workspace` is their run evidence.
3. The PASS line, the script header, `CLAUDE.md` and `CONTRIBUTING.md` now say
   the gate verifies that ticked ACs *reference* resolvable evidence and that it
   cannot verify any test was executed.

The evidence heuristics themselves keep reading only the AC's **first** line.
Widening their input would hand them more tokens to match and could turn a
should-fail plan green — the pinned `61v=FAIL` replay baseline exists to catch
exactly that.

Three consumers read the folded text: the two new checks, which can only ever
add failures, and the `[deferred — …]` accept, which can only ever remove them.
That last one is why the deferral annotation must be **anchored** to the end of
the AC (trailing whitespace and a period tolerated; a closing `)` is not). PR
#193 review caught the unanchored version laundering any AC that merely
mentioned `[deferred` in passing — a plan that failed *before* iter-154 passed
after it. A deferral nested in a parenthetical is textually indistinguishable
from a passing mention, so the stricter reading wins and the author moves the
annotation; `iteration-114` line 124 is the only plan in the repo written the
looser way, and merged plans are never re-gated.

**Why**: on 2026-08-12 PR #188 ([[iteration-151-residual-live-firefox-leak]])
ticked two ACs whose own continuation text read *"implemented and compiled;
gated behind `FF_RDP_LIVE_SUITE_CHECK=1` … and not exercised end-to-end in this
session's time budget"*. `ac-fidelity-check`: PASS. `check-iteration-ready`:
11/11 PASS. Replaying the plan as it stood at `6d07c8c` through the pre-fix
script reproduces exit 0. Two mechanisms combined: the per-line loop never saw
the continuation lines where the confession lived, and the surviving heuristic —
"the named slug resolves to an `fn` in the diff" — is satisfied by writing the
function and never calling it.

Theme B (the `[verified: …]` requirement) is friction, and forgeable, which is
why it was weighed rather than assumed. It ships because Theme A alone **rewards
silence**: once a denial list exists, the honest-but-ticking case becomes the
silent-ticking case, and this repo has already watched an iteration plan get
reworded to route around a gate. Theme A catches a confession; Theme B requires
an assertion. Restricting it to `live_*` keeps the cost where the risk is —
live tests are `#[ignore]`-gated, so nothing downstream of this gate will ever
execute them.

What this does **not** buy: a diff-reading script still cannot verify a test
ran, and no run log exists for it to read (the loop never invokes
`cargo test-live`). The ceiling was accepted, not designed away — hence Theme C.

**Applies to**: all four copies of `ac-fidelity-check.sh`
(`~/.claude/skills/{ralph-loop,new-ralph-loop}/scripts/` and both `tools/`
mirrors), `tools/tests/ac-fidelity-check/`,
`crates/xtask/tests/ac_fidelity_check.rs`, iter-154.

## DEC-031: a live sweep runs qualified tests for real and unqualified tests un-`--include-ignored`, instead of teaching each test body to self-report

**Decision** (iter-155): don't touch the ~90 individual `#[ignore]`-gated live
test bodies. Their internal `if !live_tests_enabled() { return; }` checks
(and the `FF_RDP_LIVE_NETWORK_TESTS` equivalent) stay exactly as written —
redundant for any test the sweep classifies **unqualified**, since that test's
body never runs at all.

They are **not** redundant in one reachable case, and calling them
unconditionally "not load-bearing" would overstate this decision (raised in PR
#194 review): a test *misclassified as qualified* — because its
`#[ignore = "…"]` reason text drifted from the env vars its body actually reads
— does run, hits the bare `return`, and reports `ok`. That is the original
iter-155 defect, intact. The classifier is text-derived, so the failure mode is
a documentation/code divergence rather than a logic bug, and the early returns
are the last line of defence when it happens. They stay for that reason, not
merely as inert documentation. A `live-sweep` check that cross-references each
test's ignore reason against the env vars its body reads, failing loudly on
disagreement, would close this and is worth a follow-up — filed as
[[iteration-157-live-sweep-classifier-drift]].

Instead, `cargo run -p xtask -- live-sweep`
(`crates/xtask/src/live_sweep.rs`) statically classifies every gated test
from its own `#[ignore = "…"]` reason text (an iter-155 audit found every
current reason under `tests/live/` names at least one of
`FF_RDP_LIVE_TESTS` / `FF_RDP_LIVE_NETWORK_TESTS`), then drives `cargo test`
in two phases per target:

1. **Qualified** tests (every env var they need is actually set) run for
   real with `--include-ignored` — libtest reports genuine `ok`/`FAILED`.
2. **Unqualified** tests are selected by exact name *without*
   `--include-ignored`. They still carry `#[ignore]` and nothing forces them
   to run, so libtest reports them `ignored` — its own vocabulary, not a
   fabricated status the runtime check invents.

The executed count is therefore known from classification alone, before a
single `cargo test` process spawns — `qualified.len()`, printed as
`LIVE_SWEEP_SUMMARY executed=N skipped=M total=T` — and is `0` whenever the
relevant env vars are unset, never inferred from parsing `cargo test`'s
prose output.

**Why this option over the plan's four**: the plan's own options 1–4 all
modify test bodies. Option 1 (`eprintln!` + `return`) was rejected in the
plan itself — still green. Option 2 (drop the runtime check, rely on
`#[ignore]` alone) answers the plan's own gating question wrong: a
network-less `FF_RDP_LIVE_TESTS=1` run legitimately wants the
network-gated tests to *skip*, not fail — failing them would make ordinary
partial live runs (the common case; most contributors don't have
network-dependent fixtures wired) red for no defect. Option 3 (a panicking
`skip_unless_live!()` macro) turns a skip into a libtest `FAILED`, which is
the same wrong answer as option 2 by a different mechanism — and would
require rewriting ~94 call sites across 45 files for a behaviour libtest
already has a real primitive for (`ignored`). Option 4 (a custom test
harness) is the correct primitive but nightly-only.

The sweep tool sidesteps all four: it never runs an unqualified test's body
at all, so the internal early-`return` a test would have hit is never
reached — the ~94 call sites become inert documentation rather than the
mechanism, and the one-liner-per-file rewrite the plan's Notes warned about
never happens. The cost is that a contributor who runs a single suite by
hand (`cargo test -p ff-rdp-cli --test live <module> -- --include-ignored`)
still sees the old, misleading `ok`-for-unset-gate behavior — the fix lives
in the sweep orchestration, not the test binary. `CONTRIBUTING.md` and
`CLAUDE.md` now say so explicitly and point at `live-sweep` instead.

**Answer to the plan's gating question** ("is a network-less live run
supposed to be green?"): yes for tests that don't need the network — no for
the summary line's implicit claim that all of them ran. `live-sweep`'s two
counts (`executed` / `skipped`) make both true at once instead of forcing a
single misleading number to answer both questions.

**Theme C — not implemented, and this is the considered answer, not an
omission.** `ac-fidelity-check.sh` reads only an iteration plan's checked-in
AC text; it has no run log to check the printed `LIVE_SWEEP_SUMMARY` line
against, because the loop that invokes it never runs `live-sweep` (or
`cargo test-live`) itself — same ceiling DEC-030 already named. Requiring
the `[verified: …]` annotation to quote the exact `executed=N` token would
add a second brittle string format for the gate to parse without adding any
verification power: an agent can paste a fabricated `executed=17` exactly as
easily as a fabricated `109 passed / 0 failed`. Coupling the shell gate to a
test-output format is only worth it once there is a persisted run artifact
the gate can independently open and check the annotation against — that is
a different, larger iteration (a run-log store), not a format tweak here.

**Applies to**: `crates/xtask/src/live_sweep.rs`, `crates/xtask/src/main.rs`,
`CONTRIBUTING.md`, `CLAUDE.md`, iter-155.

## DEC-032: keep `isServerTargetSwitchingEnabled: true` on the daemon's watcher — the flag was never the defect; buffer suppression, two wrong wire shapes and a late `tabNavigated` were

**Decision**: iter-159's Theme A offered two options — (a) give frame-target
enumeration its own connection and put the daemon's core watcher back on the
default acquisition path, or (b) keep the flag and widen `is_watcher_event` to
accept target actors. **Neither was implemented.** The daemon keeps
`get_watcher_with_options(..., Some(true))` and `is_watcher_event` stays a
strict `from == <daemon watcher>` equality test.

**Why**: the premise both options rest on — that server-side target switching
moves resource emission onto the per-document target actor — is false, and a
recorded frame says so. `network-event` is registered in
`ParentProcessResources` (`devtools/server/actors/resources/index.js`), so
`WatcherActor.watchResources` handles it in the parent process and
`WatcherActor.emitResources` (`devtools/server/actors/watcher.js`) emits it;
`from` is the watcher's own actor id with the flag on or off. The recording is
`crates/ff-rdp-cli/tests/fixtures/resources_available_network_server_target_switching.json`
(`from: "server1.conn0.watcher3"`), pinned by
`unit_159_daemon_resource_routing_pinned`, and the reasoning is written up in
[[rdp/actors/watcher|kb/rdp/actors/watcher.md]] together with the 153-vs-154
diff that shows the relevant regions are unchanged across the skew.

Option (a) would have bought a second connection and a second watcher
lifecycle for nothing. Option (b) would have been actively harmful:
`is_watcher_event` exists so the daemon does not steal resource events
belonging to a *proxied command's own* watcher, and those genuinely do arrive
`from: server1.conn2.watcher2.process8//windowGlobalTarget2` — the very shape
the widened predicate would have swallowed, breaking the `watchResources`
handshake forwarding.

**What actually broke it** — four independent defects, all found by tracing the
daemon's dispatch rather than by reading the flag's doc comment:

1. `dispatch_firefox_message` skipped buffering any resource whose type had an
   active stream subscriber. Since iter-138 Theme A a **plain** daemon
   `navigate` opens a `network-event` stream (so `wait_for_doc_complete` can
   read the document's HTTP status), so every navigation's resources were
   handed to that transient subscriber, used for one status field, and
   discarded. The suppression existed only to avoid double-counting the
   `store-events` push-back — the workaround this iteration deleted.
2. `daemon/buffer.rs::update_to_val` emitted the pre-iter-106 update shape
   (`{"resourceUpdates": [{"resourceId": …}]}`); the reader wants a top-level
   `resourceId` and an object-valued `resourceUpdates`, so every buffered
   update was dropped and `status`/`content_type`/`transfer_size` were null.
   iter-106 Theme D fixed this exact shape in the `store-events` serialiser and
   missed this copy.
3. `net_to_val` emitted a flat `causeType` while
   `parse_single_network_resource` reads `cause.type`, so `cause_type` was `""`
   on every daemon row and `by_cause_type` collapsed to one bucket.
4. Firefox 153 emits `tabNavigated` at load **stop** — measured 257 buffered
   entries deep — so the default `--since -1` window excluded the navigation's
   own requests. Network epochs now start at the oldest surviving
   `network-event` entry, which is sound because the server destroys the
   previous document's request actors at will-navigate.

Defects 2 and 3 are the same failure mode as the mask itself: a serialiser that
nothing exercised, because the only thing filling the buffer was the workaround
that used a different serialiser.

**Measured**: plain daemon `navigate` → `network --source watcher --detail`
returned 0 entries before, 20 entries after with 20/20 carrying non-null
`method` **and** `status`.

**Applies to**: `crates/ff-rdp-cli/src/daemon/server.rs`,
`crates/ff-rdp-cli/src/daemon/buffer.rs`,
`crates/ff-rdp-cli/src/commands/network.rs`,
`crates/ff-rdp-cli/src/commands/navigate.rs`, iter-137, iter-138, iter-159.

## DEC-033: `click` drops `entered` outright rather than aliasing it, and an obscured click exits 1

**Decision** (iter-160 Themes A/B): `click`'s result loses `entered` with no
alias or deprecation window, gaining `matched` (the selector resolved) and
`reachable` (the element's centre point hit-tests to itself or a descendant) in
its place. An unreachable target is a **failed action**: `AppError::Unsupported`
with `error_type` `click_obscured` (or `click_offscreen` when the centre point is
outside the viewport), exit 1, with `matched`/`reachable`/`obscured_by` merged
flat into the error envelope.

**Why drop rather than alias**: `entered` was assigned immediately after the
`querySelector` null check and before any `dispatchEvent` — it meant "the
selector matched" while its name claimed the pointer could enter. Keeping it as
an alias of `matched` would preserve a name that misdescribes the thing it is
now aliasing, which is the defect, not a mitigation of it. It had exactly two
producers (`js_helpers.rs`, `click.rs`) and zero consumers: no Rust code, no
test, no fixture, and no kb document read the field. The two producers are now
one — `ClickOnly` was a hand-written copy of the click JS, and that copy is
precisely how a hardcoded `entered: true` literal survived in one dispatch mode
while the other two computed it.

**Why exit 1 rather than a warning field**: a caller writing
`ff-rdp click X && ff-rdp type Y …` has to stop. An informational `reachable:
false` at exit 0 would preserve the exact shape this iteration exists to remove —
a confident success the command has not established. `AppError::Unsupported` is
reused (no new variant) and gained an optional `details` field so the failing
caller reads the covering element out of the same flat object it already parses
`error_type` from.

**The hit-test rule, and the two false failures it had to be corrected for.**
Reachable iff the hit target is the element, a **descendant** of it, or an
**ancestor** of it. The first cut compared only `hit === el || el.contains(hit)`
and broke two things that a live run caught before merge:

- `ff-rdp click body` failed with "covered by html", because `<body>` paints
  nothing at its own centre and the hit resolves to its parent. An ancestor
  cannot obscure its own descendant — an overlay is never an ancestor of what it
  covers — so `hit.contains(el)` is a sound third clause, not a loophole.
- An ordinary `<a>` inside an out-of-process iframe
  (`live_129_click_cross_origin_frame`) hit-tested to `null`, because the child
  document was never laid out. Reporting that as `click_offscreen` would break
  every cross-origin frame click.

So `reachable` is **three-valued**: `true`, `false`, or `null` for "the hit test
could not decide". `null` dispatches the events and says so; only a literal
`false` is an error. Turning "I could not tell" into exit 1 would be the same
overstatement this iteration removes, pointed the other way.

Below-the-fold is likewise not an obstruction: when the centre starts outside the
viewport the element is scrolled into view (`block: 'center'`) and the rect
re-read before any verdict. `click_offscreen` therefore means "still outside the
viewport after scrolling" — a clipped or zero-size scroll container — not "you
did not scroll first".

**Trade-off**: `click` can now fail where it previously "succeeded" — a target
under a transparent full-viewport wrapper that forwards pointer events is
reported as obscured. That is the right default: the alternative is the pre-160
behaviour, where the same page reported a click that never happened.

**Not in scope**: trusted input. `e.clientX`/`e.clientY` stay `0` and events stay
`isTrusted: false` — the hit test decides *whether* to dispatch, it does not give
the events real coordinates. See `kb/rdp/client/remote-agent-cdp.md`: Marionette
and WebDriver BiDi are peer protocols to devtools-RDP, not layers reachable
through it.

**Applies to**: `crates/ff-rdp-cli/src/commands/js_helpers.rs`,
`crates/ff-rdp-cli/src/commands/click.rs`, `crates/ff-rdp-cli/src/error.rs`.

## DEC-034: `--jq` never changes the shape it filters — `network` loses its detail-mode trigger

**Decision** (iter-160 Theme F): `cli.jq.is_some()` is removed from `network`'s
`use_detail_mode` predicate. `ff-rdp network --jq '.results | type'` now answers
`"object"`, identical to plain `ff-rdp network`; `--detail` (and
`--all`/`--headers`/`--security`/`--sort`/`--limit`/`--fields`) remain the ways
to the entry list.

**Why**, given this is a compatibility break and the alternative was one line of
documentation:

- `--jq` is a **view** applied to the envelope. If the view can change the
  document, every jq expression a caller writes becomes conditional on which
  command it is aimed at, and the one property that makes a uniform envelope
  worth having across 30 commands is gone.
- The documented contract already promised the honest behaviour ("use --jq to
  filter the envelope"). Changing the doc to match the code would have ratified
  the exception and invited the next one.
- Verified during the 2026-08-13 step-back as network-only: `console`, `a11y`,
  `perf`, `sources` and `cookies` are single-shape.
- The migration is cheap and was already anticipated: iter-126 made detail mode
  carry the full summary fields precisely because `--jq` users were forced into
  it. Adding `--detail` therefore yields a strict superset of the old envelope,
  not a trade.

`--sort`/`--limit`/`--fields` deliberately stay in the disjunction: they are
list-shaped controls whose meaning on a summary object is undefined. Only `--jq`,
which is shape-agnostic by construction, comes out.

**Applies to**: `crates/ff-rdp-cli/src/commands/network.rs`,
`crates/ff-rdp-cli/src/cli/args.rs`, iter-126, iter-159.

## DEC-035: `--fields`/`--sort` validate against the union of keys present, strictly and with no opt-out

**Decision** (iter-161, Theme D): a `--fields` or `--sort` name that appears on
*no* entry of the result set is an `AppError::User` (exit 1, JSON error
envelope) naming the flag, the offending name, and the keys that are available.
The schema is the data — the union of keys across the object entries in hand —
so no static per-command schema is introduced. Three sub-decisions:

- **Union, not intersection.** A key present on some entries and absent on
  others is legitimate (`dom` emits `text` only for elements that have it);
  intersection would break working commands.
- **Skip when there is nothing to validate against.** An empty result set, and
  a result set holding no object entries (a list of strings), both yield an
  empty union. Erroring there would turn a legitimate empty query into a
  failure, so validation is skipped and the command exits 0.
- **Strict by default, no `--fields-lax`.** A `--jq` filter resolving to
  nothing is often deliberate — jq is a query language and `.results[].maybe`
  is a legitimate probe, which is why `--jq-strict` is opt-in. A
  `--fields`/`--sort` name matching nothing never is. Callers who want
  tolerance already have `--jq`.

**Why**: measured on 2026-08-13, `ff-rdp dom 'a' --limit 2 --fields bogusfield`
printed `{"results": [{}, {}], "total": 2}` at exit 0 — the data destroyed, the
count intact — and `--sort nosuchfield` was a silent no-op (both sides of
`compare_values` are `None`, the comparison is `Equal`, the sort is stable). An
LLM agent cannot tell `[{},{}]` from a page with two empty links.

**How it is enforced**: `apply_sort`, `validate_fields` and `apply_fields_object`
return `Result`, so the compiler forces all ~29 call sites across ~12 command
modules to handle the error. An opt-in validation helper would have been
forgotten by the next command added; this is more churn but it is fail-closed
and adds no new public surface.

**Fixed in review (iter-161 PR #200)**: `validate_fields` MUST run against the
full, pre-`apply_limit` result set. `apply_fields` originally validated
internally, and every call site invoked it *after* `apply_limit` — so a field
name genuinely present in the data, but absent from the truncated `--limit`
page, was wrongly rejected as unknown. `dom`, `console`, `geometry` and
`network` default to a non-`None` `--limit` even when the caller never passes
the flag, so this was reachable in ordinary usage. Fixed by splitting
`apply_fields` into `validate_fields` (called pre-limit, alongside
`apply_sort`) and a non-validating `apply_fields` (called post-limit, as
before, projection only) — see `unit_161_fields_validated_before_limit_not_after`.

**Applies to**: `crates/ff-rdp-cli/src/output_controls.rs`, every command
module using `OutputControls`.

## DEC-036: `eval` returns the whole string and stops reporting `meta.eval_path`

**Decision** (iter-161, Themes C and E): `eval::run` resolves a
`Grip::LongString` through `LongStringActor::full_string()` before building
`results` — matching what `js_helpers::resolve_result` has done for ~18 other
commands since iter-102 — and the `meta.eval_path` field is removed from the
envelope.

**Why (Theme C)**: Firefox inlines only ~1000 characters and returns a
`longString` grip for the rest. `eval` printed that preview as if it were the
value, with no `meta.truncated` and no hint, and then released the grip — the
one handle by which the rest could have been fetched. No CLI command speaks
`substring`, so the truncated remainder was unreachable. `eval` is the command
whose entire job is handing back a raw value; `"x".repeat(5000)` returning 1000
characters is the worst possible place for a silent truncation. A
`meta.truncated` flag was rejected as the fix: the point is that nothing is
truncated. `full_string` keeps its 16 MiB `MAX_FETCH` bound, and the error
surfaces through the normal JSON error envelope.

**Why (Theme E)**: `meta.eval_path` was hard-set to `"page-await"` on every
call. Its only other value, `"chrome"`, was deleted in iter-93 and DEC-020
confirmed the deletion stands, so the field has discriminated nothing for ~70
iterations while reading like a strategy selector. The page-await guarantee it
appeared to carry is asserted properly by `live_61l`/`live_eval_csp` — `eval`
succeeding on a `script-src 'none'` page IS the Debugger.evalInGlobal path.
The `--help` prose describing that path is accurate and stays.

**Applies to**: `crates/ff-rdp-cli/src/commands/eval.rs`,
`crates/ff-rdp-cli/tests/live/live_61l.rs`,
`crates/ff-rdp-cli/tests/live/live_61r_eval.rs`,
`crates/ff-rdp-cli/tests/live/live_eval_csp.rs`,
`crates/ff-rdp-cli/tests/e2e/eval.rs`, DEC-015, DEC-020.

## DEC-037: resource subscriptions are daemon-owned; the daemon drops a client's `unwatchResources`

**Decision** (iter-164, defect 1): the daemon inspects every proxied client
frame and, for `unwatchResources`, strips the resource types it owns
(`network-event`, `console-message`, `error-message`) — forwarding the
remainder, or dropping the frame outright when nothing else is named. Symmetric
with the `unwatchTargets` drop that iter-137 introduced for the same structural
reason.

**Why**: `throttle --block <pattern>` was accepted, echoed back in
`results.blocked_urls`, and then not enforced. Intake was never the problem.
Firefox keeps the URL block-list (and the throttling config) on the
`NetworkObserver` owned by the `network-event` resource watcher, **not** on
`NetworkParentActor`. `unwatchResources(["network-event"])` destroys that
observer; the next `watchResources` builds a fresh one with an empty
block-list, and nothing anywhere reports the loss.

`navigate` subscribes to `[document-event, network-event]` through
`ResourceCommand` and unsubscribes on teardown. `ResourceCommand`'s ref-count is
per CLI **process**, so it cannot know that a *different* process
(`ff-rdp throttle --block …`) already asked the shared daemon connection to
watch `network-event`. Through the daemon that teardown landed on the shared
connection and wiped the configuration a previous command had installed —
`block → navigate → fetch` resolved, while `block → fetch` (no navigate)
rejected correctly. Confirmed by connection scoping: a `--no-daemon` navigate,
which has its own watcher, leaves a daemon-set block-list intact.

**Rejected alternative**: teaching `navigate` not to unwatch `network-event`
when `via_daemon`. It fixes exactly one command; any future command that routes
`network-event` through `ResourceCommand` reintroduces the defect, and each one
would have to remember. Ownership belongs where the subscription is installed —
the daemon — so that is where it is enforced. Dropping the frame is safe:
`unwatchResources` is `oneway: true` in `devtools/shared/specs/watcher.js`, so
no client is left awaiting a reply, and the client's paired `watchResources` was
itself a no-op on an already-watching connection.

**Applies to**: `crates/ff-rdp-cli/src/daemon/server.rs`
(`classify_client_resource_teardown`), `kb/rdp/actors/network-parent.md`,
`crates/ff-rdp-cli/tests/live/live_164_block_and_daemon_autostart.rs`.

## DEC-038: autostart waits 20 s for the registry, and a silent direct fallback is reported

**Decision** (iter-164, defect 2): `resolve_connection_target` waits
`FF_RDP_DAEMON_START_TIMEOUT_MS` (default 20 000 ms, was a hard-coded 5 s) for a
freshly spawned daemon to write its registry entry; and when it still gives up,
the `deferred_warning` that used to be discarded on the success path is
remembered and surfaced as `meta.daemon_fallback` under `--verbose`.

**Why**: iter-158's `live-sweep` ran at load average 18.6 and a daemon that
needed longer than 5 s to connect to Firefox, run `listTabs`/`getWatcher` and
install `watchResources` was abandoned — the caller then got a *direct*
connection instead of the daemon it asked for. Waiting longer costs nothing on
the failure path that matters, because `resolve_connection_target` already
fast-fails in 100 ms when Firefox's debug port is unreachable; the budget is
only ever spent when Firefox is up and a daemon really is starting.

The reporting half is the same class of dishonesty iter-158 removed from the
test harness. `ConnectionTarget::Direct::deferred_warning` is printed only if
the direct fallback *also* fails — deliberate, since the message was benign
noise on the happy path — but that left a caller who asked for daemon mode and
quietly got direct mode with no signal at all: `meta.route` said `"direct"`
without saying why, and the two registry-check error paths never reached
`daemon_status::record_autostart_failed`, so they produced no envelope warning
either. `--verbose` is the right gate: the route itself stays in default output,
the diagnosis does not.

**Applies to**: `crates/ff-rdp-cli/src/daemon/client.rs`,
`crates/ff-rdp-cli/src/connection_meta.rs`,
`crates/ff-rdp-cli/src/commands/connect_tab.rs`,
`crates/ff-rdp-cli/tests/common/mod.rs`.

## DEC-039: `eval` gives every call its own scope; `--no-isolate` becomes the real opt-out

**Decision** (iter-165): the code was wrong, not the help text. `eval`'s plain
synchronous path now routes a script that **declares something at top level**
(`const`, `let`, `class`, `var`, `function`) through the same per-call,
value-producing IIFE that `--stringify` (iter-161) and top-level `await`
(iter-132) already used, so those declarations never leak into the next `eval`
and repeating a script against one tab is idempotent. `--no-isolate` stops
being a no-op and becomes the documented opt-out: with it, a plain synchronous
script is sent to `Debugger.evalInGlobal` verbatim again, so declarations
accumulate in the tab's global lexical environment. It cannot un-wrap
`--stringify` or `await` scripts — those wraps are a syntactic necessity, not
an isolation choice.

**Why a declaration trigger rather than "wrap everything that is not a single
expression"**, which is what `--stringify` does: a script that declares nothing
cannot leak anything, so wrapping it buys no isolation while changing its
result — a function body has no script-completion-value semantics, so
`eval 'if (1) { 2 }'` would start returning `undefined` instead of `2`. The
narrower trigger also confines the wrap's known weak spot.
`top_level_statement_boundaries` is a char scanner, not a JS tokenizer: it does
not understand regex literals, so `eval '/a;b/.test("a;b")'` looks like two
statements to it and, if wrapped, becomes a SyntaxError (that input is in fact
already broken under `--stringify` on `main`). Requiring a declaration keyword
at a statement start means a scanner misfire can only bite a script that also
declares — and `unit_165_declaration_free_scripts_are_never_rewritten` pins the
regex and comment cases as byte-for-byte passthrough. Paying an inconsistency
with `--stringify`'s trigger to avoid regressing working scripts is the right
side of that trade; the *contract* both paths state is identical.

**Revisited at iter-167 (Theme C), decision unchanged.** The second half of
that paragraph no longer holds: iter-167 taught
`top_level_statement_boundaries` about regex literals, `//` and `/* */`
comments and backslash escapes, so `eval --stringify '/a;b/.test("a;b")'` —
named above as already broken on `main` — now works, and the scanner is no
longer a reason to keep the triggers apart. The *first* half is untouched and
is sufficient on its own. Measured against a live Firefox at iter-167:
`eval 'if (1) { 2 }'` returns `2` and `eval 'for (let i = 0; i < 3; i++) { i }'`
returns `2`; converging on `--stringify`'s "wrap anything that is not a single
expression" trigger would turn both into `undefined`, buying `eval 'return 1'`
(today `return not in function`) in exchange. Two working behaviours for one is
the wrong trade, so the plain path keeps the declaration trigger and
`--stringify` keeps the single-expression one. The asymmetry now rests on the
completion-value argument alone, which is where it always belonged.

**Why**: five pieces of evidence, all pointing the same way.

1. `Debugger.evalInGlobal` evaluates in the target global's *own* lexical
   environment. It bypasses page CSP (which is what iter-93 needed) but it
   does not hand each evaluation a fresh scope — the premise `eval --help` had
   asserted since iter-93 was simply false. Measured 2026-08-16 against
   Firefox on macOS: `ff-rdp eval 'const x = 1; x'` twice against one tab gave
   `1` then `{"error":"redeclaration of const x"}`.
2. Per-call scope was the *original* design, not a new idea. iter-52 added
   `--no-isolate` explicitly as the opt-out "when the user wants to share state
   across calls". iter-93 removed the `eval()`-based isolation wrapper for CSP
   reasons and silently lost the isolation with it, leaving the flag inert and
   the help text describing a contract nothing implemented.
3. ff-rdp already isolated on three of its four eval paths: `--stringify`
   (iter-161) and the top-level-`await` wrap (iter-132) both route through
   `wrap_statements_in_iife`; only the plain synchronous path did not. That is
   an inconsistency, not a design — and the asymmetry the plan called "the most
   direct evidence" for what the fix should look like.
4. Nothing in the repo depends on lexical bindings surviving between calls. The
   cross-call state that does exist (`window.__hits`, `window.__ready`, the
   `js_helpers` settle probes) is written as explicit page-global property
   assignment, which the IIFE wrap does not touch. No playbook, example script,
   skill or test declares a binding in one `eval` and reads it in the next.
5. `eval --help` *already* documented the wrapped completion-value rule ("a
   multi-statement script auto-returns its value if the LAST statement is a
   bare expression … otherwise the script needs its own explicit `return`") as
   though it were general, when it held only on the await/stringify paths.
   Wrapping the plain path makes that paragraph true too.

**Why not fix the help text instead** (outcome (b) in the plan): it would have
documented behaviour that is inconsistent across ff-rdp's own eval paths,
non-idempotent in a loop — the single most common way an agent calls `eval` —
and it would have required inventing a story for `--no-isolate` exactly
backwards from the one iter-52 gave it, with no way at all to get a fresh
scope on the plain path. The only argument for (b) was that cross-call
persistence might be load-bearing for interactive use; evidence 4 says it is
not, and `--no-isolate` preserves it for anyone who wants it.

**Cost, accepted and documented**: exactly one behaviour change, confined to
declaring scripts, stated in `eval --help` and pinned by
`live_165_wrap_trigger_is_confined_to_declaring_scripts` — a declaring script
whose LAST statement is not a bare expression (`eval 'const x = 1; if (x) { 2
}'`) now yields `undefined` instead of `2`; the fix is an explicit `return`, or
`--no-isolate`. This is the same trade iter-161 already made for `--stringify`.
Every declaration-free script is byte-for-byte unchanged, and callers already
passing `--no-isolate` see no change whatsoever, because the flag's pre-165
behaviour was identical to the pre-165 default.

**Applies to**: `crates/ff-rdp-cli/src/commands/eval.rs`,
`crates/ff-rdp-cli/src/cli/args.rs`,
`crates/ff-rdp-cli/tests/live/live_165_eval_call_scope.rs`,
`crates/ff-rdp-cli/tests/live/live_161_eval_and_flag_strictness.rs`.

---

## DEC-040: the main document is matched by canonical URL, and `status: null` always carries a `status_reason`

**Decision** (iter-166): `navigate` identifies the main document's
`network-event` resource by comparing **canonicalised** URLs
(`url::Url::parse` + fragment stripped) rather than by exact string equality
against the URL the caller typed, and it prefers the URL that **committed**
over the URL that was requested. Alongside `status`, the envelope now always
carries `status_reason`, which is `null` exactly when `status` is not, and
otherwise one of `not_observed`, `no_document_request`, `no_status_reported`.

**Why**: measured on `main` at 07a9c03, plain `ff-rdp navigate
https://example.com` reported `status: null` for a page that had returned 200 —
on the daemon route, the `--no-daemon` route and the `--with-network` route
alike. Firefox canonicalises `https://example.com` to `https://example.com/`
before requesting it, so `r.url == requested_url` never matched and no status
was ever found. Supplying the trailing slash by hand returned `200`, which is
what settled it. The bug was invisible because the two routes had two private
copies of the same matching rule and no test asserted `results.status` on a
plain `navigate` at all; iter-166 collapses both copies into one
`DocumentStatusTracker`.

**Why the committed URL wins over the requested one**: on a redirect the
requested URL is the hop that returned `301`, and the caller asked what the
page they *got* returned. Within one preference the last match wins, since a
redirect chain can emit several resources for the same URL and the final one is
the hop that committed.

**Why there is no looser fallback** — specifically not "if only one
`cause_type == "document"` resource was seen, use it": Firefox emits that cause
for subframe loads too, so on a page whose main document issued no request
(`about:blank`, a bfcache restore) that rule would report an iframe's status as
the page's. Reporting nothing, with a `status_reason` that says why, beats
reporting a number that is wrong.
`unit_extract_document_status_ignores_subframe_document_resource` (iter-138)
and `unit_166_status_null_is_distinguishable` both pin this.

**Why `status_reason` rather than dropping or restructuring `status`**: the
plan forbids dropping it — it is the only thing in `navigate`'s default
envelope that reports what the *server* said, as opposed to what the document
ended up looking like. A bare `null` conflated three situations a caller has to
tell apart, which is the [[iteration-160-envelope-honesty]] class of problem in
its milder form. Adding a sibling key keeps `status` byte-for-byte compatible
for every consumer that already reads it.

**~~Not in scope~~ — closed by iter-169** (see DEC-041): `back`/`forward`/
`reload` used to emit `{committed_url, ready_state, elapsed_ms}` with no
`status` key at all. They now subscribe to `network-event` like `navigate` and
carry the same `status`/`status_reason` pair on every path.

**Applies to**: `crates/ff-rdp-cli/src/commands/navigate.rs`,
`crates/ff-rdp-cli/src/cli/args.rs`,
`crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs`.

---

## DEC-041: every blocking round-trip issued from inside a navigation wait loop must replay what it swallows

**Decision** (iter-169): any call that resolves through `recv_reply_from`
(`evaluate_js_async`, `getTarget`, …) made from *inside*
`wait_for_doc_complete`'s drain loop goes through `with_event_replay`, which
installs a temporary transport event sink for the duration of the call and
replays everything it captured through `ResourceCommand::dispatch_event`. All
four navigation verbs now also report `status` and `status_reason` on every
path, including `--no-wait` (`not_observed`).

**Why**: `recv_reply_from` reads raw packets off the socket until it finds its
own reply and forwards every *other* packet it reads to the transport's event
sink — dropping it outright when no sink is installed, which is the case inside
that loop. `navigate`'s `Both` strategy issues a blocking `getTarget` the
instant `dom-loading` arrives, which is within milliseconds of Firefox emitting
the main document's response line. Measured on Firefox 153, 30 cold-start
`navigate https://example.com` runs: 29 delivered two `resources-updated-array`
entries for the document, the first carrying `status: "200"`; the one failure
delivered only the second, then sat out the full 2 034 ms grace window waiting
for an update that had already been read and discarded. With the fix, the same
round-trips are measured swallowing **69 packets across 30 runs** (up to 7 in a
single call) — all of them previously lost — and the failure does not recur
(30/30).

**Why not a longer wait**: iter-166 had already raised the window from 300 ms
to 2 000 ms. The measurement above shows the update never arrives *again*, so
no budget can help. `MAX_STATUS_GRACE_MS` is now a named constant pinned by
`unit_169_grace_budget_is_capped` so a future `no_status_reported` cannot be
"fixed" by waiting longer without arguing with a test.

**Why the risk was previously judged acceptable**: `ReadyStateProbe::
poll_enabled`'s doc comment named this exact race and called it narrow, trading
it for the FF152 `dom-complete`-never-fires fast path. That trade is no longer
necessary — replaying costs one channel per round-trip and keeps the fast path.
`probe_same_document_commit_safe` (iter-138) had already solved it for one of
the five call sites; iter-169 generalises that solution rather than inventing a
second one.

**Why `back`/`forward`/`reload` now subscribe rather than emitting a canned
`not_observed`**: iter-130 Theme B promised all four verbs the same envelope,
and `reload` is the verb most likely to be used to re-check a page that was
failing — a real status is what makes that useful. Measured cost on a live
`reload`, see the iteration-169 plan. A BFCache-served `back` legitimately has
no document request and reports `no_document_request`; that is information, not
a failure.

**Applies to**: `crates/ff-rdp-cli/src/commands/navigate.rs`,
`crates/ff-rdp-cli/src/commands/nav_action.rs`,
`crates/ff-rdp-cli/tests/live/live_169_nav_verb_status_parity.rs`,
`crates/ff-rdp-cli/tests/e2e/nav_action.rs`.

## DEC-042: `eval`'s statement scanner classifies every `{` it meets, and refuses to guess on the rest

**Decision** (iter-170): `top_level_statement_boundaries` now records what each
`{` opened — `Block`, `ObjectLiteral`, `Interpolation` or `Unknown` — and uses
that classification for three answers it previously guessed at or skipped:

1. `${…}` inside a template literal is **re-entered** as ordinary code
   (strings, regex literals, comments, nesting), not skipped as opaque text.
   Its `}` returns the scanner to template state.
2. A `/` after `}` opens a regex when the `{` opened a **block** and divides
   when it opened an **object literal**, instead of always dividing.
3. A top-level block's `}` **ends its own statement**, so no `;` and no
   newline is needed after it.

`brace_opens_block` commits only where a statement can start and an object
literal cannot: nothing before the `{`, or `;`, `{`, `)`, or one of
`do`/`else`/`try`/`finally` (excluding a dotted property access). Everything
else — an arrow function's `{` body, a `class` body, a labelled block — stays
`ObjectLiteral`, which reproduces iter-167's answer exactly.

**Why**: iter-167 documented both gaps and asserted both fail safe — "the worst
outcome is a boundary the scanner should not have reported, which costs at most
a wrap". iter-170 measured that against live Firefox and neither did.
``eval --stringify 'const s = `a${"`"}b`; s'`` returned `{"type":"undefined"}`
(the interpolated backtick closed the template, the `"` after it opened string
state, and the script's real `;` was swallowed, so the iter-165 wrap had
nothing to auto-return) — the silent-`undefined` failure iter-142 Theme E named
the worst mode of this wrap. `eval --stringify 'const n = 1; if (n) {}
/a;b/.test("a;b")'` returned `unterminated regular expression literal` — the
exact symptom iter-167 set out to eliminate, one form further along.

**Why the third change, which the plan did not ask for**: fixing (2) alone
turned that SyntaxError into a silent `undefined`, because `if (n) {} /re/.test(…)`
was still scanned as one statement starting with `if`, and an `if` is not
something the wrap can auto-return. Trading a loud wrong answer for a silent
one is not an improvement. (3) is only decidable *because* of the
classification, which is why it belongs in this iteration and not an earlier
one. Suppressed after `(`, `[`, a backtick, any continuation character except
`/`, a comment, and the `else`/`catch`/`finally`/`while` clause keywords.

**Evidence**: 36 scripts run through both binaries against the same live
Firefox (`kb/iterations/iteration-170-*`). Six rows changed, every one from a
wrong answer to the right one — `for (const a of [1,2]) {} 7` → 7,
`function f(){ return 5 } f()` → 5,
`let a = 1; function f(){ return a+1 } f()` → 2,
``const s = `a${"`"}b`; s`` → ``a`b``,
`const n = 1; if (n) {} /a;b/.test("a;b")` → `true`,
`switch (1) { case 1: break; } 11` → 11. Thirty rows byte-identical, including
`const o = {v:8}; o.v / 2` → 4 and `!function(){ return 1 }()`, the two the
classification could most plausibly have broken.

**Trade-off**: three more pieces of JS grammar encoded in a scanner that is
explicitly not a parser, and a `{`-classification whose wrong direction (block
read as object) is silent. That direction is the safe one — it reproduces
iter-167 — and the unsafe direction is only reachable from the four positions
listed above, where JS admits no object literal at all. Replacing the scanner
with a real parser stays rejected for the reason DEC-039 gave.

**Applies to**: `crates/ff-rdp-cli/src/commands/eval.rs`,
`crates/ff-rdp-cli/src/cli/args.rs`,
`crates/ff-rdp-cli/tests/live/live_170_eval_scanner_braces.rs`.

**Addendum (PR review, 2026-08-17)**: the trade-off paragraph above was wrong
— the unsafe direction was reachable from a *fifth* position this iteration's
own measurement never tried: a `function` *expression*'s body. `)` precedes
`{` identically for a function declaration (`function f(){}`, statement
position) and a function expression (`const f = function(){}`, expression
position), and `brace_opens_block` originally classified both as `Block`
uniformly. Live-tested regression: `main` (pre-iter-170) evaluates
`const f = function(){} / 2` to `undefined`; this branch, pre-fix, threw
`unterminated regular expression literal` on it — reading a division as a
regex, exactly the failure this iteration's own text calls "worse than the
current behaviour, because that failure is not safe." Fixed in the same PR by
`function_keyword_is_declaration`, which walks back past a leading `async` to
the same statement-start character classes `brace_opens_block` already uses,
and forces a function *expression*'s body to `ObjectLiteral` (the pre-170
answer) regardless of the `)` immediately before its `{`. Declarations
(`function f(){}`, `async function f(){}`) are unaffected. New coverage:
`unit_170_function_expression_body_is_not_a_statement_block`, three
`live_170_brace_kind_decides_regex_and_boundary` cases. The "only reachable
from four positions" trade-off claim above is superseded by this addendum,
not corrected in place, per this repo's discipline against rewriting a claim
to fit what was later found.

**Addendum (iter-176, 2026-08-23)**: the "stays `ObjectLiteral`, which
reproduces iter-167's answer exactly" paragraph above described the three
remaining positions — an arrow function's `{` body, a `class` body, a labelled
block — as a safe resting place. Measured against live Firefox, none of them
was:

- `class K { m(){ return 9 } } new K().m()` → `{"type":"undefined"}` where
  Firefox returns `9`. A silent wrong *value*, the mode iter-142 Theme E named
  the worst of this wrap.
- `const n = 1; outer: { break outer } n` → `missing ) in parenthetical` where
  Firefox returns `1`.
- With a real line terminator (real ASI, which is what makes the source valid
  JavaScript at all), all three reach the gap-2 symptom:
  `const g = () => {}\n/a;b/.test("a;b")`,
  `class K { m(){ return 9 } }\n/a;b/.test("a;b")` and
  `const n = 1; outer: { break outer }\n/a;b/.test("a;b")` each threw
  `unterminated regular expression literal` where Firefox returns `true`.

So `brace_opens_block` now commits on all three, by the same rule DEC-042
already used — commit only where JS admits no object literal:

- `=>` before the `{`. It is the only two-character token ending in `>` whose
  first character is `=`, and an arrow's block body is never an object literal
  (`() => ({a:1})` needs its parentheses precisely because of that).
- `class`, `class K`, `class K extends B` before the `{`. Both `class` and
  `extends` are reserved words, so an identifier preceded by either can only be
  a class name or a superclass.
- an identifier followed by `:` **at a statement position** — nothing before
  it, a `;`, or a *block*'s `}`. iter-170 left `:` unjudged because "`{a: 1}`
  looks the same from the right", and from the `:` alone it does; one token
  further left it does not, because no object key (`{`- or `,`-preceded), no
  ternary branch (`?`-preceded) and no `case` label (`case`-preceded) can
  occupy a statement position.

A *class expression*'s body is excluded the same way a function expression's
is, and for a stronger reason: a ClassExpression is a PrimaryExpression, so
`const C = class {} / 2` really is a division. The `expr_function_depths`
marker stack from the 2026-08-17 addendum is generalized to `expr_body_depths`
and now takes a `class` keyword seen in expression position too.

**Accepted divergence**: `const g = () => {} /re/.test(s)` with *no* line
terminator is rejected by Firefox (an ArrowFunction is not a division operand,
and ASI needs a newline) but accepted by the scanner, which reads the arrow
body's `}` as self-terminating the way a block statement's is. The same rule
is what makes the newline form — valid JavaScript — work. The divergence only
ever accepts input Firefox would reject; it never changes the value of a valid
script.

**Evidence**: `unit_176_arrow_body_is_a_block`,
`unit_176_class_declaration_body_is_a_block`,
`unit_176_labelled_block_is_a_block`, and the two-test live suite
`crates/ff-rdp-cli/tests/live/live_176_eval_scanner_brace_positions.rs`, whose
second test pins every `}`-then-`/` that must stay a division — including
DEC-042's own two guard rails, `const o = {v:8}; o.v / 2` → 4 and
`!function(){ return 1 }()` → false.

## DEC-043: `live-sweep` re-probes port 6000 per tier and counts unmet preconditions separately from failures

**Decision** (iter-173): `live-sweep` keeps the fixed port 6000 and keeps its
"classify, do not launch" policy (DEC of iter-158 Theme F, unchanged). What
changes is that the probe is no longer taken *once*: it is re-taken
immediately before every target whose tests need that browser, and again after
a phase that failed. Tests whose browser went away in between move out of
`executed` into a new `vanished=V` count and are run *without*
`--include-ignored`, so libtest reports them `ignored`. A separate
`launch_timeout=L` count is carved out for tests that panicked because Firefox
never opened its debug port within the per-test budget. The summary line
becomes
`LIVE_SWEEP_SUMMARY executed=N skipped=M preexisting=K vanished=V launch_timeout=L total=T`.

**Why**: `live-sweep` exists so a live suite cannot report results it did not
earn (iter-155). Reporting an unmet precondition as a failing test is that same
lie with the sign flipped. In iteration 168's sweep the hand-started port-6000
Firefox was killed during the 831-second `ff-rdp-cli` tier, and all seven
`ff-rdp-core` tests were reported `FAILED` with `ConnectionRefused`; they pass
7/7 against a fresh browser. A reviewer who trusts the summary either chases
seven ghosts or learns to discount core-tier reds, and the second is how a real
regression gets waved through. The same shape reached iter-170 as a 30 s launch
timeout under sweep load.

**Why not have the sweep own the browser** (the alternative Theme B offered):
binding 6000 inherits the whole ownership problem the fails-closed guard in
`daemon/client.rs` exists to prevent. Port 6000 is ff-rdp's documented default
and the port a human is most likely to be using by hand; a sweep that launches
on it either collides with the operator or has to decide whether to kill a
browser it did not start — the 2026-07-09 kill-scoping incident. Moving the
`ff-rdp-core` tests to a free port changes their contract and was out of scope.
The harm was never the fixed port; it was asserting a precondition checked once,
forty minutes earlier. Re-probing costs one TCP connect per target.

**Trade-off**: `vanished` does **not** fail the sweep (those tests never
reached a browser), but `launch_timeout` **does**. A launch timeout is a red
libtest result, and turning reds green on inference is the failure mode this
tool exists to prevent; the plan asked only for a distinct count, so that is
all it gets. Both counts are carved out of `executed`, never added on top, so
`total=T` is conserved and no reclassification can inflate the number a PR body
quotes. Attribution is by parsing libtest's `---- <name> stdout ----` blocks,
which means the phases now capture stdout instead of inheriting it — output is
teed through line by line so a 35-40 minute tier still shows progress live.

**Also**: `SELF_LAUNCH_MARKERS` (`LiveFirefox` / `RawFirefox`) now override the
`PREEXISTING_MARKERS` substring test. One of those markers is the bare word
`firefox_port`, which is also a field name in `daemon.<port>.json`, so any
`ff-rdp-cli` live test asserting on that field was silently reclassified as
needing somebody else's browser — iter-172 hit this and worked around it by not
writing the word. With nothing on 6000 such a test would be reported `ignored`
instead of run: iter-155's false green by another road.

**Applies to**: `crates/xtask/src/live_sweep.rs`, `crates/xtask/src/main.rs`,
`CONTRIBUTING.md`, `.claude/skills/iteration-close/SKILL.md`.
