---
title: Decision Log
type: reference
date: 2026-04-06
tags: [decisions, architecture]
status: active
---

# Decision Log

## DEC-001: Use Firefox RDP directly over TCP, not WebDriver BiDi

**Decision**: Communicate with Firefox using the native Remote Debugging Protocol over raw TCP, not through WebDriver BiDi or Selenium.

**Why**: Mozilla's own firefox-devtools-mcp uses WebDriver BiDi via Selenium, adding Node.js + Selenium as intermediaries. Raw RDP over TCP eliminates all middleware for minimum latency. The protocol is simple (length-prefixed JSON) and well-suited for a Rust implementation.

**Trade-off**: No formal JSON schema for RDP (unlike Chrome's CDP). Must reverse-engineer actor capabilities from geckordp source and Firefox DevTools source code.

## DEC-002: Stateless CLI (connect-per-invocation)

**Decision**: Each CLI invocation opens a TCP connection, performs one operation, and disconnects. No persistent daemon or connection pooling.

**Why**: Simplicity. Firefox maintains all state (tabs, page content, console history). A stateless CLI is trivially composable with shell pipelines and Claude Code's Bash tool. Connection overhead is ~5ms on localhost, negligible compared to the value of simplicity.

**Trade-off**: Cannot stream real-time events (e.g., live console tailing). Can add a `--follow` mode later if needed.

## DEC-003: JSON-only output initially

**Decision**: Only JSON output format. No text/table format in v1.

**Why**: The primary consumer is Claude Code (an LLM), which parses JSON natively. The built-in `--jq` flag handles all formatting needs for human readers. Adding a text format doubles the output code for minimal benefit.

**Revisit**: If human CLI usage grows, add `--format text` in a later iteration.

## DEC-004: Tab targeting by index, URL pattern, or actor ID

**Decision**: `--tab <value>` accepts an integer (index), a string (URL substring match), or `--tab-id <actor>` for precise targeting. Default is the active/selected tab.

**Why**: Inspired by cmux's `--surface <id|ref|index>` pattern. Index is fastest for interactive use, URL pattern is most intuitive, actor ID is for precise scripting. Active tab default means zero flags for the common case.

## DEC-005: Crate split — ff-rdp-core + ff-rdp-cli

**Decision**: Two crates following hyalo's pattern. Core is the protocol library (no CLI deps), CLI is the user interface.

**Why**: Core can be reused as a library (e.g., in an MCP server, test framework, or other tool). Clean dependency boundaries: core uses thiserror, CLI uses anyhow. Core is async (tokio), CLI wraps it in a single-threaded runtime.

## DEC-006: thiserror in core, anyhow in CLI

**Decision**: Core library uses typed errors via thiserror. CLI wraps them with anyhow for context chaining.

**Why**: Following hyalo's established pattern. Library consumers need typed errors for matching; CLI just needs human-readable messages with context.

## DEC-007: JSON envelope with meta field

**Decision**: Every command outputs `{"results": ..., "total": N, "meta": {"tab": {...}, "duration_ms": N}}`.

**Why**: Adapted from hyalo's envelope (`results` + `total` + `hints`). Added `meta` instead of `hints` because: (a) we don't need drill-down hints for a debugging tool, (b) knowing which tab was targeted and how long the operation took is valuable debugging context. The envelope is consistent across all commands, enabling `--jq` to operate on a predictable shape.

## DEC-008: Use eval as the implementation for most interaction commands

**Decision**: Commands like `click`, `type`, `dom`, `page-text`, `cookies`, `storage` all use `evaluateJSAsync` internally rather than native protocol actors.

**Why**: The eval approach is simpler (one actor needed), more reliable (JavaScript execution is the best-tested path), and covers 95% of use cases. Native actors (Inspector, Walker, Node) provide structured DOM trees but require complex multi-step actor initialization. Eval-based implementations can be swapped for native ones later without changing the CLI interface.

**Trade-off**: Cannot access HttpOnly cookies, cannot inspect shadow DOM internals, cannot get computed styles directly. These are edge cases deferrable to later iterations.
