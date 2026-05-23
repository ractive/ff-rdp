---
type: patterns-review
date: 2026-05-23
tags: [ff-rdp, firefox-devtools, patterns, architecture]
---

# Firefox DevTools patterns we should steal (or skip) in `ff-rdp`

Built on top of the `kb/rdp/` wiki — every pattern below cites a wiki page
*and* a Firefox source file. The goal is not "read Firefox," it is to
identify the half-dozen architectural moves whose absence is repeatedly
costing us live-vs-unit drift and dogfooding regressions (sessions 48–53,
iter-61g..l).

Each section ends with a concrete `Recommendation:` line so the next
iteration plan can lift directly from it.

---

## TL;DR — biggest mismatch

**Firefox treats actor IDs as living references managed by a typed Front
registry; `ff-rdp` treats them as `String`s sprinkled across modules.**
Almost every recurring bug (`consoleActor` staleness, full-page screenshot,
`--with-network` fallthrough, CSP-eval retry not firing) is downstream of
this single architectural gap: there is no central place that owns the
actor → Rust object mapping, listens for `target-destroyed-form`, and
invalidates stale handles. Until we have one, every actor module has to
re-solve target-lifecycle on its own, which they do inconsistently.

Ranked by impact-on-stability:

1. Front lifecycle + actor invalidation (#3)
2. Spec/Front as IDL — typed args/returns (#1)
3. Resource subscription as a shared bus (#4)
4. Multi-actor command coordination (screenshot, inspector) (#5)
5. Async result via deferred event with `mapped: { await: true }` (#6)
6. Errors as data, mapped to typed Rust variants (#8)
7. Wire-level logging toggle (`RUST_LOG=ff_rdp::wire=trace`) (#10)

---

## 1. Spec/Front framework as a code-generated IDL boundary

**Firefox does**: `devtools/shared/specs/*.js` declares every method with
typed args (`Arg(0, "string")`, `Option(0, "nullable:json")`) and typed
returns (`RetVal("targetDescriptor")`, `nullable:dom-node`, `grip`). On the
client, `FrontClassWithSpec(spec)` at
`devtools/shared/protocol/Front/FrontClassWithSpec.js:182` auto-generates
a `Front` subclass whose method calls serialize args into a packet and
unmarshal the reply into typed JS objects. See wiki [[spec-and-front]],
[[devtools-client]]. Concrete eval spec at
`devtools/shared/specs/webconsole.js:149-164`.

**ff-rdp today**: every actor module hand-rolls `json!({…})` builders and
`response.get("foo").and_then(Value::as_str)` extractors. Crate-wide
search shows ~50 such call sites across `crates/ff-rdp-core/src/actors/`.
Field names drift; "we forgot `browsingContextID`" is exactly the
full-page screenshot bug ([[take-screenshot]]).

**Sketch**: a `spec!` macro per actor that takes a spec-shaped table and
generates `struct EvaluateJsAsyncArgs { text: String, mapped: Option<EvalMapped>, … }`
plus `impl ActorMethod for EvaluateJsAsync { … }`. Code-gen at compile
time from a Rust DSL — not from the JS files — but the DSL mirrors them
1:1 so a doc-comment can quote the Firefox spec line and drift is a
review-time concern. We're not running JS; we're paraphrasing it in
typed Rust.

`Recommendation: Adopt later (after #3)` — the type-shape gap is real but
fixing it without first having a Front registry to receive the typed
return values is busywork. Start the migration *with* the registry and
let it pull the typed wrappers in actor by actor.

---

## 2. Per-actor request serialization (FIFO per actor)

**Firefox does**: `DevToolsClient.request()` at
`devtools/client/devtools-client.js:238-293` queues requests per `actor`.
One in-flight per actor; replies match the FIFO order. Different actors
can interleave. The deferred-event pattern (`evaluateJSAsync` →
`evaluationResult`) exists *because* of this rule — otherwise a
long-running eval would block autocomplete. Wiki: [[devtools-client]]
"Per-actor request serialization", [[evaluate-js]].

**ff-rdp today**: `actor_request()` in
`crates/ff-rdp-core/src/actor.rs:11-69` is *connection-level* serial — it
sends, then blocks-reads until `from == to`. Two requests to different
actors can't be pipelined. The "skip-unsolicited-events" loop (actor.rs:42-48)
is a single-connection one-shot, not a queue.

This is mostly fine for the synchronous CLI path. It bites in the daemon
where:
- The watcher pushes events while another actor request is mid-flight.
  Today the watcher events get *swallowed* by `actor_request` (line 45
  drops them silently if `from != to`).
- The `ThreadActor.attach` `{"type":"paused"}` reply case (lessons-learned
  "reply-vs-event") forced us to keep `from == to` as the correlation
  rule rather than the more strict "no `type` field" rule.

**Sketch**: a `RequestRouter` owning the `FramedReader` task; per-actor
`mpsc<oneshot::Receiver<Value>>` queues. Events for actors with active
subscribers are forked to a `broadcast::Sender`. Dropping packets is no
longer the default.

`Recommendation: Adopt now` — this is the cleanup that lets #3 and #4
exist. Today's "drop everything except the one packet we want" eats the
watcher buffer (open-gap "with-network-fallthrough"), which is exactly
the iter-61l C bug.

---

## 3. Front lifecycle + actor invalidation

**Firefox does**: every server actor that descends from a parent target is
auto-destroyed when the target is — `protocol/Pool.js` removes child
fronts and `purgeRequests(actorIDPrefix)` rejects in-flight requests
against them ([[devtools-client]] "purgeRequests"). Cross-origin
navigation publishes `target-destroyed-form` via the WatcherActor
([[watcher]]). On the client, the `TargetFront` instance for the dead
target tears down all its children, including the `WebConsoleFront`.
Source: `devtools/shared/protocol/Front.js:31` (the `Front extends Pool`
base class) and `devtools/server/actors/watcher.js:~750` for the
destroyed-form emission.

**ff-rdp today**: `TargetInfo` is a `Copy`-shaped struct holding `String`
actor IDs (`crates/ff-rdp-core/src/actors/tab.rs:115-120`). Nothing
listens for `target-destroyed-form` on the read path. The
`consoleActor` staleness bug ([[lessons-learned#consoleActor-staleness]],
[[open-gaps]] — surfaced in sessions 51 & 53 AC-K) is exactly this: after
a navigation the cached `consoleActor` string is invalid; the next
`eval` returns `noSuchActor`; we treat it as a fatal error instead of
re-resolving via `getTarget`.

**Sketch — minimal Front registry**:

```rust
struct ActorRegistry {
    // Source of truth: every live actor ID and what it stands for.
    actors: HashMap<String, ActorRef>,
    // Reverse index: targets and their children, for cascade invalidation.
    children: HashMap<String, Vec<String>>,
}

enum ActorRef {
    Console { target: String },
    Walker  { target: String, inspector: String },
    Screenshot,                  // root-scoped, never invalidates
    NetworkEvent { target: String },
    // …
}

impl ActorRegistry {
    fn invalidate(&mut self, actor: &str) -> Vec<String> { /* cascade */ }
}
```

Wired into the read loop: a `target-destroyed-form` event becomes a
`registry.invalidate(target_actor)` call; subsequent commands see
`ActorRef::missing` and trigger a `getTarget` re-resolve.

`Recommendation: Adopt now` — this is the single highest-stability
intervention. Solves consoleActor staleness, makes daemon-mode safe
across navigations, and gives #4 (resource bus) a clean place to live.

---

## 4. Resource subscription pattern

**Firefox does**: `devtools/client/shared/commands/resource-command/resource-command.js`
(a.k.a. `ResourceCommand`) is the client-side façade over WatcherActor.
Multiple panels subscribe to the same resource type; the command
deduplicates subscriptions, batches deliveries, and fans out events to
all interested consumers. Throttled at `RESOURCES_THROTTLING_DELAY = 100ms`
server-side (`devtools/server/actors/watcher.js:65`, wiki
[[watcher]]). Watch-target plus watch-resources are both required —
[[watch-resources]] step-by-step is explicit about this.

**ff-rdp today**: each command that wants events engages the watcher on
its own, then disengages. The daemon does have a `subscriber` model
(`crates/ff-rdp-cli/src/daemon/server.rs:77-102`) but it's specific to
streaming events outbound to CLI clients; the *inbound* watcher
engagement happens elsewhere and isn't shared.

The iter-61l C bug ("`--with-network` populates `buffer_sizes.network-event=209`
but the next `network` call falls through to performance-api") is the
direct symptom: the daemon has the data, but the `network` command's
source-selection logic doesn't reach into the buffer because there is no
"subscribe to network-event in the daemon's resource bus, get historical
+ live events" API. The bus exists half-built.

**Sketch**: lift the daemon's subscriber map into a first-class
`ResourceBus` in `ff-rdp-core` with two operations:

```rust
bus.engage(["network-event", "console-message"])?;  // idempotent — refcounted
let stream = bus.subscribe("network-event", Since::All);  // historical + live
```

`since: all | recent | now` mirrors what we already pass at the CLI. The
network command becomes: `if bus.engaged("network-event") { read buffer }
else { performance-api fallback }`. Headers (#5) re-use the same bus.

`Recommendation: Adopt now` — solves the with-network fallthrough,
unifies the daemon and CLI paths, and is the only way to make
`--headers` not regress `meta.source` (open-gap "headers-source-regression").

---

## 5. Multi-actor command coordination

**Firefox does**: multi-actor flows are wrapped in *command* objects, not
inlined in callers. `devtools/client/shared/screenshot.js:68-116` is the
canonical example — `captureScreenshot()` runs the three RDP calls
(`prepareCapture` on content-scope, compute `snapshotScale`, `capture` on
root-scope) inside one async function. Inspector + Walker + PageStyle
have the same shape (Walker computes the node, PageStyle queries
computed styles on that node, all under one `InspectorCommands` façade).
See wiki [[take-screenshot]] for the full screenshot dance.

**ff-rdp today**: `crates/ff-rdp-cli/src/commands/*.rs` (~50 command
modules) each open-codes their multi-actor coordination. The screenshot
command does the two-step in `commands/screenshot.rs` directly — and
five sessions running have failed to actually pass `fullpage=true`
through to `drawSnapshot` ([[open-gaps#full-page-screenshot]]).
`browsingContextID` similarly leaks across module boundaries.

**Sketch**: a `commands::` module per *protocol command*, not per CLI
verb. `Screenshot::full_page(&mut conn, target, opts)` is the unit; the
CLI verb is a 5-line wrapper that parses args and calls into it. Tests
live next to the protocol command; the CLI doesn't.

`Recommendation: Adapt to ff-rdp` — don't introduce a generic "command"
trait, just colocate multi-actor sequences. Concrete first move: extract
`Screenshot::full_page` out of `commands/screenshot.rs` into
`ff-rdp-core/src/commands/screenshot.rs` and unit-test the wire
sequence against a recorded fixture. Apply the same to Inspector +
PageStyle.

---

## 6. Async result via deferred event

**Firefox does**: `evaluateJSAsync` returns `{resultID}` synchronously,
then later emits a `{type:"evaluationResult", resultID, result, …}`
packet ([[evaluate-js]]; spec `devtools/shared/specs/webconsole.js:149-164`
for the response shape and lines 45-62 for the `evaluationResult` event).
The client's `WebConsoleFront` keeps a `Map<resultID, deferred>` and
resolves the deferred when the event arrives.

Critically: the spec also takes `mapped: Option(0, "nullable:json")`. When
the client sends `mapped: { await: true }`, the server runs the
expression through SpiderMonkey's Debugger API
(`devtools/server/actors/webconsole.js:944`), which is privileged —
it **bypasses page CSP and awaits Promise return values**. Wiki
[[ff-rdp-wins]] item #2 documents this.

**ff-rdp today**: `console.rs:116-200` implements the deferred-event
correlation correctly (it loops waiting for the matching `resultID`).
But:

- We do not send `mapped: { await: true }` — the field is missing from
  the request builder entirely (grep `mapped` in `actors/console.rs`
  returns nothing). This is why CSP-eval keeps biting us on HN and
  lit.dev — the chrome-context retry path is a band-aid that also
  doesn't fire reliably ([[open-gaps#csp-eval-fallback]]).
- We have no general "deferred-event awaiter" abstraction. Every
  actor that wants the pattern re-implements it.

**Sketch**: a `DeferredReply<K>` future. `K` is a key type (`resultID`,
`navigationID`, etc.). Sites register `bus.expect_deferred("evaluationResult", result_id)`,
get a future, and `.await` (or `block_on`) it. The wire reader routes
matching events into the future's slot; a configurable timeout caps the
wait.

`Recommendation: Adopt now` for `mapped: { await: true }` — concrete,
one-iteration change with huge live-impact. `Adopt later` for the
generic awaiter abstraction (currently only `evaluateJSAsync` needs it,
no point generalizing for one consumer).

---

## 7. Bulk packets

**Firefox does**: a second wire framing — `bulk <actor> <type> <length>:<bytes>`
— for large binary payloads (heap snapshots, large source content,
performance profiles). Wiki [[rdp/protocol/transport]] documents both
forms; `devtools/shared/transport/packets.js` parses them.

**ff-rdp today**: we don't parse bulk packets. The wiki
[[ff-rdp-wins#9]] notes this is "not urgent — our actors return base64
dataURLs which is fine". The screenshot path already loads 64 MiB JSON
frames (`MAX_FRAME_BYTES`).

**When it matters**: if Firefox starts returning a bulk packet on a path
we use (no current evidence it does — screenshots stay base64), our
transport would lock up reading framing it doesn't recognize. The
[[rdp/protocol/transport]] note "must, however, be able to *skip* an
inbound bulk packet they don't recognise" is technically violated today.

`Recommendation: Adapt to ff-rdp` — implement "parse bulk header, skip N
bytes, continue" as a defensive measure (~30 lines in `transport.rs`).
Do not implement bulk *send* or bulk-as-primary-screenshot-format until
we measure a perf cliff on the base64 path.

---

## 8. Errors as data, not transport-level

**Firefox does**: a `{from, error, message}` reply is a normal reply. The
Front framework routes it through the same awaiter; `Front.js` rejects
the pending promise with an error whose `.error` is the machine-readable
code and `.message` is the human string. Wiki [[rdp/protocol/error-handling]].

**ff-rdp today**: `crates/ff-rdp-core/src/error.rs:5-32` already does
this well. `ActorErrorKind::from_code` classifies `unknownActor`,
`wrongState`, `threadWouldRun`, `unrecognizedPacketType`; everything
else is `Other(String)`. `actor.rs:51-66` produces a typed
`ProtocolError::ActorError` and the CLI's `error.rs` discriminates IO
vs RDP-side errors.

**Gap**: the CSP-eval case shouldn't be `ActorErrorKind::Other("evalError")`
with a substring-match on `"call to eval() blocked by CSP"`. It should
be `EvalErrorKind::CspBlocked` with the test asserting against a
recorded fixture (lessons-learned "csp-blocks-eval" — *unit-pass,
live-broken*). Same for `about:neterror` post-navigate (gap
"navigate-success-on-bad-dns"): that's not a transport error, it's a
*successful navigation to an error page*, which deserves its own
discriminant.

`Recommendation: Already done` for the basic taxonomy. `Adopt now` for
adding `EvalErrorKind` and `NavigationOutcome::ErrorPage(NetError)` —
both are 30-line additions that unblock live-verify tests.

---

## 9. Hard-coded vs discovered capabilities

**Firefox does**: client always discovers. `RootActor.getRoot()`
(`devtools/server/actors/root.js`) returns the actor map; the descriptor
→ target → child-front chain re-derives actor IDs on each connection.
Hardcoded constants are limited to (a) `"root"` actor ID and (b)
spec-level method names. Wiki [[rdp/overview/connection-lifecycle]],
[[actor-model]].

**ff-rdp today**: `"root"` is hardcoded (fine — protocol-mandated).
Method names are hardcoded as `&str` literals (fine — they're the IDL).
But actor *type* names (`"screenshot"`, `"watcher"`, `"webconsole"`) and
some pseudo-IDs are sprinkled across modules. The architecture-review
agent's earlier findings flagged this.

**Rule we should write into the codebase**:

| Constant kind | Where it goes | Hard-code? |
|---|---|---|
| `"root"` actor ID | `actors::root::ROOT_ACTOR` | Yes (protocol-defined) |
| Method names (`"listTabs"`, `"evaluateJSAsync"`) | per-actor module | Yes (IDL) |
| Actor *type* names (`"screenshot"`, `"watcher"`) | one place, used by `getFront(type)` | Yes (IDL) |
| Actor *IDs* (`"server1.conn0.console5"`) | discovered, never typed | No |
| Resource type names (`"network-event"`) | one place + enum | Yes (IDL) |

**Concrete gap**: `getRoot()` returns `screenshotActor` (we use it,
screenshot.rs:24-37) but also `preferenceActor`, `deviceActor`,
`heapSnapshotFileActor`, etc. that we don't capture into a typed
`RootActors` struct. Some of these we'd want for future iterations
(preference for locale pin, gap "locale-pin").

`Recommendation: Adapt to ff-rdp` — add a `RootActors` struct parsed
once at connect time and stored on the connection. Stop hand-pulling
individual fields out of `getRoot` responses scattered across modules.

---

## 10. Logging and tracing

**Firefox does**: `DEBUG_REMOTE_DEBUG_PROTOCOL` env knob in
`devtools/shared/transport/transport.js` dumps every packet in/out;
`DEVTOOLS_DEBUG_STRINGS=1` toggles structured logging in actor
implementations. Cited via [[rdp/client/transport]].

**ff-rdp today**: zero. `grep tracing\|log::` in `transport.rs` returns
nothing; nothing in `Cargo.toml` either. When live tests fail we either
print-statement the packet or set up a `pcap` against port 6000. This
is exactly why we keep shipping unit-green / live-broken code.

**Sketch**: add the `tracing` crate to `ff-rdp-core`. Add
`tracing::trace!` calls in `transport.rs::send` and `recv` with the
full JSON. Configure via `RUST_LOG=ff_rdp_core::transport=trace`. Add a
`--trace-rdp` CLI flag that turns it on and writes to a file.

`Recommendation: Adopt now` — ~3-hour task, single biggest dev-velocity
win for the next 10 iterations. Should land before iter-61l.

---

## 11. Reconnection / connection lifecycle

**Firefox does**: `DebuggerTransport.close()` triggers
`hooks.onTransportClosed(reason)`; `DevToolsClient` rejects all
pendings and emits `closed`. There is **no** built-in reconnect — wiki
[[rdp/client/transport]] "Reconnection". The UI starts a fresh
connection from scratch.

**ff-rdp today**: the connection is single-shot. Daemon mode crashes
when Firefox dies / restarts.

**Does this matter?** For the CLI: no (each invocation is a fresh
connect). For the daemon: yes, in principle — but Firefox-restart is
rare enough that the simpler answer is "daemon detects socket close,
exits, supervisor relaunches". Implementing reconnect in-process means
re-doing target discovery, re-engaging the watcher, re-resolving every
cached actor ID — exactly the registry of #3 needs to know how to do.

`Recommendation: Reject` — until #3 lands. After #3, "rebuild the
registry on reconnect" is a small follow-up; before, it's a swamp.

---

## 12. CSP-safe eval

**Firefox does**: `evaluateJSAsync({ text, mapped: { await: true } })`
runs through the Debugger API which is chrome-privileged and ignores
page CSP. Server source `devtools/server/actors/webconsole.js:944`;
wiki [[evaluate-js]] §"the gotcha" + [[ff-rdp-wins#2]].

**ff-rdp today**: two attempts, both incomplete:
- Default `evaluate_js_async` (console.rs:116-200) — works on
  CSP-permissive sites, fails on HN/lit.dev.
- `evaluate_js_async_chrome` with `chromeContext: true` (console.rs:210-275)
  — the right idea for *chrome-scope* eval but never fires in the
  daemon path. Live broken across sessions 52, 53.

Neither sends `mapped: { await: true }`. That is the single field that
would fix CSP-eval and Promise-resolution simultaneously.

**Strategy**:
- Default `eval` sends `mapped: { await: true }`. Wrap with `(async () => { … })()`
  if the expression isn't already a valid top-level-await target;
  fall back to plain eval on parse error.
- Surface `meta.eval_path: "await" | "plain" | "chrome"` so consumers
  know which path ran.
- Live-verify against `https://news.ycombinator.com` (CSP
  `script-src 'self' 'unsafe-inline'`) — this is our canonical
  reproducer.
- Keep `chromeContext: true` as a third tier for chrome-scope needs
  (e.g. when we eventually drive `responsive` chrome).

`Recommendation: Adopt now` — concrete, one-iteration fix that closes
two open gaps (csp-eval-fallback, plus the dormant
async-eval-doesnt-resolve-promises footgun). Pair with #10 so we can
see the wire request actually carries the `mapped` field.

---

## 13. Test architecture

**Firefox does**: three tiers.
- `xpcshell` for headless protocol unit tests (`devtools/server/tests/xpcshell/`).
- `mochitest` for full-stack integration (`devtools/client/.../test/`).
- `browser-chrome` for UI-driven flows.
The protocol tests *run a real Firefox instance* — there are no mocks
for the server side. See e.g.
`devtools/server/tests/xpcshell/test_DevToolsClient.js`.

**ff-rdp today**: dual track —
- `mock-server`-based unit tests in `tests/*_test.rs` per actor module.
  Fast, hermetic.
- `live_firefox_test.rs` and `live_record_fixtures.rs` for real-Firefox
  e2e.
The recurring pattern (lessons-learned "csp-blocks-eval",
"with-network-fallthrough", every dogfooding regression) is *unit-green
/ live-broken*. The mock server reproduces what we *think* Firefox
does, not what it does.

**What forces live-verification**:
1. Make recorded fixtures the *only* path. CLAUDE.md already mandates
   "all e2e test fixtures must be recorded from live Firefox"; extend
   to unit tests by deriving them from the same recordings.
2. For every new actor method, the iteration's Definition of Done
   includes one live test, recorded via
   `crates/ff-rdp-core/tests/live_record_fixtures.rs`.
3. CI gate: a `live` job that runs the live suite on PRs touching
   `src/actors/**`. Cost: macOS runner + headless Firefox. Worth it.

`Recommendation: Adapt to ff-rdp` — don't change the dual track, change
the *defaults*. The mock server has saved us during connection-flow
work but loses its edge once the protocol stabilizes. A `#[live_test]`
attribute that skips on `cfg(no_firefox)` would let us mark
live-required tests explicitly.

---

## 14. Documentation as code: spec files

**Firefox does**: the spec files are the contract. `webconsole.js`,
`watcher.js`, `screenshot.js`, etc. under `devtools/shared/specs/`.
Drift is detectable because both client and server consume the spec —
mismatch is a runtime error.

**ff-rdp today**: actor modules under `crates/ff-rdp-core/src/actors/`
have ad-hoc doc-comments, and the wiki [[rdp/actors/README]] is a
parallel knowledgebase. Both can drift from each other and from
Firefox.

**Proposal**: every actor module starts with a doc-comment header of
the form:

```rust
//! `WebConsoleActor` — corresponds to Firefox `devtools/shared/specs/webconsole.js`.
//!
//! Method coverage (verified against Firefox commit <sha> on <date>):
//! - `evaluateJSAsync({text, mapped?, eager?, …}) -> {resultID}` ✅
//! - `evaluationResult` event ✅
//! - `getCachedMessages` ✅
//! - `autocomplete` ❌ (not implemented)
//!
//! Spec drift: when bumping the verified Firefox version, re-read the
//! spec file and update this list.
```

Code review enforces: "you added a new method, did you update the
header?" The wiki [[rdp/actors/README]] then has one canonical pointer
back to these headers.

`Recommendation: Adopt now` — pure documentation, ~1 hour to retrofit
across the 17 actor modules. Catches the next "we forgot a required
field" bug at review time, not in production.

---

## Summary table — pick the next iteration from this

| # | Pattern | Recommendation | Rough size |
|---|---|---|---|
| 3 | Front lifecycle + actor invalidation | Adopt now | M (1 iter) |
| 4 | Resource bus | Adopt now | M (1 iter) |
| 6 | `mapped:{await:true}` eval | Adopt now | S (½ iter) |
| 10 | Wire-level tracing | Adopt now | S (½ iter) |
| 12 | CSP-safe eval (overlaps #6) | Adopt now | S |
| 14 | Spec-file doc-comments | Adopt now | S (1 hr) |
| 2 | Per-actor request router | Adopt now | M |
| 8 | Typed eval / nav error variants | Adopt now | S |
| 5 | Multi-actor command colocation | Adapt to ff-rdp | M |
| 9 | `RootActors` discovery struct | Adapt to ff-rdp | S |
| 7 | Bulk packet skip | Adapt to ff-rdp | S (defensive only) |
| 13 | `#[live_test]` attribute + CI gate | Adapt to ff-rdp | S |
| 1 | Spec-shaped typed-request structs | Adopt later (after #3) | L |
| 11 | Reconnection | Reject (until #3) | — |

**Suggested iter-61m bundle**: #3 + #4 + #6 + #10 + #14. Together they
close five of the seven items in [[open-gaps]] and give every later
iteration a place to plug in.
