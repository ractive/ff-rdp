---
title: "Backlog: wait --console-includes"
type: feature
status: backlog
date: 2026-04-16
tags: [backlog, wait, console, dx]
---

# wait --console-includes

Add a `--console-includes <PATTERN>` flag to the `wait` command that blocks until
a matching string (or regex) appears in the console output.

Surfaced during [[dogfooding/dogfooding-session-nova-template-jsonforms-index]]:
after navigating to a page, the session needed to confirm that client-side
hydration had completed before running subsequent eval commands. The pattern
`console.log("hydration complete")` was used as the signal; polling
`wait --console-includes "hydration complete"` would replace a fragile
`sleep 3` with an intent-expressing guard.

## Implementation sketch

- After subscribing to the WatcherActor `console-message` resource, loop
  receiving events and match each message against the pattern.
- Respect `--timeout` for the total wait budget.
- Return `{matched: true, message: "...", elapsed_ms: N}` on success;
  error with timeout details on failure.

Out of scope for [[iterations/iteration-43-dx-fixes]].
