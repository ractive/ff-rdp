---
title: "Iteration 57: Dogfooding Session 42 Fixes"
type: iteration
date: 2026-05-13
status: planned
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

#### A1. Accept `--selector` on `click` as an alias for the positional arg [0/3]

`wait --selector '…'` ✓, `type --selector '…'` ✓, `click --selector '…'`
errors with `unexpected argument '--selector'`. The current error suggests
`-- --selector`, which is the wrong fix — the selector is positional on
`click`. Two paths: (a) accept `--selector` everywhere as an alias, (b)
document the divergence loudly. (a) is strictly better — one fewer thing
for an agent to remember.

- [ ] Add `--selector` as an optional named alias on `click` in
  `crates/ff-rdp-cli/src/cli/args.rs`. When both positional and `--selector`
  are supplied, error with a clear "specify one, not both" message; when
  neither is supplied, fall through to existing clap-required behaviour.
- [ ] Audit any other selector-taking subcommands for the same divergence
  (`scroll`, `geometry`, `styles`, `computed`, `inspect`, `dom`, `a11y`,
  `responsive`) — every selector-taking command should accept
  `--selector` whether or not it also has a positional form.
- [ ] e2e test: `click --selector 'button[type=submit]'` and
  `click 'button[type=submit]'` exercise the same code path against a
  fixture page.

#### A2. Improve `wait` failure messages to distinguish selector-not-found from tab-not-responsive [0/2]

`wait --selector 'input[type="email"]' --timeout 10` returned
`internal error: operation timed out` on a page where `snapshot` and
`dom 'input'` immediately afterward found both inputs. The user couldn't
tell whether the selector was wrong, the target tab was wrong, or the page
hadn't attached its script context yet.

- [ ] Differentiate the timeout outcomes: surface `selector 'X' not found
  after Yms on tab '<title>' (<url>)` for the "polled but never matched"
  case vs `tab '<id>' did not respond within Yms — try `tabs` to confirm
  the active target` for the "no replies at all" case. Pairs with iter-56
  A1's `124` exit code.
- [ ] Test: synthetic page where the selector genuinely never appears (must
  say "not found"); separate test where the actor stops responding mid-wait
  (must say "tab did not respond").

### B. Network observability

#### B1. Performance-API fallback: don't claim `method: "GET"` when method is unknown [0/2]

When `network --filter X --detail` returns rows whose `source =
"performance-api"`, the `method` field is hard-coded `"GET"` because the
Resource Timing API doesn't carry method. The value is sticky in the eye
and sent the user looking at routing/CORS for several seconds. Fix is one
line plus a doc nudge.

- [ ] In the network command's performance-API branch
  (`crates/ff-rdp-cli/src/commands/network*.rs` — see iter-26 / iter-37-38
  for prior work), emit `method: null` and `status: null` when the row
  source is `performance-api`. Add a sibling `meta.warning` (or
  per-record `note`) string: `"method/status not available from
  performance-api source"`.
- [ ] Update `--help` `long_about` to call out the per-source field
  fidelity. Test: assert a perf-api row has `method == null` and the
  warning surfaces.

#### B2. `network --detail --headers` surfaces request + response headers [0/3]

To confirm `Set-Cookie` was absent in session 42, the user had to drop to
`curl -i`. The network actor already exposes headers; the CLI summary
hides them.

- [ ] Add `--headers` flag to `network --detail` (or include headers in
  `--detail` by default — decide based on payload size; default-on is
  cleaner if the typical response is <few hundred bytes JSON).
- [ ] Surface both request and response headers as
  `{ request: [{name, value}...], response: [{name, value}...] }`. Don't
  flatten — duplicate headers (e.g. `Set-Cookie`) must be preserved.
- [ ] e2e test against the live recording: assert that a response with
  `Set-Cookie` shows up, and that a stripped response (no `Set-Cookie`)
  is distinguishable. Record fixture per [[CLAUDE.md]] fixture workflow.

#### B3. `click --wait-for-network <pattern>` composes click with network drain [0/3]

`click submit; sleep 4; network --filter sign-in --detail` is the
boilerplate for "submit a form, inspect the resulting request." Works,
but feels like every form-debugging session reinvents the same `sleep`.

- [ ] Add `--wait-for-network <pattern>` + `--timeout <ms>` flags to
  `click`. Semantics: perform the click, then block until a network
  request whose URL matches `<pattern>` resolves (or times out). On
  success the click command's JSON includes the captured request record
  (same shape as `network --detail`).
- [ ] Reuse the daemon's existing event stream — no new actor work.
  Pattern matches the existing `--filter` substring semantics for
  consistency.
- [ ] e2e test: synthetic page with a submit button that fires
  `fetch('/api/echo')`. `click --selector 'button' --wait-for-network
  '/api/echo'` returns the matched request without a manual `sleep`.

### C. Process

#### C1. Surface "try ff-rdp first" in the implementation-loop skills [0/2]

[[dogfooding/dogfooding-session-43]] documents an iteration that
implemented §A items from a Notes.md trace + source diffing only — no live
browser repro. For §A.1 (row click no-op) that was fine, but for §A.3
(ChunkLoadError) and §A.4 (Manifest syntax error) a 5-minute ff-rdp pass
would have produced authoritative console/network evidence and shortened
the cycle. Worth a nudge in the implementation skill prompts so the
default reflex is "open a tab, capture the trace, then code."

- [ ] Audit the implementation-loop skill prompts (the ones used during
  iteration implementation) for a "did you consider ff-rdp?" checkpoint
  when the iteration involves a frontend bug repro. Don't make it
  mandatory — for pure code refactors it's noise — but make it the
  default for any task whose AC includes "verify in browser."
- [ ] Add a short note in `kb/dogfooding/README.md` (or create it if
  absent) describing when ff-rdp pays for itself vs when code-only
  spelunking is faster. Reference session 43's table of decisions as
  the worked example.

## Acceptance Criteria

- [ ] `cargo fmt` / `cargo clippy --workspace --all-targets -- -D warnings`
  / `cargo test --workspace -q` clean.
- [ ] `click --selector 'X'` and `click 'X'` are interchangeable; same
  applies to the other selector-taking commands audited in A1.
- [ ] `wait` timeout error messages name the selector and tab, and
  distinguish "selector not found" from "tab unresponsive."
- [ ] `network … --detail` rows sourced from `performance-api` have
  `method: null` (not `"GET"`).
- [ ] `network --detail --headers` includes request + response headers,
  preserving duplicates like `Set-Cookie`.
- [ ] `click --wait-for-network <pattern>` returns the matched request
  record without requiring a manual `sleep`.

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
