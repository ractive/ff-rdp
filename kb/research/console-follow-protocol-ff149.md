---
title: "Console Follow Protocol Research (Firefox 149)"
type: research
status: completed
date: 2026-04-08
tags: [firefox, rdp, console, protocol]
---

# Console Follow Protocol Research (Firefox 149)

## Console Event Delivery Channels

Firefox 149 has two distinct channels for delivering console events:

### 1. Direct Push (`consoleAPICall` / `pageError`)

Arrives on ANY connection that has called `startListeners(["PageError", "ConsoleAPI"])`. This is the **primary channel** for eval-triggered console output (i.e., output produced by `evaluateJSAsync`).

### 2. WatcherActor Stream (`resources-available-array` with `console-message`)

Firefox reports supporting this resource type, but in practice **zero events arrive** via this channel when `console.log()` is called via `evaluateJSAsync`. Events may arrive for page-script console output only.

## Cross-Connection Delivery

Firefox **broadcasts** `consoleAPICall` events to ALL connections with active `startListeners`, not just the connection that triggered the eval. This is important for daemon architectures where multiple clients may be listening.

## Resource Types Confirmed Working

`"console-message"` and `"error-message"` are valid watcher resource types, confirmed by `watchResources` response containing `"console-message": true`.

## startListeners is Required

Without calling `startListeners`, no console events arrive at all -- neither via direct push nor via watcher stream. This is a hard prerequisite for any console monitoring.

## Bug Reproduction Attempt

The reported bug (console `--follow` producing zero output) **could NOT be reproduced**. Both direct mode and daemon mode work correctly. The code already handles both delivery channels:

- `parse_console_notification` handles direct push events
- `parse_console_resources` handles watcher stream events

## Daemon Architecture

The daemon handles console events correctly:

- Daemon calls `startListeners` at startup
- `is_console_push_event()` identifies direct push events
- `dispatch_console_push_event()` forwards to registered stream subscribers
- Console push events are **not buffered** (unlike watcher events) -- they are only forwarded to active subscribers
