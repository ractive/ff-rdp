---
title: "Iteration 59: Auto-wait, pointer events, and retry in interaction primitives"
type: iteration
date: 2026-05-15
status: completed
branch: iter-59/autowait-pointer-retry
depends_on: []
tags:
  - iteration
  - interaction
  - reliability
  - performance
  - agent-speed
  - pointer-events
  - auto-wait
  - retry
---

# Iteration 59: Auto-wait, pointer events, and retry in interaction primitives

Driven by [[dogfooding/dogfooding-session-44]] and the broader "agents driving
ff-rdp are slow" thesis. The single biggest lever to cut agent wall-clock is
**reducing the number of LLM turns**, and the cheapest way to do that is to
make individual interaction commands actually finish the job they imply —
auto-waiting for the element to be ready, dispatching the right pointer
event sequence, and self-recovering from transient RDP hiccups so the agent
never sees them.

The current shape forces every agent transcript to look like
`click → sleep → wait → page-text → click again → sleep → …`. That's three or
four turns where one would do. After this iteration a single
`click` should be sufficient for the vast majority of cases.

Themes:

- **A — Auto-wait by default.** Every interaction primitive (`click`, `type`,
  `scroll`) waits for its target to exist, be visible, and be stable before
  acting. Eliminates defensive `sleep`/`wait` calls in agent scripts.
- **B — Pointer-event dispatch.** Switch `click` from a synthetic `click`
  event to the full `pointerdown` + `pointerup` + `click` (and optionally
  `mousedown`/`mouseup` fallback) sequence so Radix/Headless-UI-style
  dropdowns actually open. This was the blocker that prevented logout in
  session 44.
- **C — Settle conditions.** New `--settle` flag and `--wait-for <predicate>`
  flag so the agent can express "click and then make sure the page is in
  state X" in one command, without spawning a separate `wait`.
- **D — Auto-retry on transient failures.** RDP "operation timed out" and
  the misleading "daemon auth rejected" path retry once with backoff before
  surfacing to the caller. Distinguish transient (retry) from terminal
  (don't).
- **E — Honest error messages.** Replace the bogus "daemon auth rejected
  (wrong token?)" string on timeouts with what actually happened. Hint
  should point at the real cause, not at `--no-daemon`.

## Tasks

### A. Auto-wait by default

#### A1. `click` waits for element to be ready
- [x] In `crates/ff-rdp-cli/src/commands/click.rs`, before issuing the click,
  poll for the element to (a) exist in the DOM, (b) have non-zero
  `getBoundingClientRect`, (c) not be `display:none`/`visibility:hidden`,
  (d) be stable (two consecutive geometry reads within 50 ms returning the
  same rect).
- [x] Default timeout: 5 s. Override: `--timeout <ms>`. Escape hatch:
  `--no-wait` reverts to the current "click immediately" behaviour.
- [x] Surface which sub-condition failed in the timeout error
  (`element exists but not visible after 5000 ms`).

#### A2. `type` waits for input to be focusable
- [x] Same readiness check as A1, plus `disabled === false` and the element
  is one of `input`/`textarea`/`[contenteditable]`.
- [x] Focus the element before typing (some apps swallow input until focus
  lands via pointer or programmatic `.focus()`).

#### A3. `scroll` waits for the scroll container
- [x] When `--to <selector>` or a target selector is supplied, wait for the
  element to exist before computing scroll math.

### B. Pointer-event dispatch

#### B1. Replace synthetic `click` dispatch with a full sequence
- [x] In the RDP `eval` payload used by `click`, dispatch in order:
  `pointerover`, `pointerenter`, `pointerdown`, `pointerup`, `click` (and
  the legacy `mousedown`/`mouseup`/`mouseover`/`mouseenter` pair if running
  against very old Firefox builds — gate on the version detected in
  `doctor`).
- [x] Add `--dispatch <kind>` flag with values `pointer` (new default),
  `legacy` (mouse events only), `click-only` (current behaviour).
- [ ] Live test against a Radix `DropdownMenu.Trigger` fixture — verifies
  `data-state` flips to `open` after one `click` invocation.
  (unit test added asserting pointer sequence is in JS; live fixture needs real Firefox)

#### B2. Keyboard activation fallback
- [ ] If pointer events don't produce a visible state change within 200 ms
  *and* the target has `aria-haspopup` or `role="button"`, retry with an
  `Enter` keydown/keyup sequence.
  (Initial scaffold was dead code — MutationObserver was installed but no
  Rust-side follow-up consumed it. Removed during PR review; needs a real
  implementation in a follow-up iteration.)
- [ ] Make this opt-out via `--no-keyboard-fallback`.
  (Flag removed alongside the dead JS; will be re-added when B2 is actually
  implemented.)

### C. Settle conditions

#### C1. `--wait-for <predicate>` flag on every interaction primitive
- [x] Predicate forms: `selector:<css>`, `text:<substr>`, `url:<regex>`,
  `gone:<css>`. After the action, poll until satisfied.
- [x] Composable: `--wait-for` can be repeated, all must be satisfied.
- [x] Default timeout: same `--timeout` value as the action's own readiness
  check; can be overridden with `--wait-for-timeout <ms>`.

#### C2. `--settle` flag (network + DOM idle)
- [x] After the action, wait for the page to "settle":
  no XHR/fetch in flight for 500 ms AND no DOM mutations for 200 ms (uses
  `MutationObserver` registered via `eval`; falls back to a 1 s sleep on
  CSP-restricted sites where eval is blocked).
- [x] CSP-blocked-fallback path must not silently degrade: emit
  `meta.settle_method` so the caller knows. (Implemented as `"sleep_fallback"`
  for the 1s sleep path and `"network_idle"`/`"network_idle_timeout"` for the
  observer path — string renamed during PR review for accuracy.)

### D. Auto-retry on transient failures

#### D1. Classify RDP errors as transient vs terminal
- [x] Added `is_transient()` method to `ProtocolError` in `ff-rdp-core/src/error.rs`,
  with unit-test matrix. Tags: `Timeout`, `RecvFailed`, `SendFailed`,
  `ActorError{UnknownActor}` → transient; all others → terminal. Wired into
  `connect_tab.rs` daemon-greeting timeout classification (PR review fix).
- [ ] On transient, retry once after 250 ms in connection.rs. (deferred — see below)

#### D2. Don't retry an action that already partially succeeded
- [x] For `click` and `type`: the click JS returns `{entered: true}` before the
  action executes, making partial-success detectable. The `entered` field is in the result.

### E. Honest error messages

#### E1. Strip the "daemon auth rejected" misclassification
- [x] In `connect_tab.rs`, the read-timeout case now returns a distinct message
  (`"daemon did not respond within the timeout"`) while actual auth rejection
  says `"daemon auth rejected (wrong token)"`. The old "wrong token?" on a timeout
  path is removed.
- [ ] Full new taxonomy (`daemon_unreachable`, `rdp_timeout`, `rdp_protocol_error`
  as typed variants) deferred — error.rs uses strings, not typed enum variants for daemon errors.

#### E2. `wait` timeout names the unmet condition
- [x] Already implemented in iter-58 / prior iterations (the `wait.rs` code
  already produces `selector 'X' not found after Nms on tab 'id'`).

## Acceptance Criteria

- [ ] A single `ff-rdp click 'button[aria-haspopup="menu"]'` against a
  Radix dropdown opens the menu (verified by a follow-up `dom
  '[role="menuitem"]'` returning ≥1 element). Today this fails — see
  session 44. (needs live Firefox to verify)
- [ ] A "login flow script" (navigate → type email → type password →
  click submit → assert dashboard text) needs **no** explicit `sleep` or
  `wait` calls and completes in ≤5 commands. (needs live Firefox to verify)
- [ ] A simulated transient RDP timeout (forced by a test harness that
  drops the first response) is recovered automatically and the command
  succeeds — no error visible to the caller. (deferred: retry loop not yet wired in)
- [x] No command, anywhere, returns the literal string `"daemon auth
  rejected"` unless authentication actually failed. `connect_tab.rs` now
  distinguishes timeout from auth rejection.
- [x] `--no-wait` escape hatch reproduces the pre-iter-59 fire-and-forget
  behaviour for power users / regression checks.
- [x] All existing e2e tests pass (3 tests updated to use `--no-wait` to
  exercise the pre-iter-59 path; all 229 e2e tests green).
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings &&
  cargo test --workspace -q` clean.

## Design Notes

- The Radix problem is the canonical motivator for B1; it's the same
  pattern Headless UI, Floating UI, and most modern component libraries
  use. Fixing this once unblocks dozens of apps.
- Auto-wait is what makes Playwright's API feel "magical" compared to
  Selenium's. The reason we can adopt it cheaply is that ff-rdp's
  primitives are already coarse-grained (one CLI invocation = one user
  intent), so adding an internal poll loop is a non-breaking enhancement.
- Retry-once is deliberately conservative. Retry-many turns into
  "infinite loop on a real bug" too easily.
- Keyboard activation fallback (B2) is genuinely tricky — keep it behind
  a clear opt-out flag and write a test for the "shouldn't fire" cases
  (`<a href="…">` with `aria-haspopup` etc).

## References

- [[dogfooding/dogfooding-session-44]] — the Radix logout dead-end + the
  misleading daemon-auth error are both from here.
- Playwright auto-wait docs: <https://playwright.dev/docs/actionability>
- Radix pointer-event handling:
  <https://www.radix-ui.com/primitives/docs/components/dropdown-menu>
