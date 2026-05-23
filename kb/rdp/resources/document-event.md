---
type: rdp-note
tags: [rdp, firefox-server, resource, navigation]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/document-event.js
  - devtools/server/actors/resources/parent-process-document-event.js
  - devtools/server/actors/webconsole/listeners/document-events.js
---

# Resource: `document-event`

Frame-target resource. Backed by a `DocumentEventsListener`. Emits one resource per DOM/navigation milestone.

## Trigger events (the `name` field)

- `dom-loading` — sent very early. Includes `url` (current document being loaded).
- `dom-interactive` — DOM tree parsed. Includes `title` and `url`.
- `dom-complete` — load event fired. Includes `hasNativeConsoleAPI` (false if a content script overrides `console`).
- `will-navigate` — about to navigate (emitted from **parent-process-document-event.js**, since the content process won't see itself disappearing). Includes `newURI`.

## Payload shape

```
{
  name: "dom-loading" | "dom-interactive" | "dom-complete" | "will-navigate",
  time: number,                  // milliseconds
  isFrameSwitching?: boolean,    // true when devtools' frame picker switched docs
  title?: string,                // only on dom-interactive
  url?: string,                  // only on dom-loading / dom-interactive
  newURI?: string,               // only on will-navigate
  hasNativeConsoleAPI?: boolean, // only on dom-complete
}
```

## Split between frame-target and parent-process watchers

`document-event.js` skips `will-navigate` unless it's a frame-switch — `will-navigate` from real navigations comes from `parent-process-document-event.js`. This is because by the time `will-navigate` would fire in the content process, the WindowGlobal is already being torn down.

This split means **`will-navigate` may arrive AFTER the new target is announced** in some edge cases (the comment in `parent-process-document-event.js` explicitly calls this out as known).

## Gotchas for ff-rdp

- For "wait for page load" semantics, listen for `dom-complete` (vs `dom-interactive` if you only need the DOM tree).
- `will-navigate` is NOT reliable as a pre-navigation hook — it can be reordered with target swap.
- Bug 1975277: iframes that are destroying may not have a valid window — the watcher early-returns; you may miss events on rapidly-disappearing frames.
- Test-only path: setting `devtools.testing.force-server-error` pref makes the watcher throw — used to test toolbox error UI.
