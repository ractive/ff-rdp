---
title: Firefox Remote Debugging Protocol (RDP) — Wiki Index
type: index
tags: [rdp, index]
date: 2026-05-23
---

# Firefox Remote Debugging Protocol (RDP) — Wiki

Working knowledgebase about the protocol `ff-rdp` speaks. Sourced from:

- Official Firefox docs (firefox-source-docs, MDN).
- Firefox source under `/Users/james/devel/firefox/devtools/` (server actors, client fronts, shared specs).
- The `ff-rdp` codebase itself + our dogfooding sessions 48–53 and iterations 61g–61l.

Use this as a lookup table when implementing or debugging RDP-facing code. Every page cites its source.

## Quick lookup by question

- **"How does the protocol move bytes?"** → [[rdp/protocol/transport|transport]] + [[message-format]]
- **"What's an actor / a grip / a resource?"** → [[glossary]] · [[actor-model]] · [[resources]]
- **"How do I connect a new client?"** → [[connect-and-list-tabs]] · [[attach-target]] · [[connection-lifecycle]]
- **"How do I read network traffic with headers?"** → [[watch-resources]] · [[rdp/actors/network-event|network-event]] · [[network-content]]
- **"How do I evaluate JS that may be blocked by CSP?"** → [[evaluate-js]] · [[console]] (look for the `mapped: { await: true }` note)
- **"How do I take a full-page screenshot for real?"** → [[take-screenshot]] · [[screenshot]] · [[screenshot-content]]
- **"What does Firefox do when DevTools wants this?"** → search [[devtools-client]] then drill into the relevant [[rdp/actors/README|actor]] / [[rdp/flows/README|flow]]
- **"What do we already know / what's broken in `ff-rdp`?"** → [[actors-we-use]] · [[lessons-learned]] · [[open-gaps]]
- **"What should the next ff-rdp iteration tackle?"** → [[ff-rdp-wins]]

## Folders

- **[[rdp/overview/README|overview/]]** — what RDP is, the actor model, the connection lifecycle.
- **[[rdp/protocol/README|protocol/]]** — wire-level details: transport framing, message format, error shape, resource streams.
- **[[rdp/actors/README|actors/]]** — every Firefox server actor we care about, grouped by category. The protocol contract per actor.
- **[[rdp/resources/README|resources/]]** — every resource type the WatcherActor can stream.
- **[[rdp/client/README|client/]]** — Firefox's own client-side RDP implementation (DevToolsClient, Front framework, transport).
- **[[rdp/flows/README|flows/]]** — end-to-end walkthroughs of common consumer flows (connect, attach, eval, watch, screenshot).
- **[[rdp/from-our-codebase/README|from-our-codebase/]]** — what `ff-rdp` already uses, what we've learned the hard way, what's still broken.

## Top three actionable findings (TL;DR for ff-rdp)

These came out of building this wiki and are big enough that they deserve their own iteration. See [[ff-rdp-wins]] for the full list.

1. **`screenshot --full-page` (broken 5+ sessions running).** The DevTools flow uses *two* RDP calls in sequence: `screenshot-content.prepareCapture({fullpage:true})` on the **content-process-scoped** actor returns the rect; then `screenshot.capture({fullpage, rect, snapshotScale, browsingContextID, …})` on the **root-scoped** `screenshot` actor (got via `client.mainRoot.getFront("screenshot")`, NOT the target) invokes `drawSnapshot(rect, ratio, "rgb(255,255,255)", fullpage=true)`. The 4th positional arg `fullpage` is what actually makes Gecko render outside the viewport — a custom rect alone is not enough. See [[take-screenshot]] and [[screenshot]].

2. **`eval` blocked by CSP.** `evaluateJSAsync` with `mapped: { await: true }` is awaited on the server using SpiderMonkey's Debugger API, which is privileged and ignores page CSP. The Firefox DevTools console uses this whenever the input has top-level `await`. Our memory `project_rdp_async_constraints` — "evaluateJSAsync won't resolve Promises" — was only half-right; it just needs the flag. See [[evaluate-js]] and [[console]].

3. **WatcherActor engagement.** `watchTargets("frame")` AND `watchResources([...])` are *both* required before any events arrive — subscribing to resources without `watchTargets` returns nothing. Events are throttled at `RESOURCES_THROTTLING_DELAY = 100ms` and batched as `[[type, [resources…]], …]` — the outer array is what handlers iterate. See [[watcher]] and [[watch-resources]].

## Conventions

- All files have YAML frontmatter (`type: rdp-note`, tags including `rdp`, a date).
- Cross-references use `[[wikilinks]]` to the slug (the file's `name`/basename).
- File paths into the Firefox checkout are quoted relative (e.g. `devtools/server/actors/watcher.js:42`) so they're greppable across machines.
- Pages cap at ~150 lines; bigger topics get split.

## Out of scope (intentionally)

- WebDriver BiDi protocol — separate spec, separate code path in `/firefox/remote/`. RDP is *not* CDP and is *not* BiDi.
- The (now-removed) CDP-over-RDP bridge — see [[remote-agent-cdp]] for the historical note.
- Marionette — also separate, used by gecko-driver and mozregression.

## References

- [[ff-rdp-wins]] — distilled bug-lookups and improvement opportunities for our own codebase
