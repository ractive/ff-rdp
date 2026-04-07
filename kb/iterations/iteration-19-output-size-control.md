---
title: "Iteration 19: Output Size Control"
type: iteration
status: completed
date: 2026-04-07
tags:
  - iteration
  - output
  - ux
  - ai-agent
  - architecture
branch: iter-19/output-size-control
---

# Iteration 19: Output Size Control

Make all list-returning commands LLM-friendly by default. This is foundational —
every subsequent iteration must follow these patterns.

## Design

→ [[output-size-control]]

## Tasks

### Core flags (add to clap argument parser)

- [x] `--limit N` global flag for all list-returning commands, with per-command defaults
- [x] `--all` flag to override limit and return everything
- [x] `--sort <field>` with `--asc`/`--desc` flags. Default sort per command:
  - `network`: duration desc
  - `perf --type resource`: duration desc
  - `console`: time desc
  - `dom`: document order
- [x] `--fields <field1,field2,...>` to select which fields appear in each entry
- [x] `--detail` flag to switch from summary mode to individual entries

### Apply to existing commands

- [x] `network`: default to summary mode (count by cause_type, total transfer bytes,
  top 20 slowest). `--detail` for per-request entries.
- [x] `perf --type resource`: default to summary mode (count by initiator_type/domain,
  top 20 slowest, total weight). `--detail` for per-resource entries.
- [x] `navigate --with-network`: same summary default for the network portion
- [x] `dom`: default limit 20 matches, with total count in output
- [x] `console`: default limit 50 messages

### Output envelope

- [x] Add `truncated: true` and `total: N` to output envelope when results are
  limited, so the agent knows data was omitted
- [x] Add hint in output: `"hint": "showing 20 of 84, use --all for complete list"`

### Tree output controls (design only — implemented in iterations 22-24)

- [x] Document the `--depth N` and `--max-chars N` pattern with truncation markers
  (`"[... 42 more children]"`) so that `snapshot`, `a11y`, and `dom tree` follow
  a consistent design when they are built

### Document the pattern

- [x] Add output design principles to CLAUDE.md or a decision log entry so all
  future iterations follow them
