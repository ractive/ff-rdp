---
title: click gives the page no user activation
type: bug
status: open
priority: medium
discovered: 2026-10-19
tags:
  - click
  - input
---

# click gives the page no user activation

Found while checking saturnus's "Connect your AI tool" (its iteration 55).
Every `--dispatch` mode of `ff-rdp click` (pointer, legacy, click-only)
dispatches script-made events: they are untrusted (`isTrusted: false`) and
give the page no transient user activation. A page that checks
`navigator.userActivation.isActive` before acting refused the click:

```
ff-rdp click '#ai-connect'
→ the page: "connect() must be called from a user action, such as a click"
```

The same goes for anything the browser gates on user activation:
`window.open`, the clipboard, fullscreen, `requestPermission`, media
playback, `showModal` of some pickers.

Chrome's DevTools protocol has `Input.dispatchMouseEvent`, which goes
through the browser's input pipeline and counts as a user gesture. A way
to get the same in Firefox (a trusted-input mode for `click`, e.g. through
Marionette/WebDriver BiDi `input.performActions`, or a privileged
`windowUtils.sendMouseEvent` from the parent process) would let `click`
drive these pages. Until then the docs of `click` could say that its
events are untrusted.
