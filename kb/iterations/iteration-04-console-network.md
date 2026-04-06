---
title: "Iteration 4: Console + Network Monitoring"
type: iteration
date: 2026-04-06
tags: [iteration, console, network, monitoring]
status: completed
branch: iter-4/console-network
---

# Iteration 4: Console + Network Monitoring

Read console messages and network requests — the two most important debugging data sources after eval.

## Tasks

- [x] Implement `ff-rdp-core/src/actors/watcher.rs` — `WatcherActor` with `watch_resources(types)`, `unwatch_resources(types)`
- [x] Extend `ff-rdp-core/src/actors/console.rs` — add `start_listeners(["PageError", "ConsoleAPI"])`, `get_cached_messages(types)` to existing `WebConsoleActor`
- [x] Implement console message parsing: level, message text, source file, line number, timestamp
- [x] Implement `ff-rdp-core/src/actors/network.rs` — network event parsing from `resource-available-form` events
- [x] Implement `NetworkEventActor` methods: `get_request_headers()`, `get_response_headers()`, `get_response_content()`, `get_event_timings()`
- [x] Implement `ff-rdp-cli/src/commands/console.rs` — `ff-rdp console [--tab ...] [--pattern <regex>] [--level <error|warn|info|log>]`
- [x] Implement `ff-rdp-cli/src/commands/network.rs` — `ff-rdp network [--tab ...] [--filter <url-pattern>] [--method <GET|POST|...>]`
- [x] Console output format: array of `{level, message, source, line, timestamp}`
- [x] Network output format: array of `{method, url, status, duration_ms, size_bytes, content_type, is_xhr}`
- [x] Handle the resource watcher event flow: watchResources → collect resource-available-form events → parse

## Additional Work (post-PR)

- [x] Implement `LongStringActor` in ff-rdp-core for fetching truncated eval results
- [x] Add `network --cached` using Performance Resource Timing API (temporary home, moves to `perf` command in iter 8)
- [x] Add `navigate --with-network` — same-connection watcher subscribe → navigate → drain events
- [x] Extract shared `network_events` module (drain, merge, build helpers) used by both `network` and `navigate`
- [x] Research: watcher does NOT buffer historical events — verified with live Firefox probe
- [x] Research: connection persistence patterns documented in `research/connection-persistence.md`
- [x] Iteration 8 planned: `perf` command + `navigate --with-network` improvements

## Acceptance Criteria

- `ff-rdp console` shows cached console messages from the active tab
- `ff-rdp console --level error` filters to errors only
- `ff-rdp console --pattern "API"` filters by message content
- `ff-rdp network` shows recent network requests with status codes and timing
- `ff-rdp network --filter "api/"` filters by URL pattern
- `ff-rdp network --jq '.results[] | select(.status >= 400)'` finds failed requests
- `ff-rdp network --cached` shows retrospective resource data via Performance API
- `ff-rdp navigate https://example.com --with-network` captures traffic during navigation
- Both commands work with tab targeting
