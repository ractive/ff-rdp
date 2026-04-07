---
title: Future Features Backlog
type: reference
date: 2026-04-06
tags: [backlog, future]
status: active
---

# Future Features Backlog

Features not yet implemented. Items completed in past iterations are marked done.

## Connection & Transport

- [ ] WebSocket transport mode (`ws:` prefix) for browser-based clients
- [x] Connection pooling / persistent daemon mode for reduced latency on repeated commands *(done: iteration 13)*
- [ ] `--follow` / streaming mode for real-time console and network event tailing
- [ ] Unix socket support for local-only connections

## Debugging

- [ ] Breakpoint management: `ff-rdp breakpoint set <url:line>`, `ff-rdp breakpoint list/remove`
- [ ] Step debugging: `ff-rdp step <into|over|out>`, `ff-rdp resume`, `ff-rdp pause`
- [ ] Stack frame inspection: `ff-rdp frames` when paused at breakpoint
- [x] Source listing: `ff-rdp sources` to enumerate loaded JS files *(done: iteration 10)*
- [ ] Source reading: `ff-rdp source <url> [--line-range]` to fetch source code
- [ ] Blackboxing: `ff-rdp blackbox <url>` to skip library code during debugging

## DOM & CSS (Native Actors)

- [ ] Native Inspector/Walker actor implementation for structured DOM trees
- [ ] Computed styles: `ff-rdp styles <selector>` via PageStyleActor
- [ ] DOM mutation watching: notify on DOM changes
- [ ] Accessibility tree inspection via AccessibilityActor

## Network

- [ ] Request/response body capture: `ff-rdp network <id> --body`
- [ ] Network blocking: `ff-rdp network block <url-pattern>`
- [ ] Network throttling: simulate slow connections
- [ ] HAR export: `ff-rdp network --har` for HTTP Archive format

## Browser Management

- [ ] Profile management: `ff-rdp profile create/list/delete` for isolated sessions
- [ ] Extension debugging: `ff-rdp extensions list` via WebExtensionDescriptorActor
- [ ] Multi-process debugging via ParentProcessDescriptorActor
- [ ] Worker debugging: `ff-rdp workers list` for web/service/shared workers

## Output & Integration

- [ ] `--format text` output mode with human-readable tables
- [ ] `ff-rdp perf audit` — single-command page performance report: TTFB, CWV, resource breakdown by type/domain, top-N slowest resources, third-party weight, DOM stats (node count, inline script size, render-blocking resources). Replaces multi-step jq workflows with one structured JSON output.
- [ ] Cookbook / recipes in `ff-rdp --help`: curated `--jq` one-liners for common tasks (top-N slowest resources, network summary by type, DOM size audit, third-party breakdown, etc.)
- [ ] Shell completions: `ff-rdp completions <bash|zsh|fish>`
- [ ] Configuration file: `.ff-rdp.toml` for default host/port/timeout settings

## Performance

- [x] Connection caching across invocations (socket reuse via background daemon) *(done: iteration 13)*
- [ ] Parallel tab operations: query multiple tabs in one invocation
- [ ] Lazy grip resolution: only fetch full object data when requested

## Distribution

- [ ] Homebrew tap: `brew install ractive/tap/ff-rdp`
- [ ] Scoop bucket for Windows
- [ ] Winget package
- [ ] AUR package for Arch Linux
- [ ] crates.io publication
- [ ] Nix flake
