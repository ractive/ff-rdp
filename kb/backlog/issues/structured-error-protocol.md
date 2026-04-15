---
title: Improve RDP error protocol handling
type: feature
status: resolved
priority: low
discovered: 2026-04-07
tags:
  - protocol
  - errors
  - robustness
---

# Improve RDP error protocol handling

The RDP protocol defines structured error responses (`{ "from": actor, "error": name,
"message": msg }`) with specific error types. Currently we handle `ActorError` generically
but don't distinguish:

- `threadWouldRun` (with `cause` field) — operation requires thread pause
- `wrongState` — actor not in expected state
- `unknownActor` — actor ID no longer valid
- `noSuchActor` — actor never existed

Better handling would improve error messages and enable automatic recovery
(e.g. re-resolve actor on `unknownActor`).
