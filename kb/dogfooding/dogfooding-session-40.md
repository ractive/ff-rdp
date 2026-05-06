---
title: "Dogfooding Session 40"
type: dogfooding
date: 2026-05-06
status: completed
site: "http://localhost:3120/de/suche (nova-contentpages, real migration task)"
commands_tested: [launch, tabs, navigate, type, eval, page-text, snapshot, screenshot]
tags: [dogfooding, ai-ergonomics, onboarding, port-collision, react-input, error-messages]
---

# Dogfooding Session 40

First contact during a real task: I was migrating a search page into a Next.js app (`nova-contentpages`) and asked to verify it interactively. The user pointed me at `ff-rdp` cold — I had never used it before. Goal: navigate to `http://localhost:3120/de/suche` and confirm the InstantSearch widget actually works post-hydration. It took **way too long** to even reach a page render. This file documents the full friction.

## TL;DR

- Spent ~10 minutes flailing before the first successful navigate.
- Root cause: a stale Firefox process (running for 13 days, port-bound) that ff-rdp couldn't see tabs in, combined with error messages that suggested I "launch" — even though I had already launched. The CLI never told me port 6000 was already in use.
- After the breakthrough (using `--port 6010`), the rest worked, with three more papercuts: `type` flag-vs-positional confusion, `eval` global scope leaks, and a broken `screenshot` actor.

## Timeline of failure

| Step | What I tried | What happened |
|---|---|---|
| 1 | `ff-rdp tabs` | `[]` — no tabs. I assumed Firefox wasn't running yet. |
| 2 | `ff-rdp launch --temp-profile` | Returned PID 94495, port 6000. |
| 3 | `ff-rdp tabs` (after 3s sleep) | Still `[]`. Confused. |
| 4 | `ff-rdp launch --temp-profile --headless` | Returned PID 95895, also port 6000. (Two Firefoxes, both nominally on 6000?) |
| 5 | `ff-rdp tabs` | Still `[]`. |
| 6 | `ff-rdp --no-daemon tabs` | Still `[]`. |
| 7 | Read `~/.ff-rdp/daemon.log` → `internal error: no tabs available`. | OK so it's not that the daemon is broken; there really are no tabs. |
| 8 | `ff-rdp navigate ...` | `error: no tabs available — is a page open in Firefox? Use \`ff-rdp launch --headless --temp-profile\` to start one` |
| 9 | I had **just launched twice**. The error told me to do exactly what I'd done. Dead end. | |
| 10 | `ps aux \| grep firefox` → discovered **PID 74513** running since April 23 (13 days), bound to port 6000. | This is the user's daily-driver Firefox-with-debug. My new launches couldn't bind. |
| 11 | `pkill -f ff-rdp-profile` (kills my abandoned Firefoxes only, leaves 74513). `ff-rdp launch --temp-profile --port 6010`. | Worked. |
| 12 | `ff-rdp --port 6010 tabs` | Got a tab! `about:blank`. |
| 13 | `ff-rdp --port 6010 navigate http://...` with `--wait-text` | `error: actor error from server1.conn2.child5/consoleActor3: noSuchActor (unknownActor) — No such actor for ID: ... — the tab may have been closed or navigated away; try again` |
| 14 | Same command without `--wait-text` | Worked. Page loaded. |

## Why it took so long to get "somewhere"

**The single biggest issue: silent port collision with no diagnostic.**

`ff-rdp launch --temp-profile` printed `port: 6000` and returned a fresh PID. Looked successful. The Firefox process actually started — but on macOS, Firefox just becomes a no-op ghost when `--start-debugger-server <port>` collides with an existing listener: it boots, but nothing is listening on 6000 from this process. There was zero indication of this from the `launch` output.

Meanwhile, `ff-rdp tabs` happily talked to the *other* Firefox (PID 74513) that had been listening on 6000 for 13 days but had no debuggable tabs (probably because all its tabs were unloaded by session-restore lazy-load). So I got `[]` — which is indistinguishable from "your launch failed silently."

I had no way to tell from the CLI:
- That something else was on port 6000.
- That the launch had effectively no-op'd.
- That the existing port-6000 Firefox was a different process than my launch's PID.

I only solved this by using `lsof -i :6000` outside ff-rdp.

**Hint #1: `launch` should verify the port.** After spawning Firefox, poll `localhost:<port>` for ~5s. If something was already listening *before* the spawn, fail loudly:

```
ff-rdp launch --temp-profile
error: port 6000 is already in use by an existing process (firefox, PID 74513).
hint: pass --port <N> to use a different port, or stop the existing listener.
```

**Hint #2: `tabs` and `navigate` should report the connected target.** When I get `[]` from `tabs`, knowing *which Firefox I'm talking to* would have caught this immediately. Show the connected PID/profile in `meta`:

```json
{ "meta": { "host": "localhost", "port": 6000, "connected_pid": 74513, "uptime_s": 1123456 }, ... }
```

A 13-day uptime would have been a huge red flag.

## Was the help clear enough?

Mostly yes — `ff-rdp --help` is excellent (concise command list, clear options). `ff-rdp launch --help` and `ff-rdp navigate --help` are also good. **But there was no troubleshooting section** for the most common new-user failure mode: *"I launched, but tabs returns empty."*

The `--help` is structured around "here are the commands and flags," not around "here's what to do when X goes wrong." For an AI agent or new user, a `ff-rdp doctor` subcommand that probes the connection and reports what's wrong (port held by another process, connected to a session with no tabs, daemon stale, etc.) would have saved every minute of this.

## Why didn't the "hints" help?

Two error messages had hints. Both were wrong-direction in this scenario.

**Hint A: `error: no tabs available — is a page open in Firefox? Use \`ff-rdp launch --headless --temp-profile\` to start one`**

This is the canonical "first time using ff-rdp" error and the hint is actively misleading when you've *already* launched. It assumes the user hasn't run `launch` yet. It should detect "the user just launched, the port is occupied by a different process" and say so.

Improvement: distinguish between "no Firefox connected" (suggest launch) and "Firefox connected but no tabs" (suggest `--tab 1`, or report PID/uptime so the user knows they're talking to a stale process).

**Hint B: `error: actor error from server1.conn2.child5/consoleActor3: noSuchActor (unknownActor) — ... — the tab may have been closed or navigated away; try again`**

This fired on my **first** `navigate ... --wait-text` after a fresh launch. The tab definitely wasn't navigated away — the navigate hadn't started yet. The hint "try again" worked (running navigate without `--wait-text` succeeded). But the underlying issue is that `--wait-text` resolves a console actor *before* navigation, then the actor becomes invalid when navigation tears down the docshell.

Improvement: `--wait-text` should re-resolve the console actor after the navigation commits, not before.

## Bugs / error messages I hit (verbatim)

1. **Port collision is silent.** `launch --temp-profile` returned a healthy-looking JSON result, but the new Firefox was a ghost. No error.

2. **`tabs` returns `[]` for a stale Firefox-on-port-6000 with unloaded tabs.** Indistinguishable from "no Firefox running."

3. **Daemon registry race:**
   ```
   warning: daemon started but registry not found: timed out after 5s waiting for daemon to write registry, connecting directly (check /Users/james/.ff-rdp/daemon.log for details)
   ```
   This appeared on a `navigate` call that otherwise worked. Just visual noise — I had to read the log to confirm it was harmless. Could be downgraded or suppressed when the direct connection succeeds.

4. **First-navigate `noSuchActor`:**
   ```
   error: actor error from server1.conn2.child5/consoleActor3: noSuchActor (unknownActor) — No such actor for ID: server1.conn2.child5/consoleActor3 — the tab may have been closed or navigated away; try again
   ```
   Reproducible: fresh launch, then `navigate URL --wait-text "..."`. Without `--wait-text` the navigate succeeds.

5. **`type` flag/positional confusion:**
   ```
   $ ff-rdp --port 6010 --tab 1 type --selector 'input[type="search"]' --text "Krankenkasse" --clear
   error: unexpected argument '--selector' found
     tip: to pass '--selector' as a value, use '-- --selector'
     Usage: ff-rdp type [OPTIONS] <SELECTOR> <TEXT>
   ```
   The "tip" is unhelpful here — it tells me how to *pass `--selector` as a value*, not how to actually use the command. Other ff-rdp commands take `--selector` as a flag (e.g. `dom`, `wait`), so reaching for it was natural. The tip should instead read: *"hint: `type` takes selector and text positionally — try `ff-rdp type 'input[type=search]' 'Krankenkasse'`."*

   Even better: accept `--selector` as a synonym across all commands, since that's what every other command uses.

6. **`eval` shares global scope across invocations:**
   ```
   $ ff-rdp eval 'const input = document.querySelector("input[type=search]"); input.value;'
   $ ff-rdp eval 'const input = document.querySelector("input[type=search]"); JSON.stringify({value: input.value});'
   error: redeclaration of const input
   { "class": "SyntaxError", "message": "redeclaration of const input", ... }
   ```
   I had to wrap each eval in an IIFE to avoid this. Surprising default — most REPL/eval tools either run each call in fresh scope or use `let` shadowing. Easy fix: wrap user code in `(function(){ ... })()` automatically, or document this loudly in `eval --help`.

7. **Setting `<input>` value programmatically doesn't trigger React onChange.** Not technically an ff-rdp bug, but the `type` command sets `input.value` directly without firing React-aware events. For a CLI advertised as ergonomic for testing modern web apps, this matters: **most React/Vue apps will look unresponsive after `ff-rdp type`.** I had to switch to:
   ```js
   const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
   setter.call(input, "Krankenkasse");
   input.dispatchEvent(new Event("input", { bubbles: true }));
   ```
   Suggestion: `type` should always dispatch `input` and `change` events with `bubbles: true` after value mutation, and use the prototype setter so React's value tracker is invalidated. This is a one-time fix that would make `type` work on every modern framework.

8. **`screenshot` actor broken:**
   ```
   error: screenshot: screenshotActor.capture failed (actor error from server1.conn5.screenshotActor9: unknownError (unknownError) — Error occurred while creating actor' server1.conn5.screenshotActor9: Error: Unable to load actor module 'devtools/server/actors/screenshot'
   ChromeUtils.importESModule: global option is required in DevTools distinct global
   @resource://devtools/server/actors/utils/capture-screenshot.js:8:49
   ```
   Firefox version mismatch. ff-rdp should detect this on `screenshot` invocation and fall back to `Page.captureScreenshot` over CDP, or at minimum print a one-line "your Firefox version (X) doesn't expose the screenshot actor; minimum version Y."

## Improvement ideas (priority-ordered)

1. **Detect port collisions in `launch`.** This was the entire 10-minute hole.
2. **Include connected-Firefox metadata in every response** (PID, profile path, uptime, version). Catches stale-process scenarios immediately.
3. **`ff-rdp doctor` subcommand** that probes: port owner, daemon status, connected tab count, Firefox version compatibility for each actor (screenshot, etc.). One command to run when stuck.
4. **`type` should dispatch React-compatible events.** Without this, `type` is unusable on the modern web.
5. **Wrap `eval` user code in IIFE by default**, or expose `--isolate` flag.
6. **Improve the canonical "no tabs" error.** Branch on root cause: never launched vs launched-but-port-collision vs tabs-loaded-but-not-visible.
7. **`type` should accept `--selector` as a synonym for the positional**, matching other commands.
8. **`navigate --wait-text` should re-resolve actors post-navigation** instead of failing on `noSuchActor`.
9. **Suppress the daemon "registry not found" warning** when the direct fallback works.
10. **`screenshot` graceful fallback or version warning.**

## What worked well (credit where it's due)

Once past the connection hurdle, the experience was great:

- `snapshot` is genuinely the right shape for an LLM — semantic roles, attributes, interactive flags, truncation hints. Used it once and got my bearings instantly.
- `page-text` is perfect for "did the page render the text I expected" verification. Used it to confirm localized titles/placeholders/headings on `/de/suche` and `/fr/suche`.
- `eval` returning the result as JSON-stringified scalar is exactly right; I composed `.querySelectorAll` checks with `JSON.stringify({ ... })` and got compact, readable output.
- `--port` flag let me sidestep the port-6000 daily-driver instance without disrupting the user's other work — clean separation.
- `--tab 1` (1-based index) is the right ergonomic for CLI.
- `--format text` for tabular outputs was helpful.
- The eventual end-to-end verification was tight: navigate → page-text → eval-with-DOM-query → conclude. ~15 seconds per locale once the path was clear.

The CLI **is** good. The ramp-up to "first successful interaction" is what hurt — and that ramp-up is exactly when AI agents and new humans most need it to be smooth.
