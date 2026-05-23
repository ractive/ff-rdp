---
type: rdp-note
tags: [rdp, firefox-server, resource, console, errors]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/error-messages.js
  - devtools/server/actors/resources/utils/nsIConsoleListenerWatcher.js
---

# Resource: `error-message`

Frame-target resource. Errors and warnings from the nsIConsoleService — JS exceptions, CSP violations, mixed-content warnings, deprecation notices, etc.

Subclass of `nsIConsoleListenerWatcher` (shared with [[css-message]], [[platform-message]]).

## Payload

```
{
  resourceType: "error-message",
  pageError: {
    errorMessage: longstring,
    errorMessageName, exception, exceptionDocURL,
    sourceName, sourceId, lineText, lineNumber, columnNumber,
    category, innerWindowID, timeStamp,
    warning: boolean, info: boolean, error: boolean,
    private: boolean, stacktrace: array,
    cssSelectors, notes,
    isPromiseRejection,
  }
}
```

## Gotchas

- Filtered to the target's window (by innerWindowID). Cross-window errors are routed to the right target.
- **CSP violations come through here**, not as a separate type — look for `category === "CSP"`.
- Promise rejections (unhandled) have `isPromiseRejection: true`.
- Pre-existing errors in the console buffer: get via `console.getCachedMessages(["PageError"])`.
