---
title: "navigate: a file: URL crashes the headless Firefox"
type: bug
status: resolved
priority: high
discovered: 2026-10-26
reporter: ff-rdp
ff_rdp_version: ff-rdp 0.4.2 (86baf13dfb53 2026-10-26)
tags: [navigate]
resolved_by: 347
---

Found during the 2026-10-26 triage (Firefox 158.0.1, macOS, headless). This crashed 3 out of 3
times, with and without `--window-size`, and even for `file:///etc/hosts`.

## Repro

```sh
ff-rdp launch --headless --debug-port 7401
ff-rdp navigate https://example.com --port 7401
ff-rdp navigate --allow-file-urls file:///etc/hosts --port 7401
→ {"error":"navigate: transport error waiting for dom-complete: recv failed: failed to fill whole buffer"}
```

After that the Firefox process is gone. The crash report
(`~/Library/Application Support/Firefox/Crash Reports/pending/*.extra`) shows
`MozCrashReason: MOZ_CRASH(IPC FatalError in the parent process!)` and
`IPCFatalErrorMsg: Illegal load attempt of file: URL from webIsolated`. The tab's web content
process (`webIsolated`) is asked to load a `file:` URL. The parent treats that as a compromised
content process and aborts. With the crash reporter enabled, a crash-reporter dialog opens on
the desktop even though Firefox is headless.

## Expected

`--allow-file-urls` loads the file. Or, if that can't be done safely from the tab's current
process, ff-rdp refuses it with a clear error before Firefox crashes.

## Proposed fix

Find out how DevTools navigates a tab across a process switch (`navigateTo` with a `file:` URL
should trigger a process switch to the file content process). The likely cause is that ff-rdp
loads the URL through a content-process path (for example `location.href` via eval) instead of
the parent's `browsingContext.loadURI`. Check `searchfox` for `navigateTo` in
`devtools/server/actors/targets/window-global.js`.
