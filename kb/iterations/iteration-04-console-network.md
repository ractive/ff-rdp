---
title: "Iteration 4: Console + Network Monitoring"
type: iteration
date: 2026-04-06
tags: [iteration, console, network, monitoring]
status: planned
branch: iter-4/console-network
---

# Iteration 4: Console + Network Monitoring

Read console messages and network requests — the two most important debugging data sources after eval.

## Tasks

- [ ] Implement `ff-rdp-core/src/actors/watcher.rs` — `WatcherActor` with `watch_resources(types)`, `unwatch_resources(types)`
- [ ] Extend `ff-rdp-core/src/actors/console.rs` — add `start_listeners(["PageError", "ConsoleAPI"])`, `get_cached_messages(types)` to existing `WebConsoleActor`
- [ ] Implement console message parsing: level, message text, source file, line number, timestamp
- [ ] Implement `ff-rdp-core/src/actors/network.rs` — network event parsing from `resource-available-form` events
- [ ] Implement `NetworkEventActor` methods: `get_request_headers()`, `get_response_headers()`, `get_response_content()`, `get_event_timings()`
- [ ] Implement `ff-rdp-cli/src/commands/console.rs` — `ff-rdp console [--tab ...] [--pattern <regex>] [--level <error|warn|info|log>]`
- [ ] Implement `ff-rdp-cli/src/commands/network.rs` — `ff-rdp network [--tab ...] [--filter <url-pattern>] [--method <GET|POST|...>]`
- [ ] Console output format: array of `{level, message, source, line, timestamp}`
- [ ] Network output format: array of `{method, url, status, duration_ms, size_bytes, content_type, is_xhr}`
- [ ] Handle the resource watcher event flow: watchResources → collect resource-available-form events → parse

## Acceptance Criteria

- `ff-rdp console` shows cached console messages from the active tab
- `ff-rdp console --level error` filters to errors only
- `ff-rdp console --pattern "API"` filters by message content
- `ff-rdp network` shows recent network requests with status codes and timing
- `ff-rdp network --filter "api/"` filters by URL pattern
- `ff-rdp network --jq '.results[] | select(.status >= 400)'` finds failed requests
- Both commands work with tab targeting
