---
type: rdp-note
tags: [rdp, firefox-server, resource, console]
date: 2026-05-23
firefox_files:
  - devtools/server/actors/resources/console-messages.js
  - devtools/server/actors/webconsole/listeners/console-api.js
---

# Resource: `console-message`

Frame-target resource. One entry per `console.log/info/warn/error/debug/trace/dir/table/group/groupEnd/time/timeEnd/count/assert/clear/profile/dirxml/exception` call from page JS.

## Payload

```
{
  resourceType: "console-message",
  message: {
    arguments: [grip, grip, ...],
    columnNumber, lineNumber, filename, sourceId,
    counter, timer, timeStamp,
    level: "log" | "info" | "warn" | "error" | "debug" | "trace" | "dir" | "table" | "group" | "groupEnd" | "groupCollapsed" | "time" | "timeEnd" | "timeLog" | "count" | "countReset" | "assert" | "clear" | "profile" | "profileEnd" | "dirxml" | "exception",
    private: boolean,
    innerWindowID, prefix, styles, stacktrace,
  }
}
```

## Listener

Wraps `ConsoleAPIListener` from `webconsole/listeners/console-api.js`, which subscribes to the `console-api-log-event` observer notification.

## Gotchas for ff-rdp

- **`clonedFromContentProcess`** flag exists on the [[rdp/actors/console]]'s live `consoleAPICall` event — when relayed across processes (Browser Toolbox), arguments are pre-serialized as grips on the source side and shipped clones, which means object lookups via Inspector might not resolve.
- `arguments` are *grips* — string/number/boolean inline; objects are ObjectActor references you may have to follow.
- Cached messages from before the listener started are obtainable via `console.getCachedMessages(["ConsoleAPI"])`.
- The legacy non-watcher path emits these as `consoleAPICall` events on [[rdp/actors/console]] directly, rather than as resources.
