---
title: "Iteration 21: Page Understanding for AI Agents"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - ai-agent
  - snapshot
  - screenshot
  - geometry
branch: iter-21/page-understanding
---

# Iteration 21: Page Understanding for AI Agents

The features that make ff-rdp useful as an AI agent's browser tool — structured
page understanding without relying on vision.

## Motivation

Chrome MCP's `read_page` + `computer screenshot` are the two tools AI agents use
most. ff-rdp needs equivalents that return structured data an agent can reason
about precisely, reducing reliance on expensive/imprecise vision inference.

## Tasks

- [x] `ff-rdp screenshot --base64` — return screenshot as base64 in JSON output
  instead of saving to file. Essential for AI agents to "see" the page.
  → [[screenshot-inline-data]]
- [x] `ff-rdp snapshot` — combined page structure dump optimized for LLM consumption:
  DOM tree with semantic roles, key attributes, interactive elements, text content.
  Supports `--depth N` and `--max-chars N` with truncation markers
  (`"[... 42 more children]"`). This is ff-rdp's answer to Chrome MCP's `read_page`.
  → [[page-snapshot-command]]
- [x] `ff-rdp geometry <selector> [<selector>...]` — bounding rects, positions,
  z-index, visibility, overflow, with automatic overlap detection between elements.
  Via eval using `getBoundingClientRect()` + `getComputedStyle()`.
  → [[element-geometry-command]]

## Test Fixtures

All e2e test fixtures must be recorded from a real Firefox instance — never hand-craft them.
Run with `FF_RDP_LIVE_TESTS_RECORD=1 cargo test -p ff-rdp-core --test live_record_fixtures -- --ignored` to record fixtures.
