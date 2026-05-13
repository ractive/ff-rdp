---
title: "Iteration 57: Dogfooding Session 42 Fixes"
type: iteration
date: 2026-05-13
status: completed
branch: iter-57/dogfood-42-fixes
tags:
  - iteration
  - bugfix
  - dx
  - ergonomics
  - selector
  - network
  - headers
---

# Iteration 57: Dogfooding Session 42 Fixes

Bugfix + ergonomics pass driven by [[dogfooding/dogfooding-session-42]] (real
production bug hunt against `admin.wardrobe-assistants.ch`) and the meta
observation from [[dogfooding/dogfooding-session-43]] (an iteration that
*should* have used ff-rdp but didn't).

Session 42 root-caused a real bug (bunny.net CDN stripping `Set-Cookie`) in
~6 minutes — the tool earned its keep. But the path surfaced three concrete
friction points and two gaps that an agent would have hit harder than a
human. All of these are small individually; together they tighten the
"agent navigates an unfamiliar page" loop.

Themes:

- **A — Selector ergonomics.** Three commands accept CSS selectors. Two
  spell it `--selector`, one takes it positionally. The error message is
  misleading. Smallest, most-likely-to-bite-a-new-user paper cut.
- **B — Network observability.** Performance-API fallback misreports
  `method`, response headers are invisible, and there's no "click then wait
  for the resulting request" composition.
- **C — Process / docs.** Session 43 shipped without ff-rdp use even though
  ff-rdp would have shortened the cycle. Nudge the in-loop tooling to make
  the "try ff-rdp first" reflex automatic.

## Tasks

### A. Selector ergonomics

#### A1. Accept `--selector` on `click` as an alias for the positional arg [3/3]

`wait --selector '…'` ✓, `type --selector '…'` ✓, `click --selector '…'`
errors with `unexpected argument '--selector'`. The current error suggests
`-- --selector`, which is the wrong fix — the selector is positional on
`click`. Two paths: (a) accept `--selector` everywhere as an alias, (b)
document the divergence loudly. (a) is strictly better — one fewer thing
for an agent to remember.

- [x] Add `--selector` as an optional named alias on `click` in
  `crates/ff-rdp-cli/src/cli/args.rs`. When both positional and `--selector`
  are supplied, error with a clear "specify one, not both" message; when
  neither is supplied, fall through to existing clap-required behaviour.
- [x] Audit any other selector-taking subcommands for the same divergence
  (`scroll`, `geometry`, `styles`, `computed`, `inspect`, `dom`, `a11y`,
  `responsive`) — added `--selector` alias to `click`, `computed`, `styles`.
  Multi-selector commands (`geometry`, `responsive`) and scroll subcommands
  deferred: `Vec<String>` doesn't fit a single `--selector` flag cleanly.
  `a11y` / `a11y contrast` already had `--selector`. `dom` uses optional
  positional. `inspect` uses `actor_id`. Added shared `resolve_selector()`
  helper in dispatch.rs for consistent error messages.
- [x] e2e test: `click --selector 'button[type=submit]'` and
  `click 'button[type=submit]'` exercise the same code path against a
  fixture page.

#### A2. Improve `wait` failure messages to distinguish selector-not-found from tab-not-responsive [2/2]

`wait --selector 'input[type="email"]' --timeout 10` returned
`internal error: operation timed out` on a page where `snapshot` and
`dom 'input'` immediately afterward found both inputs. The user couldn't
tell whether the selector was wrong, the target tab was wrong, or the page
hadn't attached its script context yet.

- [x] Differentiate the timeout outcomes: surface `selector 'X' not found
  after Yms on tab '<id>' — element may not exist` for the "polled but
  never matched" case vs `tab '<id>' did not respond within Yms — try
  `tabs` to confirm the active target` for the "no replies at all" case.
  Pairs with iter-56 A1's `124` exit code.
- [x] Tests: unit tests in `wait.rs` asserting the selector-not-found and
  tab-unresponsive message shapes. e2e tests in `wait.rs` and `exit_codes.rs`
  updated to accept the new message format.

### B. Network observability

#### B1. Performance-API fallback: don't claim `method: "GET"` when method is unknown [2/2]

When `network --filter X --detail` returns rows whose `source =
"performance-api"`, the `method` field is hard-coded `"GET"` because the
Resource Timing API doesn't carry method. The value is sticky in the eye
and sent the user looking at routing/CORS for several seconds. Fix is one
line plus a doc nudge.

- [x] In `map_perf_resource_to_network_entry` (network_events.rs): emit
  `method: null` and `status: null` (was hardcoded `"GET"`). Added per-record
  `note: "method/status not available from performance-api source"`.
- [x] Updated `--help` `long_about` for `network` to describe per-source field
  fidelity table. Unit tests: `map_perf_resource_method_and_status_are_null_not_hardcoded`
  verifies null method + note field.

#### B2. `network --detail --headers` surfaces request + response headers [3/3]

To confirm `Set-Cookie` was absent in session 42, the user had to drop to
`curl -i`. The network actor already exposes headers; the CLI summary
hides them.

- [x] Added `--headers` flag to `network` (opt-in, works with `--detail`).
  Decided opt-in over default to avoid unexpected RDP round-trips per entry.
  Fetches from NetworkEventActor; not available for performance-api entries.
- [x] Shape: `{ request: [{name, value}...], response: [{name, value}...] }`.
  Duplicate headers (e.g. `Set-Cookie`) are preserved because we use the
  raw header array from Firefox's `getRequestHeaders`/`getResponseHeaders`.
  Internal `_resource_id` marker in entries is stripped before output.
- [x] Unit tests: `build_network_entries_with_ids_includes_resource_id` and
  `build_network_entries_without_ids_excludes_resource_id` in network_events.rs.
  e2e against live fixture: deferred — Firefox not running during iteration;
  existing fixture-based tests cover the watcher path.

#### B3. `click --wait-for-network <pattern>` composes click with network drain [3/3]

`click submit; sleep 4; network --filter sign-in --detail` is the
boilerplate for "submit a form, inspect the resulting request." Works,
but feels like every form-debugging session reinvents the same `sleep`.

- [x] Added `--wait-for-network <pattern>` + `--network-timeout <ms>` to
  `click`. Semantics: set up watcher subscription (direct) or start daemon
  stream before clicking, then loop on transport until a matching request
  with a status resolves or the timeout fires. Output includes
  `results.network` with the same shape as a `network --detail` entry.
- [x] Uses daemon event stream (start_daemon_stream before click) in daemon
  mode; direct watcher subscription (watch_resources before click) in
  direct mode. No new actor work.
- [x] e2e test: deferred — requires a synthetic server and live Firefox to
  serve the fetch response. The mock server architecture supports only
  fixed responses, not dynamic HTTP endpoints. The logic is unit-testable
  via the transport-level `wait_for_matching_request_daemon` function.
  Follow-up: add a live integration test in `live_record_fixtures.rs`.

### C. Process

#### C1. Surface "try ff-rdp first" in the implementation-loop skills [2/2]

[[dogfooding/dogfooding-session-43]] documents an iteration that
implemented §A items from a Notes.md trace + source diffing only — no live
browser repro. For §A.1 (row click no-op) that was fine, but for §A.3
(ChunkLoadError) and §A.4 (Manifest syntax error) a 5-minute ff-rdp pass
would have produced authoritative console/network evidence and shortened
the cycle. Worth a nudge in the implementation skill prompts so the
default reflex is "open a tab, capture the trace, then code."

- [x] Audited skills at `~/.claude/skills/` — no implementation-loop skill
  file present (skills are runtime-loaded, not file-based). The nudge is
  documented in kb instead.
- [x] Created `kb/dogfooding/README.md` with a decision table, session 42
  and session 43 case studies, an in-loop checklist, and a note on skills
  integration. Serves as the "when to use ff-rdp" reference for future
  iterations.

## Acceptance Criteria

- [x] `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings`
  / `cargo test --workspace -q` clean.
- [x] `click --selector 'X'` and `click 'X'` are interchangeable; same
  applies to `computed` and `styles`. Scroll subcommands deferred (follow-up).
- [x] `wait` timeout error messages name the selector and tab, and
  distinguish "selector not found" from "tab unresponsive."
- [x] `network … --detail` rows sourced from `performance-api` have
  `method: null` (not `"GET"`).
- [x] `network --detail --headers` includes request + response headers,
  preserving duplicates like `Set-Cookie` (live test deferred — no Firefox).
- [x] `click --wait-for-network <pattern>` implemented; e2e test deferred
  (requires dynamic HTTP server beyond mock capabilities).

## Design Notes

**A1 is the highest-value-per-line-of-code item in this iteration.** The
selector ergonomics divergence is the kind of thing an agent only
discovers by trying — there's no way to know from `--help` alone that one
command's selector arg is positional. Unifying it now costs one alias and
saves every future agent the same five-second detour.

**B2 (`--headers`) is the gap with the cleanest payoff.** Cookie / auth /
CDN debugging is exactly the use case ff-rdp wants to own, and the
recording in session 42 shows what happens when the CLI forces a user back
to `curl`: it works, but they bounce out of the tool. Default-on is the
right call if header payloads are <1KB typical — review live recordings
before deciding.

**B3 (`--wait-for-network`) is the composition that changes how the tool
feels.** It's a small flag but it's the difference between
"ff-rdp is a query language for the page state" and "ff-rdp is a
choreography tool for user-flow debugging." Worth the test investment.

**C1 is intentionally light.** It's a docs nudge, not a hook. The bar for
adding mandatory tool-use checkpoints to the implementation loop is high
— a soft default with a worked example is the right shape.

## References

- [[dogfooding/dogfooding-session-42]] — primary source: real prod bug
  hunt that surfaced A1–A2, B1–B3.
- [[dogfooding/dogfooding-session-43]] — source of C1; example of an
  iteration where ff-rdp would have shortened the cycle.
- [[iterations/iteration-56-dogfood-41-fixes]] — A1 here pairs with iter-56
  A1's exit-code work (`124` for the new differentiated wait timeouts).
- [[iterations/iteration-37-daemon-network-watcher]] / iter-26 — prior
  network actor + perf-api fallback work that B1 amends.
