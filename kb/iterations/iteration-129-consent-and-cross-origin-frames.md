---
branch: iter-129/consent-and-cross-origin-frames
date: 2026-07-19
depends_on: []
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://www.theguardian.com --auto-consent
  # → results.consent = {"cmp": "sourcepoint", "action": "accepted"} (see DEC-023:
  #   native consent is a new navigate-level --auto-consent flag, not an implicit
  #   consequence of launch's flag — launch --auto-consent still only installs
  #   Consent-O-Matic, unchanged)
  ff-rdp eval 'document.documentElement.className.includes("sp-message-open")'
  # → false (CMP dismissed), page scrollable:
  ff-rdp scroll bottom --jq '.results.scrollHeight'
  # → substantially larger than the viewport height
first_call_sites:
  - primitive: enumerate_frame_targets
    site: crates/ff-rdp-cli/src/commands/click.rs (frame-scan fallback)
  - primitive: TabActor::get_watcher isServerTargetSwitchingEnabled arg
    site: crates/ff-rdp-core enumerate_frame_targets (opt-in frame-aware path)
  - primitive: TargetEvent console_actor/browsing_context_id fields
    site: crates/ff-rdp-cli consent flow (auto-consent CMP accept via frame consoleActor)
status: planned
---

# Iteration 129: consent handling + cross-origin frame reach

The single biggest blocker from [[dogfooding-session-62]] (finding 1, MAJOR): on
theguardian.com, `--auto-consent` (Consent-O-Matic) never records consent for the
Sourcepoint CMP in headless mode. The `sp_message_iframe_*` modal persists on every
page, `html.sp-message-open` sets `overflow:hidden`, so `scroll bottom` silently no-ops
(`atEnd:true`, `scrollHeight` == viewport) and content stays covered. Combined with
finding 6 — `click` cannot reach targets inside the cross-origin CMP iframe and times
out after 10 s with a generic "not ready" — there is **no CLI-native way to accept
consent**, and an agent is fully blocked on Sourcepoint-gated sites.

## Design (settled by the [[frame-targets]] research spike, 2026-07-20)

All mechanisms empirically verified against live headless Firefox 152; no open
questions remain. Verdicts:

1. **Frame enumeration works, gated by one flag.** `watchTargets("frame")` delivers
   `target-available-form` for every window-global target (top + all iframes,
   same-origin AND cross-origin/OOP, uniformly) **only when `getWatcher` is called
   with `isServerTargetSwitchingEnabled: true`**. ff-rdp today calls `getWatcher`
   with no config and therefore receives zero target events. Each form carries
   `actor`, `url`, `title`, `isTopLevelTarget`, `browsingContextID`, `processID`,
   `innerWindowId`, `consoleActor`, `inspectorActor`. The stream stays dark until
   BOTH `watchTargets` and `watchResources` are sent; drain for a settle window,
   dedupe by actor, honour destroyed events.
2. **Click-in-frame = eval on the frame target's own `consoleActor`.** The existing
   eval-based `do_click`/`build_click_js` path works verbatim — only the console
   actor id changes. Console eval is the CSP-bypassing Debugger sandbox, so it works
   on strict-CSP CMP frames (verified: clicked a link inside the OOP example.com
   frame end-to-end). The walker/node path is strictly more work for no payoff —
   not used.
3. **No spec drift.** `isServerTargetSwitchingEnabled` is a published
   `Option(0,"boolean")` on getWatcher (tab.js:24-28); target forms are
   `Arg(0,"json")` opaque blobs (watcher.js:96-105), so reading extra fields is
   spec-compliant. No `// allow-spec-drift` annotations needed.
4. **Sourcepoint confirmed reachable**: on theguardian.com, `sp_message_iframe_*`
   appears as a distinct frame target whose document contains the "Accept all" /
   "Reject all" buttons.
5. **Subtlety to respect:** the flag changes where the TOP-LEVEL target is delivered
   (via the watcher instead of `getTarget`). Implement frame-awareness as an
   **opt-in path** used by the frame-scan/consent flows — do NOT flip the default
   target-acquisition path globally.

## Themes

- **A — core plumbing** (`ff-rdp-core`): optional `isServerTargetSwitchingEnabled`
  arg on `TabActor::get_watcher`; extend `TargetEvent` with
  `consoleActor`/`inspectorActor`/`browsingContextID`/`processID` (pure parse from
  the existing blob); new `enumerate_frame_targets` helper (watchTargets +
  watchResources, settle-drain, dedupe, destroyed handling). Pair with the
  `kb/rdp/actors/` doc updates (check-actor-kb-sync requires it).
- **B — frame-aware `click`.** Fast path: top-document console eval as today. On
  not-found: scan non-top frame targets, evaling querySelector in each until a
  match, then click via that frame's `consoleActor`. Result meta gains `frame_url`
  when the action happened in a frame. Zero matches → error "selector matched in 0
  of N frames (<urls>)" instead of the bare 10 s timeout. Optional
  `--frame <url-substring>` to target a frame directly and skip the scan.
  **iter-128 lesson applied:** `meta.frame_url` must be an always-present key —
  `null` on a top-frame click, the frame's URL string otherwise — not omitted on
  the top-frame path. iter-128 Theme A hit exactly this bug (`hint` silently
  missing instead of `null`, breaking `.frame_url` under `--jq` on the common
  case); do not repeat it here.
- **C — native consent acceptance.** CMP detection + accept flow (Sourcepoint
  selector set first), wired into `--auto-consent` post-navigate and an explicit
  `consent accept`; envelope reports `{cmp: "sourcepoint", action: "accepted"}` when
  a CMP was found and handled, `{cmp: null, action: null}` otherwise — both keys
  always present (same always-present/null-not-omitted discipline as B and as
  iter-128's `hint`/`meta.route`, so the key set never varies with page content and
  `--jq '.results.action'` never throws). Document the Consent-O-Matic headless
  limitation.
- **D — scroll honesty on locked pages.** When `html`/`body` carries
  `overflow:hidden` and a scroll command moves nothing, emit a warning naming the
  locking element/class (e.g. `sp-message-open`) instead of a silent `atEnd:true`.

## Tasks

- [x] A: get_watcher flag + TargetEvent fields + enumerate_frame_targets + actor kb
      sync (opt-in path; default target acquisition untouched).
- [x] B: click frame-scan fallback + `--frame` + `meta.frame_url` + N-frames error.
- [x] C: CMP table + accept flow + `consent accept` + `--auto-consent` wiring +
      envelope reporting; help/cookbook for the consent workflow.
- [x] D: scroll-lock detection + warning.

## Acceptance Criteria [6/6]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [x] live_129_frame_targets_enumerated: on a fixture embedding a cross-origin
      iframe (data: top + https://example.com child), `enumerate_frame_targets`
      yields ≥2 targets including a non-top target with the example.com url and a
      distinct `processID` from the top target.
      (`crates/ff-rdp-core/tests/live_129_frame_targets.rs`; run live —
      confirmed out-of-process child with distinct pids against Firefox 153.)
- [x] live_129_click_cross_origin_frame: `click` actuates an element that exists
      only inside the cross-origin example.com frame (click JS observable effect
      asserted), with `meta.frame_url` reporting the frame.
      (`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs::live_129_click_cross_origin_frame`;
      run live — clicked the example.com anchor via the default auto-wait path.)
- [x] live_129_click_zero_match_error: a selector matching nothing anywhere fails
      fast with the "matched in 0 of N frames (<urls>)" error — no 10 s timeout.
      (`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs::live_129_click_zero_match_error`;
      run live — failed in ~1.1s with "matched in 0 of 2 frames".)
- [x] live_129_sourcepoint_consent (network-gated): navigate theguardian.com with
      the consent flow active → `document.documentElement.className` does NOT
      contain `sp-message-open`, and `scroll bottom` reaches a `scrollHeight` > 2×
      viewport height.
      (`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs::live_129_sourcepoint_consent`;
      run live against the real site — consent accepted, scrollHeight 20470 vs
      viewport 683.)
- [x] live_129_consent_envelope_no_cmp: the consent flow reports `cmp:null` on a
      CMP-free page (example.com); the `cmp:"sourcepoint"` half of this AC is
      covered by `live_129_sourcepoint_consent` above (kept as a separate,
      network-gated test rather than merged in, since it depends on a specific
      real site's current CMP configuration).
      (`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs::live_129_consent_envelope_no_cmp`;
      run live.)
- [x] live_129_scroll_lock_warning: on a fixture with `html{overflow:hidden}`,
      `scroll bottom` emits a warning identifying the scroll lock.
      (`crates/ff-rdp-cli/tests/live/live_129_frames_and_consent.rs::live_129_scroll_lock_warning`;
      run live — warning named the `<html>` element and its `sp-message-open` class.)

## Notes

Design fully settled by [[frame-targets]] — **sonnet-implementable** (additive,
all APIs verified live); use `model-implement sonnet` via new-ralph-loop. The new
core pub items (get_watcher flag, TargetEvent fields, enumerate_frame_targets) get
their first consumers in this same PR per `first_call_sites`.
Sibling plans from the same findings batch: [[iteration-128-network-hint-always-present]],
[[iteration-130-navigation-truthfulness]], [[iteration-131-measurement-honesty]],
[[iteration-132-cli-polish]], [[iteration-133-viewport-emulation]].

**Adapted post-iter-128 (2026-08-09):**
- Always-present/null-not-omitted key discipline (see B and C above) — iter-128
  landed exactly this bug for `hint` (the iter-126 AC test caught a *key-set*
  regression, not a value regression) and had to fix it as a dedicated theme.
  Apply the discipline to `meta.frame_url` and the consent envelope from the
  start instead of re-discovering it here.
- iter-128 Theme D ("all commands") turned out bigger than the rest of that
  iteration combined and was cut to a 2-command slice + a deferred follow-up
  ([[iteration-134-meta-route-all-commands]]). If Theme A's frame-enumeration
  plumbing or Theme C's CMP-selector-table work balloons similarly during
  implementation, cut it the same way — a deferred sibling plan with a named
  scope, not a bloated single PR — rather than force-fitting all four themes
  into one branch.
- Live-test environment note: this dev machine runs many concurrent agent
  sessions launching headless Firefox in parallel; under that load a fresh
  `ff-rdp launch` can occasionally miss the default 30 s debugger-port wait
  even though the same command succeeds in under a second when run alone.
  Given iter-129 is live-test-heavy (5 of 6 ACs need a real Firefox, including
  a cross-origin fixture and the network-gated Guardian test), if `live_129_*`
  tests report "Firefox not available" during CI/ralph-loop runs, retry once
  with `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS=90` before treating it as a real
  failure — it is very likely contention, not a regression. (Confirmed during
  this iteration's own implementation: 2 of 6 new CLI-level live tests
  soft-skipped on the first parallel run and passed cleanly when retried
  alone with `FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS=90`.)

**Live-testing found 3 real protocol bugs the mock-based unit tests could not
catch** (all fixed in this PR, all confirmed live against Firefox 153):

1. **Early-event loss.** `WatcherActor::watch_targets`/`watch_resources`
   (via `actor_request`) can receive `target-available-form` before either
   call's own ACK; with no event sink installed, `recv_reply_from` silently
   dropped those "stray" packets — `enumerate_frame_targets` returned **0**
   targets, not even the top-level one, until a temporary
   `swap_event_sink` was added around both calls (the same class of bug
   iter-121 fixed for `StorageActor::list_cookies`).
2. **Read-timeout clobbering.** `enumerate_frame_targets`'s teardown reset
   the transport's read timeout to `None` instead of restoring the
   connection's actual prior value — every `recv()` after it returned then
   blocked **forever** instead of erroring. A subsequent `evaluateJSAsync`
   call hung indefinitely until `RdpTransport::read_timeout()` (new) let the
   function save-and-restore the exact prior value.
3. **Unwatching destroys the targets you just enumerated.** With
   `isServerTargetSwitchingEnabled: true`, calling `unwatchTargets("frame")`
   tears down **every** target Firefox spawned under that switching regime —
   top level included — destroying their console actors. The original
   design called `unwatchTargets` at the end of `enumerate_frame_targets`
   (mirroring `navigate.rs`'s unrelated prelude pattern); fixed by never
   unwatching inside `enumerate_frame_targets` at all, and by ensuring
   `click`'s auto-wait pre-check and the actual click share **one**
   `enumerate_frame_targets` call (via `prefetched_targets`) rather than
   two — a second `watchTargets("frame")` call on an already-watched
   connection is a silent no-op (doesn't re-deliver known targets), so a
   naive double-call also returned 0 targets on the second pass.

None of these were caught by the unit-test suite (all of which pass against
a scripted mock server that never reproduces Firefox's actual event-ordering
or target-lifecycle behaviour) — only running against real Firefox surfaced
them. See `kb/rdp/actors/watcher.md`'s iter-129 section and the doc comments
on `enumerate_frame_targets` / `RdpTransport::read_timeout` for the durable
record.
