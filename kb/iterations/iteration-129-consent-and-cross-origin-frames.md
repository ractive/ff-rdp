---
branch: iter-129/consent-and-cross-origin-frames
date: 2026-07-19
depends_on: []
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://www.theguardian.com
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
- **C — native consent acceptance.** CMP detection + accept flow (Sourcepoint
  selector set first), wired into `--auto-consent` post-navigate and an explicit
  `consent accept`; envelope reports `{cmp: "sourcepoint", action: "accepted"}` or
  `{cmp: null}`. Document the Consent-O-Matic headless limitation.
- **D — scroll honesty on locked pages.** When `html`/`body` carries
  `overflow:hidden` and a scroll command moves nothing, emit a warning naming the
  locking element/class (e.g. `sp-message-open`) instead of a silent `atEnd:true`.

## Tasks

- [ ] A: get_watcher flag + TargetEvent fields + enumerate_frame_targets + actor kb
      sync (opt-in path; default target acquisition untouched).
- [ ] B: click frame-scan fallback + `--frame` + `meta.frame_url` + N-frames error.
- [ ] C: CMP table + accept flow + `consent accept` + `--auto-consent` wiring +
      envelope reporting; help/cookbook for the consent workflow.
- [ ] D: scroll-lock detection + warning.

## Acceptance Criteria [0/6]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_129_frame_targets_enumerated: on a fixture embedding a cross-origin
      iframe (data: top + https://example.com child), `enumerate_frame_targets`
      yields ≥2 targets including a non-top target with the example.com url and a
      distinct `processID` from the top target.
- [ ] live_129_click_cross_origin_frame: `click` actuates an element that exists
      only inside the cross-origin example.com frame (click JS observable effect
      asserted), with `meta.frame_url` reporting the frame.
- [ ] live_129_click_zero_match_error: a selector matching nothing anywhere fails
      fast with the "matched in 0 of N frames (<urls>)" error — no 10 s timeout.
- [ ] live_129_sourcepoint_consent (network-gated): navigate theguardian.com with
      the consent flow active → `document.documentElement.className` does NOT
      contain `sp-message-open`, and `scroll bottom` reaches a `scrollHeight` > 2×
      viewport height.
- [ ] live_129_consent_envelope: the consent flow reports `cmp:"sourcepoint"` on
      Guardian and `cmp:null` on a CMP-free page (example.com).
- [ ] live_129_scroll_lock_warning: on a fixture with `html{overflow:hidden}`,
      `scroll bottom` emits a warning identifying the scroll lock.

## Notes

Design fully settled by [[frame-targets]] — **sonnet-implementable** (additive,
all APIs verified live); use `model-implement sonnet` via new-ralph-loop. The new
core pub items (get_watcher flag, TargetEvent fields, enumerate_frame_targets) get
their first consumers in this same PR per `first_call_sites`.
Sibling plans from the same findings batch: [[iteration-128-network-hint-always-present]],
[[iteration-130-navigation-truthfulness]], [[iteration-131-measurement-honesty]],
[[iteration-132-cli-polish]], [[iteration-133-viewport-emulation]].
