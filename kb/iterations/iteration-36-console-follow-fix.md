---
title: "Iteration 36: Fix Console --follow (No Output)"
type: iteration
status: complete
date: 2026-04-08
tags: [iteration, bugfix, console, firefox-149, protocol, research]
branch: iter-36/console-follow-fix
---

# Iteration 36: Fix Console --follow (No Output)

`console --follow` produces zero output even when console.log/warn/error
messages are generated via `eval` in a parallel connection.

## Symptom

```bash
# Terminal 1: start follow
ff-rdp console --follow > /tmp/console.txt &

# Terminal 2: generate messages
ff-rdp eval 'console.log("test1"); console.warn("test2"); console.error("test3")'

# Terminal 1: check output
cat /tmp/console.txt
# → empty file
```

The non-follow `console` command (getCachedMessages) works fine — messages are
there. Only the streaming/follow subscription fails to deliver events.

## Current code flow

**File:** `crates/ff-rdp-cli/src/commands/console.rs`

### Daemon mode (`run_follow_daemon`):
1. Get console actor from target info
2. Call `WebConsoleActor::start_listeners(["PageError", "ConsoleAPI"])` → succeeds
3. Call `start_daemon_stream("console-message")` → tells daemon to forward events
4. Call `start_daemon_stream("error-message")` → same for errors
5. Enter `follow_loop()` → reads from transport, looking for:
   - `resources-available-array` with `console-message` or `error-message`
   - Direct `consoleAPICall` or `pageError` pushes from console actor
6. Nothing ever arrives → timeout → loop continues forever

### Direct mode (`run_follow_direct`):
1. Get watcher actor
2. Call `WatcherActor::watch_resources(["console-message", "error-message"])`
3. Call `WebConsoleActor::start_listeners(["PageError", "ConsoleAPI"])`
4. Enter `follow_loop()` → same as above
5. Same problem: nothing arrives

## Hypotheses

1. **Firefox 149 changed event names.** The resource types might not be
   `"console-message"` and `"error-message"` anymore. If the daemon subscribes
   to the wrong types, no events get buffered/forwarded.

2. **`startListeners` no longer sends push notifications.** Firefox 149 may
   have removed the direct `consoleAPICall`/`pageError` push behavior.

3. **Events arrive but are filtered out.** The `follow_loop` might be looking
   for the wrong message format or the wrong `from` actor.

4. **Daemon watcher intercepts console events.** The daemon's own watcher
   subscription might be consuming the events before the client sees them
   (similar to the cookies issue).

5. **Timing issue.** The subscription might not be established before the
   messages are generated. But this seems unlikely since the follow is started
   first and waits.

## Research Tasks

**Completed 2026-04-08 via live protocol research against Firefox 149.**

- [x] **Raw protocol exploration.** Full trace recorded — see
  [[research/console-follow-protocol-ff149]].

- [x] **Test with different resource type names.** `"console-message"` is the
  correct type; the WatcherActor confirms it via `traits.resources["console-message"]=true`.
  The WatcherActor subscription itself does not deliver events for eval-triggered messages
  in Firefox 149 — only the direct `consoleAPICall` push path works.

- [x] **Check if the daemon's watcher swallows console events.** The daemon
  subscribes to `["network-event", "console-message", "error-message"]` but
  correctly distinguishes watcher events (via `is_watcher_event`) from direct
  push events (via `is_console_push_event`). Console pushes are forwarded
  separately to stream subscribers via `dispatch_console_push_event`.

- [x] **Compare with `network --follow`.** Network works via the watcher stream
  (`resources-available-array` with `network-event`). Console works via the direct
  `consoleAPICall` push path because the watcher's `console-message` channel does
  not deliver events for eval-triggered messages in Firefox 149.

- [x] **Document findings** in [[research/console-follow-protocol-ff149]].

## Findings

**The bug described does not reproduce on the current codebase.** Both direct and
daemon mode correctly receive console messages.

The fix was implemented during iterations 27 and earlier iterations:
- `run_follow_direct` calls `start_listeners` (required to activate Firefox's push delivery)
- `follow_loop` handles the direct `consoleAPICall`/`pageError` push events via
  `parse_console_notification`
- The daemon's `dispatch_console_push_event` forwards push events to stream subscribers

The WatcherActor `console-message` subscription is redundant but harmless — it never
delivers events for eval-triggered messages in Firefox 149. The only functional path is
the direct console actor push.

## Verification

Tested manually against Firefox 149:

```bash
# Direct mode: messages from separate connection
cargo run --quiet -- --port 6000 console --follow &
cargo run --quiet -- --port 6000 eval 'console.log("test1"); console.warn("test2")'
# Output: {"level":"log",...,"message":"test1",...}
#         {"level":"warn",...,"message":"test2",...}
```

Both direct and daemon modes confirmed working.
