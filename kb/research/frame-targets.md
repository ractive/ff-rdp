---
title: "Frame Targets over RDP: enumerating & acting inside cross-origin (Fission) iframes"
date: 2026-07-20
type: research
status: completed
tags:
  - rdp
  - frames
  - research
firefox_refs:
  - devtools/shared/specs/descriptors/tab.js:24-28
  - devtools/shared/specs/watcher.js:10-15
  - devtools/shared/specs/watcher.js:96-105
  - devtools/server/actors/watcher.js:270-310
  - devtools/server/actors/targets/window-global.js:520-560
---

# Frame Targets over RDP

Research spike for iteration-129 (consent + cross-origin frame reach). Question:
over Firefox's TCP Remote Debugging Protocol (**not** WebDriver BiDi), how can a
client enumerate a tab's iframe targets — including cross-origin, out-of-process
(Fission) iframes — and act inside one (querySelector + click), so ff-rdp's
`click` can reach a Sourcepoint CMP iframe (`sp_message_iframe_*`) on
theguardian.com?

**All verdicts below are empirically verified** against a live headless Firefox
(macOS, Firefox from `/Applications/Firefox.app`) using probe binaries in the
session scratchpad. Probe source and full transcripts are cited inline.

## TL;DR verdicts

| Question | Verdict |
|---|---|
| 1. Frame-target enumeration via `watchTargets("frame")` | **YES — but only if `getWatcher` is called with `isServerTargetSwitchingEnabled: true`.** Without that flag, ff-rdp today gets ZERO `target-available-form` events (this is the current gap). |
| 2. Acting inside a frame target | **YES via the frame target's own `consoleActor`** (present in the target form). Reuses ff-rdp's entire existing eval-based click path — just point it at a different console actor ID. High confidence, verified end-to-end. |
| 3. Same-origin (in-process) iframes | **Also delivered as separate frame targets** (same `processID` as top, own `consoleActor`). Uniform handling — no need to special-case in-process vs OOP for enumeration. |
| 4. Spec drift | **None.** `isServerTargetSwitchingEnabled` is a published `Option` on `getWatcher`; target forms are `Arg(0,"json")` opaque blobs, so reading any field is spec-compliant. |

---

## Question 1 — Frame-target enumeration

### The critical finding: `isServerTargetSwitchingEnabled` gates frame targets

ff-rdp's `TabActor::get_watcher` (`crates/ff-rdp-core/src/actors/tab.rs:140-154`)
calls `getWatcher` with **no config**. In that mode, `watchTargets("frame")`
delivers **nothing** — not even the top-level target. Empirically confirmed:

```
# dump2 (fresh connection, no getWatcher config, watchTargets first):
#1 type=resources-available-array ...
#3 type=<none> from=...watcher4        # watchTargets ACK
total messages: 4 target-events: 0     # <-- ZERO target forms
```

The reason is in `devtools/server/actors/watcher.js` `watchTargets` (~L270-310)
combined with the browser-element session context: when server-side target
switching is **disabled**, the top-level target is instantiated by the descriptor's
`getTarget` (not by the watcher), and child window-global targets are simply not
spawned server-side — the legacy DevTools client was expected to walk frames
itself. `getWatcherSupportedTargets()` lists `FRAME: true` universally, but
"supported" is not "spawned".

Passing `isServerTargetSwitchingEnabled: true` flips this. The same probe, only
difference being the getWatcher config, delivers every window-global target:

```
# dump3 (getWatcher {isServerTargetSwitchingEnabled:true}, watchTargets("frame")):
#1 type=target-available-form  isTopLevelTarget=true   url=data:text/html,<h1>top</h1>...  processID=71266
#4 type=target-available-form  isTopLevelTarget=false  url=https://example.com/            processID=71328
total messages: 8 target-events: 2
```

Fixture: top document is a `data:` URL (a unique origin) with
`<iframe src="https://example.com">`, so the child is genuinely cross-origin.
Note **`processID` differs** (71266 vs 71328) → the child is out-of-process under
Fission. (Independently confirmed: from the top document,
`document.querySelector('iframe').contentDocument` returns **null** —
`canAccessChild: "NULL_CONTENTDOC"` — so the top walker cannot reach it.)

### What identifies each target — the `target-available-form` body

Verified raw form for the cross-origin child (excerpt, real bytes off the wire):

```json
{
  "type": "target-available-form",
  "target": {
    "actor": "server1.conn4.watcher3.process7//windowGlobalTarget2",
    "targetType": "frame",
    "browsingContextID": 8589934593,
    "processID": 71328,
    "innerWindowId": 15032385537,
    "parentInnerWindowId": 8589934594,
    "topInnerWindowId": 8589934594,
    "isTopLevelTarget": false,
    "isPopup": false,
    "isPrivate": false,
    "title": "Example Domain",
    "url": "https://example.com/",
    "outerWindowID": 35,
    "consoleActor":   "server1.conn4.watcher3.process7//consoleActor3",
    "inspectorActor": "server1.conn4.watcher3.process7//inspectorActor4",
    "styleSheetsActor": "…", "accessibilityActor": "…", "threadActor": "…",
    "screenshotContentActor": "…", "networkContentActor": "…"
    /* …full extra-actor set… */
  },
  "from": "server1.conn4.watcher4"
}
```

Fields that matter for iteration-129:
- **`url`** — match target against a selector's expected frame / CMP host.
- **`isTopLevelTarget`** — `false` marks a subframe.
- **`browsingContextID`** — stable per-frame id (also usable with the ScreenshotActor).
- **`consoleActor`** — **the payoff**: each frame target ships its own web-console
  actor, ready for `evaluateJSAsync`.
- **`inspectorActor`** — the frame's own inspector (→ walker) if a node-based path is
  ever wanted (not needed for the eval approach).
- **`processID`** — top vs child differ ⇒ OOP; equal ⇒ in-process (see Q3).

The Firefox `form()` that produces this is `WindowGlobalTargetActor.form()`
(`devtools/server/actors/targets/window-global.js` ~L520). The `consoleActor`/
`inspectorActor`/etc. keys come from the target's `_createExtraActors()` pool and
are appended to the base form.

### What ff-rdp already has vs needs

Already present (`crates/ff-rdp-core`):
- `WatcherActor::watch_targets(transport, watcher, "frame")` and the
  `WatcherFront::watch_targets` typed wrapper — send the request fine.
- `WatcherEvent` + `parse_target_event` + `TargetEvent`
  (`src/actors/watcher.rs:146-257`) — already parse `target-available-form` /
  `target-destroyed-form`, extracting `actor`, `url`, `title`, `targetType`,
  `isTopLevelTarget`.
- `dispatch_watcher_event` + `Registry::invalidate_target` — target lifecycle
  already wired (used by the daemon, `daemon/server.rs:1296+`).
- The daemon already receives and counts `target-available-form` (server.rs:3679
  test) — so the plumbing for the events exists.

Gaps iteration-129 must close:
1. **`get_watcher` must send `isServerTargetSwitchingEnabled: true`** (currently
   omitted). This is the single change that turns on frame-target delivery. The
   spec field is published (see Q4). CAUTION: this changes the actor-id shape of
   the top-level target too (it now arrives via the watcher as
   `watcher3.process4//windowGlobalTarget2` rather than the `getTarget` `frame`
   form), so audit callers of `TabActor::get_target` before flipping it globally —
   safest to add an opt-in path used only by the frame-aware `click`.
2. **`TargetEvent` does not extract `consoleActor` / `inspectorActor` /
   `browsingContextID` / `processID`.** These live in the same JSON blob; add
   fields. No spec change (blob is opaque `Arg(0,"json")`).
3. **No enumeration/collection step.** Need a small helper that issues
   `watchTargets("frame")` + `watchResources([...])` (the stream stays dark until
   BOTH are sent — see `commands/navigate.rs:864-872`), drains
   `target-available-form` for a short settle window, dedupes by actor id, and
   applies `target-destroyed-form` removals.

---

## Question 2 — Acting inside a frame target

### Mechanism: eval on the frame's own `consoleActor` (recommended)

Because the frame target form already carries a `consoleActor`, the simplest and
most reliable path is to run the **existing** click JS via
`WebConsoleActor::evaluate_js_async(transport, frame_console_actor, js)` — the very
same call `do_click` already uses, only the actor id differs.

Verified end-to-end inside the cross-origin, out-of-process example.com frame
(probe `act`):

```
cross-origin frame url="https://example.com/" consoleActor=…process7//consoleActor3
  [location.href]          "https://example.com/"
  [document.title]         "Example Domain"
  [querySelectorAll count] 4
  [anchor text]            "Learn more"
  [click anchor]           "CLICKED:https://iana.org/domains/example"
```

`document.querySelectorAll` sees the frame's DOM, and `element.click()` fires in
the frame. This is a full querySelector + click inside an OOP iframe over RDP.

### Confirmed on the real target — theguardian.com Sourcepoint CMP

Probe `guardian` (navigate to `https://www.theguardian.com`, which redirects the
CMP into `sp_message_iframe_1482251`):

```
== TOP DOC ==
  top iframes: [{"id":"","src":""},
                {"id":"sp_message_iframe_1482251",
                 "src":"https://sourcepoint.theguardian.com/index.html?..."}]
== SOURCEPOINT CMP FRAME (separate target) ==
  url:          https://sourcepoint.theguardian.com/index.html?...
  button count: 6
  button labels: ["X","Store and/or access information on a device",
                  "Personalised advertising…","Personalised content…",
                  "Accept all","Reject all and subscribe"]
```

The `Accept all` / `Reject all and subscribe` consent buttons live **inside** the
Sourcepoint frame target and are reachable via that target's `consoleActor`. This
is exactly the element ff-rdp's `click` needs to reach.

### Why eval-in-frame over walker+node interaction

The alternative is: from the frame's `inspectorActor` → `getWalker` →
`querySelector` → node → an interaction/`clickNode` on the node front. This is
more actors, more round-trips, and ff-rdp's current click path is **entirely
eval-based** (`build_click_js`, `do_click` in
`crates/ff-rdp-cli/src/commands/click.rs`). Reusing that path against a different
console actor is a near-trivial wiring change; the walker path would be new
infrastructure with no payoff for click. **Recommendation: eval-in-frame.**
Keep the walker path in mind only if a future feature needs node geometry.

Note on CSP: the console eval path is the `Debugger.evalInGlobal` "console
sandbox" (see `actors/console.rs:149-166`), which is not subject to page CSP — so
the click JS runs even on strict-CSP CMP frames, provided the JS itself does not
call page-`eval()`. `build_click_js` already satisfies that.

---

## Question 3 — Same-origin (in-process) iframes

Same-origin iframes are **also** delivered as separate frame targets. Verified with
a `srcdoc` (same-origin) child:

```
== 2 FRAME TARGETS (data: top with same-origin srcdoc child) ==
  [SAME-PROC(in-process)] pid=71266 url=about:srcdoc            console=true
  [TOP]                   pid=71266 url=data:text/html,<h1>top>… console=true
```

The child has the **same `processID`** as the top (in-process) yet is still its own
window-global target with its own `consoleActor`. On theguardian.com the Sourcepoint
frame was likewise in-process (same pid as top) but still a distinct target.

**Design consequence:** enumeration is uniform. Every iframe — same-origin or
cross-origin, in-process or OOP — shows up as a `target-available-form` with a
`consoleActor`, and the eval-in-frame click works identically. The design does
**not** need a separate "same-origin via top walker `contentDocument`" branch;
the single target-based path covers both. (A same-origin frame *is* additionally
reachable through the top walker's `contentDocument`, but there is no reason to
implement that second path.)

---

## Question 4 — Spec-drift check

**No `// allow-spec-drift` annotation required.** Everything used is in the
published spec dicts:

- `getWatcher` request (`devtools/shared/specs/descriptors/tab.js:24-28`):
  ```
  request: {
    isServerTargetSwitchingEnabled: Option(0, "boolean"),
    isPopupDebuggingEnabled: Option(0, "boolean"),
  }
  ```
  So sending `isServerTargetSwitchingEnabled: true` is spec-declared.
- `watchTargets` (`devtools/shared/specs/watcher.js:10-15`): `targetType: Arg(0,"string")`.
- `target-available-form` / `target-destroyed-form`
  (`devtools/shared/specs/watcher.js:96-105`): the target is `Arg(0,"json")` — an
  **opaque JSON blob**. The spec does not enumerate the blob's fields, so reading
  `consoleActor`, `inspectorActor`, `browsingContextID`, `processID`,
  `isTopLevelTarget`, `url` from it is not drift — there is no declared shape to
  drift from. (The server-side producer is `WindowGlobalTargetActor.form()`.)

The only care item is behavioural, not spec: `isServerTargetSwitchingEnabled: true`
changes how/where the **top-level** target is delivered (via the watcher, not
`getTarget`), which existing ff-rdp code does not expect. Handle via an opt-in
frame-aware path rather than flipping the default (see Q1 gap #1).

---

## Design recommendation for iteration-129

**Mechanism for click-in-frame:** enumerate frame targets via
`getWatcher(isServerTargetSwitchingEnabled=true)` → `watchTargets("frame")` +
`watchResources([...])`, then run the **existing** click JS through
`WebConsoleActor::evaluate_js_async` pointed at the matching frame's own
`consoleActor`. No walker/node machinery.

**Selector→frame resolution & error strategy.** `click SELECTOR` should:
1. Try the top-level console first (current behaviour) — fast path, no watcher.
2. On "element not found", enumerate frame targets and, for each non-top frame,
   eval `document.querySelector(SELECTOR)` existence until one matches, then click
   there. Optionally add a `--frame <url-substring>` flag to target a frame
   directly and skip the scan.
3. If a match exists **only** in a frame and no explicit frame was requested,
   still click it but annotate the result meta (`frame_url`, `frame: true`) so the
   caller knows the action crossed a frame boundary. If nothing matches anywhere,
   the error message should say *"selector matched in 0 of N frames (top + N-1
   subframes: <urls>)"* — turning today's misleading "element not found" into a
   frame-aware diagnostic. This directly fixes the Sourcepoint case where
   `Accept all` was invisible to a top-only query.

**New code in ff-rdp-core (small):**
- Add optional `isServerTargetSwitchingEnabled` arg to `TabActor::get_watcher`
  (or a `get_watcher_with_config`). Spec-declared field.
- Extend `TargetEvent` (`src/actors/watcher.rs`) with `console_actor:
  Option<ActorId>`, `inspector_actor: Option<ActorId>`, `browsing_context_id:
  Option<u64>`, `process_id: Option<u64>`, `parent_inner_window_id: Option<u64>`.
  Pure additive parse from the existing JSON blob.
- A `enumerate_frame_targets(transport, watcher, settle_ms) -> Vec<TargetEvent>`
  helper: send watchTargets+watchResources, drain `target-available-form` for a
  short settle window, dedupe by actor, honour `target-destroyed-form`. (This is
  the one genuinely new primitive; everything else is wiring.)

**Already present, reuse as-is:** `WatcherActor::watch_targets` /
`watch_resources`, `WatcherEvent`/`parse_target_event`/`dispatch_watcher_event`,
`Registry::invalidate_target`, the whole `do_click`/`build_click_js` eval path,
`WebConsoleActor::evaluate_js_async`.

**Relationship to the consent story:** the Consent-O-Matic extension
([[consent-dismissal]]) auto-dismisses most CMPs. Frame-aware `click` is the
manual complement for the long tail (and for deliberately choosing *Reject all*
rather than *Accept*, which the extension does not guarantee).

## Feasibility / staffing

Design is fully settled and empirically proven; the remaining work is additive
parsing + one enumeration helper + click wiring, all against APIs verified live.
**A sonnet-class agent can implement iteration-129** from this doc — no further
protocol research needed. The one subtlety to flag in the plan is the top-level
target-delivery behaviour change under `isServerTargetSwitchingEnabled` (use an
opt-in path); everything else is mechanical.

## Probes (session scratchpad, path deps on ff-rdp-core)

- `.../scratchpad/probe/src/bin/dump2.rs` — no getWatcher config ⇒ 0 target events.
- `.../scratchpad/probe/src/bin/dump3.rs` — `isServerTargetSwitchingEnabled:true` ⇒
  top + cross-origin frame forms (raw bodies).
- `.../scratchpad/probe/src/bin/act.rs` — querySelector + click inside OOP example.com frame.
- `.../scratchpad/probe/src/bin/frames.rs` — generic frame enumerator (in-proc vs OOP).
- `.../scratchpad/probe/src/bin/guardian.rs` — Sourcepoint `sp_message_iframe_*` + consent buttons.

Firefox instances left running (do not kill): PID **62522** (port 6400),
PID **71232** (port 6401).
