---
title: launch --headless --window-size exits at once
type: bug
status: resolved
priority: medium
discovered: 2026-10-21
tags:
  - launch
  - viewport
resolved_by: 347
---

# launch --headless --window-size exits at once

Found while taking saturnus's iteration 56 screenshots (ff-rdp 0.4.2,
Firefox 158, macOS). With a window size the launched Firefox exits
before the debug port opens; without it the same command works:

```
ff-rdp launch --headless --window-size 1280x800
→ {"error":"Firefox exited immediately with exit status: 0", ...
   "startup stderr: 38 bytes observed, 38 retained (content redacted)"}
ff-rdp launch --headless
→ works (viewport 1366x683)
```

Later the same session `--window-size 1920x1080` and `600x900` launched
fine (after killing the earlier instance), so it is intermittent or
depends on a leftover instance. The redacted stderr hides why. Wanted: the launch works with a window
size, or the error says what Firefox printed (or a hint how to see it).

## Resolution (iter-296): cannot reproduce

The 2026-10-26 triage ran `launch --headless --window-size 1280x800` 5/5 times without a failure
(ff-rdp 0.4.2, Firefox 158.0.1). The 38 redacted stderr bytes match
`*** You are running in headless mode.` plus a newline, so they did not explain the exit. The
"exited immediately" error now ends with a hint: rerun with `--log-level debug` and
`FF_RDP_TRACE_RAW=1` to see Firefox's stderr.
