---
name: project-stateless-cli
description: The connection daemon was deleted 2026-10-02 (DEC-056); every command opens its own RDP connection — what replaced each daemon feature and what can no longer work
metadata:
  type: project
---

The daemon (`_daemon`, `daemon status/stop`, `--no-daemon`, registry, launch records,
`meta.route`) was removed on branch `reset/remove-daemon` (DEC-056). Every command
opens its own RDP connection and disconnects; nothing outlives a command.

**Why:** a separate direct connection's watcher sees traffic other connections cause
(measured 2026-10-02), so cross-command capture is a caller-owned `network --follow`
stream; the daemon was the largest single defect source.

**How to apply:**
- Anything connection-scoped dies with the command: object grips (so `inspect` takes a
  JS expression and walks it on one connection), network-parent throttling/blocking (so
  they are `--throttle`/`--block` flags on `navigate`/`reload`), target-configuration
  emulation (Firefox resets it on disconnect — `emulate` was deleted). Do not propose a
  standalone command whose effect must outlive its own connection.
- Refs are stamped in the page (`data-ffrdp-ref`, counter `window.__ffrdp_refs`) and
  resolve as `[data-ffrdp-ref="eN"]`; JS helper `STAMP_REF_JS_FN` in
  `commands/js_helpers.rs`. They die on navigation and are top-document only.
- `launch` ownership (no-op and `--replace`) rests only on the owner-PID marker of a
  managed profile (`commands/launch/replace.rs`).
- Frame actor IDs (`eval --frame/--node`) are also connection-scoped; flags that take an
  actor ID from an earlier command are dead weight — see [[project-ff-rdp-registry]].
