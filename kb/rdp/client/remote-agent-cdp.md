---
type: rdp-note
tags:
  - rdp
  - firefox-client
  - remote-agent
  - cdp
  - webdriver-bidi
date: 2026-05-23
firefox_files:
  - remote/README.md
  - remote/components/RemoteAgent.sys.mjs
  - remote/webdriver-bidi/
  - remote/marionette/
title: Remote Agent — historical CDP→RDP bridge
---

# Remote Agent (the `--remote-debugging-port` server)

`/remote/` is **not** the RDP server — it's Firefox's *other* debugging
surface, exposed by `--remote-debugging-port=N` and used historically by
Puppeteer-for-Firefox.

## Current state: CDP is gone

Important: **The Chrome DevTools Protocol (CDP) bridge has been removed from
Firefox.** `remote/README.md` now lists only:

- WebDriver classic (aka Marionette) — `remote/marionette/`
- WebDriver BiDi — `remote/webdriver-bidi/`

`remote/components/RemoteAgent.sys.mjs` only wires up `WebDriverBiDi` (look
for `#webDriverBiDi` and the lazy import of
`chrome://remote/content/webdriver-bidi/WebDriverBiDi.sys.mjs`). There is no
`#cdp` field anymore. Puppeteer-against-Firefox now uses WebDriver BiDi
instead.

The HTTP listener still binds to the same default port (9222 — see
`DEFAULT_PORT` in `RemoteAgent.sys.mjs`), but the protocol on the wire is BiDi
over WebSocket, not CDP.

## Implication for ff-rdp

- The Remote Agent is **a peer to** the DevTools RDP server, not a consumer
  of it. It does not delegate to RDP underneath; it talks straight to Gecko
  via internal APIs (`MarionetteFrameActor`, `WindowGlobalBiDiModule`, etc.).
- Therefore it's not a useful reference for "what an external client needs from
  RDP". Ignore for our purposes — except as motivation for *why* CDP-over-RDP
  was unmaintainable and a thinner protocol won out.
- ff-rdp's niche (lightweight CLI over the DevTools RDP TCP socket) is distinct
  from both Marionette (test automation) and BiDi (WebDriver standard).

## Where CDP-equivalent functionality lives in the current tree

If you're looking for the *RDP* equivalents of common CDP commands (because
e.g. you want to know what to implement in ff-rdp), use the spec files:

| CDP command (gone)             | RDP equivalent (still here)                        | Spec |
|--------------------------------|----------------------------------------------------|------|
| `Target.getTargets`            | `RootFront.listTabs / listProcesses / listWorkers` | `specs/root.js` |
| `Target.attachToTarget`        | `descriptor.getTarget()` then `target.attach()`    | `specs/descriptors/*` |
| `Page.navigate`                | `target.navigateTo({url})`                         | `specs/targets/browsing-context.js` |
| `Page.captureScreenshot`       | `(screenshot|screenshot-content) front.capture()` | `specs/screenshot.js`, `specs/screenshot-content.js` |
| `Runtime.evaluate`             | `WebConsoleFront.evaluateJSAsync({text})`          | `specs/webconsole.js` |
| `DOM.getDocument`              | `WalkerFront.getDocument()` etc.                   | `specs/walker.js`, `specs/node.js` |
| `Network.enable`               | `WatcherFront.watchResources(['networkEvent'])`    | `specs/watcher.js` |
| `Console.enable`               | `WatcherFront.watchResources(['console-message'])` | `specs/watcher.js` |

So the answer to "what do external consumers actually need" is mostly the
*list-targets / attach / resource-watching / evaluate / screenshot* axis —
which is exactly what ff-rdp implements.

## WebDriver BiDi as a worked example

If you want to see "a non-DevTools consumer driving Gecko at the protocol
level", `remote/webdriver-bidi/modules/` is the clearest reference. Each
file is one BiDi module (`browsingContext`, `script`, `network`,
`storage`, ...). They illustrate how a thin protocol over Gecko's internal
APIs can satisfy real automation workflows without DevTools' actor machinery.
ff-rdp is similar in spirit but goes through DevTools' RDP rather than via
internal Gecko APIs.
