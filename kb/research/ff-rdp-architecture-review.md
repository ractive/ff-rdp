---
type: architecture-review
date: 2026-05-23
tags: [ff-rdp, architecture, review]
---

# ff-rdp Architecture Review — 2026-05-23

Candid, concrete analysis of how the workspace is structured today, where it
is brittle, and where changes will land. Not a pitch deck. The reader is
expected to have also read `kb/rdp/` alongside this.

---

## 1. Workspace Layout and Crate Boundaries

Two crates, no others.

| Crate | Purpose | Public surface |
|---|---|---|
| `crates/ff-rdp-core` | RDP protocol library: transport, actor wrappers, types | `ProtocolError`, `RdpTransport`, `RdpConnection`, one struct per actor |
| `crates/ff-rdp-cli` | Binary: clap commands, daemon server, output pipeline, e2e tests | not a library |

The boundary is clean in principle, but there are small leaks in both
directions:

- `ff-rdp-core/src/connection.rs:14-16` hard-codes `COMPATIBLE_FIREFOX_MIN` /
  `MAX` version constants. These are conceptually CLI/UX concerns (they drive
  user-facing warnings) and are re-exported to the CLI via
  `ff-rdp-core/src/lib.rs:38`. The core library should not own compatibility
  policy.

- `ff-rdp-cli/src/commands/screenshot.rs:8` imports `COMPATIBLE_FIREFOX_MIN`
  from core purely to embed it in an error message string. The version window
  is CLI policy.

- The daemon (`ff-rdp-cli/src/daemon/server.rs:241-251`) re-implements the
  greeting validation that `RdpConnection::validate_greeting` already does in
  core, rather than reusing the shared code. Two diverging definitions of "a
  valid Firefox greeting".

No crate depends on the other in the wrong direction. `ff-rdp-cli` → `ff-rdp-core` only.

---

## 2. Module Layout

### ff-rdp-core

| Module | Responsibility | Key public types | Smell |
|---|---|---|---|
| `transport` | Length-prefixed JSON framing, TCP socket management | `RdpTransport`, `FramedReader`, `FramedWriter` | None — clean, well-tested |
| `connection` | Greeting handshake, Firefox version parse | `RdpConnection` | Owns version-compat constants (CLI concern) |
| `actor` | Single `actor_request()` send/receive primitive | (free function) | Event-skipping loop is a "skip unknown `from`" heuristic — see §4 |
| `error` | `ProtocolError`, `ActorErrorKind` | — | Clean |
| `types` | `ActorId`, `Grip` | — | Clean |
| `actors/console` | JS eval, message parsing | `WebConsoleActor`, `EvalResult` | `evaluate_js_async` and `evaluate_js_async_chrome` are near-identical — 80 lines duplicated (lines 116–275) |
| `actors/screenshot` | Root screenshotActor (Fx 149+ step 2) | `ScreenshotActor` | Clean |
| `actors/screenshot_content` | Content screenshotContentActor (step 1 + legacy) | `ScreenshotContentActor`, `PrepareCapture` | Clean |
| `actors/watcher` | `watchResources`, `watchTargets`; parse watcher events | `WatcherActor`, `NetworkResource`, `ConsoleResource` | `parse_single_console_resource` duplicates the argument-joining code already in `parse_console_message` in `console.rs` |
| `actors/tab` | `getTarget`, `getWatcher`, process-descriptor target | `TabActor`, `TabInfo`, `TargetInfo` | `TargetInfo` is a growing bag-of-optional-actor-IDs struct; any new actor type requires editing it |
| `actors/network` | Per-request detail fetch (headers, body, timing) | `NetworkEventActor` | Clean |
| `actors/dom_walker` | DOM tree traversal | `DomWalkerActor`, `DomNode` | |
| `actors/inspector` | InspectorActor | `InspectorActor` | Thin; mostly passthrough |
| `actors/page_style` | CSS computed/applied rules | `PageStyleActor` | |
| `actors/root` | `listTabs`, `listProcesses`, `getRoot` | `RootActor` | |
| `actors/object` | Object grip property expansion | `ObjectActor` | |
| Others | accessibility, device, responsive, storage, string, target, thread | — | Thin wrappers |

### ff-rdp-cli

| Module | Responsibility | Key public types | Smell |
|---|---|---|---|
| `commands/*` (40 files) | One file per CLI subcommand | — | No shared `Command` trait; each file owns its whole pipeline |
| `commands/connect_tab` | Connection handshake, tab resolution, `ConnectedTab` | `ConnectedTab` | `ConnectedTab.via_daemon` leaks daemon topology into every command |
| `commands/screenshot` | Screenshot capture, 3 fallback paths | — | **867 lines of business logic in one function chain**; chrome-scope JS embedded in a Rust `format!` string at line 647 |
| `commands/eval` | JS evaluation with CSP fallback | — | Clean |
| `commands/network` | Network event drain | — | Daemon path vs direct path forked at line 43; daemon path calls a different helper entirely |
| `daemon/server` | Daemon process: Firefox reader, client accept loop, buffer | `SharedState`, `RefStore` | `SharedState` is a single struct with 10 fields, all behind `Mutex`; one Mutex per sub-concern but every client handler must acquire several |
| `daemon/buffer` | `EventBuffer`: ring VecDeque per resource type | `EventBuffer`, `NavBoundary` | Clean |
| `daemon/client` | Daemon discovery, auth | `ConnectionTarget` | Clean |
| `dispatch` | Top-level clap dispatch | — | Giant match arm, no trait abstraction |
| `output_pipeline` | `--jq`, `--format`, `--hints` pipeline | `OutputPipeline` | Clean |
| `output` | Envelope builder, jq runner | — | Clean |
| `output_controls` | `--fields`, `--sort`, `--limit` | `OutputControls` | `--fields` applied manually in each command (see §5) |
| `script/` | Script recorder and runner | — | |
| `hints` | Contextual hint generation | `HintContext` | |
| `error` | `AppError` variants | — | Clean |
| `tab_target` | Tab selection (`--tab`, `--tab-id`) | — | |
| `connection_meta` | Version/host metadata for `--verbose` | — | Global mutable state via a thread-local (or similar) |

---

## 3. RDP Protocol Layer

**Transport**: `crates/ff-rdp-core/src/transport.rs`

The Firefox RDP framing (`<len>:<json>`) is correctly implemented. `recv_from`
reads one byte at a time for the length prefix, then reads the body in one
allocation. `MAX_FRAME_BYTES = 64 MiB` guards against OOM. The `split()` API
returns typed `FramedReader` / `FramedWriter` halves used by the daemon.

**Message model**: All messages are `serde_json::Value`. There is no typed
enum per actor-method pair. Every actor method is a free function or associated
function that:
1. Calls `actor_request()` with a string method name and a `json!({...})` params blob.
2. Pulls fields out of the response `Value` with `.get("field").and_then(Value::as_str)`.

This is the dominant pattern everywhere: `actors/console.rs:120-193`,
`actors/watcher.rs:26-35`, `actors/screenshot.rs:75-96`, etc.

There is no typed Firefox "Front" equivalent. The closest approximation is the
per-actor struct (`WebConsoleActor`, `WatcherActor`, …) whose methods know the
field layout. But these structs carry no state — they are namespaces for free
functions that each take `&mut RdpTransport` as their first arg.

This means shape errors (wrong field name, wrong nesting) are caught at runtime
against a live Firefox, not at compile time. See §10.

**Per-actor request/reply shapes**: Hand-rolled in each actor file. No code
generation, no schema. The shapes are documented implicitly in the response
parsing code.

---

## 4. Actor Handling

**Send-and-await primitive**: `crates/ff-rdp-core/src/actor.rs:11-69`

`actor_request(transport, to, method, params)` is the single send/receive
primitive. It:
1. Merges `to` and `type` into the params object.
2. Sends the frame.
3. Loops reading frames until it sees one with `from == to`, skipping others.
4. Checks for an `"error"` field and maps it to `ProtocolError::ActorError`.

The skip-unknown-from loop (lines 42-48) is the main event-routing mechanism.
It works for simple request-reply actors because Firefox's replies always
include `from`. However:

- **It is not a fanout bus.** When the daemon is not in use and a command both
  subscribes to watcher resources AND sends RDP requests on the same
  connection, `actor_request`'s skip loop silently discards watcher events.
  The daemon solves this by splitting the connection; direct-mode commands that
  also subscribe (e.g. `network` without daemon, line 97 of `network.rs`) must
  drain the watcher themselves after the RPC.

- **Stale actor IDs**: When a page navigates, Firefox invalidates the
  `consoleActor` for the old docshell. The next `actor_request` to the old ID
  returns `{ "error": "noSuchActor" }` which becomes
  `ProtocolError::ActorError { kind: UnknownActor }`. The CLI handles this in
  `connect_tab::ConnectedTab::refresh_target` (line 218) which re-calls
  `TabActor::get_target` to get fresh actor IDs. However `refresh_target` is
  only called from `commands/navigate.rs` after an explicit navigation.
  Commands that do their own navigation (e.g. `eval` on a page that redirects)
  are not protected.

**Async events (notifications from server)**: There is no event bus. Every
actor method that needs to receive unsolicited server events implements its own
inner receive loop:
- `evaluate_js_async` loops waiting for a `evaluationResult` with matching
  `resultID` (console.rs:166-192).
- `network.rs` in direct mode loops reading frames until timeout after
  `watchResources`.
- The daemon's `firefox_reader_loop` runs a dedicated thread and routes by
  inspecting the `type` field (server.rs:295-355).

**`watchTargets` is never called** outside of the `WatcherActor` wrapper
definition. The daemon startup (`server.rs:175-178`) calls only
`watchResources`. This means the daemon never subscribes to
`target-available-form` events, so it does not know when a new top-level frame
target appears after a navigation. The consequences are covered in §6 and §13.

---

## 5. Command Surface

**CLI wiring**: `crates/ff-rdp-cli/src/cli/args.rs` (clap derive) and
`crates/ff-rdp-cli/src/dispatch.rs` (giant match).

Every subcommand is a `Command` enum variant in `args.rs`. The dispatch switch
at the bottom of `dispatch.rs` calls the right `commands::*.run()` function.
No `Command` trait — each `run()` has a bespoke signature.

**Request → execute → format pipeline**:
```
Cli flags → connect_and_get_target() → actor calls → build serde_json::Value results
  → output::envelope() → OutputPipeline::finalize_with_hints()
```

`OutputPipeline` (output_pipeline.rs) handles `--jq`, `--format`, `--hints`.
`OutputControls` (output_controls.rs) handles `--fields`, `--sort`, `--limit`.
Both are instantiated per-command with no shared boilerplate remover; every
command that needs `--fields` must call `output_controls.apply(&mut results)`
manually.

**Known inconsistencies**:

- `--fields` is not applied uniformly. Some commands (e.g. `tabs`) returned a
  different JSON shape per session-51 findings, because `--fields` filtering
  ran after the envelope was built, missing certain shapes.

- `computed` command returns different JSON shapes depending on Firefox
  version: an object map vs an array of `{property, value}` items. The
  fix from iter-61k normalised this but the shape had been polymorphic for
  many iterations.

- `--headers` regression (session-51): `network --headers` stopped working
  after a refactor because the headers-fetch code path was only exercised in
  one branch of the daemon/direct fork in `network.rs`. Unit tests passed
  because the mock server always returns the same fixture regardless of branch.

- No `error_type` / `error_code` machine-readable field in JSON error output.
  CLI errors go to stderr as plain strings; JSON stdout is always the happy
  path. Callers cannot distinguish "tab not found" from "actor timeout" without
  parsing human-readable English from stderr.

---

## 6. Daemon Mode

**Process model**: `ff-rdp _daemon` is a hidden subcommand spawned by
`commands/launch.rs`. It runs as a long-lived TCP proxy on `127.0.0.1:<random
port>`. The port and an auth token are written to a registry file on disk
(`daemon/registry.rs`).

**Internal threads** (server.rs):
- Main thread: `accept_loop` — accepts CLI clients, dispatches daemon protocol
  messages.
- `firefox-reader` thread: `firefox_reader_loop` — reads from Firefox
  indefinitely; routes watcher events to `EventBuffer` or stream subscribers,
  forwards everything else to the `rpc_writer`.
- One thread per CLI client connection: client handler (accept_loop inline).

**How CLI commands communicate with the daemon**: CLI connects to the daemon's
proxy port. After auth, it receives the Firefox greeting. It then sends normal
RDP frames which the daemon forwards verbatim to Firefox. Responses from
Firefox come back through the `rpc_writer` field (the write half of the last
connected "RPC" client). Only one concurrent RPC client is supported.

**Watcher engagement**: The daemon subscribes to
`["network-event", "console-message", "error-message"]` at startup via
`watch_resources` (server.rs:177). **It does not call `watchTargets`.**

This is the root of several known bugs:

- After a navigation, Firefox creates a new `WindowGlobalTarget`. The daemon
  has no `target-available-form` subscription, so it never learns about the
  new target. The `consoleActor` ID on the new target differs from the one the
  daemon held. The daemon's `startListeners` call (server.rs:167) is done once
  on the initial tab target. After navigation, console events via the watcher
  stop flowing until the next direct connection's `startListeners`.

- The `network` command in daemon mode calls `drain_network_from_daemon_since`
  which sends a daemon-level `drain-network` RPC. This bypasses the watcher
  buffer entirely when `--since` is not set to daemon default, per
  `kb/research/network-daemon-issues.md`.

**Known heavy-SPA timeout issue (iter-61l N2)**: For pages that do many
navigations, the daemon's `nav_generation` counter increments and clears the
`RefStore` — but watcher events for resource types can arrive interleaved with
RPC responses in ways the `rpc_writer` single-target model cannot demultiplex.
The timeout manifests because the accept loop can block waiting for an RPC
response while the Firefox reader is still delivering watcher events from a
prior navigation.

**Buffer not consulted by default `network` (iter-61l C)**: When a CLI client
runs `ff-rdp network` (daemon mode), it sends a `drain-network` daemon RPC.
The daemon drains `EventBuffer` and returns buffered events. But the daemon's
watcher subscription for `network-event` must be engaged and the `tabNavigated`
boundary must have been recorded for the buffer to contain the right slice.
If the CLI connects after page load but before the daemon saw `tabNavigated`,
the `--since -1` default returns zero events.

---

## 7. Testing

**Mock server**: `crates/ff-rdp-cli/tests/e2e/support/mock_server.rs` — a
`TcpListener` with a `HashMap<method, handler>`. Supports fixed responses and
sequence handlers. `serve_one()` handles a single connection. No state
machine, no watcher simulation.

**Core unit tests**: Each actor file has `#[cfg(test)]` that spins up
`TcpListener` pairs. These are synchronous and fast. Transport tests use
`Cursor<Vec<u8>>` — no sockets needed. Good coverage of happy-path shapes.

**CLI e2e tests** (`tests/e2e/`): One file per command. Tests use the
`MockRdpServer` to replay pre-recorded fixture JSON. These tests run on CI
without Firefox.

**Live fixture recording**: `crates/ff-rdp-core/tests/live_record_fixtures.rs`
and a `live_61l.rs` companion in the CLI crate. Run with
`FF_RDP_LIVE_TESTS_RECORD=1 cargo test --ignored`. Per CLAUDE.md, all fixture
JSON must be recorded from real Firefox.

**Coverage holes (connection between test passes and live failures)**:

The mock server does not simulate:
1. **Watcher event interleaving**: it sends one response per request. A command
   that subscribes via `watchResources` and then needs to drain events until
   timeout cannot be mocked faithfully — the mock sends the "response" to
   `watchResources` and closes the loop, never delivering `resources-available-array`
   push events.
2. **Navigation events**: `tabNavigated`, `willNavigate` arrive asynchronously.
   Any command that reacts to them (navigate, eval with redirect detection) can
   only be tested against a fixture that pre-embeds these events in the fixed
   handler sequence.
3. **Two-step actor protocols**: the screenshot two-step
   (`prepareCapture` then `screenshotActor.capture`) is tested with separate
   unit tests per step but never end-to-end through the full
   `screenshot::run()` path with a mock. The `try_chrome_scope_screenshot` path
   writes to the filesystem and polls — completely untestable without a real
   Firefox process.
4. **Daemon behavior**: `tests/e2e/daemon.rs` and `daemon_parity.rs` exist but
   test the CLI invoking a real daemon subprocess, which requires a live
   Firefox. Daemon unit tests are absent; `daemon/server.rs` has no `#[cfg(test)]`.

The pattern from sessions 51–53 / iter-61k C/F/G/H/K: mock tests pass because
the mock always replies with the pre-recorded fixture. If the field layout
changed in a newer Firefox version, or if the code path that reads the field is
in a branch not exercised by the mock, the unit test is silent.

---

## 8. Error Handling

`thiserror` in core, `anyhow` in CLI — per CLAUDE.md. Adhered to.

`ProtocolError` (core/error.rs) has six variants with typed `ActorErrorKind`
for the actor-level errors. `is_unknown_actor()`, `is_unrecognized_packet_type()`,
`is_transient()` predicates avoid string matching.

`AppError` (cli/error.rs) has `User`, `Internal`, `Exit`, `Connection`,
`Timeout`, `Diagnostics` variants. `From<ProtocolError>` maps actor error kinds
to user-facing messages with hints.

**Machine-readable errors**: The JSON output envelope (`output::envelope`) only
carries `results`, `total`, `meta`. When a command fails, it exits non-zero
with a human-readable message on stderr. There is no `{ "error": { "type":
"unknownActor", "code": 3 } }` in the JSON output. Callers (agents, scripts)
cannot distinguish error classes without parsing stderr text.

The `Diagnostics` variant (`error.rs:22-26`) is the only structured error; it
is used only by the script runner's assert steps.

**RDP errors vs transport errors vs IO errors**: Well-separated in core.
`RecvFailed(io::Error)` / `SendFailed(io::Error)` / `Timeout` are distinct
from `ActorError`. The CLI's `From<ProtocolError>` preserves this distinction
through to the `AppError` variants (`Connection`, `Timeout`, `User`).

---

## 9. Concurrency and Async

**Not async**. The entire stack is `std::thread` + `std::net::TcpStream` with
`set_read_timeout`. No tokio, no futures.

**Thread model** (daemon): Firefox-reader thread owns `FramedReader`
exclusively. Main accept thread owns `FramedWriter` (wrapped in
`Arc<Mutex<FramedWriter>>`). Client handler threads get a clone of the
`Arc<Mutex<FramedWriter>>` for the Firefox write direction.

**Timeouts**: Socket-level `SO_RCVTIMEO` via `set_read_timeout`. When the
timeout fires, `read_exact` returns `WouldBlock` / `TimedOut` which maps to
`ProtocolError::Timeout`. All blocking loops terminate via this mechanism.

**The timeout-as-success bug (iter-61l F/G)**: Some commands (network drain in
daemon mode, `navigate --wait-text`) treat a read timeout as "we have all the
data" — the drain loop breaks on `ProtocolError::Timeout` and returns whatever
it collected so far. If Firefox is slow and the timeout fires before all events
arrive, the command reports success with partial data. This is intentional for
the drain pattern but produces the misleading success-shaped JSON when the
page needed more time.

**Cancellation**: None. A thread blocked in `read_exact` cannot be cancelled.
The daemon's shutdown path sets `AtomicBool::shutdown` and the Firefox-reader
loop checks it every 1 second (between 1-second read timeouts). Client handler
threads run until the TCP connection closes naturally.

---

## 10. Polymorphic Output and Shape Consistency

The output envelope shape is consistent: `{ results, total, meta? }`. But
`results` itself is polymorphic:
- Scalar object: `eval`, `screenshot`, `perf`.
- Array of objects: `tabs`, `network`, `computed`, `dom`.
- Nested tree: `dom` in ARIA-tree mode, `snapshot`.
- Raw HTML string: `dom --format html`.

`--fields` filtering (output_controls.rs) works on array-of-objects only. It
is applied in each command's run function after building the result. If a
command returns a scalar object and the user passes `--fields`, the filter is
silently ignored.

There is no shared normalisation for the "array of objects with a specific
field schema" pattern. Each command defines its own field names and builds its
own objects. Field names are inconsistent across commands (e.g. `duration_ms`
vs `total_time`, `source` vs `filename`).

The recurring polymorphism issue: `computed` returned either an object or an
array depending on Firefox version before iter-61k fixed it. The root cause is
that the response parsing in `page_style.rs` had to handle two response shapes
(from different Firefox versions) but the normalisation was done inside the
parser rather than at the command boundary.

---

## 11. Hard-coded vs Discovered Actor Names

**Hard-coded (fragile)**:
- `"root"` as the target for `listTabs`, `listProcesses`, `getRoot`
  (`actors/root.rs:11,39,57`). This is correct — root is stable by spec.
- `WATCHED_RESOURCE_TYPES = ["network-event", "console-message",
  "error-message"]` hard-coded in `daemon/server.rs:20`. If Firefox adds or
  renames a resource type, the daemon must be updated.
- Method names in fallback arrays: `CAPTURE_METHODS = ["captureScreenshot",
  "screenshot", "capture"]` (`screenshot_content.rs:13`). This is explicitly
  a version-probing mechanism.

**Discovered at runtime**:
- All actor IDs (consoleActor, screenshotActor, screenshotContentActor, etc.)
  are obtained from `getTarget` responses and stored in `TargetInfo`. This is
  correct — actor IDs are session-scoped.
- The watcher actor ID is obtained via `TabActor::get_watcher` (tab.rs).
- The screenshot actor ID is obtained via `ScreenshotActor::get_actor_id` →
  `getRoot` (screenshot.rs:24).

**Capabilities**: No capability negotiation. Whether a given actor supports a
method is discovered by sending the request and checking for
`unrecognizedPacketType`. This is the fallback mechanism used in
`screenshot_content.rs` and implicitly in all other per-version code paths.

---

## 12. Connection and Lifecycle

The modern flow:

```
RdpTransport::connect_raw()       # TCP, no greeting
greeting = transport.recv()       # Firefox pushes greeting
RootActor::list_tabs()            # listTabs → [TabInfo]
TabActor::get_target(tab_actor)   # getTarget → TargetInfo (consoleActor, etc.)
TabActor::get_watcher(tab_actor)  # getWatcher → watcher ActorId
```

This is the `handshake_and_resolve_tab` path in `connect_tab.rs:152-195`.

**The descriptor → target → watcher flow is implemented correctly** for the
main connection path. `getTarget` is called on the tab descriptor, not on a
`WindowGlobalTarget` actor directly. The `getWatcher` call returns the
`WatcherActor` for the tab.

**What is NOT done**:
- `watchTargets("frame")` is never called on the watcher. The kb/rdp wiki
  documents this as a prerequisite for watcher resource events to flow after
  navigation. Without it, `watchResources` subscriptions survive only for the
  initial load.
- There is no reconnect path. If Firefox closes the connection or crashes,
  the CLI returns an error. The daemon exits. There is no automatic retry.

**Old explicit-attach pattern**: Not used. The codebase never calls `attach` on
a `ThreadActor` for its own purposes (though `thread.rs` exposes the method for
potential future use). Navigation is done via `WindowGlobalTarget::navigateTo`,
not via legacy `navigate` packets to the tab actor.

---

## 13. Three Known Bugs Through an Architectural Lens

### A. `screenshot --full-page` (5+ sessions broken)

**Where the two-actor coordination lives**: `commands/screenshot.rs:283-465`,
specifically `try_two_step_screenshot`.

The two-step protocol IS implemented:
1. `ScreenshotContentActor::prepare_capture` (content process) — lines 300-310.
2. `ScreenshotActor::capture` (root screenshotActor) — lines 373-386.

**What is still broken**: `screenshotActor.capture` calls
`browsingContext.drawSnapshot` under the hood, which ignores the `rect`
argument on Firefox 149-151+ and clips to the viewport. The code detects this
at line 397 (comparing PNG height to expected scroll height) and falls back to
`try_chrome_scope_screenshot`. That path embeds a 100-line JavaScript string
(lines 647-747) that uses `ChromeUtils.importESModule`, `captureScreenshot`,
`nsIFile`, `nsIBinaryOutputStream` and polls the filesystem.

**The missing abstraction**: The chrome-scope path is a fully separate capture
strategy with its own async-JS + filesystem-polling mechanism, embedded inside
a Rust `format!` string. It has no unit tests. Its failure modes are detected
by reading `.err` sentinel files. There is no shared interface between the
three capture strategies (canvas JS, legacy actor, two-step actor, chrome-scope
JS). Adding a fourth strategy would require more nested fallback arms.

**What would need to change**: Extract a `CaptureStrategy` trait or enum with
a `capture(&mut ctx, opts) -> Result<String, AppError>` method. Each strategy
becomes a separate testable unit. The chrome-scope JS could be a `.js.template`
file embedded via `include_str!` rather than a `format!` blob.

### B. `eval` CSP block — `mapped: { await: true }`

**Current approach** (`commands/eval.rs:176-208`): `eval_with_csp_fallback`
sends the normal `evaluateJSAsync` request, detects a CSP exception, then
retries with `evaluate_js_async_chrome` (which sets `chromeContext: true`).

**What the kb/rdp wiki reveals**: `mapped: { await: true }` is a better
solution. It causes Firefox to evaluate the script via SpiderMonkey's `Debugger`
API, which is privileged and bypasses page CSP entirely — without needing
`chromeContext`. It also enables Promise resolution (currently broken without
this flag).

**Where the flag would go**: `WebConsoleActor::evaluate_js_async`
(`core/actors/console.rs:121`). The request JSON at line 120-127 would gain
`"mapped": { "await": true }`. The response shape is identical. The current
fallback path (`eval_with_csp_fallback`) could then be removed or simplified to
a no-op.

**Why the current approach is fragile**: `evaluate_js_async_chrome` with
`chromeContext: true` evaluates in a privileged context where `document` /
`window` may not be the content page's globals. The `mapped.await` approach
evaluates via `Debugger.Object.call` which stays in the content sandbox but
bypasses CSP. The chrome-context fallback is a workaround for a problem the
protocol already has a first-class solution for.

**The deferred `evaluationResult` event pattern**: Already correctly implemented.
`evaluate_js_async` loops reading frames until it finds `type ==
"evaluationResult"` with matching `resultID` (console.rs:166-192). This works
for both sync and async (awaited) results — the event arrives in both cases;
the difference is only when it arrives.

### C. WatcherActor not engaged — `watchTargets` missing

**Current daemon startup** (server.rs:175-178):
```rust
let watcher_actor = TabActor::get_watcher(&mut transport, &tab_actor)...;
WatcherActor::watch_resources(&mut transport, &watcher_actor, WATCHED_RESOURCE_TYPES)...;
```

`watchTargets("frame")` is never called. The `WatcherActor::watch_targets`
method exists in core (watcher.rs:60-71) but is never invoked anywhere in the
CLI.

**Consequence**: After a SPA navigation, Firefox creates a new
`WindowGlobalTarget`. Without `watchTargets("frame")`, the daemon never
receives `target-available-form` events for the new target. Resources from the
new target (network events, console messages) are not routed through the
daemon's watcher subscription. The daemon's `EventBuffer` stops receiving new
data after the first navigation until the next reconnect.

**Where the call would go**: `daemon/server.rs` immediately after
`WatcherActor::watch_resources`, using the same `watcher_actor`:
```rust
WatcherActor::watch_targets(&mut transport, &watcher_actor, "frame")?;
```

The daemon's `firefox_reader_loop` would then receive `target-available-form`
events. It would need to handle them: extract the new `consoleActor` from the
target form and call `startListeners` on it to re-enable console message
delivery. The current code has no handler for `target-available-form` — adding
it would require an extension to the reader loop's dispatch logic.

---

## 14. Friction Surfaces Summary

| Surface | Where | Impact |
|---|---|---|
| No typed protocol message schema | All actor files — `Value` everywhere | Runtime-only shape errors; mock tests can pass while live fails |
| `screenshot.rs` 1059 lines with 4 fallback strategies | `cli/src/commands/screenshot.rs` | Hard to test, extend, or reason about; JS embedded as format strings |
| Daemon has no `watchTargets("frame")` | `cli/src/daemon/server.rs:175` | Network/console events stop flowing after SPA navigation |
| `mapped: { await: true }` missing from eval | `core/src/actors/console.rs:120` | CSP bypass and Promise resolution are implemented as workarounds instead of using the first-class protocol mechanism |
| `evaluate_js_async` / `evaluate_js_async_chrome` duplication | `core/src/actors/console.rs:116-275` | 80 lines duplicated; divergence risk |
| Mock server cannot simulate watcher push-event streams | `tests/e2e/support/mock_server.rs` | Any command relying on watcher event draining has no unit-testable mock path |
| `TargetInfo` bag-of-optionals | `core/src/actors/tab.rs:35-59` | Adding any new per-target actor requires editing the struct; callers can't know which actors are present for a given target type |
| No machine-readable error shape in JSON output | `cli/src/error.rs`, `cli/src/output.rs` | Agents/scripts can't classify errors without parsing stderr strings |
| `version-compat constants` in core library | `core/src/connection.rs:14-16` | CLI policy leaks into the library; version window must be updated in core, not in the CLI where it's used |
| `--fields` applied per-command, not in shared pipeline | `cli/src/output_pipeline.rs`, every `commands/*.rs` | Inconsistent application; field-filter bugs are silently per-command |
